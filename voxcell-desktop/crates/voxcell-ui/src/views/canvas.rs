//! P4-01 Spatial Canvas (Hermes Dots) — web parity (`BrailleSessionsCanvas.tsx`).
//!
//! * GPU pan/zoom viewport: `core::canvas` clamp/zoom math (`CanvasViewport`,
//!   `clamp_canvas_pan`, `zoom_viewport_around_point`), background drag-to-pan
//!   with the TS 4px threshold, toolbar zoom step/reset controls.
//! * Hermes Dots: the 9 SDF shape engines with live breathing / ear-wiggle /
//!   eye-lead come from `voxcell_halftone::shapes::render_canvas_dot_rgba`
//!   (driven each frame by `AppState::step_animations` into `dot_pixels`).
//! * Constellation/Orbit/Grid transitions: `core::canvas::arrange_sessions_formation`
//!   targets + rune-motion `Spring::SMOOTH` sampled as a pure function of
//!   `time_s` with `core::motion` stagger delays (headless-deterministic).
//! * Inline dot prompt + voice bar: popover prompt dispatches through
//!   `dispatch_user_prompt` -> existing `SendPrompt`; mic rides `ToggleMic`.
//! * Edge-clamped popovers: `core::layout::compute_canvas_popover_alignment`
//!   (left <24% / right >76% / center).
//! * Pin persistence: `duet.canvasSessions.v2` / `duet.canvasPins.v1` payloads
//!   via `core::canvas` save/load/normalize; mutations push `PersistDots`.
//!
//! GPU dot rendering uses the spike's Compositor/EffectHook chain
//! (`examples/gpu_halftone_spike.rs`, frozen): each dot sprite reserves an
//! `El::effect(CANVAS_DOTS_MARK)` slot; [`gpu::CanvasDotCompositor`] draws the
//! instanced SDF dots inside each slot's live render pass. Gated behind the
//! `gpu-halftone` feature until theme-chrome adds wgpu/bytemuck to
//! voxcell-ui/Cargo.toml and attaches the compositor in app/main.rs; the
//! default path is the headless-safe `pixels_fit` CPU surface.
#![allow(unexpected_cfgs)] // `gpu-halftone` feature is declared by theme-chrome (handoff note)

use crate::state::{AppState, CanvasPanDrag, UiCommand, CANVAS_DESIGN_H, CANVAS_DESIGN_W};
use crate::theme::{VoxcellTheme, parse_hex_color};
use rune::{Cx, El, Phase, SurfaceFit, TextStyle, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, TextInput};
use voxcell_core::canvas::{
    CanvasFormation, CanvasPinKind, CanvasShapeKind, clamp_canvas_pan, format_canvas_dot_label,
    get_shape_meta,
};
use voxcell_core::layout::CanvasPopoverAlign;
use voxcell_core::{ActiveView, EntryRole};
use voxcell_halftone::DotInstance;

/// Painter-slot mark for canvas dot GPU sprites (home stage uses 71).
pub const CANVAS_DOTS_MARK: u32 = 72;
/// Halftone grid per dot sprite (matches `step_animations` CPU surface).
pub const CANVAS_DOT_COLS: usize = 22;
pub const CANVAS_DOT_ROWS: usize = 11;
pub const CANVAS_DOT_PX: u32 = 96;

/// Whether GPU dot sprites replace the CPU `pixels_fit` surfaces. Requires the
/// app-side compositor (feature `gpu-halftone`), the opt-in flag and the Canvas
/// view; anything else stays on the headless-safe fallback.
#[allow(unexpected_cfgs)]
pub fn canvas_gpu_dots_active(state: &AppState) -> bool {
    state.canvas_gpu_dots
        && cfg!(feature = "gpu-halftone")
        && state.active_view == ActiveView::Canvas
}

/// Mic live for the inline voice bar (hands-free VAD or engine listening).
pub fn canvas_mic_active(state: &AppState) -> bool {
    state.voice.hands_free_vad || state.audio.is_listening
}

fn pin_kind_label(kind: CanvasPinKind) -> &'static str {
    match kind {
        CanvasPinKind::Brief => "Brief",
        CanvasPinKind::Repo => "Repo",
        CanvasPinKind::Note => "Note",
        CanvasPinKind::Skill => "Skill",
    }
}

// ---------------------------------------------------------------------------
// Pure GPU instance emission (headless-testable, no wgpu imports).
// ---------------------------------------------------------------------------

