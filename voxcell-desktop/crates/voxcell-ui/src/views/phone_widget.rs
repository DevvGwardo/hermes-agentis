//! P4-02 — Floating Phone Video Widget ("Cyber Comms" HUD).
//!
//! Renders the 320×420 call widget: hero halftone portrait, elapsed call timer,
//! live spectrum and Mute / PTT / Hangup controls wired through `UiCommand`.
//! The same view backs three hosts (see `app/bridge.rs`):
//!   1. the embedded floating dock in the main window (`views/mod.rs`),
//!   2. a true detached always-on-top frameless translucent window
//!      (`rune::open_window` + `WindowOptions` per `os_integration_probe.rs`),
//!   3. the `--scene phone` companion subprocess fallback.
//! All chrome comes from `VoxcellTheme`; emerald/amber/rose appear only as
//! semantic live/listening/error telemetry.

use crate::state::{AppState, UiCommand};
use crate::theme::VoxcellTheme;
use rune::{Cx, El, SurfaceFit, TextStyle, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

/// View entry for the detached phone window / headless tests: builds the theme
/// from state and delegates to [`render_phone_widget`].
pub fn render_phone_window(state: &AppState, cx: &mut Cx<AppState>) -> El {
    let theme = VoxcellTheme::from_config(&state.ui_config);
    render_phone_widget(state, &theme, cx)
}

pub fn render_phone_widget(state: &AppState, theme: &VoxcellTheme, _cx: &mut Cx<AppState>) -> El {
    let k = &theme.kit;
    let is_speaking = state.audio.is_speaking || state.stage_mood == voxcell_core::ExpressionMood::Speaking;
    let mic_on = state.voice.hands_free_vad || state.audio.is_listening;
    let call_live = state.phone_widget_open;

    // Live spectrum: real FFT bands while speaking/listening, idle shimmer otherwise.
    let mut spectrum = row().key("phone-spectrum").w_full().h(32.0).gap(4.0).items_end().justify_center();
    for (idx, &band) in state.audio.bands.iter().enumerate() {
        let lvl = if is_speaking || mic_on {
            band.clamp(0.08, 1.0)
        } else {
            0.08 + ((state.time_s * 2.0 + idx as f32 * 0.45).sin().abs() * 0.14)
        };
        spectrum = spectrum.child(
            el().w(14.0)
                .h(4.0 + lvl * 26.0)
                .radius(2.0)
                .fill(if is_speaking {
                    theme.accent
                } else if mic_on {
                    theme.amber
                } else {
                    theme.accent.alpha(0.45)
                }),
        );
    }

    // Header doubles as the drag region for the frameless detached window.
    let header = row()
        .key("phone-header")
        .w_full()
        .window_drag()
        .justify_between()
        .items_center()
        .child(
            row()
                .gap(6.0)
                .items_center()
                .child(
                    el().key("phone-status-dot")
                        .w(8.0)
                        .h(8.0)
                        .radius(4.0)
                        .fill(if is_speaking {
                            theme.accent
                        } else if mic_on {
                            theme.amber
                        } else {
                            theme.text_faint
                        }),
                )
                .child(
                    text("CYBER COMMS // HUD")
                        .font(TextStyle::new(11.0, 700.0).tracking(0.8).mono())
                        .color(theme.text),
                ),
        )
        .child(
            Button::new("Close")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Mini)
                .on_press(|s: &mut AppState| {
                    s.phone_widget_open = false;
                    s.phone_call_started_ms = None;
                    s.pending_commands.push(UiCommand::TogglePhoneWindow(false));
                })
                .build(k),
        );

    // Elapsed call timer + call state line.
    let timer_row = row()
        .key("phone-timer-row")
        .w_full()
        .justify_between()
        .items_center()
        .child(
            text(if call_live { "LIVE CALL" } else { "STANDBY" })
                .font(TextStyle::new(10.0, 600.0).tracking(0.6).mono())
                .color(if call_live { theme.accent } else { theme.text_faint }),
        )
        .child(
            text(state.phone_call_timer_label())
                .key("phone-call-timer")
                .font(TextStyle::new(13.0, 700.0).mono())
                .color(theme.text),
        );

    col()
        .key("phone-widget-window")
        .w_full()
        .h_full()
        .padding(12.0)
        .gap(10.0)
        .radius(14.0)
        // Translucent card: the detached window itself is transparent + borderless
        // (WindowOptions in bridge.rs), so the fill must carry alpha.
        .fill(theme.bg.alpha(0.88))
        .border(1.0, theme.border_strong)
        .child(header)
        .child(timer_row)
        .child(
            el().key("phone-portrait-box")
                // P5 uphill-a11y: name the call portrait for AccessKit.
                .label(format!(
                    "Call portrait — {:?} mood{}",
                    state.stage_mood,
                    if state.audio.is_speaking { ", speaking" } else { "" }
                ))
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .radius(12.0)
                .fill(theme.surface.alpha(0.9))
                .border(1.0, theme.border)
                .center()
                .on_press(|s: &mut AppState| s.toggle_stage_mood())
                .child(
                    el().w(240.0)
                        .h(210.0)
                        .pixels_fit(&state.phone_pixels, SurfaceFit::Fit),
                ),
        )
        .child(spectrum)
        .child(
            row()
                .key("phone-controls")
                .w_full()
                .justify_between()
                .gap(6.0)
                .child(
                    Button::new(if mic_on { "PTT Active" } else { "Talk (PTT)" })
                        .variant(if mic_on {
                            ButtonVariant::Submit
                        } else {
                            ButtonVariant::Neutral
                        })
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| {
                            let next = !s.voice.hands_free_vad;
                            s.voice.hands_free_vad = next;
                            s.pending_commands.push(UiCommand::ToggleMic(next));
                        })
                        .build(k),
                )
                .child(
                    Button::new(if state.voice.muted { "Unmute" } else { "Mute TTS" })
                        .variant(if state.voice.muted {
                            ButtonVariant::Warning
                        } else {
                            ButtonVariant::Neutral
                        })
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| {
                            let next = !s.voice.muted;
                            s.voice.muted = next;
                            s.pending_commands.push(UiCommand::ToggleMute(next));
                        })
                        .build(k),
                )
                .child(
                    Button::new("Hangup")
                        .variant(ButtonVariant::Danger)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| {
                            s.phone_widget_open = false;
                            s.phone_call_started_ms = None;
                            s.voice.hands_free_vad = false;
                            s.pending_commands.push(UiCommand::PhoneHangup);
                            s.pending_commands.push(UiCommand::TogglePhoneWindow(false));
                        })
                        .build(k),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    fn phone_state() -> AppState {
        let mut s = AppState::new();
        s.apply_scene_preset("phone");
        s.now_ms = 100_000;
        s.phone_call_started_ms = Some(35_000); // 65s elapsed -> 01:05
        s
    }

    fn click_text(rt: &mut Runtime<AppState>, label: &str) {
        let el = rt
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("button {label:?} present"));
        let cx = el.target[0] + el.target[2] * 0.5;
        let cy = el.target[1] + el.target[3] * 0.5;
        rt.pointer_down(cx, cy);
        rt.pointer_up(cx, cy);
        rt.frame(0.016);
    }

    #[test]
    fn phone_scene_renders_hero_timer_and_controls_at_320x420() {
        let mut rt = Runtime::new(phone_state(), render_phone_window, (320.0, 420.0), Env::new(true, 1.0));
        rt.frame(0.0);

        assert!(rt.find("phone-widget-window").is_some());
        assert!(rt.find("phone-portrait-box").is_some());
        assert!(rt.find("phone-spectrum").is_some());
        assert!(rt.find("phone-controls").is_some());

        let timer = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("phone-call-timer"))
            .expect("call timer element");
        assert_eq!(timer.text.as_deref(), Some("01:05"));

        for label in ["Talk (PTT)", "Mute TTS", "Hangup", "Close"] {
            assert!(
                rt.elements().into_iter().any(|e| e.text.as_deref() == Some(label)),
                "control {label:?} rendered"
            );
        }
        // Hero portrait pixels were produced by the phone scene preset.
        assert!(rt.state.phone_pixels.size().0 > 0);
    }

    #[test]
    fn phone_mute_ptt_and_hangup_dispatch_ui_commands() {
        let mut rt = Runtime::new(phone_state(), render_phone_window, (320.0, 420.0), Env::new(true, 1.0));
        rt.frame(0.0);

        // Mute toggles TTS and dispatches ToggleMute(true).
        click_text(&mut rt, "Mute TTS");
        assert!(rt.state.voice.muted);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(c, UiCommand::ToggleMute(true))));
        assert!(rt.elements().into_iter().any(|e| e.text.as_deref() == Some("Unmute")));

        // PTT toggles the mic and dispatches ToggleMic(true).
        click_text(&mut rt, "Talk (PTT)");
        assert!(rt.state.voice.hands_free_vad);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(c, UiCommand::ToggleMic(true))));

        // Hangup closes the call, clears the timer and dispatches PhoneHangup + close.
        rt.state.pending_commands.clear();
        click_text(&mut rt, "Hangup");
        assert!(!rt.state.phone_widget_open);
        assert_eq!(rt.state.phone_call_started_ms, None);
        assert!(!rt.state.voice.hands_free_vad);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(c, UiCommand::PhoneHangup)));
        assert!(rt.state.pending_commands.iter().any(|c| matches!(c, UiCommand::TogglePhoneWindow(false))));
    }

    #[test]
    fn call_timer_formats_and_syncs_with_widget_open_state() {
        let mut s = AppState::new();
        s.now_ms = 10_000;
        assert_eq!(s.phone_call_timer_label(), "00:00");
        assert_eq!(s.phone_call_elapsed_ms(), 0);

        // Opening anchors the timer at now_ms; idempotent while open.
        s.phone_widget_open = true;
        s.sync_phone_call_timer();
        assert_eq!(s.phone_call_started_ms, Some(10_000));
        s.now_ms = 75_000;
        s.sync_phone_call_timer();
        assert_eq!(s.phone_call_started_ms, Some(10_000));
        assert_eq!(s.phone_call_timer_label(), "01:05");

        // Long calls accumulate minutes past 59.
        s.now_ms = 10_000 + 3_600_000;
        assert_eq!(s.phone_call_timer_label(), "60:00");

        // Closing clears the anchor; reopening starts a fresh call.
        s.phone_widget_open = false;
        s.sync_phone_call_timer();
        assert_eq!(s.phone_call_started_ms, None);
        assert_eq!(s.phone_call_timer_label(), "00:00");
        s.phone_widget_open = true;
        s.sync_phone_call_timer();
        assert_eq!(s.phone_call_started_ms, Some(s.now_ms));
    }

    // ── P5 uphill-audit fix: call portrait accessible name ────

    /// Walk the AccessKit JSON like voxcell-uphill's `audit_a11y`:
    /// collect (role, name, key) for every node that carries a role.
    fn a11y_nodes(tree: &serde_json::Value, out: &mut Vec<(String, String, String)>) {
        match tree {
            serde_json::Value::Object(map) => {
                if let Some(role) = map.get("role").and_then(|r| r.as_str()) {
                    out.push((
                        role.to_string(),
                        map.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                        map.get("key").and_then(|k| k.as_str()).unwrap_or("").to_string(),
                    ));
                }
                for (k, child) in map {
                    if k != "role" && k != "name" && k != "key" {
                        a11y_nodes(child, out);
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    a11y_nodes(child, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn phone_portrait_box_is_named_in_accesskit_tree() {
        let mut rt = Runtime::new(phone_state(), render_phone_window, (320.0, 420.0), Env::new(true, 1.0));
        rt.frame(0.0);
        let bx = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("phone-portrait-box"))
            .expect("portrait box");
        let label = bx.label.as_deref().expect("box carries a label");
        assert!(label.contains("Call portrait"), "label: {label}");
        assert!(label.contains("mood"), "label: {label}");

        // Audit method: the name must surface in accessibility_json.
        let mut nodes = Vec::new();
        a11y_nodes(&rt.accessibility_json(), &mut nodes);
        let node = nodes
            .iter()
            .find(|(_, _, k)| k == "phone-portrait-box")
            .expect("box present in AccessKit tree");
        assert_eq!(node.1.trim(), label);
    }
}
