//! P3-03 Home & Unified Composer — web parity with `VoiceWorkspacePane` (Home
//! stage), `AsciiPortraitCanvas` and `BrailleHorizonCanvas`.
//!
//! Layout: responsive centered portrait stage (renderer toggle + shader pills),
//! mood/status readout with the inline `@profile` parallel-session selector
//! (`UiCommand::SwitchProfile`), the 12-band FFT meter + Braille horizon strip,
//! and the unified composer (text + VAD mic + latching PTT + Send + Thread).
//!
//! ## GPU halftone hook chain (spec §7.1, P2-02 render node)
//!
//! When the app attaches [`HomeHalftoneCompositor`] (feature `gpu-halftone`)
//! and sets `AppState::home_gpu_halftone`, the portrait renders through
//! `El::effect(HOME_HALFTONE_MARK)` → `Compositor::render` → `ui.draw_with`
//! with one instanced quad per `voxcell_halftone::gpu::DotInstance` (analytic
//! SDF dot + quadratic corona, slate tonal palette uploaded as uniforms).
//! Without the feature/flag — or in ASCII mode — the stage falls back to the
//! CPU `pixels_fit` surface, so headless tests never need a GPU device.
//!
//! Stage header height tiers come from `core::layout::resolve_stage_header_size`
//! (Home always resolves `Full`; the tier→dims mapping is shared with the
//! compact/strip headers other views use).
#![allow(unexpected_cfgs)] // `gpu-halftone` feature is declared by theme-chrome (see handoff note)

use crate::state::{AppState, UiCommand};
use crate::theme::VoxcellTheme;
use rune::{Cx, El, SurfaceFit, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, TextInput};
use voxcell_core::layout::{PanelView, StageHeaderSize, resolve_stage_header_size};
use voxcell_core::{
    ActiveView, AsciiShaderMode, AvatarMode, EntryRole, ExpressionMood, MouthViseme,
    TaskLifecycleStatus,
};
use voxcell_halftone::{
    DotInstance, emit_portrait_dot_instances,
    pack_braille_subpixels_ordered_bayer, portrait_assets, resolve_portrait_tone_palette,
    sample_portrait_luminance_field,
};

/// Painter-stream slot mark for the Home GPU halftone pass. Unique across the
/// app's effect marks so foreign slots never collide with the portrait.
pub const HOME_HALFTONE_MARK: u32 = 71;

/// Portrait dot grid — identical to the CPU stage raster in `step_animations`
/// so GPU and CPU dots land on the same subpixel geometry.
pub const STAGE_COLS: usize = 58;
pub const STAGE_ROWS: usize = 27;
pub const STAGE_PIXEL_W: u32 = 320;
pub const STAGE_PIXEL_H: u32 = 294;

// ---------------------------------------------------------------------------
// Pure logic (headless-testable, no rune/wgpu imports beyond El construction)
// ---------------------------------------------------------------------------

/// Effective stage expression derived exactly like `AppState::step_animations`
/// (approval > speaking > listening > thinking > base mood). The GPU emitter
/// re-derives it from state because the animator inputs are not stored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageExpression {
    pub mood: ExpressionMood,
    pub viseme: MouthViseme,
    pub energy: f32,
    pub is_speaking: bool,
    pub is_listening: bool,
}

pub fn stage_expression(state: &AppState) -> StageExpression {
    let running_count = state
        .tasks
        .iter()
        .filter(|t| t.status == TaskLifecycleStatus::Running)
        .count();
    let pending_approval_count = state.approvals.iter().filter(|a| !a.resolved).count();
    let any_thinking = state.thinking_by_channel.values().any(|&v| v);
    let is_speaking = state.audio.is_speaking || state.stage_mood == ExpressionMood::Speaking;
    let is_listening = state.audio.is_listening || state.voice.hands_free_vad;
    let voice_amp = state.audio.output_level.max(state.audio.input_level);

    let mood = if pending_approval_count > 0 {
        ExpressionMood::Alert
    } else if is_speaking {
        ExpressionMood::Speaking
    } else if is_listening {
        ExpressionMood::Listening
    } else if running_count > 0 || any_thinking {
        ExpressionMood::Thinking
    } else {
        state.stage_mood
    };
    let viseme = if is_speaking {
        let amp = if voice_amp > 0.05 {
            voice_amp
        } else {
            ((state.time_s * 7.5).sin().abs() * 0.65 + 0.25).clamp(0.0, 1.0)
        };
        voxcell_core::sidebar_char::amplitude_to_viseme(amp)
    } else if is_listening {
        MouthViseme::Rest
    } else {
        state.stage_viseme
    };
    let energy = if is_speaking {
        voice_amp.max(0.55)
    } else if running_count > 0 || any_thinking {
        0.55
    } else if is_listening {
        0.38
    } else {
        0.16
    };
    StageExpression { mood, viseme, energy, is_speaking, is_listening }
}

/// GPU instance emission for the Home stage (spec §7.1 step 2): luminance
/// field → ordered-Bayer braille grid → tonal palette → `DotInstance` buffer.
/// Pure data; the wgpu render node lives in [`gpu`] behind `gpu-halftone`.
pub fn emit_stage_dot_instances(state: &AppState) -> Vec<DotInstance> {
    let expr = stage_expression(state);
    let raw = sample_portrait_luminance_field(
        STAGE_COLS,
        STAGE_ROWS,
        state.time_s,
        expr.mood,
        expr.viseme,
        expr.energy,
        expr.is_speaking,
        expr.is_listening,
        state.stage_anim.state(),
        portrait_assets(),
    );
    let grid = pack_braille_subpixels_ordered_bayer(&raw, STAGE_COLS, STAGE_ROWS);
    let palette = resolve_portrait_tone_palette(
        expr.mood,
        expr.is_speaking,
        expr.is_listening,
        false,
        false,
        expr.energy,
        state.ui_config.portrait_tint_hex.as_deref(),
    );
    emit_portrait_dot_instances(
        &grid,
        &palette,
        STAGE_PIXEL_W,
        STAGE_PIXEL_H,
        state.ui_config.dot_profile,
    )
}