/// Downsample a dot's CPU RGBA surface (`render_canvas_dot_rgba`, 96x96) into
/// a `cols x rows` halftone instance grid. ABI mirrors the frozen spike:
/// `pos_xy` is normalized [0..1] inside the dot sprite rect and `radius` is a
/// fraction of sprite height — the canvas WGSL converts both to device pixels
/// from the per-slot rect, so layout/zoom/DPI stay in RUNE's hands. Luminance
/// is alpha-weighted Rec.709; tier cutoffs/radius/alpha match the spike.
pub fn emit_canvas_dot_instances(
    rgba: &[u8],
    pixel_w: u32,
    pixel_h: u32,
    cols: usize,
    rows: usize,
) -> Vec<DotInstance> {
    if cols == 0 || rows == 0 || pixel_w == 0 || pixel_h == 0 {
        return Vec::new();
    }
    if rgba.len() < (pixel_w as usize) * (pixel_h as usize) * 4 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(cols * rows);
    let cell = 1.0 / rows as f32;
    for r in 0..rows {
        let y0 = (r as u32 * pixel_h) / rows as u32;
        let y1 = (((r + 1) as u32 * pixel_h) / rows as u32).max(y0 + 1).min(pixel_h);
        for c in 0..cols {
            let x0 = (c as u32 * pixel_w) / cols as u32;
            let x1 = (((c + 1) as u32 * pixel_w) / cols as u32).max(x0 + 1).min(pixel_w);
            let mut sum = 0.0_f32;
            let mut n = 0_u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let idx = ((y * pixel_w + x) * 4) as usize;
                    let a = rgba[idx + 3] as f32 / 255.0;
                    let l = (0.2126 * rgba[idx] as f32
                        + 0.7152 * rgba[idx + 1] as f32
                        + 0.0722 * rgba[idx + 2] as f32)
                        / 255.0;
                    sum += l * a;
                    n += 1;
                }
            }
            let l = if n > 0 { (sum / n as f32).clamp(0.0, 1.0) } else { 0.0 };
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
            out.push(DotInstance::new(
                [
                    (c as f32 + 0.5) / cols as f32,
                    (r as f32 + 0.5) / rows as f32,
                ],
                cell * (0.10 + 0.50 * l.sqrt()),
                tier,
                (l * 1.6).clamp(0.0, 1.0),
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn render_canvas(state: &AppState, theme: &VoxcellTheme, _cx: &mut Cx<AppState>) -> El {
    let k = &theme.kit;
    let zoom = state.canvas_viewport.zoom;
    let gpu = canvas_gpu_dots_active(state);
    let (stage_w, stage_h) = state.canvas_stage_size();

    // 1. Toolbar: formations, zoom controls, pin composer, add dot.
    let formation_btn = |label: &'static str, f: CanvasFormation, key: &'static str| {
        Button::new(label)
            .variant(if state.canvas_formation == f {
                ButtonVariant::Submit
            } else {
                ButtonVariant::Ghost
            })
            .control_size(ControlSize::Small)
            .on_press(move |s: &mut AppState| s.canvas_set_formation(f))
            .build(k)
            .key(key)
    };
    let toolbar = row()
        .key("canvas-toolbar")
        .w_full()
        .justify_between()
        .items_center()
        .padding(10.0)
        .radius(theme.radius_card)
        .fill(theme.card_fill)
        .border(1.0, theme.border)
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(
                    text("Hermes Dots")
                        .font(TextStyle::new(14.0, 700.0))
                        .color(theme.text),
                )
                .child(
                    text(format!("{} spatial nodes", state.dots.len()))
                        .font(theme.mono_sm)
                        .color(theme.text_muted),
                )
                .child(formation_btn("Constellation", CanvasFormation::Constellation, "canvas-form-constellation"))
                .child(formation_btn("Orbit", CanvasFormation::Orbit, "canvas-form-orbit"))
                .child(formation_btn("Grid", CanvasFormation::Grid, "canvas-form-grid")),
        )
        .child(
            row()
                .gap(6.0)
                .items_center()
                .child(
                    Button::new("−")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| s.canvas_zoom_step(-1))
                        .build(k)
                        .key("canvas-zoom-out"),
                )
                .child(
                    text(format!("{}%", (zoom * 100.0).round() as i32))
                        .key("canvas-zoom-label")
                        .font(theme.mono_sm)
                        .color(theme.text_muted),
                )
                .child(
                    Button::new("+")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| s.canvas_zoom_step(1))
                        .build(k)
                        .key("canvas-zoom-in"),
                )
                .child(
                    Button::new("Reset view")
                        .variant(if state.canvas_viewport_active() {
                            ButtonVariant::Neutral
                        } else {
                            ButtonVariant::Ghost
                        })
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| s.canvas_reset_viewport())
                        .build(k)
                        .key("canvas-zoom-reset"),
                )
                .child(
                    Button::new("Pin")
                        .variant(if state.canvas_pin_composer_open {
                            ButtonVariant::Submit
                        } else {
                            ButtonVariant::Ghost
                        })
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| {
                            s.canvas_pin_composer_open = !s.canvas_pin_composer_open;
                        })
                        .build(k)
                        .key("canvas-pin-toggle"),
                )
                .child(
                    Button::new("+ Add Dot")
                        .variant(ButtonVariant::Neutral)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| {
                            s.add_dot_modal_open = true;
                            s.step_animations(0.016);
                        })
                        .build(k)
                        .key("canvas-add-dot"),
                ),
        );

    // 2. Spatial field: pan background, GPU slot sprites, pins, dots, popover.
    let mut field = el()
        .key("canvas-field")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .radius(theme.radius_card)
        .fill(theme.bg.alpha(0.55))
        .border(1.0, theme.border)
        .on_layout(|s: &mut AppState, r: [f32; 4]| {
            s.canvas_field_rect = Some(r);
        })
        // TS handleStageWheel: wheel zooms about the cursor (1.08 / 0.92).
        .on_wheel(|s: &mut AppState, e: rune::WheelEvent, _tx| {
            if e.dy == 0.0 {
                return false;
            }
            let factor = if e.dy > 0.0 { 0.92 } else { 1.08 };
            let next = s.canvas_viewport.zoom * factor;
            s.canvas_zoom_at(next, e.x, e.y);
            true
        });

    // Background drag-to-pan (TS panMetaRef: 4px threshold, clamped pan).
    field = field.child(
        el()
            .key("canvas-bg")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .on_drag(|s: &mut AppState, drag, _tx| {
                if s.canvas_dragging_dot.is_some() {
                    return;
                }
                match drag.phase {
                    Phase::Down => {
                        s.canvas_pan_drag = Some(CanvasPanDrag {
                            orig_x: s.canvas_viewport.pan_x,
                            orig_y: s.canvas_viewport.pan_y,
                            moved: false,
                        });
                    }
                    Phase::Move => {
                        if let Some(mut pd) = s.canvas_pan_drag {
                            if !pd.moved && drag.dx.hypot(drag.dy) >= 4.0 {
                                pd.moved = true;
                            }
                            if pd.moved {
                                let (px, py) = clamp_canvas_pan(
                                    pd.orig_x + drag.dx,
                                    pd.orig_y + drag.dy,
                                    s.canvas_viewport.zoom,
                                );
                                s.canvas_viewport.pan_x = px;
                                s.canvas_viewport.pan_y = py;
                            }
                            s.canvas_pan_drag = Some(pd);
                        }
                    }
                    Phase::Up => {
                        s.canvas_pan_drag = None;
                    }
                }
            }),
    );

    if state.dots.is_empty() {
        field = field.center().child(
            col()
                .key("canvas-empty")
                .items_center()
                .gap(10.0)
                .padding(24.0)
                .radius(theme.radius_card)
                .fill(theme.card_fill)
                .border(1.0, theme.border)
                .child(
                    text("No spatial character nodes on Canvas yet")
                        .font(TextStyle::new(14.0, 600.0))
                        .color(theme.text),
                )
                .child(
                    text("Click + Add Dot to spawn an interactive character node, or Pin to Canvas from the sidebar.")
                        .font(theme.mono_sm)
                        .color(theme.text_muted),
                )
                .child(
                    Button::new("Spawn First Character Dot")
                        .variant(ButtonVariant::Submit)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| {
                            s.add_dot_modal_open = true;
                            s.step_animations(0.016);
                        })
                        .build(k)
                        .key("canvas-empty-spawn"),
                ),
        );
    } else {
        // Pins (below dots in painter order).
        for pin in &state.canvas_pins {
            let (sx, sy) = state.canvas_world_to_screen(
                pin.x_pct / 100.0 * CANVAS_DESIGN_W,
                pin.y_pct / 100.0 * CANVAS_DESIGN_H,
            );
            let linked = pin.linked_channel_id.clone();
            field = field.child(
                col()
                    .key(format!("canvas-pin-{}", pin.id))
                    .absolute(sx - 75.0, sy - 24.0)
                    .w(150.0)
                    .padding(8.0)
                    .gap(2.0)
                    .radius(theme.radius_control)
                    .fill(theme.card_fill)
                    .border(0.75, theme.border_subtle)
                    .on_press(move |s: &mut AppState| {
                        if let Some(ch) = linked.clone() {
                            s.active_channel_id = ch;
                        }
                    })
                    .child(
                        text(pin_kind_label(pin.kind))
                            .font(theme.mono_sm)
                            .color(theme.text_faint),
                    )
                    .child(
                        text(pin.title.clone())
                            .font(TextStyle::new(11.5, 600.0).lh(14.0))
                            .color(theme.text),
                    ),
            );
        }

        // Dots.
        for (i, dot) in state.dots.iter().enumerate() {
            let selected = state.selected_dot_id.as_deref() == Some(dot.id.as_str());
            let busy = state.canvas_dot_busy(&dot.channel_id);
            let accent = parse_hex_color(&dot.color_hex, theme.accent);
            let ring = if busy {
                theme.semantic_working
            } else if selected {
                accent
            } else {
                theme.border
            };

            let (dx, dy) = state.canvas_layout_display_pos(i);
            let (sx, sy) = state.canvas_world_to_screen(dx, dy);
            let node_w = 136.0 * zoom;
            let node_h = 136.0 * zoom;
            let sprite_sz = 84.0 * zoom;

            let dot_id_drag = dot.id.clone();
            let dot_id_press = dot.id.clone();

            let sprite = if gpu {
                // GPU path: the compositor draws instanced SDF dots in this slot.
                el().w(sprite_sz).h(sprite_sz).effect(CANVAS_DOTS_MARK)
            } else {
                state
                    .dot_pixels
                    .get(&dot.id)
                    .map(|px| el().w(sprite_sz).h(sprite_sz).pixels_fit(px, SurfaceFit::Fit))
                    .unwrap_or_else(|| {
                        el().w(sprite_sz).h(sprite_sz).radius(sprite_sz * 0.5).fill(accent.alpha(0.25))
                    })
            };

            let node = col()
                .key(format!("canvas-node-{}", dot.id))
                .absolute(sx - node_w * 0.5, sy - node_h * 0.5)
                .w(node_w)
                .padding(8.0 * zoom)
                .gap(4.0 * zoom)
                .items_center()
                .radius(theme.radius_card)
                .fill(if selected {
                    theme.card_fill
                } else {
                    theme.surface.alpha(0.85)
                })
                .border(if selected || busy { 1.25 } else { 0.75 }, ring)
                .on_press(move |s: &mut AppState| {
                    s.canvas_open_popover(&dot_id_press);
                })
                .on_drag(move |s: &mut AppState, drag, _tx| {
                    match drag.phase {
                        Phase::Down => s.canvas_begin_dot_drag(&dot_id_drag),
                        Phase::Move => s.canvas_move_dot_drag(&dot_id_drag, drag.dx, drag.dy),
                        Phase::Up => s.canvas_end_dot_drag(),
                    }
                })
                .child(sprite)
                .child(
                    text(dot.label.clone())
                        .font(TextStyle::new(12.0 * zoom, 600.0).lh(15.0 * zoom))
                        .color(theme.text),
                )
                .child(
                    text(if busy { "thinking..." } else { dot.shape.label() })
                        .font(theme.mono_sm)
                        .color(if busy { theme.semantic_working } else { accent }),
                );
            field = field.child(node);
        }

        // Edge-clamped popover for the open dot.
        if let Some(dot) = state
            .canvas_popover_dot
            .as_deref()
            .and_then(|id| state.dots.iter().find(|d| d.id == id))
        {
            let align = state.canvas_popover_align(&dot.id);
            let (dx, dy) = (dot.x, dot.y);
            let (sx, sy) = state.canvas_world_to_screen(dx, dy);
            let pop_w = 264.0_f32;
            let pop_h = 238.0_f32;
            let gap = 24.0 * zoom;
            let raw_left = match align {
                CanvasPopoverAlign::Left => sx + gap,
                CanvasPopoverAlign::Right => sx - gap - pop_w,
                CanvasPopoverAlign::Center => sx - pop_w * 0.5,
            };
            let left = raw_left.clamp(8.0, (stage_w - pop_w - 8.0).max(8.0));
            let top = (sy + 52.0 * zoom).clamp(8.0, (stage_h - pop_h - 8.0).max(8.0));

            let braille = AppState::canvas_shape_to_braille(dot.shape);
            let meta = get_shape_meta(braille);
            let turns = state
                .entries
                .iter()
                .filter(|e| e.channel_id == dot.channel_id && e.role == EntryRole::User)
                .count();
            let secondary = state
                .canvas_sessions
                .iter()
                .find(|s| s.channel_id == dot.channel_id)
                .map(|s| format_canvas_dot_label(s, None, turns).secondary_label)
                .unwrap_or_else(|| format!("{} engine", dot.shape.label()));

            let dot_id_thread = dot.id.clone();
            let dot_id_del = dot.id.clone();
            let ch_thread = dot.channel_id.clone();
            let mic = canvas_mic_active(state);

            field = field.child(
                col()
                    .key("canvas-popover")
                    .absolute(left, top)
                    .w(pop_w)
                    .padding(12.0)
                    .gap(8.0)
                    .radius(theme.radius_card)
                    .fill(theme.surface_elevated)
                    .border(1.0, theme.border_strong)
                    .child(
                        row()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                text(dot.label.clone())
                                    .font(TextStyle::new(13.0, 700.0))
                                    .color(theme.text),
                            )
                            .child(
                                Button::new("×")
                                    .variant(ButtonVariant::Ghost)
                                    .control_size(ControlSize::Mini)
                                    .on_press(|s: &mut AppState| s.canvas_close_popover())
                                    .build(k)
                                    .key("canvas-popover-close"),
                            ),
                    )
                    .child(
                        text(format!("{} · {}", meta.tagline, dot.shape.label()))
                            .font(theme.mono_sm)
                            .color(theme.text_muted),
                    )
                    .child(
                        text(secondary)
                            .font(theme.mono_sm)
                            .color(theme.text_faint),
                    )
                    .child(
                        TextInput::new(&state.canvas_prompt_edit)
                            .placeholder("Prompt this dot…")
                            .label("Dot prompt")
                            .width(None)
                            .on_edit(|s: &mut AppState| &mut s.canvas_prompt_edit)
                            .on_submit(|s: &mut AppState, _tx| s.canvas_submit_prompt_edit())
                            .build(k)
                            .key("canvas-popover-prompt"),
                    )
                    .child(
                        row()
                            .w_full()
                            .gap(6.0)
                            .items_center()
                            .child(
                                Button::new(if mic { "Listening…" } else { "Voice" })
                                    .variant(if mic {
                                        ButtonVariant::Submit
                                    } else {
                                        ButtonVariant::Ghost
                                    })
                                    .control_size(ControlSize::Small)
                                    .on_press(|s: &mut AppState| s.canvas_toggle_voice())
                                    .build(k)
                                    .key("canvas-popover-voice"),
                            )
                            .child(
                                Button::new("Open Thread")
                                    .variant(ButtonVariant::Submit)
                                    .control_size(ControlSize::Small)
                                    .on_press(move |s: &mut AppState| {
                                        s.active_channel_id = ch_thread.clone();
                                        s.active_view = ActiveView::Thread;
                                        s.canvas_close_popover();
                                        let _ = dot_id_thread;
                                    })
                                    .build(k)
                                    .key("canvas-popover-thread"),
                            )
                            .child(
                                Button::new("Remove")
                                    .variant(ButtonVariant::Danger)
                                    .control_size(ControlSize::Small)
                                    .on_press(move |s: &mut AppState| {
                                        s.dots.retain(|d| d.id != dot_id_del);
                                        if s.selected_dot_id.as_deref() == Some(dot_id_del.as_str()) {
                                            s.selected_dot_id = None;
                                        }
                                        s.canvas_close_popover();
                                        s.canvas_sync_sessions();
                                        s.pending_commands.push(UiCommand::PersistDots);
                                    })
                                    .build(k)
                                    .key("canvas-popover-remove"),
                            ),
                    ),
            );
        }

        // Pin composer (toolbar "Pin").
        if state.canvas_pin_composer_open {
            let kind_btn = |label: &'static str, kind: CanvasPinKind, key: &'static str| {
                Button::new(label)
                    .variant(if state.canvas_pin_kind == kind {
                        ButtonVariant::Submit
                    } else {
                        ButtonVariant::Neutral
                    })
                    .control_size(ControlSize::Mini)
                    .on_press(move |s: &mut AppState| s.canvas_pin_kind = kind)
                    .build(k)
                    .key(key)
            };
            field = field.child(
                col()
                    .key("canvas-pin-composer")
                    .absolute((stage_w - 252.0).max(8.0), 12.0)
                    .w(240.0)
                    .padding(10.0)
                    .gap(8.0)
                    .radius(theme.radius_card)
                    .fill(theme.surface_elevated)
                    .border(1.0, theme.border_strong)
                    .child(
                        text("Pin spatial context")
                            .font(theme.mono_sm)
                            .color(theme.text_muted),
                    )
                    .child(
                        TextInput::new(&state.canvas_pin_title_edit)
                            .placeholder("Pin title…")
                            .label("Pin title")
                            .width(None)
                            .on_edit(|s: &mut AppState| &mut s.canvas_pin_title_edit)
                            .build(k)
                            .key("canvas-pin-title"),
                    )
                    .child(
                        row()
                            .w_full()
                            .gap(4.0)
                            .child(kind_btn("Brief", CanvasPinKind::Brief, "canvas-pin-kind-brief"))
                            .child(kind_btn("Repo", CanvasPinKind::Repo, "canvas-pin-kind-repo"))
                            .child(kind_btn("Note", CanvasPinKind::Note, "canvas-pin-kind-note"))
                            .child(kind_btn("Skill", CanvasPinKind::Skill, "canvas-pin-kind-skill")),
                    )
                    .child(
                        row()
                            .w_full()
                            .justify_end()
                            .gap(6.0)
                            .child(
                                Button::new("Cancel")
                                    .variant(ButtonVariant::Ghost)
                                    .control_size(ControlSize::Small)
                                    .on_press(|s: &mut AppState| s.canvas_pin_composer_open = false)
                                    .build(k)
                                    .key("canvas-pin-cancel"),
                            )
                            .child(
                                Button::new("Add Pin")
                                    .variant(ButtonVariant::Submit)
                                    .control_size(ControlSize::Small)
                                    .on_press(|s: &mut AppState| {
                                        let title = s.canvas_pin_title_edit.text.clone();
                                        let kind = s.canvas_pin_kind;
                                        s.canvas_add_pin(
                                            kind,
                                            &title,
                                            "Pinned spatial context for connected Dots.",
                                        );
                                    })
                                    .build(k)
                                    .key("canvas-pin-add"),
                            ),
                    ),
            );
        }
    }

    // 3. Selected-node quick action bar + inline voice bar.
    let mic = canvas_mic_active(state);
    let voice_btn = Button::new(if mic { "Listening…" } else { "Voice" })
        .variant(if mic { ButtonVariant::Submit } else { ButtonVariant::Ghost })
        .control_size(ControlSize::Small)
        .on_press(|s: &mut AppState| s.canvas_toggle_voice())
        .build(k)
        .key("canvas-voice-toggle");

    let action_bar = if let Some(sel) = state
        .selected_dot_id
        .as_deref()
        .and_then(|id| state.dots.iter().find(|d| d.id == id))
    {
        let ch_id_thread = sel.channel_id.clone();
        let dot_id_del = sel.id.clone();
        let popover_open = state.canvas_popover_dot.is_some();
        let mut bar = row()
            .key("canvas-selected-bar")
            .w_full()
            .padding(8.0)
            .gap(8.0)
            .radius(theme.radius_control)
            .fill(theme.card_fill)
            .border(1.0, theme.border_strong)
            .items_center()
            .child(
                text(format!("Selected: {} ({})", sel.label, sel.shape.label()))
                    .font(theme.mono_sm)
                    .color(theme.accent),
            );
        if !popover_open {
            bar = bar.child(
                el().grow(1.0).min_w(0.0).child(
                    TextInput::new(&state.canvas_prompt_edit)
                        .placeholder("Dispatch quick prompt to this character node…")
                        .label("Node prompt")
                        .width(None)
                        .on_edit(|s: &mut AppState| &mut s.canvas_prompt_edit)
                        .on_submit(|s: &mut AppState, _tx| s.canvas_submit_prompt_edit())
                        .build(k)
                        .key("canvas-bar-prompt"),
                ),
            );
        } else {
            bar = bar.child(el().grow(1.0).min_w(0.0));
        }
        bar.child(voice_btn)
            .child(
                Button::new("Open Thread")
                    .variant(ButtonVariant::Submit)
                    .control_size(ControlSize::Small)
                    .on_press(move |s: &mut AppState| {
                        s.active_channel_id = ch_id_thread.clone();
                        s.active_view = ActiveView::Thread;
                    })
                    .build(k)
                    .key("canvas-bar-thread"),
            )
            .child(
                Button::new("Remove")
                    .variant(ButtonVariant::Danger)
                    .control_size(ControlSize::Small)
                    .on_press(move |s: &mut AppState| {
                        s.dots.retain(|d| d.id != dot_id_del);
                        if s.selected_dot_id.as_deref() == Some(dot_id_del.as_str()) {
                            s.selected_dot_id = None;
                        }
                        s.canvas_close_popover();
                        s.canvas_sync_sessions();
                        s.pending_commands.push(UiCommand::PersistDots);
                    })
                    .build(k)
                    .key("canvas-bar-remove"),
            )
    } else {
        row()
            .key("canvas-selected-bar")
            .w_full()
            .padding(8.0)
            .gap(8.0)
            .radius(theme.radius_control)
            .fill(theme.card_fill)
            .border(1.0, theme.border)
            .items_center()
            .child(voice_btn)
            .child(
                text(if mic {
                    "Listening for hands-free prompts…"
                } else {
                    "Select a dot to prompt it, or enable voice input."
                })
                .key("canvas-voice-bar")
                .font(theme.mono_sm)
                .color(if mic { theme.semantic_live } else { theme.text_muted }),
            )
    };

    col()
        .key("canvas-view")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .h_full()
        .padding(14.0)
        .gap(10.0)
        .child(toolbar)
        .child(field)
        .child(action_bar)
}

