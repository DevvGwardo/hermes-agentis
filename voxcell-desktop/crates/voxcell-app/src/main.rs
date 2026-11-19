mod bridge;

use rune::{AppSpec, Cx, El, Key, KeyEvent, Tx};
use voxcell_core::ActiveView;
use voxcell_ui::{AppState, VoxcellTheme, render_main_window, render_phone_widget};

fn init_state() -> AppState {
    let mut s = AppState::new();
    // P3-03 handoff: GPU compositor is attached in main() below.
    s.home_gpu_halftone = true;
    s
}

fn render_root(state: &AppState, cx: &mut Cx<AppState>) -> El {
    // If `--scene phone` was requested, render the dedicated 320x420 Cyber Comms widget window
    // directly when no workspace chrome is needed, or alongside main window if requested.
    if state.phone_widget_open
        && state.active_view == ActiveView::Home
        && std::env::args().any(|a| a == "phone")
    {
        let theme = VoxcellTheme::from_config(&state.ui_config);
        return render_phone_widget(state, &theme, cx);
    }
    render_main_window(state, cx)
}

fn setup_app(state: &mut AppState, waker: rune::Waker) {
    if !rune::is_headless() {
        bridge::init_bridge(state, waker);
    }
}

fn poll_app(state: &mut AppState, t_seconds: f64, tx: &mut Tx) -> bool {
    let target_t = t_seconds as f32;
    let dt = if target_t > state.time_s {
        (target_t - state.time_s).clamp(0.008, 0.05)
    } else {
        0.016
    };

    if !rune::is_headless() {
        let _ = bridge::poll_bridge(state, dt);
    }

    // Drive 60Hz character portrait, horizon FFT, and active task pulse
    state.step_animations(dt);
    tx.wake_after(std::time::Duration::from_millis(33));
    true
}

/// The chord character of a key event, if any: single-char `Key::Text` or the
/// native `Key::Command` form (⌘B arrives as `Command('b')` on macOS).
fn shortcut_char(ev: &KeyEvent) -> Option<char> {
    match &ev.key {
        Key::Text(t) => {
            let mut chars = t.chars();
            let c = chars.next()?;
            if chars.next().is_none() {
                Some(c.to_ascii_lowercase())
            } else {
                None
            }
        }
        Key::Command(c) => Some(c.to_ascii_lowercase()),
        _ => None,
    }
}

/// P3-01 keyboard shortcuts: Ctrl/Cmd+1..5 switch views, B toggles the
/// sidebar, ',' toggles settings, P toggles the phone widget, Shift+Space
/// toggles hands-free VAD. Pure over state so headless tests can drive it
/// without a `Tx`. Returns true when the event was consumed.
fn handle_shortcut(state: &mut AppState, ev: &KeyEvent) -> bool {
    // `cmd` is the platform primary (⌘ on macOS, Ctrl elsewhere, reported
    // alongside `ctrl`); accept explicit Ctrl too for cross-platform parity.
    if ev.mods.cmd || ev.mods.ctrl {
        if let Some(c) = shortcut_char(ev) {
            match c {
                '1' => {
                    state.active_view = ActiveView::Home;
                    return true;
                }
                '2' => {
                    state.active_view = ActiveView::Canvas;
                    return true;
                }
                '3' => {
                    state.active_view = ActiveView::Thread;
                    return true;
                }
                '4' => {
                    state.active_view = ActiveView::Queue;
                    return true;
                }
                '5' => {
                    state.active_view = ActiveView::Cli;
                    return true;
                }
                'b' => {
                    state.ui_config.sidebar_collapsed = !state.ui_config.sidebar_collapsed;
                    return true;
                }
                'p' => {
                    let next = !state.phone_widget_open;
                    state.phone_widget_open = next;
                    state.pending_commands.push(voxcell_ui::UiCommand::TogglePhoneWindow(next));
                    return true;
                }
                ',' => {
                    state.settings_open = !state.settings_open;
                    return true;
                }
                ' ' if ev.mods.shift => {
                    let next = !state.voice.hands_free_vad;
                    state.voice.hands_free_vad = next;
                    state.pending_commands.push(voxcell_ui::UiCommand::ToggleMic(next));
                    return true;
                }
                _ => {}
            }
        }
    }
    if ev.key == Key::Escape {
        if state.settings_open {
            state.settings_open = false;
            return true;
        }
        if state.add_dot_modal_open {
            state.add_dot_modal_open = false;
            return true;
        }
    }
    false
}

fn on_key(state: &mut AppState, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    handle_shortcut(state, ev)
}