/// Stage header tier for the Home panel via the TS-parity core resolver.
/// Home always resolves `Full`; the viewport height is still threaded through
/// so the call site matches `resolveStageHeaderSize({ panelView, viewportHeightPx })`.
pub fn home_stage_header_tier(viewport_height_px: f64) -> StageHeaderSize {
    resolve_stage_header_size(PanelView::Home, viewport_height_px)
}

/// Portrait stage dimensions per header tier (Full = web stage canvas,
/// Compact/Strip = the smaller headers Thread/Queue/CLI use on short windows).
pub fn stage_portrait_dims(tier: StageHeaderSize) -> (f32, f32) {
    match tier {
        StageHeaderSize::Full => (320.0, 294.0),
        StageHeaderSize::Compact => (240.0, 220.0),
        StageHeaderSize::Strip => (160.0, 147.0),
    }
}

/// Whether the GPU effect slot should replace the CPU pixels surface.
/// Requires the app-side compositor (feature `gpu-halftone`), the opt-in flag,
/// Halftone mode and the Home view; anything else stays on `pixels_fit`.
#[allow(unexpected_cfgs)]
pub fn home_gpu_halftone_active(state: &AppState) -> bool {
    state.home_gpu_halftone
        && cfg!(feature = "gpu-halftone")
        && state.avatar_mode == AvatarMode::Halftone
        && state.active_view == ActiveView::Home
}

/// Mic is live when hands-free VAD, latching PTT or the audio engine reports
/// listening (web `voiceState.isListening` parity).
pub fn home_mic_active(state: &AppState) -> bool {
    state.voice.hands_free_vad || state.voice.push_to_talk_active || state.audio.is_listening
}

/// Hands-free VAD toggle (web mic button). VAD and PTT are mutually exclusive;
/// `ToggleMic` always reflects the combined mic-on state for the bridge.
pub fn toggle_home_vad(state: &mut AppState) {
    let next = !state.voice.hands_free_vad;
    state.voice.hands_free_vad = next;
    if next {
        state.voice.push_to_talk_active = false;
    }
    let mic_on = state.voice.hands_free_vad || state.voice.push_to_talk_active;
    state.pending_commands.push(UiCommand::ToggleMic(mic_on));
    state.step_animations(0.016);
}

/// Latching push-to-talk toggle. rune exposes no pointer-release handler, so
/// the on-screen PTT latches (press to talk, press to release); true hold
/// semantics live in the global Ctrl/Cmd+Shift+Space hotkey (app bridge).
pub fn toggle_home_ptt(state: &mut AppState) {
    let next = !state.voice.push_to_talk_active;
    state.voice.push_to_talk_active = next;
    if next {
        state.voice.hands_free_vad = false;
    }
    let mic_on = state.voice.hands_free_vad || state.voice.push_to_talk_active;
    state.pending_commands.push(UiCommand::ToggleMic(mic_on));
    state.step_animations(0.016);
}

/// Inline `@profile` selection for parallel side sessions — same dispatch as
/// the CLI `/profile <slug>` path (`UiCommand::SwitchProfile` to the bridge).
pub fn select_home_profile(state: &mut AppState, profile: &str) {
    if profile.is_empty() {
        return;
    }
    state.gateway.active_profile = profile.to_string();
    state
        .pending_commands
        .push(UiCommand::SwitchProfile { profile: profile.to_string() });
}