pub fn render_add_dot_dialog(
    state: &AppState,
    theme: &VoxcellTheme,
    overlay_layer: &mut Vec<El>,
) {
    let k = &theme.kit;
    if !state.add_dot_modal_open {
        return;
    }

    let mut shape_grid = row().w_full().gap(6.0);
    for &shape in CanvasShapeKind::all() {
        let active = state.add_dot_shape == shape;
        shape_grid = shape_grid.child(
            Button::new(shape.label())
                .variant(if active {
                    ButtonVariant::Submit
                } else {
                    ButtonVariant::Neutral
                })
                .control_size(ControlSize::Mini)
                .on_press(move |s: &mut AppState| {
                    s.add_dot_shape = shape;
                    s.step_animations(0.016);
                })
                .build(k),
        );
    }

    let body = col()
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .child(
            row()
                .w_full()
                .gap(14.0)
                .items_center()
                .child(
                    el().w(96.0)
                        .h(96.0)
                        .radius(10.0)
                        .fill(theme.bg)
                        .border(1.0, theme.border_strong)
                        .child(
                            el().w(96.0)
                                .h(96.0)
                                .pixels_fit(&state.add_dot_preview_pixels, SurfaceFit::Fit),
                        ),
                )
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(8.0)
                        .child(
                            text("Character Node Name")
                                .font(theme.mono_sm)
                                .color(theme.text_muted),
                        )
                        .child(
                            TextInput::new(&state.add_dot_label_edit)
                                .placeholder("e.g. Research Kitsune")
                                .label("Character Name")
                                .width(None)
                                .on_edit(|s: &mut AppState| &mut s.add_dot_label_edit)
                                .build(k),
                        ),
                ),
        )
        .child(
            text("Choose 9-Shape Halftone Form")
                .font(theme.mono_sm)
                .color(theme.text_muted),
        )
        .child(shape_grid)
        .child(
            row()
                .w_full()
                .justify_end()
                .gap(8.0)
                .child(
                    Button::new("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| s.add_dot_modal_open = false)
                        .build(k),
                )
                .child(
                    Button::new("Create Node")
                        .variant(ButtonVariant::Submit)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| {
                            let label = s.add_dot_label_edit.text.clone();
                            let shape = s.add_dot_shape;
                            s.add_canvas_dot(&label, shape);
                        })
                        .build(k),
                ),
        );

    Dialog::new("add-dot", "Spawn Canvas Character Dot")
        .open(true)
        .description("Pin a draggable halftone character dot bound to its own Hermes session.")
        .width(620.0)
        .fit()
        .child(body)
        .on_close(|s: &mut AppState| s.add_dot_modal_open = false)
        .build(k, overlay_layer);
}

