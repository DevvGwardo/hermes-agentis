//! P0-02 Gate-1 spike: 1,600-dot instanced GPU halftone through RUNE's public compositor hook.
//!
//! Verifies spec §2.2 Gate-1 check 2 ("custom wgpu instanced render-pass hook OR zero-copy
//! shared texture upload, < 3.5 ms/frame for 1,600+ dots at 2x DPI, headless"):
//!
//! * `rune::El::effect(mark)` reserves a slot in the painter stream
//!   (third_party/rune/crates/rune/src/el/effect.rs:43).
//! * `rune::Compositor` + `rune::Frame` hand the app the raw `wgpu::Device`, `Queue`,
//!   `CommandEncoder` and target view (crates/rune/src/compose.rs:36-92).
//! * `Renderer::draw_with(.., hook)` calls the hook at each slot with a live
//!   `&mut wgpu::RenderPass` (`EffectHook`, crates/rune/src/render.rs:234,395) — this is the
//!   instanced render-pass hook. We draw one 4-vertex quad x 1,600 instances with an analytic
//!   SDF dot + corona fragment shader, clipped/transformed by the slot's own layer.
//! * Headless GPU: `rune::render::Gpu::try_with_features` (render/device.rs:39) makes a real
//!   DX12 device with no surface/window; `Offscreen` + `write_png` (render/offscreen.rs) give
//!   deterministic PNG readback. No display server is involved.
//!
//! Run (from voxcell-desktop/):
//!   cargo run -p voxcell-app --example gpu_halftone_spike --release
//!
//! Prints adapter info, per-frame CPU/GPU timings (p50/p95/max over 120 measured frames after
//! 30 warmup frames), a PASS/FAIL verdict against the 3.5 ms budget, writes
//! `crates/voxcell-app/gpu_halftone_spike.png` and asserts two same-state renders are
//! byte-identical. Unit tests (cargo test -p voxcell-app --example gpu_halftone_spike) cover
//! dot emission and a 500-row virtualized list with multi-line selection, headless.

use std::collections::HashMap;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use rune::compose::{Compositor, DeviceKey, Frame};
use rune::render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer, write_png};
use rune::{Cx, El, Env, Runtime, col, color, el, text};
use voxcell_core::{ExpressionMood, MouthViseme};
use voxcell_halftone::{PortraitMotionState, portrait_assets, sample_portrait_luminance_field};

const MARK_HALFTONE: u32 = 1;
const COLS: usize = 40;
const ROWS: usize = 40;
const DOTS: usize = COLS * ROWS; // 1,600
const WIN: (f32, f32) = (1440.0, 900.0); // spec §2.2 canvas size, points
const SCALE: f32 = 2.0; // 2x Retina DPI
const BUDGET_MS: f64 = 3.5;

/// GPU-ready instance record (spec §7.1 `DotInstance`, padded to 32 bytes for the vertex ABI).
/// `uv` is normalized inside the effect slot; `radius` is a fraction of slot height; the shader
/// converts both to device pixels from the slot rect, so layout/zoom/DPI stay in RUNE's hands.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct DotInstance {
    uv: [f32; 2],
    radius: f32,
    alpha: f32,
    tier: u32,
    _pad: [u32; 3],
}

/// Emit 1,600 dots from the real portrait luminance field (voxcell-halftone, framework-free).
/// The field is supersampled `[2*cols x 4*rows]`; average each 2x4 subpixel block per dot.
fn emit_dots(t: f32) -> Vec<DotInstance> {
    emit_dots_with(t, ExpressionMood::Calm, MouthViseme::Rest, false)
}