/// Last spoken Hermes line for the decoded readout (web `lastSpokenLine`).
fn home_decoded_line(state: &AppState) -> String {
    if state.active_is_thinking() {
        return format!("Routing turn inside {}...", state.active_session_id());
    }
    if let Some(entry) = state
        .active_entries()
        .iter()
        .rev()
        .find(|e| e.role == EntryRole::Hermes && !e.text.trim().is_empty())
    {
        let mut line: String = entry.text.chars().take(140).collect();
        if entry.text.chars().count() > 140 {
            line.push('…');
        }
        return line;
    }
    "Ask Hermes anything — voice or text.".to_string()
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn render_home(state: &AppState, theme: &VoxcellTheme, _cx: &mut Cx<AppState>) -> El {
    let k = &theme.kit;

    // Responsive clamp from the measured Home root rect (first frame assumes
    // the 680pt web stage column). Stage header tier via core::layout.
    let measured = state.home_stage_rect;
    let avail_w = (measured.map(|r| r[2]).unwrap_or(712.0) - 32.0).max(240.0);
    let viewport_h = measured.map(|r| r[3]).unwrap_or(800.0) as f64;
    let tier = home_stage_header_tier(viewport_h);
    let (base_w, base_h) = stage_portrait_dims(tier);
    let stage_scale = (avail_w / base_w).min(1.0);
    let (pw, ph) = (base_w * stage_scale, base_h * stage_scale);
    let strip_w = avail_w.min(680.0);

    // 1. Stage header strip: renderer toggle + ASCII shader pills.
    let mut mode_bar = row()
        .key("home-stage-controls")
        .gap(6.0)
        .items_center()
        .child(
            Button::new(match state.avatar_mode {
                AvatarMode::Halftone => "Mode: Halftone",
                AvatarMode::Ascii3d => "Mode: 3D ASCII",
            })
            .variant(ButtonVariant::Neutral)
            .control_size(ControlSize::Small)
            .on_press(|s: &mut AppState| {
                s.avatar_mode = match s.avatar_mode {
                    AvatarMode::Halftone => AvatarMode::Ascii3d,
                    AvatarMode::Ascii3d => AvatarMode::Halftone,
                };
                s.step_animations(0.016);
            })
            .build(k)
            .key("home-mode-toggle"),
        );

    if state.avatar_mode == AvatarMode::Ascii3d {
        for (shader, label) in [
            (AsciiShaderMode::Solid, "Solid"),
            (AsciiShaderMode::Wire, "Wire"),
            (AsciiShaderMode::Depth, "Depth"),
            (AsciiShaderMode::Holo, "Holo"),
            (AsciiShaderMode::Norm, "Norm"),
        ] {
            let active = state.shader_mode == shader;
            mode_bar = mode_bar.child(
                Button::new(label)
                    .variant(if active {
                        ButtonVariant::Submit
                    } else {
                        ButtonVariant::Ghost
                    })
                    .control_size(ControlSize::Mini)
                    .on_press(move |s: &mut AppState| {
                        s.shader_mode = shader;
                        s.step_animations(0.016);
                    })
                    .build(k)
                    .key(format!("home-shader-{}", label.to_ascii_lowercase())),
            );
        }
    }

    // 2. Centered portrait stage — GPU effect slot when the compositor is
    //    attached, CPU pixels_fit fallback otherwise (headless-safe).
    let gpu_active = home_gpu_halftone_active(state);
    let thinking = state.active_is_thinking();
    let expr = stage_expression(state);
    let glow = match expr.mood {
        ExpressionMood::Thinking => theme.semantic_working,
        ExpressionMood::Alert => theme.semantic_error,
        _ => theme.accent,
    };
    let portrait_surface = if gpu_active {
        el().key("home-portrait-gpu")
            .w(pw)
            .h(ph)
            .effect(HOME_HALFTONE_MARK)
    } else {
        el().key("home-portrait-pixels")
            .w(pw)
            .h(ph)
            .pixels_fit(&state.stage_pixels, SurfaceFit::Fit)
    };
    let stage_portrait = el()
        .key("home-portrait-stage")
        .w(pw)
        .h(ph)
        .radius(16.0)
        .fill(theme.bg.alpha(0.35))
        .border(
            1.0,
            if thinking {
                theme.semantic_working.alpha(0.35)
            } else {
                theme.accent.alpha(0.22)
            },
        )
        .radial_gradient(
            [0.5, 0.45],
            165.0 * stage_scale,
            [
                (0.0, glow.alpha(0.14)),
                (0.7, glow.alpha(0.03)),
                (1.0, theme.bg.alpha(0.0)),
            ],
        )
        .on_pointer_move_local(move |s: &mut AppState, ev, _tx| {
            let nx = ((ev.x / pw) * 2.0 - 1.0).clamp(-1.0, 1.0);
            let ny = ((ev.y / ph) * 2.0 - 1.0).clamp(-1.0, 1.0);
            let inside = ev.x >= 0.0 && ev.x <= pw && ev.y >= 0.0 && ev.y <= ph;
            s.pointer_stage = if inside { Some((nx, ny)) } else { None };
        })
        .on_press(|s: &mut AppState| {
            s.toggle_stage_mood();
        })
        .child(portrait_surface);

    // 3. Status readout: mood pulse + inline @profile selector (parallel side
    //    sessions) or the single-profile session badge; offline retry parity.
    let mut mood_line = row()
        .key("home-mood-readout")
        .gap(8.0)
        .items_center()
        .child(
            el().key("home-status-dot")
                .w(7.0)
                .h(7.0)
                .radius(3.5)
                .fill(match expr.mood {
                    ExpressionMood::Thinking => theme.semantic_working,
                    ExpressionMood::Alert => theme.semantic_error,
                    _ => theme.accent,
                }),
        )
        .child(
            text(format!(
                "{} · {}",
                state.stage_mood.label(),
                state.active_channel().map(|c| c.name.as_str()).unwrap_or("Home Stage")
            ))
            .font(theme.mono_sm)
            .color(theme.text_muted),
        );

    let slugs = state.cli_profile_slugs();
    if slugs.len() > 1 {
        let mut selector = row().key("home-profile-select").gap(4.0).items_center();
        for slug in slugs {
            let active = state.gateway.active_profile == slug;
            let name = slug.clone();
            selector = selector.child(
                Button::new(format!("@{slug}"))
                    .variant(if active {
                        ButtonVariant::Submit
                    } else {
                        ButtonVariant::Ghost
                    })
                    .control_size(ControlSize::Mini)
                    .on_press(move |s: &mut AppState| select_home_profile(s, &name))
                    .build(k)
                    .key(format!("home-profile-{slug}"))
                    .checked(active),
            );
        }
        mood_line = mood_line.child(selector);
    } else {
        mood_line = mood_line.child(
            text(format!("@{}", state.gateway.active_profile))
                .key("home-session-badge")
                .font(theme.mono_sm)
                .color(theme.accent_soft),
        );
    }

    if !state.gateway.connected {
        mood_line = mood_line
            .child(
                text("Gateway offline")
                    .key("home-gateway-offline")
                    .font(theme.mono_sm)
                    .color(theme.semantic_error.alpha(0.85)),
            )
            .child(
                Button::new("Retry")
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Mini)
                    .on_press(|s: &mut AppState| {
                        s.pending_commands.push(UiCommand::RefreshGateway);
                    })
                    .build(k)
                    .key("home-gateway-retry"),
            );
    }

    // Decoded speech/routing line (web home-decoded-line parity).
    let decoded_line = text(home_decoded_line(state))
        .key("home-decoded-line")
        .font(theme.ui_sm)
        .color(theme.text.alpha(0.92));

    // 4. 12-band FFT meter + pointer/audio-reactive Braille horizon strip.
    //    The wave itself is FFT-driven via `horizon_anim.step(.., audio.bands)`
    //    in step_animations; the meter exposes the same bands for parity.
    let mut fft_row = row()
        .key("home-fft-bands")
        .gap(2.0)
        .items_center()
        .h(18.0)
        .a11y_hidden();
    for (i, band) in state.audio.bands.iter().enumerate() {
        let b = band.clamp(0.0, 1.0);
        fft_row = fft_row.child(
            el().key(format!("home-fft-band-{i}"))
                .w(4.0)
                .h(2.0 + 16.0 * b)
                .radius(1.0)
                .fill(theme.accent.alpha(0.35 + 0.55 * b)),
        );
    }

    let horizon_strip = el()
        .key("home-horizon-strip")
        .w_full()
        .max_w(strip_w)
        .h(88.0)
        .radius(10.0)
        .on_pointer_move_local(move |s: &mut AppState, ev, _tx| {
            let nx = ((ev.x / strip_w) * 2.0 - 1.0).clamp(-1.0, 1.0);
            let ny = ((ev.y / 88.0) * 2.0 - 1.0).clamp(-1.0, 1.0);
            s.pointer_horizon = (nx, ny);
        })
        .child(
            el().key("home-horizon-pixels")
                .w_full()
                .h(88.0)
                .pixels_fit(&state.horizon_pixels, SurfaceFit::Fill),
        );

    // 5. Unified composer: prompt + VAD mic + latching PTT + Send + Thread.
    let mic_active = home_mic_active(state);
    let vad_on = state.voice.hands_free_vad;
    let ptt_on = state.voice.push_to_talk_active;
    let composer = row()
        .key("home-composer")
        .w_full()
        .max_w(strip_w)
        .padding(8.0)
        .gap(8.0)
        .radius(10.0)
        .fill(theme.card_fill)
        .backdrop_blur(10.0)
        .border(1.0, theme.border_strong)
        .items_center()
        .child(
            el().grow(1.0).min_w(0.0).child(
                TextInput::new(&state.composer_edit)
                    .placeholder("Ask Hermes anything, or type /help...")
                    .label("Prompt Hermes")
                    .plain()
                    .width(None)
                    .on_edit(|s: &mut AppState| &mut s.composer_edit)
                    .on_submit(|s: &mut AppState, _tx| s.submit_composer_prompt())
                    .build(k)
                    .key("home-composer-input"),
            ),
        )
        .child(
            Button::new(if vad_on { "Mic ON" } else { "Mic" })
                .variant(if mic_active {
                    ButtonVariant::Submit
                } else {
                    ButtonVariant::Neutral
                })
                .control_size(ControlSize::Small)
                .on_press(toggle_home_vad)
                .build(k)
                .key("home-mic-toggle"),
        )
        .child(
            Button::new(if ptt_on { "PTT On" } else { "PTT" })
                .variant(if ptt_on {
                    ButtonVariant::Submit
                } else {
                    ButtonVariant::Neutral
                })
                .control_size(ControlSize::Small)
                .on_press(toggle_home_ptt)
                .build(k)
                .key("home-ptt-toggle"),
        )
        .child(
            Button::new("Send")
                .variant(ButtonVariant::Submit)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| s.submit_composer_prompt())
                .build(k)
                .key("home-send"),
        )
        .child(
            Button::new("Thread")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| s.active_view = ActiveView::Thread)
                .build(k)
                .key("home-open-thread"),
        );

    let mut center = col()
        .key("home-stage-center")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .items_center()
        .justify_center()
        .gap(10.0)
        .child(stage_portrait)
        .child(mood_line)
        .child(el().max_w(560.0).child(decoded_line));
    if state.ui_config.horizon_visible {
        center = center.child(fft_row);
    }

    let mut bottom = col().key("home-bottom").w_full().gap(10.0).items_center();
    if state.ui_config.horizon_visible {
        bottom = bottom.child(horizon_strip);
    }
    bottom = bottom.child(composer);

    col()
        .key("home-view")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .h_full()
        .padding(16.0)
        .gap(12.0)
        .items_center()
        .justify_between()
        .on_layout(|s: &mut AppState, r| s.home_stage_rect = Some(r))
        .child(mode_bar)
        .child(center)
        .child(bottom)
}