// ---------------------------------------------------------------------------
// GPU render node (spec §7.1 step 3) — the spike's Compositor/EffectHook chain.
// Gated behind `gpu-halftone` until theme-chrome adds wgpu/bytemuck to
// voxcell-ui/Cargo.toml; the pure emitter above is compiled + tested always.
// Enable: add the deps, declare the feature, attach in app/main.rs via
// `rune::App::compositor(canvas::gpu::CanvasDotCompositor::new())` and set
// `state.canvas_gpu_dots = true` (mirrors the home stage handoff).
// ---------------------------------------------------------------------------

#[cfg(feature = "gpu-halftone")]
pub mod gpu {
    use super::*;
    use rune::compose::{Compositor, DeviceKey, Frame};
    use std::collections::HashMap;
    use voxcell_core::{ExpressionMood, MouthViseme};
    use voxcell_halftone::gpu::dot_instances_as_bytes;
    use voxcell_halftone::render_canvas_dot_rgba;

    /// Uniform stride: wgpu min_uniform_buffer_offset_alignment is 256.
    const UNIFORM_STRIDE: u64 = 256;

    const CANVAS_DOTS_WGSL: &str = r#"
struct Uniforms {
    rect: vec4f,        // dot sprite rect in device px (x, y, w, h)
    origin_size: vec4f, // pass origin xy + target size zw (device px)
    color_time: vec4f,  // dot accent (linear rgb) + time
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
      @location(2) tier: u32,
      @location(3) alpha: f32) -> VsOut {
    let corner = vec2f(f32(vi & 1u), f32(vi >> 1u)) * 2.0 - 1.0;
    let center_px = u.rect.xy + uv * u.rect.zw;
    let r_px = max(radius * u.rect.w, 0.5);
    let extent = r_px * 1.6;
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

// slate-v1 tonal tiers; tier 4 carries the dot's own accent color.
fn tier_color(t: u32, accent: vec3f) -> vec3f {
    switch t {
        case 0u: { return srgb_to_linear(vec3f(0.392, 0.455, 0.545)); }
        case 1u: { return srgb_to_linear(vec3f(0.580, 0.639, 0.722)); }
        case 2u: { return srgb_to_linear(vec3f(0.796, 0.835, 0.882)); }
        case 3u: { return mix(srgb_to_linear(vec3f(0.886, 0.910, 0.941)), accent, 0.45); }
        default: { return accent; }
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
    let c = tier_color(in.tier, u.color_time.rgb);
    return vec4f(c * a, a);
}
"#;

    fn hex_to_linear(hex: &str, fallback: [f32; 3]) -> [f32; 3] {
        let h = hex.trim_start_matches('#');
        if h.len() != 6 {
            return fallback;
        }
        let Ok(v) = u32::from_str_radix(h, 16) else { return fallback; };
        [
            (((v >> 16) & 255) as f32 / 255.0).powf(2.2),
            (((v >> 8) & 255) as f32 / 255.0).powf(2.2),
            ((v & 255) as f32 / 255.0).powf(2.2),
        ]
    }

    /// Instanced canvas-dot compositor: one effect slot per dot sprite; each
    /// site draws that dot's halftone instances inside its own clip/layer.
    pub struct CanvasDotCompositor {
        device_key: Option<DeviceKey>,
        pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
        bind_layout: Option<wgpu::BindGroupLayout>,
        uniform_buf: Option<wgpu::Buffer>,
        instance_buf: Option<wgpu::Buffer>,
        uniform_sites: usize,
        instance_count: usize,
    }

    impl CanvasDotCompositor {
        pub fn new() -> Self {
            Self {
                device_key: None,
                pipelines: HashMap::new(),
                bind_layout: None,
                uniform_buf: None,
                instance_buf: None,
                uniform_sites: 0,
                instance_count: 0,
            }
        }

        fn build(&mut self, device: &wgpu::Device) {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("canvas-dots-bind"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZeroU64::new(48),
                    },
                    count: None,
                }],
            });
            self.bind_layout = Some(layout);
            self.uniform_buf = None;
            self.instance_buf = None;
            self.uniform_sites = 0;
            self.instance_count = 0;
            self.pipelines.clear();
        }

        fn ensure_buffers(&mut self, device: &wgpu::Device, sites: usize, instances: usize) {
            if self.uniform_sites < sites || self.uniform_buf.is_none() {
                self.uniform_sites = sites.max(1);
                self.uniform_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("canvas-dots-uniforms"),
                    size: self.uniform_sites as u64 * UNIFORM_STRIDE,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            if self.instance_count < instances || self.instance_buf.is_none() {
                self.instance_count = instances.max(1);
                self.instance_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("canvas-dots-instances"),
                    size: (self.instance_count * std::mem::size_of::<DotInstance>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
        }

        fn pipeline(
            &mut self,
            device: &wgpu::Device,
            format: wgpu::TextureFormat,
        ) -> &wgpu::RenderPipeline {
            let layout = self.bind_layout.as_ref().expect("bind layout");
            self.pipelines.entry(format).or_insert_with(|| {
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("canvas-dots-wgsl"),
                    source: wgpu::ShaderSource::Wgsl(CANVAS_DOTS_WGSL.into()),
                });
                let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("canvas-dots-pipeline-layout"),
                    bind_group_layouts: &[Some(layout)],
                    immediate_size: 0,
                });
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("canvas-dots-instanced"),
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
                                // tier u8 + 3 zero pad bytes read as one u32.
                                wgpu::VertexAttribute { format: wgpu::VertexFormat::Uint32, offset: 12, shader_location: 2 },
                                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 16, shader_location: 3 },
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

    impl Compositor<AppState> for CanvasDotCompositor {
        fn features(&self) -> wgpu::Features {
            wgpu::Features::empty()
        }

        fn render(&mut self, f: &mut Frame<'_, AppState>) {
            let key = f.device_key();
            if self.device_key != Some(key) {
                self.build(f.device);
                self.device_key = Some(key);
            }
            let state = &f.rt.state;
            let dot_profile = state.ui_config.dot_profile;
            let bloom = state.ui_config.bloom;
            // Per-dot batches in painter order (one effect slot per sprite).
            let mut batches: Vec<Vec<DotInstance>> = Vec::with_capacity(state.dots.len());
            let mut colors: Vec<[f32; 3]> = Vec::with_capacity(state.dots.len());
            for dot in &state.dots {
                let busy = state.canvas_dot_busy(&dot.channel_id);
                let rgba = render_canvas_dot_rgba(
                    CANVAS_DOT_PX, CANVAS_DOT_PX, CANVAS_DOT_COLS, CANVAS_DOT_ROWS,
                    dot.shape, state.time_s,
                    if busy { ExpressionMood::Thinking } else { ExpressionMood::Calm },
                    MouthViseme::Rest,
                    if busy { 0.72 } else { 0.2 },
                    false, false,
                    state.canvas_dragging_dot.as_deref() == Some(dot.id.as_str()),
                    Some(&dot.color_hex), dot_profile, bloom,
                );
                batches.push(emit_canvas_dot_instances(&rgba, CANVAS_DOT_PX, CANVAS_DOT_PX, CANVAS_DOT_COLS, CANVAS_DOT_ROWS));
                colors.push(hex_to_linear(&dot.color_hex, [0.204_f32.powf(2.2), 0.827_f32.powf(2.2), 0.600_f32.powf(2.2)]));
            }
            let total: usize = batches.iter().map(|b| b.len()).sum();
            self.ensure_buffers(f.device, batches.len(), total);
            let flat: Vec<DotInstance> = batches.iter().flatten().copied().collect();
            let instance_buf = self.instance_buf.as_ref().expect("instances").clone();
            f.queue.write_buffer(&instance_buf, 0, dot_instances_as_bytes(&flat));

            let clear = f.rt.clear_color();
            f.ui.prepare(f.device, f.queue, &mut f.rt.text.atlas, &f.rt.instances, f.size);

            let uniform_buf = self.uniform_buf.as_ref().expect("uniforms").clone();
            let bind_layout = self.bind_layout.as_ref().expect("layout").clone();
            let t = state.time_s;
            let mut byte_ranges: Vec<(u64, u64)> = Vec::with_capacity(batches.len());
            let mut off = 0_u64;
            for b in &batches {
                let len = (b.len() * std::mem::size_of::<DotInstance>()) as u64;
                byte_ranges.push((off, off + len));
                off += len;
            }
            let mut site_no = 0_usize;
            f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
                if site.mark != CANVAS_DOTS_MARK {
                    return;
                }
                let i = site_no;
                site_no += 1;
                let Some(batch) = batches.get(i) else { return; };
                if batch.is_empty() {
                    return;
                }
                let rect = site.instance.rect;
                let c = colors.get(i).copied().unwrap_or([0.0; 3]);
                let uniforms: [f32; 12] = [
                    rect[0], rect[1], rect[2], rect[3],
                    site.origin[0], site.origin[1], site.size[0] as f32, site.size[1] as f32,
                    c[0], c[1], c[2], t,
                ];
                f.queue.write_buffer(&uniform_buf, i as u64 * UNIFORM_STRIDE, bytemuck::cast_slice(&uniforms));
                let bind = f.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("canvas-dots-bind"),
                    layout: &bind_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &uniform_buf,
                            offset: 0,
                            size: std::num::NonZeroU64::new(48),
                        }),
                    }],
                });
                let pipeline = self.pipeline(f.device, site.format);
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &bind, &[(i as u64 * UNIFORM_STRIDE) as u32]);
                let (start, end) = byte_ranges[i];
                pass.set_vertex_buffer(0, instance_buf.slice(start..end));
                pass.draw(0..4, 0..batch.len() as u32);
            });
        }

        fn animating(&self) -> bool {
            true
        }
    }
}