fn scene_idle(s: &mut AppState) {
    s.apply_scene_preset("idle");
}
fn scene_working(s: &mut AppState) {
    s.apply_scene_preset("working");
}
fn scene_attention(s: &mut AppState) {
    s.apply_scene_preset("attention");
}
fn scene_speaking(s: &mut AppState) {
    s.apply_scene_preset("speaking");
}
fn scene_canvas(s: &mut AppState) {
    s.apply_scene_preset("canvas");
}
fn scene_thread(s: &mut AppState) {
    s.apply_scene_preset("thread");
}
fn scene_queue(s: &mut AppState) {
    s.apply_scene_preset("queue");
}
fn scene_cli(s: &mut AppState) {
    s.apply_scene_preset("cli");
}
fn scene_settings(s: &mut AppState) {
    s.apply_scene_preset("settings");
}
fn scene_phone(s: &mut AppState) {
    s.apply_scene_preset("phone");
}

fn main() {
    rune::set_app_id("dev.voxcell.desktop");

    let is_phone_scene = std::env::args().any(|a| a == "phone");
    let win_size = if is_phone_scene {
        (320.0, 420.0)
    } else {
        (1280.0, 800.0)
    };

    let spec = AppSpec {
        min_size: Some((720.0, 520.0)),
        scenes: &[
            ("idle", scene_idle),
            ("working", scene_working),
            ("attention", scene_attention),
            ("speaking", scene_speaking),
            ("canvas", scene_canvas),
            ("thread", scene_thread),
            ("queue", scene_queue),
            ("cli", scene_cli),
            ("settings", scene_settings),
            ("phone", scene_phone),
        ],
        setup: Some(setup_app),
        poll: Some(poll_app),
        on_key: Some(on_key),
        ..AppSpec::new("Hermes Duet // Voxcell", win_size, init_state, render_root)
    };

    // P4-03 (phone-tray handoff): register the global PTT hotkey at startup.
    // rune reports/retains registration errors (headless registers virtually);
    // the bridge classifies failures into a StatusNote and the focused-window
    // Cmd/Ctrl+Shift+Space fallback in `on_key` keeps working regardless.
    rune::App::new(spec)
        .compositor(voxcell_ui::views::home::HomeHalftoneCompositor::new())
        .global_shortcut(bridge::ptt_accelerator(), bridge::ptt_shortcut_handler)
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};
    use voxcell_core::{ActiveView, ExpressionMood};

    #[test]
    fn stage_tap_cycles_mood_and_triggers_tap_impulse() {
        let mut s = AppState::new();
        s.apply_scene_preset("idle");
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);

        let before_mood = rt.state.stage_mood;
        let (px, py) = rt.find("home-portrait-stage").expect("portrait stage element");
        rt.pointer_down(px, py);
        rt.pointer_up(px, py);
        rt.frame(0.016);

        assert_ne!(rt.state.stage_mood, before_mood);
        assert!(rt.state.tap_impulse > 0.5);
    }

    #[test]
    fn navigation_tabs_and_cli_slash_commands_work_end_to_end() {
        let mut s = AppState::new();
        s.apply_scene_preset("idle");
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);

        // Click CLI tab in header
        let cli_tab = rt
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref() == Some("CLI"))
            .expect("CLI tab");
        let cx = cli_tab.target[0] + cli_tab.target[2] * 0.5;
        let cy = cli_tab.target[1] + cli_tab.target[3] * 0.5;
        rt.pointer_down(cx, cy);
        rt.pointer_up(cx, cy);
        rt.frame(0.016);
        assert_eq!(rt.state.active_view, ActiveView::Cli);

        // Execute slash commands and confirm UI customization + /help output
        rt.state.execute_cli_line("/help");
        rt.state.execute_cli_line("/ui cyan crisp");
        rt.frame(0.016);

        assert_eq!(rt.state.ui_config.accent_hex, "#22D3EE");
        assert!(rt.state.cli_log.iter().any(|l| l.contains("Hermes Duet CLI Commands")));
    }

    #[test]
    fn sidebar_working_and_attention_approval_resolution_updates_state() {
        let mut s = AppState::new();
        s.apply_scene_preset("attention");
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);

        assert_eq!(rt.state.pending_approvals().len(), 1);
        assert!(rt.find("sidebar-attention-card").is_some());

        let approve_btn = rt
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref() == Some("Approve"))
            .expect("Approve button");
        let bx = approve_btn.target[0] + approve_btn.target[2] * 0.5;
        let by = approve_btn.target[1] + approve_btn.target[3] * 0.5;
        rt.pointer_down(bx, by);
        rt.pointer_up(bx, by);
        rt.frame(0.016);

        assert_eq!(rt.state.pending_approvals().len(), 0);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            voxcell_ui::UiCommand::ResolveApproval { decision, .. } if decision == "once"
        )));

        // Accessibility tree exposes semantic roles cleanly
        let a11y = rt.accessibility_json().to_string();
        assert!(a11y.contains("Home"));
        assert!(a11y.contains("Canvas"));
        assert!(a11y.contains("Settings"));
    }

    #[test]
    fn canvas_scene_and_settings_dialog_render_without_panics() {
        let mut s = AppState::new();
        s.apply_scene_preset("canvas");
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);
        assert_eq!(rt.state.dots.len(), 5);
        assert!(rt.find("canvas-node-dot-0").is_some());

        // Open settings dialog by clicking the Settings button in the top bar
        let settings_btn = rt
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref() == Some("Settings"))
            .expect("Settings button");
        let sx = settings_btn.target[0] + settings_btn.target[2] * 0.5;
        let sy = settings_btn.target[1] + settings_btn.target[3] * 0.5;
        rt.pointer_down(sx, sy);
        rt.pointer_up(sx, sy);
        rt.frame(0.016);
        assert!(rt.state.settings_open);
        assert!(rt.find("settings-dialog").is_some());

        // Speaking state drives non-calm stage mood
        rt.state.apply_scene_preset("speaking");
        rt.frame(0.032);
        assert_eq!(rt.state.stage_mood, ExpressionMood::Speaking);
    }

    // ---- P3-01 keyboard shortcuts (headless, pure handler) ----

    fn chord(key: Key, cmd: bool, ctrl: bool, shift: bool) -> KeyEvent {
        KeyEvent {
            key,
            mods: rune::Mods { cmd, ctrl, shift, ..rune::Mods::default() },
        }
    }

    #[test]
    fn ctrl_cmd_digits_switch_all_five_views() {
        let cases = [
            ("1", ActiveView::Home),
            ("2", ActiveView::Canvas),
            ("3", ActiveView::Thread),
            ("4", ActiveView::Queue),
            ("5", ActiveView::Cli),
        ];
        for (digit, view) in cases {
            // Cmd (platform primary) form.
            let mut s = AppState::new();
            let ev = chord(Key::Text(digit.to_string()), true, false, false);
            assert!(handle_shortcut(&mut s, &ev), "cmd+{digit} consumed");
            assert_eq!(s.active_view, view);
            // Explicit Ctrl form (Windows/Linux report cmd+ctrl together).
            let mut s = AppState::new();
            let ev = chord(Key::Text(digit.to_string()), true, true, false);
            assert!(handle_shortcut(&mut s, &ev));
            assert_eq!(s.active_view, view);
            // Ctrl alone must also work.
            let mut s = AppState::new();
            let ev = chord(Key::Text(digit.to_string()), false, true, false);
            assert!(handle_shortcut(&mut s, &ev));
            assert_eq!(s.active_view, view);
        }
    }

    #[test]
    fn b_shortcut_toggles_sidebar_including_command_key_form() {
        let mut s = AppState::new();
        assert!(!s.ui_config.sidebar_collapsed);
        assert!(handle_shortcut(&mut s, &chord(Key::Text("b".into()), true, false, false)));
        assert!(s.ui_config.sidebar_collapsed);
        // Uppercase B (shift held) still toggles.
        assert!(handle_shortcut(&mut s, &chord(Key::Text("B".into()), true, false, true)));
        assert!(!s.ui_config.sidebar_collapsed);
        // Native macOS Command('b') form.
        assert!(handle_shortcut(&mut s, &chord(Key::Command('b'), true, false, false)));
        assert!(s.ui_config.sidebar_collapsed);
    }

    #[test]
    fn comma_shortcut_toggles_settings_and_escape_closes() {
        let mut s = AppState::new();
        assert!(!s.settings_open);
        assert!(handle_shortcut(&mut s, &chord(Key::Text(",".into()), true, false, false)));
        assert!(s.settings_open);
        assert!(handle_shortcut(&mut s, &chord(Key::Text(",".into()), true, false, false)));
        assert!(!s.settings_open);
        // Escape closes an open settings dialog (and is consumed).
        s.settings_open = true;
        assert!(handle_shortcut(&mut s, &chord(Key::Escape, false, false, false)));
        assert!(!s.settings_open);
        // Escape with nothing open passes through.
        assert!(!handle_shortcut(&mut s, &chord(Key::Escape, false, false, false)));
    }

    #[test]
    fn unmodified_keys_pass_through_and_phone_vad_chords_keep_working() {
        let mut s = AppState::new();
        // No modifier: digits/b/comma are plain typing, not shortcuts.
        for t in ["1", "5", "b", ","] {
            assert!(!handle_shortcut(&mut s, &chord(Key::Text(t.into()), false, false, false)));
        }
        assert_eq!(s.active_view, ActiveView::Home);
        assert!(!s.ui_config.sidebar_collapsed);
        // Cmd+P phone widget toggle queues the window command.
        assert!(handle_shortcut(&mut s, &chord(Key::Text("p".into()), true, false, false)));
        assert!(s.phone_widget_open);
        assert!(s.pending_commands.iter().any(|c| matches!(c, voxcell_ui::UiCommand::TogglePhoneWindow(true))));
        // Cmd+Shift+Space hands-free VAD toggle queues the mic command.
        assert!(handle_shortcut(&mut s, &chord(Key::Text(" ".into()), true, false, true)));
        assert!(s.voice.hands_free_vad);
        assert!(s.pending_commands.iter().any(|c| matches!(c, voxcell_ui::UiCommand::ToggleMic(true))));
    }
}