// ---------------------------------------------------------------------------
// GPU render node (spec §7.1 step 3). Gated behind `gpu-halftone` until
// theme-chrome adds wgpu/bytemuck to voxcell-ui/Cargo.toml; the pure emitter
// above and the pixels_fit fallback compile unconditionally.
// ---------------------------------------------------------------------------

/// WGSL for the instanced SDF dot/corona pass. `DotInstance` is 20 bytes:
/// pos_xy f32x2 @0, radius f32 @8, tier u8(+3 zero pad) @12, alpha f32 @16.
/// The tier attribute reads Uint32 @12 (pad bytes are zero ⇒ value == tier).
#[cfg(feature = "gpu-halftone")]
const HOME_HALFTONE_WGSL: &str = r#"
struct Uniforms {
    rect: vec4f,        // slot rect in device px (x, y, w, h)
    origin_size: vec4f, // pass origin xy + target size zw (device px)
    meta_v: vec4f,      // surface_w, surface_h, time, bloom (`meta` is WGSL-reserved)
    tiers: array<vec4f, 5>, // tonal palette: srgb xyz + core alpha
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
      @location(0) pos_xy: vec2f,
      @location(1) radius: f32,
      @location(2) tier: u32,
      @location(3) alpha: f32) -> VsOut {
    let corner = vec2f(f32(vi & 1u), f32(vi >> 1u)) * 2.0 - 1.0;
    let scale = u.rect.zw / u.meta_v.xy;
    let center_px = u.rect.xy + pos_xy * scale;
    let r_px = max(radius * scale.y, 0.5);
    let extent = r_px * 2.2; // corona headroom
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

fn tier_color(t: u32) -> vec3f {
    switch t {
        case 0u: { return srgb_to_linear(u.tiers[0].xyz); }
        case 1u: { return srgb_to_linear(u.tiers[1].xyz); }
        case 2u: { return srgb_to_linear(u.tiers[2].xyz); }
        case 3u: { return srgb_to_linear(u.tiers[3].xyz); }
        default: { return srgb_to_linear(u.tiers[4].xyz); }
    }
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4f {
    let d = length(in.local);
    let aa = max(fwidth(d), 0.75);
    let core = 1.0 - smoothstep(in.radius_px - aa, in.radius_px + aa, d);
    // Soft quadratic corona around tiers 3-4 (spec §7.1), scaled by bloom.
    let falloff = clamp(1.0 - max(d - in.radius_px, 0.0) / (in.radius_px * 1.2), 0.0, 1.0);
    let corona = falloff * falloff * 0.30 * u.meta_v.w * select(0.0, 1.0, in.tier >= 3u);
    let a = clamp((core + corona) * in.alpha, 0.0, 1.0);
    let c = tier_color(in.tier);
    return vec4f(c * a, a); // premultiplied linear (Rune blend convention)
}
"#;

#[cfg(feature = "gpu-halftone")]
#[allow(unexpected_cfgs)]
pub mod gpu {
    use super::*;
    use rune::compose::{Compositor, DeviceKey, Frame};
    use std::collections::HashMap;
    use voxcell_halftone::dot_instances_as_bytes;

    /// Home portrait GPU halftone compositor. Attach via
    /// `rune::App::compositor(HomeHalftoneCompositor::new())` and set
    /// `state.home_gpu_halftone = true`; every frame still draws the full UI
    /// through `ui.draw_with`, with the instanced dot pass injected at the
    /// `HOME_HALFTONE_MARK` slot (spike: examples/gpu_halftone_spike.rs).
    pub struct HomeHalftoneCompositor {
        device_key: Option<DeviceKey>,
        pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
        bind_layout: Option<wgpu::BindGroupLayout>,
        uniform_buf: Option<wgpu::Buffer>,
        instance_buf: Option<wgpu::Buffer>,
        instance_cap: usize,
    }

    impl HomeHalftoneCompositor {
        pub fn new() -> Self {
            Self {
                device_key: None,
                pipelines: HashMap::new(),
                bind_layout: None,
                uniform_buf: None,
                instance_buf: None,
                instance_cap: 0,
            }
        }

        fn build(&mut self, device: &wgpu::Device) {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("home-halftone-bind"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(128),
                    },
                    count: None,
                }],
            });
            self.uniform_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("home-halftone-uniforms"),
                size: 128,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.bind_layout = Some(layout);
            self.pipelines.clear();
            self.instance_buf = None;
            self.instance_cap = 0;
            self.device_key = None; // set by caller via Frame::device_key
        }

        fn ensure_instances(&mut self, device: &wgpu::Device, dots: usize) {
            if dots <= self.instance_cap && self.instance_buf.is_some() {
                return;
            }
            let cap = dots.next_power_of_two().max(1024);
            self.instance_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("home-halftone-instances"),
                size: (cap * std::mem::size_of::<DotInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.instance_cap = cap;
        }

        fn pipeline(
            &mut self,
            device: &wgpu::Device,
            format: wgpu::TextureFormat,
        ) -> &wgpu::RenderPipeline {
            let layout = self.bind_layout.as_ref().expect("bind layout");
            self.pipelines.entry(format).or_insert_with(|| {
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("home-halftone-wgsl"),
                    source: wgpu::ShaderSource::Wgsl(HOME_HALFTONE_WGSL.into()),
                });
                let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("home-halftone-pipeline-layout"),
                    bind_group_layouts: &[Some(layout)],
                    immediate_size: 0,
                });
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("home-halftone-instanced-dots"),
                    layout: Some(&pl),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<DotInstance>() as u64,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &[
                                wgpu::VertexAttribute {
                                    format: wgpu::VertexFormat::Float32x2,
                                    offset: 0,
                                    shader_location: 0,
                                },
                                wgpu::VertexAttribute {
                                    format: wgpu::VertexFormat::Float32,
                                    offset: 8,
                                    shader_location: 1,
                                },
                                // tier u8 + 3 zero pad bytes read as one u32.
                                wgpu::VertexAttribute {
                                    format: wgpu::VertexFormat::Uint32,
                                    offset: 12,
                                    shader_location: 2,
                                },
                                wgpu::VertexAttribute {
                                    format: wgpu::VertexFormat::Float32,
                                    offset: 16,
                                    shader_location: 3,
                                },
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

    impl Default for HomeHalftoneCompositor {
        fn default() -> Self {
            Self::new()
        }
    }

    /// f32 uniform block → little-endian bytes without a bytemuck dependency.
    fn uniform_bytes(vals: &[f32]) -> Vec<u8> {
        vals.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    impl Compositor<AppState> for HomeHalftoneCompositor {
        fn features(&self) -> wgpu::Features {
            wgpu::Features::empty()
        }

        fn render(&mut self, f: &mut Frame<'_, AppState>) {
            let key = f.device_key();
            if self.device_key != Some(key) {
                self.build(f.device);
                self.device_key = Some(key);
            }

            let active = home_gpu_halftone_active(&f.rt.state);
            let clear = f.rt.clear_color();
            f.ui.prepare(f.device, f.queue, &mut f.rt.text.atlas, &f.rt.instances, f.size);

            if !active {
                // Other views / ASCII mode / fallback: plain UI pass.
                f.ui.draw_with(f.encoder, f.target, clear, &mut |_pass, _site| {});
                return;
            }

            // Per-frame CPU → GPU instance upload (spec §7.1 step 2).
            let dots = emit_stage_dot_instances(&f.rt.state);
            self.ensure_instances(f.device, dots.len());
            let instance_buf = self.instance_buf.as_ref().expect("instance buffer").clone();
            f.queue.write_buffer(&instance_buf, 0, dot_instances_as_bytes(&dots));

            // Uniform block: rect/origin filled per slot; palette + meta now.
            let expr = stage_expression(&f.rt.state);
            let palette = resolve_portrait_tone_palette(
                expr.mood,
                expr.is_speaking,
                expr.is_listening,
                false,
                false,
                expr.energy,
                f.rt.state.ui_config.portrait_tint_hex.as_deref(),
            );
            let mut uniforms = [0.0f32; 32];
            uniforms[8] = STAGE_PIXEL_W as f32;
            uniforms[9] = STAGE_PIXEL_H as f32;
            uniforms[10] = f.rt.state.time_s;
            uniforms[11] = f.rt.state.ui_config.bloom;
            for (i, tier) in palette.iter().enumerate() {
                let base = 12 + i * 4;
                uniforms[base] = tier.rgb[0] as f32 / 255.0;
                uniforms[base + 1] = tier.rgb[1] as f32 / 255.0;
                uniforms[base + 2] = tier.rgb[2] as f32 / 255.0;
                uniforms[base + 3] = tier.alpha;
            }

            let uniform_buf = self.uniform_buf.as_ref().expect("uniforms").clone();
            let bind_layout = self.bind_layout.as_ref().expect("layout").clone();
            let n = dots.len() as u32;
            f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
                if site.mark != HOME_HALFTONE_MARK {
                    return;
                }
                let rect = site.instance.rect;
                uniforms[0] = rect[0];
                uniforms[1] = rect[1];
                uniforms[2] = rect[2];
                uniforms[3] = rect[3];
                uniforms[4] = site.origin[0];
                uniforms[5] = site.origin[1];
                uniforms[6] = site.size[0] as f32;
                uniforms[7] = site.size[1] as f32;
                f.queue.write_buffer(&uniform_buf, 0, &uniform_bytes(&uniforms));
                let bind = f.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("home-halftone-bind"),
                    layout: &bind_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buf.as_entire_binding(),
                    }],
                });
                let pipeline = self.pipeline(f.device, site.format);
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.set_vertex_buffer(0, instance_buf.slice(..));
                pass.draw(0..4, 0..n);
            });
        }

        fn animating(&self) -> bool {
            true
        }
    }
}