fn emit_dots_with(t: f32, mood: ExpressionMood, viseme: MouthViseme, is_speaking: bool) -> Vec<DotInstance> {
    let sub = sample_portrait_luminance_field(
        COLS,
        ROWS,
        t,
        mood,
        viseme,
        0.35,
        is_speaking,
        false,
        &PortraitMotionState::default(),
        portrait_assets(),
    );
    assert_eq!(sub.len(), DOTS * 8, "luminance field must be 2x4 subpixels per dot");
    let sub_w = COLS * 2;
    let lum: Vec<f32> = (0..DOTS)
        .map(|i| {
            let (cx, cy) = (i % COLS, i / COLS);
            let mut sum = 0.0;
            for dy in 0..4 {
                for dx in 0..2 {
                    sum += sub[(cy * 4 + dy) * sub_w + (cx * 2 + dx)];
                }
            }
            sum / 8.0
        })
        .collect();
    let cell = 1.0 / ROWS as f32;
    lum.iter()
        .enumerate()
        .map(|(i, &l)| {
            let l = l.clamp(0.0, 1.0);
            let tier = if l < 0.06 {
                0
            } else if l < 0.18 {
                1
            } else if l < 0.38 {
                2
            } else if l < 0.62 {
                3
            } else {
                4
            };
            DotInstance {
                uv: [
                    ((i % COLS) as f32 + 0.5) / COLS as f32,
                    ((i / COLS) as f32 + 0.5) / ROWS as f32,
                ],
                radius: cell * (0.10 + 0.50 * l.sqrt()),
                alpha: (l * 1.6).clamp(0.0, 1.0),
                tier,
                _pad: [0; 3],
            }
        })
        .collect()
}

struct SpikeState {
    t: f32,
    dots: Vec<DotInstance>,
}

fn spike_view(_s: &SpikeState, _cx: &mut Cx<SpikeState>) -> El {
    let bg = color::hex("#0B0D12").expect("slate bg");
    col()
        .key("spike-root")
        .w(WIN.0)
        .h(WIN.1)
        .fill(bg)
        .child(
            text("Voxcell P0-02 — 1,600-dot instanced GPU halftone (Compositor hook)")
                .key("spike-title")
                .color(color::hex("#94A3B8").expect("slate accent")),
        )
        .child(el().key("halftone-slot").effect(MARK_HALFTONE).grow(1.0))
}

const WGSL: &str = r#"
struct Uniforms {
    rect: vec4f,        // slot rect in device px, window coords (x, y, w, h)
    origin_size: vec4f, // pass origin xy + pass target size zw (device px)
    time_pad: vec4f,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) pos: vec4f,
    @location(0) local: vec2f,
    @location(1) radius_px: f32,
    @location(2) alpha: f32,
    @location(3) @interpolate(flat) tier: u32,
};

@vertex
fn vs(@builtin(vertex_index) vi: u32,
      @location(0) uv: vec2f,
      @location(1) radius: f32,
      @location(2) alpha: f32,
      @location(3) tier: u32) -> VsOut {
    let corner = vec2f(f32(vi & 1u), f32(vi >> 1u)) * 2.0 - 1.0;
    let center_px = u.rect.xy + uv * u.rect.zw;
    let r_px = max(radius * u.rect.w, 0.5);
    let extent = r_px * 1.6; // corona headroom
    let px = center_px + corner * extent - u.origin_size.xy;
    let sz = u.origin_size.zw;
    var out: VsOut;
    out.pos = vec4f(px.x / sz.x * 2.0 - 1.0, 1.0 - px.y / sz.y * 2.0, 0.0, 1.0);
    out.local = corner * extent;
    out.radius_px = r_px;
    out.alpha = alpha;
    out.tier = tier;
    return out;
}

fn srgb_to_linear(c: vec3f) -> vec3f { return pow(c, vec3f(2.2)); }