// ---------------------------------------------------------------------------
// Headless tests (Runtime pattern from voxcell-app/src/main.rs).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_main_window;
    use rune::{Env, Key, KeyEvent, Mods, Runtime};
    use voxcell_core::{DotProfile, ExpressionMood, MouthViseme};
    use voxcell_halftone::render_canvas_dot_rgba;
    use voxcell_core::canvas::{
        CANVAS_BOUNDS, CANVAS_ZOOM_MAX, CANVAS_ZOOM_MIN, DEFAULT_CANVAS_VIEWPORT,
        arrange_sessions_formation, load_canvas_pins_from_json, load_canvas_sessions_from_json,
    };
    use voxcell_core::layout::compute_canvas_popover_alignment;
    use voxcell_halftone::gpu::dot_instances_as_bytes;

    fn canvas_rt() -> Runtime<AppState> {
        let mut s = AppState::new();
        s.apply_scene_preset("canvas");
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);
        rt
    }

    fn click_key(rt: &mut Runtime<AppState>, key: &str) {
        let e = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("element {key}"));
        let cx = e.target[0] + e.target[2] * 0.5;
        let cy = e.target[1] + e.target[3] * 0.5;
        rt.pointer_down(cx, cy);
        rt.pointer_up(cx, cy);
        rt.frame(0.016);
    }

    fn rect_of(rt: &Runtime<AppState>, key: &str) -> [f32; 4] {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("element {key}"))
            .target
    }

    // ---- pan/zoom state ----

    #[test]
    fn canvas_zoom_clamps_and_steps() {
        let mut rt = canvas_rt();
        assert_eq!(rt.state.canvas_viewport, DEFAULT_CANVAS_VIEWPORT);
        rt.state.canvas_zoom_step(1);
        assert!((rt.state.canvas_viewport.zoom - 1.15).abs() < 1e-3);
        for _ in 0..20 {
            rt.state.canvas_zoom_step(-1);
        }
        assert_eq!(rt.state.canvas_viewport.zoom, CANVAS_ZOOM_MIN);
        for _ in 0..30 {
            rt.state.canvas_zoom_step(1);
        }
        assert_eq!(rt.state.canvas_viewport.zoom, CANVAS_ZOOM_MAX);
        rt.state.canvas_reset_viewport();
        assert_eq!(rt.state.canvas_viewport, DEFAULT_CANVAS_VIEWPORT);
        assert!(!rt.state.canvas_viewport_active());
    }

    #[test]
    fn canvas_pan_clamps_to_ts_bounds() {
        let mut rt = canvas_rt();
        rt.state.canvas_pan_by(100_000.0, -100_000.0);
        let max_pan = 420.0 * rt.state.canvas_viewport.zoom;
        assert_eq!(rt.state.canvas_viewport.pan_x, max_pan);
        assert_eq!(rt.state.canvas_viewport.pan_y, -max_pan);
        assert!(rt.state.canvas_viewport_active());
        rt.state.canvas_reset_viewport();
        rt.state.canvas_pan_by(30.0, 20.0);
        assert_eq!(rt.state.canvas_viewport.pan_x, 30.0);
        assert_eq!(rt.state.canvas_viewport.pan_y, 20.0);
    }

    #[test]
    fn canvas_zoom_about_cursor_keeps_point_fixed() {
        let mut rt = canvas_rt();
        let (cx, cy) = (140.0_f32, 90.0_f32);
        let before = rt.state.canvas_screen_to_world(cx, cy);
        rt.state.canvas_zoom_at(1.3, cx, cy);
        let after = rt.state.canvas_screen_to_world(cx, cy);
        assert!((before.0 - after.0).abs() < 0.5, "world x drift: {before:?} -> {after:?}");
        assert!((before.1 - after.1).abs() < 0.5, "world y drift: {before:?} -> {after:?}");
    }

    #[test]
    fn canvas_zoom_buttons_and_reset_headless() {
        let mut rt = canvas_rt();
        click_key(&mut rt, "canvas-zoom-in");
        assert!(rt.state.canvas_viewport.zoom > 1.0);
        click_key(&mut rt, "canvas-zoom-out");
        assert!((rt.state.canvas_viewport.zoom - 1.0).abs() < 1e-3);
        click_key(&mut rt, "canvas-zoom-in");
        click_key(&mut rt, "canvas-zoom-reset");
        assert_eq!(rt.state.canvas_viewport, DEFAULT_CANVAS_VIEWPORT);
    }

    #[test]
    fn canvas_wheel_zooms_about_cursor() {
        let mut rt = canvas_rt();
        // Settle one layout frame so `on_layout` populates `canvas_field_rect`
        // before sampling: the boot frame lays out the tree, but the state rect
        // lands on the next frame — otherwise `before` uses the design-size
        // fallback while the handler zooms about the real stage.
        rt.frame(0.016);
        let field = rect_of(&rt, "canvas-field");
        let (px, py) = (field[0] + 220.0, field[1] + 160.0);
        let local = (220.0_f32, 160.0_f32);
        let before = rt.state.canvas_screen_to_world(local.0, local.1);
        // `rt.wheel` only scrolls viewports 1:1; `on_wheel` handlers hear
        // `rt.wheel_event` (rune gesture dispatch: handlers first, then scroll).
        rt.wheel_event(rune::WheelEvent::new(0.0, -10.0).at(px, py));
        rt.frame(0.016);
        assert!((rt.state.canvas_viewport.zoom - 1.08).abs() < 1e-3, "zoom: {}", rt.state.canvas_viewport.zoom);
        let after = rt.state.canvas_screen_to_world(local.0, local.1);
        assert!((before.0 - after.0).abs() < 0.5);
        assert!((before.1 - after.1).abs() < 0.5);
        rt.wheel_event(rune::WheelEvent::new(0.0, 10.0).at(px, py));
        rt.frame(0.016);
        assert!((rt.state.canvas_viewport.zoom - 1.0).abs() < 0.01);
    }

    #[test]
    fn canvas_background_drag_pans_headless() {
        let mut rt = canvas_rt();
        let bg = rect_of(&rt, "canvas-bg");
        let (x0, y0) = (bg[0] + 12.0, bg[1] + 12.0);
        rt.pointer_down(x0, y0);
        rt.pointer_move(x0 + 30.0, y0 + 8.0);
        rt.frame(1.0 / 120.0);
        rt.pointer_move(x0 + 64.0, y0 + 18.0);
        rt.frame(1.0 / 120.0);
        rt.pointer_up(x0 + 64.0, y0 + 18.0);
        rt.frame(0.016);
        assert!(rt.state.canvas_viewport.pan_x > 2.0, "pan: {:?}", rt.state.canvas_viewport);
        assert!(rt.state.canvas_viewport.pan_y > 2.0);
        assert!(rt.state.canvas_pan_drag.is_none());
    }

    // ---- layout switching (springs) ----

    #[test]
    fn canvas_formation_switch_springs_then_settles() {
        let mut rt = canvas_rt();
        let before: Vec<(f32, f32)> = rt.state.dots.iter().map(|d| (d.x, d.y)).collect();
        click_key(&mut rt, "canvas-form-orbit");
        assert_eq!(rt.state.canvas_formation, CanvasFormation::Orbit);
        assert!(rt.state.canvas_layout_anim.is_some());
        // Targets match the core TS-parity arrangement.
        let expected = arrange_sessions_formation(&rt.state.canvas_sessions, CanvasFormation::Orbit);
        for (dot, s) in rt.state.dots.iter().zip(expected.iter()) {
            assert!((dot.x - s.x_pct / 100.0 * CANVAS_DESIGN_W).abs() < 0.01);
            assert!((dot.y - s.y_pct / 100.0 * CANVAS_DESIGN_H).abs() < 0.01);
        }
        // Mid-flight: display position springs from the old spot.
        let mid = rt.state.canvas_layout_display_pos(0);
        assert!((mid.0 - before[0].0).abs() < 0.01 || !rt.state.canvas_layout_settled());
        assert!(!rt.state.canvas_layout_settled());
        // Advance past the spring + stagger settle window.
        rt.state.time_s += 3.0;
        rt.invalidate();
        rt.frame(0.016);
        assert!(rt.state.canvas_layout_settled());
        let settled = rt.state.canvas_layout_display_pos(0);
        assert_eq!(settled, (rt.state.dots[0].x, rt.state.dots[0].y));
        assert!(rt.state.pending_commands.iter().any(|c| matches!(c, UiCommand::PersistDots)));

        click_key(&mut rt, "canvas-form-grid");
        assert_eq!(rt.state.canvas_formation, CanvasFormation::Grid);
        let grid = arrange_sessions_formation(&rt.state.canvas_sessions, CanvasFormation::Grid);
        assert_eq!(grid.len(), 5);
        rt.state.time_s += 3.0;
        rt.invalidate();
        rt.frame(0.016);
        click_key(&mut rt, "canvas-form-constellation");
        assert_eq!(rt.state.canvas_formation, CanvasFormation::Constellation);
    }

    #[test]
    fn canvas_reduced_motion_skips_springs() {
        let mut rt = canvas_rt();
        rt.state.ui_config.reduced_motion = true;
        rt.state.canvas_set_formation(CanvasFormation::Grid);
        assert!(rt.state.canvas_layout_anim.is_none());
        assert!(rt.state.canvas_layout_settled());
    }

    // ---- popover alignment at the 24%/76% boundaries ----

    #[test]
    fn canvas_popover_alignment_boundaries() {
        assert_eq!(compute_canvas_popover_alignment(10.0), CanvasPopoverAlign::Left);
        assert_eq!(compute_canvas_popover_alignment(23.99), CanvasPopoverAlign::Left);
        assert_eq!(compute_canvas_popover_alignment(24.0), CanvasPopoverAlign::Center);
        assert_eq!(compute_canvas_popover_alignment(50.0), CanvasPopoverAlign::Center);
        assert_eq!(compute_canvas_popover_alignment(76.0), CanvasPopoverAlign::Center);
        assert_eq!(compute_canvas_popover_alignment(76.01), CanvasPopoverAlign::Right);
        assert_eq!(compute_canvas_popover_alignment(95.0), CanvasPopoverAlign::Right);
        assert_eq!(compute_canvas_popover_alignment(f64::NAN), CanvasPopoverAlign::Center);

        let mut rt = canvas_rt();
        let set_pct = |rt: &mut Runtime<AppState>, pct: f32| {
            rt.state.dots[0].x = pct / 100.0 * CANVAS_DESIGN_W;
            rt.invalidate();
            rt.frame(0.016);
        };
        set_pct(&mut rt, 20.0);
        assert_eq!(rt.state.canvas_popover_align("dot-0"), CanvasPopoverAlign::Left);
        set_pct(&mut rt, 24.0);
        assert_eq!(rt.state.canvas_popover_align("dot-0"), CanvasPopoverAlign::Center);
        set_pct(&mut rt, 76.0);
        assert_eq!(rt.state.canvas_popover_align("dot-0"), CanvasPopoverAlign::Center);
        set_pct(&mut rt, 80.0);
        assert_eq!(rt.state.canvas_popover_align("dot-0"), CanvasPopoverAlign::Right);
    }

    #[test]
    fn canvas_popover_clamps_inside_field_edges() {
        let mut rt = canvas_rt();
        let field = rect_of(&rt, "canvas-field");
        // Left-edge dot: popover extends rightward from the dot.
        rt.state.dots[0].x = CANVAS_BOUNDS.min_x / 100.0 * CANVAS_DESIGN_W;
        rt.state.dots[0].y = 46.0 / 100.0 * CANVAS_DESIGN_H;
        rt.state.canvas_open_popover("dot-0");
        rt.invalidate();
        rt.frame(0.016);
        let pop = rect_of(&rt, "canvas-popover");
        let node = rect_of(&rt, "canvas-node-dot-0");
        let node_cx = node[0] + node[2] * 0.5;
        assert!(pop[0] >= node_cx - 1.0, "left-aligned popover must extend right: pop={pop:?} node_cx={node_cx}");
        assert!(pop[0] >= field[0] && pop[0] + pop[2] <= field[0] + field[2] + 1.0);
        // Right-edge dot: popover extends leftward.
        rt.state.dots[0].x = CANVAS_BOUNDS.max_x / 100.0 * CANVAS_DESIGN_W;
        rt.invalidate();
        rt.frame(0.016);
        let pop = rect_of(&rt, "canvas-popover");
        let node = rect_of(&rt, "canvas-node-dot-0");
        let node_cx = node[0] + node[2] * 0.5;
        assert!(pop[0] + pop[2] <= node_cx + 1.0, "right-aligned popover must extend left: pop={pop:?} node_cx={node_cx}");
        assert!(pop[0] >= field[0] - 1.0);
        // Center dot: popover centered on the dot.
        rt.state.dots[0].x = 50.0 / 100.0 * CANVAS_DESIGN_W;
        rt.invalidate();
        rt.frame(0.016);
        let pop = rect_of(&rt, "canvas-popover");
        let node = rect_of(&rt, "canvas-node-dot-0");
        let node_cx = node[0] + node[2] * 0.5;
        assert!((pop[0] + pop[2] * 0.5 - node_cx).abs() < 40.0);
        // Close button works (settle layout flights first: the popover moved
        // between frames, so hit rects lag the final `.target` rects).
        rt.settle_all();
        rt.frame(0.016);
        click_key(&mut rt, "canvas-popover-close");
        assert!(rt.state.canvas_popover_dot.is_none());
    }

    // ---- pin persistence roundtrip (duet.canvasSessions.v2 / canvasPins.v1) ----

    #[test]
    fn canvas_pin_and_session_persistence_roundtrip() {
        let mut rt = canvas_rt();
        // Legacy scene dots can sit outside the TS bounds; the v2 vault clamps
        // on normalize (web parity). Arrange into bounds before the roundtrip.
        rt.state.canvas_set_formation(CanvasFormation::Constellation);
        let pin_id = rt.state.canvas_add_pin(
            CanvasPinKind::Repo,
            "Workspace Brief",
            "Isolated sandboxes share portable skills.",
        );
        assert_eq!(rt.state.canvas_pins.len(), 1);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(c, UiCommand::PersistDots)));
        let pins_json = rt.state.canvas_pins_json();
        let sessions_json = rt.state.canvas_sessions_json();
        assert!(pins_json.contains("Workspace Brief"));
        assert!(sessions_json.contains("Hermes Core"));

        // Fresh state hydrates both vaults through core normalize/prune.
        let mut fresh = AppState::new();
        let n_pins = fresh.canvas_hydrate_pins_json(&pins_json);
        let n_sess = fresh.canvas_hydrate_sessions_json(&sessions_json);
        assert_eq!(n_pins, 1);
        assert_eq!(n_sess, 5);
        assert_eq!(fresh.canvas_pins, load_canvas_pins_from_json(&pins_json));
        assert_eq!(fresh.canvas_pins[0].id, pin_id);
        assert_eq!(fresh.canvas_pins[0].kind, CanvasPinKind::Repo);
        assert_eq!(fresh.dots.len(), 5);
        for (dot, orig) in fresh.dots.iter().zip(rt.state.dots.iter()) {
            assert_eq!(dot.channel_id, orig.channel_id);
            assert_eq!(dot.label, orig.label);
            assert!((dot.x - orig.x).abs() < 0.5, "x roundtrip: {} vs {}", dot.x, orig.x);
            assert!((dot.y - orig.y).abs() < 0.5, "y roundtrip: {} vs {}", dot.y, orig.y);
        }
        // Re-saving the hydrated vault is idempotent.
        assert_eq!(fresh.canvas_pins_json(), pins_json);
        let resaved = fresh.canvas_sessions_json();
        assert_eq!(
            load_canvas_sessions_from_json(&resaved).len(),
            load_canvas_sessions_from_json(&sessions_json).len()
        );
        // Removal persists too.
        fresh.canvas_remove_pin(&pin_id);
        assert!(fresh.canvas_pins.is_empty());
    }

    #[test]
    fn canvas_pin_composer_headless() {
        let mut rt = canvas_rt();
        click_key(&mut rt, "canvas-pin-toggle");
        assert!(rt.state.canvas_pin_composer_open);
        assert!(rt.find("canvas-pin-composer").is_some());
        click_key(&mut rt, "canvas-pin-kind-repo");
        assert_eq!(rt.state.canvas_pin_kind, CanvasPinKind::Repo);
        rt.state.canvas_pin_title_edit.replace_text("Release Checklist");
        rt.invalidate();
        rt.frame(0.016);
        click_key(&mut rt, "canvas-pin-add");
        assert_eq!(rt.state.canvas_pins.len(), 1);
        assert_eq!(rt.state.canvas_pins[0].title, "Release Checklist");
        assert!(!rt.state.canvas_pin_composer_open);
        assert!(rt.find("canvas-pin-composer").is_none());
    }

    // ---- dot prompt dispatch ----

    #[test]
    fn canvas_dot_prompt_dispatch_pure() {
        let mut rt = canvas_rt();
        let ch = rt.state.dots[2].channel_id.clone();
        rt.state.canvas_open_popover("dot-2");
        rt.state.canvas_prompt_edit.replace_text("audit the shader pipeline");
        rt.state.canvas_submit_prompt_edit();
        assert_eq!(rt.state.active_channel_id, ch);
        assert_eq!(rt.state.active_view, ActiveView::Canvas);
        assert!(rt.state.canvas_prompt_edit.text.is_empty());
        let last_user = rt
            .state
            .entries
            .iter()
            .rev()
            .find(|e| e.role == EntryRole::User)
            .expect("user entry");
        assert_eq!(last_user.text, "audit the shader pipeline");
        assert_eq!(last_user.channel_id, ch);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::SendPrompt { channel_id, prompt, .. }
                if channel_id == &ch && prompt == "audit the shader pipeline"
        )));
        // Empty drafts are ignored.
        let n = rt.state.entries.len();
        rt.state.canvas_submit_prompt_edit();
        assert_eq!(rt.state.entries.len(), n);
    }

    #[test]
    fn canvas_dot_prompt_typing_headless() {
        let mut rt = canvas_rt();
        let ch = rt.state.dots[1].channel_id.clone();
        rt.state.canvas_open_popover("dot-1");
        rt.invalidate();
        rt.frame(0.016);
        let input = rect_of(&rt, "canvas-popover-prompt");
        let cx = input[0] + input[2] * 0.5;
        let cy = input[1] + input[3] * 0.5;
        rt.pointer_down(cx, cy);
        rt.pointer_up(cx, cy);
        rt.frame(0.016);
        for c in "hi".chars() {
            rt.key(KeyEvent { key: Key::Text(c.to_string()), mods: Mods::default() });
            rt.frame(0.016);
        }
        rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
        rt.frame(0.016);
        assert!(rt.state.canvas_prompt_edit.text.is_empty(), "draft: {:?}", rt.state.canvas_prompt_edit.text);
        assert!(rt.state.entries.iter().any(|e| e.role == EntryRole::User && e.text == "hi" && e.channel_id == ch));
    }

    // ---- voice bar ----

    #[test]
    fn canvas_voice_toggle_dispatches_toggle_mic() {
        let mut rt = canvas_rt();
        click_key(&mut rt, "canvas-voice-toggle");
        assert!(rt.state.voice.hands_free_vad);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(c, UiCommand::ToggleMic(true))));
        click_key(&mut rt, "canvas-voice-toggle");
        assert!(!rt.state.voice.hands_free_vad);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(c, UiCommand::ToggleMic(false))));
    }

    // ---- drag clamping ----

    #[test]
    fn canvas_dot_drag_clamps_to_percentage_bounds() {
        let mut rt = canvas_rt();
        rt.state.canvas_begin_dot_drag("dot-0");
        rt.state.canvas_move_dot_drag("dot-0", 1_000_000.0, 1_000_000.0);
        let d = &rt.state.dots[0];
        assert!((d.x / CANVAS_DESIGN_W * 100.0 - CANVAS_BOUNDS.max_x).abs() < 0.01);
        assert!((d.y / CANVAS_DESIGN_H * 100.0 - CANVAS_BOUNDS.max_y).abs() < 0.01);
        rt.state.canvas_move_dot_drag("dot-0", -2_000_000.0, -2_000_000.0);
        let d = &rt.state.dots[0];
        assert!((d.x / CANVAS_DESIGN_W * 100.0 - CANVAS_BOUNDS.min_x).abs() < 0.01);
        assert!((d.y / CANVAS_DESIGN_H * 100.0 - CANVAS_BOUNDS.min_y).abs() < 0.01);
        rt.state.canvas_end_dot_drag();
        assert!(rt.state.canvas_dragging_dot.is_none());
        assert!(rt.state.pending_commands.iter().any(|c| matches!(c, UiCommand::PersistDots)));
        assert_eq!(rt.state.canvas_sessions.len(), 5);
    }


    // ---- GPU emitter + headless fallback ----

    #[test]
    fn canvas_gpu_emitter_matches_spike_abi() {
        let rgba = render_canvas_dot_rgba(
            CANVAS_DOT_PX, CANVAS_DOT_PX, CANVAS_DOT_COLS, CANVAS_DOT_ROWS,
            CanvasShapeKind::HermesOrb, 0.35, ExpressionMood::Calm, MouthViseme::Rest,
            0.2, false, false, false, Some("#34D399"), DotProfile::Balanced, 0.3,
        );
        let inst = emit_canvas_dot_instances(&rgba, CANVAS_DOT_PX, CANVAS_DOT_PX, CANVAS_DOT_COLS, CANVAS_DOT_ROWS);
        assert_eq!(inst.len(), CANVAS_DOT_COLS * CANVAS_DOT_ROWS);
        assert!(inst.iter().any(|d| d.tier >= 3), "sprite must produce highlight tiers");
        for d in &inst {
            assert!(d.pos_xy[0] > 0.0 && d.pos_xy[0] < 1.0);
            assert!(d.pos_xy[1] > 0.0 && d.pos_xy[1] < 1.0);
            assert!(d.radius > 0.0 && d.radius < 0.5);
            assert!(d.alpha >= 0.0 && d.alpha <= 1.0);
            assert!(d.tier <= 4);
            assert_eq!(d._pad, [0; 3]);
        }
        let bytes = dot_instances_as_bytes(&inst);
        assert_eq!(bytes.len(), inst.len() * 20);
        // Degenerate inputs are safe.
        assert!(emit_canvas_dot_instances(&[], 96, 96, 22, 11).is_empty());
        assert!(emit_canvas_dot_instances(&rgba, 96, 96, 0, 11).is_empty());
    }

    #[test]
    fn canvas_headless_fallback_renders_pixels_without_gpu() {
        let mut rt = canvas_rt();
        assert!(!canvas_gpu_dots_active(&rt.state));
        assert!(rt.find("canvas-node-dot-0").is_some());
        assert!(rt.find("canvas-node-dot-4").is_some());
        assert!(rt.state.dot_pixels.contains_key("dot-0"));
        // With the opt-in flag set, the compiled feature decides (home.rs
        // `gpu_fallback_renders_pixels_surface_headless` parity): feature on ->
        // GPU effect slots; feature off -> still the headless-safe CPU fallback.
        rt.update(|s, _tx| s.canvas_gpu_dots = true);
        rt.invalidate();
        rt.frame(0.016);
        if cfg!(feature = "gpu-halftone") {
            assert!(canvas_gpu_dots_active(&rt.state));
        } else {
            assert!(!canvas_gpu_dots_active(&rt.state));
        }
        assert!(rt.find("canvas-node-dot-0").is_some());
        // Forcing the opt-in flag back off always restores the CPU fallback,
        // even with the feature compiled in — no adapter is ever required.
        rt.update(|s, _tx| s.canvas_gpu_dots = false);
        rt.invalidate();
        rt.frame(0.016);
        assert!(!canvas_gpu_dots_active(&rt.state));
        assert!(rt.find("canvas-node-dot-0").is_some());
        assert!(rt.state.dot_pixels.contains_key("dot-0"));
    }

    #[test]
    fn canvas_shape_catalog_maps_nine_engines_both_ways() {
        for &shape in CanvasShapeKind::all() {
            let braille = AppState::canvas_shape_to_braille(shape);
            assert_eq!(AppState::canvas_braille_to_shape(braille), shape);
        }
        assert_eq!(CanvasShapeKind::all().len(), 9);
    }
}