#[cfg(feature = "gpu-halftone")]
pub use gpu::HomeHalftoneCompositor;

// ---------------------------------------------------------------------------
// Headless tests (Runtime pattern from voxcell-app/src/main.rs)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Key, KeyEvent, Mods, Runtime};
    #[cfg(feature = "gpu-halftone")]
    use rune::compose::Compositor;
    use voxcell_halftone::dot_instances_as_bytes;

    fn home_view(s: &AppState, cx: &mut Cx<AppState>) -> El {
        let theme = VoxcellTheme::from_config(&s.ui_config);
        render_home(s, &theme, cx)
    }

    fn boot(scene: &str) -> Runtime<AppState> {
        let mut s = AppState::new();
        s.apply_scene_preset(scene);
        let mut rt = Runtime::new(s, home_view, (1280.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);
        rt.frame(0.016); // let on_layout measure the Home root
        rt
    }

    fn click_key(rt: &mut Runtime<AppState>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.016);
    }

    fn has_command<F>(rt: &Runtime<AppState>, pred: F) -> bool
    where
        F: Fn(&UiCommand) -> bool,
    {
        rt.state.pending_commands.iter().any(pred)
    }

    // ---- Inline @profile selector (parallel side sessions) ----

    #[test]
    fn profile_selector_dispatches_switch_profile() {
        let mut rt = boot("idle"); // preset registers default + ops profiles
        assert!(rt.find("home-profile-select").is_some());
        assert!(rt.find("home-profile-default").is_some());
        assert!(rt.find("home-profile-ops").is_some());
        assert_eq!(rt.state.gateway.active_profile, "default");

        click_key(&mut rt, "home-profile-ops");
        assert_eq!(rt.state.gateway.active_profile, "ops");
        assert!(has_command(&rt, |c| matches!(
            c,
            UiCommand::SwitchProfile { profile } if profile == "ops"
        )));

        // Switching back dispatches again (parallel sessions keep routing).
        click_key(&mut rt, "home-profile-default");
        assert_eq!(rt.state.gateway.active_profile, "default");
        assert!(has_command(&rt, |c| matches!(
            c,
            UiCommand::SwitchProfile { profile } if profile == "default"
        )));
    }

    #[test]
    fn single_profile_shows_badge_not_selector() {
        let mut s = AppState::new(); // only the default profile, no caps
        s.apply_scene_preset("idle");
        s.gateway_profiles.truncate(1);
        s.gateway.capabilities.profiles.clear();
        let mut rt = Runtime::new(s, home_view, (1280.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);
        rt.frame(0.016);
        assert!(rt.find("home-profile-select").is_none());
        let badge = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("home-session-badge"))
            .expect("session badge");
        assert_eq!(badge.text.as_deref(), Some("@default"));
    }

    // ---- Unified composer ----

    #[test]
    fn composer_send_dispatches_prompt_and_opens_thread() {
        let mut rt = boot("idle");
        rt.update(|s, _tx| s.composer_edit.replace_text("Hello Hermes"));
        rt.invalidate();
        rt.frame(0.016);
        click_key(&mut rt, "home-send");

        assert!(has_command(&rt, |c| matches!(
            c,
            UiCommand::SendPrompt { prompt, .. } if prompt == "Hello Hermes"
        )));
        assert_eq!(rt.state.active_view, ActiveView::Thread);
        assert!(rt.state.composer_edit.text.is_empty());
        assert!(rt.state.active_entries().iter().any(|e| e.role == EntryRole::User));
    }

    #[test]
    fn composer_empty_submit_is_a_noop() {
        let mut rt = boot("idle");
        click_key(&mut rt, "home-send");
        assert!(!has_command(&rt, |c| matches!(c, UiCommand::SendPrompt { .. })));
        assert_eq!(rt.state.active_view, ActiveView::Home);
    }

    #[test]
    fn composer_enter_submits_typed_text() {
        let mut rt = boot("idle");
        // Focus the input wrapper center (CLI headless-typing pattern), then
        // type all chars, one frame, Enter.
        let (ix, iy) = rt.find("home-composer-input").expect("composer input");
        rt.pointer_down(ix, iy);
        rt.pointer_up(ix, iy);
        rt.frame(0.016);
        for ch in "hi".chars() {
            rt.key(KeyEvent { key: Key::Text(ch.to_string()), mods: Mods::default() });
        }
        rt.frame(0.016);
        assert_eq!(rt.state.composer_edit.text, "hi");
        rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
        rt.frame(0.016);
        assert!(has_command(&rt, |c| matches!(
            c,
            UiCommand::SendPrompt { prompt, .. } if prompt == "hi"
        )));
    }

    // ---- VAD / PTT voice controls ----

    #[test]
    fn vad_and_ptt_toggle_mic_commands_exclusively() {
        let mut rt = boot("idle");
        assert!(!home_mic_active(&rt.state));

        click_key(&mut rt, "home-mic-toggle"); // VAD on
        assert!(rt.state.voice.hands_free_vad);
        assert!(!rt.state.voice.push_to_talk_active);
        assert!(has_command(&rt, |c| matches!(c, UiCommand::ToggleMic(true))));

        click_key(&mut rt, "home-ptt-toggle"); // PTT replaces VAD
        assert!(!rt.state.voice.hands_free_vad);
        assert!(rt.state.voice.push_to_talk_active);
        assert!(home_mic_active(&rt.state));

        click_key(&mut rt, "home-ptt-toggle"); // PTT release -> mic off
        assert!(!rt.state.voice.push_to_talk_active);
        assert!(!home_mic_active(&rt.state));
        assert!(has_command(&rt, |c| matches!(c, UiCommand::ToggleMic(false))));
    }

    #[test]
    fn vad_listening_drives_stage_expression() {
        let mut s = AppState::new();
        toggle_home_vad(&mut s);
        let expr = stage_expression(&s);
        assert!(expr.is_listening);
        assert_eq!(expr.mood, ExpressionMood::Listening);
        toggle_home_ptt(&mut s);
        assert!(!s.voice.hands_free_vad);
        assert!(s.voice.push_to_talk_active);
    }

    // ---- Stage header tiers (core::layout consumption) ----

    #[test]
    fn stage_header_tiers_resolve_via_core_layout() {
        // Home always Full regardless of viewport height (TS parity).
        for h in [400.0, 768.0, 900.0, 2000.0] {
            assert_eq!(home_stage_header_tier(h), StageHeaderSize::Full);
        }
        assert_eq!(stage_portrait_dims(StageHeaderSize::Full), (320.0, 294.0));
        assert_eq!(stage_portrait_dims(StageHeaderSize::Compact), (240.0, 220.0));
        assert_eq!(stage_portrait_dims(StageHeaderSize::Strip), (160.0, 147.0));
        // Cross-check the shared resolver's other-panel boundaries.
        assert_eq!(
            resolve_stage_header_size(PanelView::Queue, 860.0),
            StageHeaderSize::Compact
        );
        assert_eq!(
            resolve_stage_header_size(PanelView::Voice, 699.9),
            StageHeaderSize::Strip
        );
    }

    #[test]
    fn home_portrait_renders_full_tier_at_any_window_height() {
        for size in [(1280.0, 800.0), (1280.0, 600.0)] {
            let mut s = AppState::new();
            s.apply_scene_preset("idle");
            let mut rt = Runtime::new(s, home_view, size, Env::new(true, 1.0));
            rt.frame(0.0);
            rt.frame(0.016);
            let stage = rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("home-portrait-stage"))
                .expect("portrait stage");
            assert!((stage.rect[2] - 320.0).abs() < 1.0, "full-tier width at {size:?}");
            assert!((stage.rect[3] - 294.0).abs() < 1.0, "full-tier height at {size:?}");
        }
    }

    // ---- GPU fallback render path ----

    #[test]
    fn gpu_fallback_renders_pixels_surface_headless() {
        let mut rt = boot("idle");
        // Default: no compositor attached -> CPU pixels_fit surface.
        assert!(!rt.state.home_gpu_halftone);
        assert!(rt.find("home-portrait-pixels").is_some());
        assert!(rt.find("home-portrait-gpu").is_none());
        let pixels = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("home-portrait-pixels"))
            .expect("pixels surface");
        assert!((pixels.rect[2] - 320.0).abs() < 1.0);
        assert!((pixels.rect[3] - 294.0).abs() < 1.0);

        // Even with the flag set, no GPU feature/compositor -> still fallback.
        rt.update(|s, _tx| s.home_gpu_halftone = true);
        rt.invalidate();
        rt.frame(0.016);
        if cfg!(feature = "gpu-halftone") {
            assert!(rt.find("home-portrait-gpu").is_some());
            assert!(rt.find("home-portrait-pixels").is_none());
        } else {
            assert!(rt.find("home-portrait-pixels").is_some());
            assert!(rt.find("home-portrait-gpu").is_none());
        }

        // ASCII mode always uses the CPU surface, flag or not.
        rt.update(|s, _tx| s.avatar_mode = AvatarMode::Ascii3d);
        rt.invalidate();
        rt.frame(0.016);
        assert!(rt.find("home-portrait-pixels").is_some());
        assert!(rt.find("home-portrait-gpu").is_none());
    }

    #[test]
    fn stage_dot_emission_is_bounded_deterministic_and_expression_reactive() {
        let mut s = AppState::new();
        s.apply_scene_preset("idle");
        let a = emit_stage_dot_instances(&s);
        let b = emit_stage_dot_instances(&s);
        assert!(!a.is_empty(), "portrait must emit dots");
        assert_eq!(
            dot_instances_as_bytes(&a),
            dot_instances_as_bytes(&b),
            "same state -> identical instance buffer"
        );
        for d in &a {
            assert!(d.pos_xy[0] >= 0.0 && d.pos_xy[0] <= STAGE_PIXEL_W as f32);
            assert!(d.pos_xy[1] >= 0.0 && d.pos_xy[1] <= STAGE_PIXEL_H as f32);
            assert!(d.radius > 0.0);
            assert!(d.tier <= 4);
            assert!(d.alpha >= 0.0 && d.alpha <= 1.0);
            assert_eq!(d._pad, [0; 3]);
        }
        assert!(a.iter().any(|d| d.tier >= 3), "bright tiers present");
        // Speaking (waveform halo + mouth pose) must change the buffer.
        s.apply_scene_preset("speaking");
        let c = emit_stage_dot_instances(&s);
        assert_ne!(dot_instances_as_bytes(&a), dot_instances_as_bytes(&c));
    }

    // ---- Horizon FFT parity ----

    #[test]
    fn fft_meter_and_horizon_reflect_12_band_audio_state() {
        let mut rt = boot("idle");
        assert!(rt.find("home-horizon-strip").is_some());
        assert!(rt.find("home-fft-bands").is_some());
        for i in 0..12 {
            assert!(rt.find(&format!("home-fft-band-{i}")).is_some(), "band {i}");
        }
        // Inject the speaking preset's spectrum; the tallest bar must be band 2.
        rt.update(|s, _tx| {
            s.audio.bands = [0.4, 0.7, 0.85, 0.75, 0.6, 0.5, 0.65, 0.45, 0.35, 0.25, 0.18, 0.12];
        });
        rt.invalidate();
        rt.frame(0.016);
        let heights: Vec<f32> = (0..12)
            .map(|i| {
                rt.elements()
                    .into_iter()
                    .find(|e| e.key.as_deref() == Some(&format!("home-fft-band-{i}")))
                    .map(|e| e.rect[3])
                    .unwrap_or(0.0)
            })
            .collect();
        let max_idx = heights
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert_eq!(max_idx, 2, "band 2 (0.85) is the tallest: {heights:?}");
        assert!(heights[2] > heights[11], "meter scales with band energy");

        // horizon_visible=false hides the strip + meter (web parity).
        rt.update(|s, _tx| s.ui_config.horizon_visible = false);
        rt.invalidate();
        rt.frame(0.016);
        assert!(rt.find("home-horizon-strip").is_none());
        assert!(rt.find("home-fft-bands").is_none());
    }

    // ---- Real headless GPU render (only when the feature is enabled) ----

    #[cfg(feature = "gpu-halftone")]
    #[test]
    fn gpu_compositor_renders_instanced_dots_headless() {
        use rune::render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer};

        let gpu = match Gpu::try_with_features(Gpu::instance(), None, wgpu::Features::empty()) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("no headless GPU device ({e}); fallback path covered by gpu_fallback_renders_pixels_surface_headless");
                return;
            }
        };
        let mut s = AppState::new();
        s.apply_scene_preset("idle");
        s.home_gpu_halftone = true;
        let (wp, hp) = (640u32, 588u32);
        let mut rt = Runtime::new(s, home_view, (640.0, 588.0), Env::new(true, 1.0));
        rt.frame(0.0);
        rt.frame(0.016);
        let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
        renderer.attach_text(&mut rt.text.atlas);
        let target = Offscreen::new(&gpu.device, wp, hp);
        let mut comp = gpu::HomeHalftoneCompositor::new();
        for _ in 0..2 {
            let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("home-gpu-test"),
            });
            {
                let mut f = rune::compose::Frame {
                    device: &gpu.device,
                    queue: &gpu.queue,
                    encoder: &mut encoder,
                    target: &target.view,
                    format: OFFSCREEN_FORMAT,
                    size: (wp, hp),
                    dt: 0.016,
                    update_ms: 0.0,
                    rt: &mut rt,
                    ui: &mut renderer,
                };
                comp.render(&mut f);
            }
            target.copy(&mut encoder);
            let idx = renderer.submit(&gpu.queue, encoder);
            gpu.device
                .poll(wgpu::PollType::Wait { submission_index: Some(idx), timeout: None })
                .expect("poll");
        }
        let snap = target.read(&gpu.device);
        assert_eq!(snap.len(), (wp * hp * 4) as usize);
        let bg = (11u8, 13u8, 18u8); // slate #0B0D12 clear
        let lit = snap
            .chunks_exact(4)
            .filter(|p| {
                p[0].abs_diff(bg.0) > 3 || p[1].abs_diff(bg.1) > 3 || p[2].abs_diff(bg.2) > 3
            })
            .count();
        assert!(lit > 1_000, "GPU dots must rasterize (got {lit} lit pixels)");
    }
}