// slate-v1 tonal tiers (spec §6.1); tier 4 carries the voice-emerald telemetry accent.
fn tier_color(t: u32) -> vec3f {
    switch t {
        case 0u: { return srgb_to_linear(vec3f(0.392, 0.455, 0.545)); } // #64748B
        case 1u: { return srgb_to_linear(vec3f(0.580, 0.639, 0.722)); } // #94A3B8
        case 2u: { return srgb_to_linear(vec3f(0.796, 0.835, 0.882)); } // #CBD5E1
        case 3u: { return srgb_to_linear(vec3f(0.886, 0.910, 0.941)); } // #E2E8F0
        default: { return srgb_to_linear(vec3f(0.204, 0.827, 0.600)); } // #34D399
    }
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4f {
    let d = length(in.local);
    let aa = max(fwidth(d), 0.75);
    let core = 1.0 - smoothstep(in.radius_px - aa, in.radius_px + aa, d);
    let corona = exp(-max(d - in.radius_px, 0.0) / (in.radius_px * 0.5))
        * 0.18 * select(0.0, 1.0, in.tier >= 3u);
    let a = clamp((core + corona) * in.alpha, 0.0, 1.0);
    let c = tier_color(in.tier);
    return vec4f(c * a, a); // premultiplied, linear (frame srgb_blend = false)
}
"#;

struct Halftone {
    device_key: Option<DeviceKey>,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    bind_layout: Option<wgpu::BindGroupLayout>,
    uniform_buf: Option<wgpu::Buffer>,
    instance_buf: Option<wgpu::Buffer>,
    queries: Option<wgpu::QuerySet>,
    resolve_buf: Option<wgpu::Buffer>,
    read_buf: Option<wgpu::Buffer>,
    timing: bool,
    last_gpu_ns: Option<u64>,
}

impl Halftone {
    fn new() -> Self {
        Self {
            device_key: None,
            pipelines: HashMap::new(),
            bind_layout: None,
            uniform_buf: None,
            instance_buf: None,
            queries: None,
            resolve_buf: None,
            read_buf: None,
            timing: false,
            last_gpu_ns: None,
        }
    }

    fn build(&mut self, device: &wgpu::Device) {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("halftone-bind"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(48),
                },
                count: None,
            }],
        });
        self.uniform_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("halftone-uniforms"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.instance_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("halftone-instances"),
            size: (DOTS * std::mem::size_of::<DotInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.timing = device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);
        if self.timing {
            self.queries = Some(device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("halftone-timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            }));
            self.resolve_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("halftone-ts-resolve"),
                size: 16,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }));
            self.read_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("halftone-ts-read"),
                size: 16,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        self.bind_layout = Some(layout);
        self.pipelines.clear();
        self.device_key = None; // set by caller via Frame::device_key
    }

    fn pipeline(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) -> &wgpu::RenderPipeline {
        let layout = self.bind_layout.as_ref().expect("bind layout");
        self.pipelines.entry(format).or_insert_with(|| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("halftone-wgsl"),
                source: wgpu::ShaderSource::Wgsl(WGSL.into()),
            });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("halftone-pipeline-layout"),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("halftone-instanced-dots"),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<DotInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 },
                            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 8, shader_location: 1 },
                            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 12, shader_location: 2 },
                            wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32, offset: 16, shader_location: 3 },
                        ],
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        // premultiplied alpha, matching Rune's own blending
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        })
    }
}

impl Compositor<SpikeState> for Halftone {
    fn features(&self) -> wgpu::Features {
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
    }

    fn render(&mut self, f: &mut Frame<'_, SpikeState>) {
        let key = f.device_key();
        if self.device_key != Some(key) {
            self.build(f.device);
            self.device_key = Some(key);
        }
        // Per-frame CPU -> GPU instance upload (the DotInstance emitter, spec §7.1 step 2).
        let dots = &f.rt.state.dots;
        f.queue.write_buffer(
            self.instance_buf.as_ref().expect("instance buffer"),
            0,
            bytemuck::cast_slice(dots),
        );
        let clear = f.rt.clear_color();
        f.ui.prepare(f.device, f.queue, &mut f.rt.text.atlas, &f.rt.instances, f.size);

        let uniform_buf = self.uniform_buf.as_ref().expect("uniforms").clone();
        let instance_buf = self.instance_buf.as_ref().expect("instances").clone();
        let bind_layout = self.bind_layout.as_ref().expect("layout").clone();
        let queries = self.queries.clone();
        let timing = self.timing;
        let t = f.rt.state.t;
        let mut drew = false;
        // The hook runs inside the slot's clip/layer with a live wgpu::RenderPass.
        f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
            if site.mark != MARK_HALFTONE {
                return;
            }
            drew = true;
            let rect = site.instance.rect;
            let uniforms: [f32; 12] = [
                rect[0], rect[1], rect[2], rect[3],
                site.origin[0], site.origin[1], site.size[0] as f32, site.size[1] as f32,
                t, 0.0, 0.0, 0.0,
            ];
            f.queue.write_buffer(&uniform_buf, 0, bytemuck::cast_slice(&uniforms));
            let bind = f.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("halftone-bind"),
                layout: &bind_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                }],
            });
            let pipeline = self.pipeline(f.device, site.format);
            if timing {
                if let Some(q) = &queries {
                    pass.write_timestamp(q, 0);
                }
            }
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_vertex_buffer(0, instance_buf.slice(..));
            pass.draw(0..4, 0..DOTS as u32);
            if timing {
                if let Some(q) = &queries {
                    pass.write_timestamp(q, 1);
                }
            }
        });
        assert!(drew, "halftone effect slot must be in the painter stream");
        if timing {
            let q = self.queries.as_ref().expect("queries");
            let resolve = self.resolve_buf.as_ref().expect("resolve");
            let read = self.read_buf.as_ref().expect("read");
            f.encoder.resolve_query_set(q, 0..2, resolve, 0);
            f.encoder.copy_buffer_to_buffer(resolve, 0, read, 0, 16);
        }
    }

    fn animating(&self) -> bool {
        true
    }
}

fn pct(samples: &mut [f64], p: f64) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let i = ((samples.len() as f64 - 1.0) * p).round() as usize;
    samples[i]
}

fn main() {
    let want = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
    let gpu = Gpu::try_with_features(Gpu::instance(), None, want)
        .unwrap_or_else(|e| panic!("headless GPU device failed: {e}"));
    let info = gpu.adapter.get_info();
    println!(
        "adapter: {} | backend: {:?} | type: {:?} | timestamps-inside-pass: {}",
        info.name,
        info.backend,
        info.device_type,
        gpu.device.features().contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES)
    );

    let (wp, hp) = ((WIN.0 * SCALE) as u32, (WIN.1 * SCALE) as u32);
    let state = SpikeState { t: 0.0, dots: emit_dots(0.0) };
    let mut rt = Runtime::new(state, spike_view, WIN, Env::new(true, SCALE));
    rt.poll = Some(|s: &mut SpikeState, t: f64, _tx| {
        s.t = t as f32;
        s.dots = emit_dots(s.t);
        true
    });
    let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
    renderer.attach_text(&mut rt.text.atlas);
    let target = Offscreen::new(&gpu.device, wp, hp);
    let mut comp = Halftone::new();

    let dt = 1.0 / 120.0;
    let warmup = 30;
    let measured = 120;
    let mut cpu_update = Vec::new();
    let mut cpu_encode = Vec::new();
    let mut gpu_pass = Vec::new();
    let mut frame_wall = Vec::new();

    let render_frame = |rt: &mut Runtime<SpikeState>,
                        renderer: &mut Renderer,
                        comp: &mut Halftone,
                        dt: f64|
     -> (f64, f64, f64, Option<u64>) {
        let t0 = Instant::now();
        rt.frame(dt);
        let update_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let t1 = Instant::now();
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("spike-frame"),
        });
        {
            let mut f = Frame {
                device: &gpu.device,
                queue: &gpu.queue,
                encoder: &mut encoder,
                target: &target.view,
                format: OFFSCREEN_FORMAT,
                size: (wp, hp),
                dt,
                update_ms,
                rt,
                ui: renderer,
            };
            comp.render(&mut f);
        }
        let encode_ms = t1.elapsed().as_secs_f64() * 1000.0;
        let t2 = Instant::now();
        let idx = renderer.submit(&gpu.queue, encoder);
        if comp.timing {
            let read = comp.read_buf.as_ref().expect("read buf");
            read.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("map timestamps"));
            gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None })
                .expect("poll");
            let ns = {
                let range = read.slice(..).get_mapped_range().expect("mapped timestamp range");
                let ticks: [u64; 2] = bytemuck::pod_read_unaligned(&range[0..16]);
                ticks[1].saturating_sub(ticks[0])
            };
            read.unmap();
            comp.last_gpu_ns = Some(ns);
        } else {
            gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None })
                .expect("poll");
            comp.last_gpu_ns = None;
        }
        let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let _ = t2;
        (update_ms, encode_ms, wall_ms, comp.last_gpu_ns)
    };

    for k in 0..(warmup + measured) {
        let (u, e, w, g) = render_frame(&mut rt, &mut renderer, &mut comp, dt);
        if k >= warmup {
            cpu_update.push(u);
            cpu_encode.push(e);
            frame_wall.push(w);
            if let Some(ns) = g {
                gpu_pass.push(ns as f64 * gpu.queue.get_timestamp_period() as f64 / 1e6);
            }
        }
    }

    let report = |name: &str, s: &mut Vec<f64>| {
        if s.is_empty() {
            println!("{name}: n/a");
            return;
        }
        let (p50, p95, max) = (pct(s, 0.50), pct(s, 0.95), s[s.len() - 1]);
        println!("{name}: p50 {p50:.3} ms | p95 {p95:.3} ms | max {max:.3} ms (n={})", s.len());
    };
    println!("--- {DOTS} instanced dots, {WIN:?} pt @ {SCALE}x = {wp}x{hp} px, 120 Hz cadence ---");
    report("cpu update (view+layout+emit)", &mut cpu_update);
    report("cpu encode (prepare+draw+submit)", &mut cpu_encode);
    report("gpu pass (timestamp queries) ", &mut gpu_pass);
    report("frame wall (update->gpu done)", &mut frame_wall);

    // GPU timestamps when the adapter supports them; otherwise frame wall time
    // (update -> GPU done, a conservative upper bound on per-frame cost).
    let (gpu_ok, metric) = if !gpu_pass.is_empty() {
        (pct(&mut gpu_pass.clone(), 0.95) < BUDGET_MS, "gpu-pass p95")
    } else {
        (pct(&mut frame_wall.clone(), 0.95) < BUDGET_MS, "frame-wall p95 (no timestamp support)")
    };
    let cpu_ok = {
        let mut sum: Vec<f64> = cpu_update.iter().zip(&cpu_encode).map(|(a, b)| a + b).collect();
        pct(&mut sum, 0.95) < BUDGET_MS
    };
    println!(
        "GATE-1 CHECK 2 verdict: {metric} < {BUDGET_MS} ms = {} | cpu p95 < {BUDGET_MS} ms = {}",
        if gpu_ok { "PASS" } else { "FAIL" },
        if cpu_ok { "PASS" } else { "FAIL" }
    );

    // Determinism + PNG: freeze state, render twice, compare bytes, write the evidence PNG.
    rt.poll = None;
    rt.update(|s, _tx| {
        s.t = 1.0;
        s.dots = emit_dots(1.0);
    });
    let mut snaps: Vec<Vec<u8>> = Vec::new();
    for _ in 0..2 {
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("spike-snap"),
        });
        {
            let mut f = Frame {
                device: &gpu.device,
                queue: &gpu.queue,
                encoder: &mut encoder,
                target: &target.view,
                format: OFFSCREEN_FORMAT,
                size: (wp, hp),
                dt: 0.0,
                update_ms: 0.0,
                rt: &mut rt,
                ui: &mut renderer,
            };
            comp.render(&mut f);
        }
        target.copy(&mut encoder);
        let idx = renderer.submit(&gpu.queue, encoder);
        gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None })
            .expect("poll snap");
        snaps.push(target.read(&gpu.device));
    }
    assert_eq!(snaps[0].len(), (wp * hp * 4) as usize);
    let identical = snaps[0] == snaps[1];
    println!("deterministic PNG (two same-state renders byte-identical): {identical}");
    assert!(identical, "headless renders must be deterministic");
    let bg = (11u8, 13u8, 18u8); // #0B0D12
    let lit = snaps[0]
        .chunks_exact(4)
        .filter(|p| {
            (p[0].abs_diff(bg.0) > 3) || (p[1].abs_diff(bg.1) > 3) || (p[2].abs_diff(bg.2) > 3)
        })
        .count();
    println!("non-background pixels: {lit} (of {})", wp * hp);
    assert!(lit > 50_000, "dots must actually rasterize (got {lit} lit pixels)");
    let png = concat!(env!("CARGO_MANIFEST_DIR"), "/gpu_halftone_spike.png");
    write_png(std::path::Path::new(png), wp, hp, &snaps[0]).expect("write png");
    println!("png: {png}");
    let slot = rt.find("halftone-slot").expect("slot element");
    println!("halftone-slot center (pt): {slot:?}");
    println!("SPIKE OK");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{GrowthSpec, ScrollSpec};
    use std::borrow::Cow;

    #[test]
    fn dot_emission_is_1600_bounded_and_deterministic() {
        let a = emit_dots(0.7);
        let b = emit_dots(0.7);
        assert_eq!(a.len(), 1600);
        assert_eq!(bytemuck::cast_slice::<DotInstance, u8>(&a), bytemuck::cast_slice::<DotInstance, u8>(&b));
        for d in &a {
            assert!(d.uv[0] >= 0.0 && d.uv[0] <= 1.0 && d.uv[1] >= 0.0 && d.uv[1] <= 1.0);
            assert!(d.radius > 0.0 && d.radius <= 0.025);
            assert!(d.alpha >= 0.0 && d.alpha <= 1.0);
            assert!(d.tier <= 4);
        }
        assert!(a.iter().any(|d| d.tier >= 3), "portrait must produce bright tiers");
        assert!(a.iter().any(|d| d.alpha == 0.0), "empty field regions stay transparent");
        // The sprite-bank field is pose-driven: speaking (waveform halo + mouth pose) must change the dots.
        let c = emit_dots_with(0.7, ExpressionMood::Speaking, MouthViseme::Wide, true);
        assert_ne!(bytemuck::cast_slice::<DotInstance, u8>(&a), bytemuck::cast_slice::<DotInstance, u8>(&c));
    }

    struct ListState;

    fn list_view(_s: &ListState, _cx: &mut Cx<ListState>) -> El {
        col().key("root").w(800.0).h(600.0).child(
            col()
                .key("viewport")
                .scroll(ScrollSpec { stick_bottom: false })
                .grow(1.0)
                .min_h(0.0)
                .child(
                    col()
                        .key("rows")
                        .growth(GrowthSpec::default())
                        .selection_group(true)
                        .virtual_rows(
                            500,
                            24.0,
                            |_s: &ListState, i| Cow::Owned(format!("r{i}")),
                            |_s: &ListState, i| {
                                text(format!("row {i:03} — the quick brown fox jumps over the lazy dog"))
                                    .selectable(true)
                                    .h(24.0)
                            },
                        ),
                ),
        )
    }

    fn built_rows(rt: &Runtime<ListState>) -> Vec<usize> {
        // Virtual rows surface under their key_fn key ("r{i}"); non-row keys fail to parse.
        let mut idx: Vec<usize> = rt
            .elements()
            .iter()
            .filter_map(|e| e.key.as_deref()?.strip_prefix("r")?.parse().ok())
            .collect();
        idx.sort_unstable();
        idx
    }

    #[test]
    fn virtualized_500_row_list_builds_only_the_window_and_selects_across_rows() {
        let mut rt = Runtime::new(ListState, list_view, (800.0, 600.0), Env::new(true, 1.0));
        rt.frame(0.0);
        while rt.frame(1.0 / 120.0) {}
        let rows = built_rows(&rt);
        assert!(rows.len() >= 10, "viewport rows must be built, got {}", rows.len());
        assert!(rows.len() < 150, "500-row list must not build all rows, got {}", rows.len());
        assert!(rows.contains(&0) && rows.contains(&20));

        // Multi-line drag selection across virtual rows (copy payload via selected_text).
        // Gesture mirrors rune's own remount.rs test: anchor near the row's top-left, frames between moves.
        let (_, ay) = rt.find("r2").expect("row 2 text");
        let (_, by) = rt.find("r6").expect("row 6 text");
        let (sx, sy) = (1.0, ay - 4.0);
        let (ex, ey) = (120.0, by - 4.0);
        rt.pointer_down(sx, sy);
        rt.pointer_move((sx + ex) * 0.5, (sy + ey) * 0.5);
        rt.frame(1.0 / 120.0);
        rt.pointer_move(ex, ey);
        rt.frame(1.0 / 120.0);
        rt.pointer_up(ex, ey);
        rt.frame(1.0 / 120.0);
        let sel = rt.selected_text().expect("drag must select text");
        assert!(sel.contains("row 002"), "selection starts at row 2: {sel:?}");
        assert!(sel.contains("row 006"), "selection spans to row 6: {sel:?}");
        assert!(sel.contains("row 004"), "selection includes middle rows: {sel:?}");

        // Scroll far down: the window moves, row 0 unbuilds, total built stays bounded.
        rt.wheel(400.0, 300.0, 3000.0);
        for _ in 0..40 {
            rt.frame(0.016);
        }
        let mut rows2 = built_rows(&rt);
        if rows2.iter().max().copied().unwrap_or(0) < 100 {
            rt.wheel(400.0, 300.0, -3000.0);
            for _ in 0..40 {
                rt.frame(0.016);
            }
            rows2 = built_rows(&rt);
        }
        assert!(rows2.iter().max().copied().unwrap_or(0) > 100, "scroll must reach deep rows: {rows2:?}");
        assert!(rows2.len() < 150, "window stays bounded after scroll, got {}", rows2.len());
        assert!(!rows2.contains(&0), "row 0 must unbuild once far away");
    }
}
