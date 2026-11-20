//! Main window chrome (P3-01): custom titlebar with Home/Canvas/Thread/Queue/
//! CLI tabs, a gliding active-tab indicator driven by
//! `core::motion::compute_sliding_indicator_rect`, staggered tab entrances via
//! `core::motion::compute_stagger_delay_ms` (zeroed under reduced motion), and
//! the amber Queue approval badge. View bodies live in the sibling modules.

pub mod canvas;
pub mod cli;
pub mod home;
pub mod phone_widget;
pub mod queue;
pub mod settings;
pub mod sidebar;
pub mod thread;

use crate::state::{AppState, UiCommand};
use crate::theme::{VoxcellTheme, render_ambient_backdrop};
use rune::{Cx, El, TextStyle, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};
use voxcell_core::motion::{
    IndicatorContainerRect, IndicatorItemRect, compute_sliding_indicator_rect,
};
use voxcell_core::{ActiveView, SidebarPosition};

/// Height of the in-app titlebar band (points). Shared with the native-chrome
/// `Titlebar::new` declaration so the OS caption buttons line up with the row.
pub const TITLEBAR_HEIGHT: f32 = 46.0;

/// Titlebar tab order — matches `AppState::nav_tab_rects` indices and the
/// Ctrl/Cmd+1..5 shortcuts.
pub const NAV_TABS: [(ActiveView, &str); 5] = [
    (ActiveView::Home, "Home"),
    (ActiveView::Canvas, "Canvas"),
    (ActiveView::Thread, "Thread"),
    (ActiveView::Queue, "Queue"),
    (ActiveView::Cli, "CLI"),
];

pub fn nav_tab_index(view: ActiveView) -> usize {
    match view {
        ActiveView::Home => 0,
        ActiveView::Canvas => 1,
        ActiveView::Thread => 2,
        ActiveView::Queue => 3,
        ActiveView::Cli => 4,
    }
}

fn tab_key(view: ActiveView) -> &'static str {
    match view {
        ActiveView::Home => "nav-tab-home",
        ActiveView::Canvas => "nav-tab-canvas",
        ActiveView::Thread => "nav-tab-thread",
        ActiveView::Queue => "nav-tab-queue",
        ActiveView::Cli => "nav-tab-cli",
    }
}

/// The gliding underline indicator, container-relative, computed by core
/// motion from the measured tab rects (web MorphTabs parity). Hidden until
/// both rects are measured (`ready: false` on first layout).
fn render_tab_indicator(state: &AppState, theme: &VoxcellTheme) -> Option<El> {
    let container = state.nav_container_rect.map(|r| IndicatorContainerRect {
        left: r[0] as f64,
        top: r[1] as f64,
        scroll_left: None,
        scroll_top: None,
    });
    let item = state.nav_tab_rects[nav_tab_index(state.active_view)].map(|r| IndicatorItemRect {
        left: r[0] as f64,
        top: r[1] as f64,
        width: r[2] as f64,
        height: r[3] as f64,
    });
    let rect = compute_sliding_indicator_rect(container, item);
    if !rect.ready {
        return None;
    }
    // Underline variant: 2px accent bar at the tab's bottom edge, soft glow.
    Some(
        el().key("nav-tab-indicator")
            .absolute(rect.x as f32, (rect.y + rect.height - 2.0) as f32)
            .w(rect.width as f32)
            .h(2.0)
            .radius(1.0)
            .fill(theme.accent)
            .shadow(0.0, 10.0, theme.accent.alpha(0.38))
            .motion(theme.tab_spring())
            .a11y_hidden(),
    )
}

/// Custom titlebar tab strip (web MorphTabs, underline variant).
fn render_nav_tabs(state: &AppState, theme: &VoxcellTheme) -> El {
    let mut tabs = row().key("top-nav-tabs").gap(2.0).items_center();
    if let Some(indicator) = render_tab_indicator(state, theme) {
        tabs = tabs.child(indicator);
    }

    for (i, (view, label)) in NAV_TABS.iter().enumerate() {
        let view = *view;
        let active = state.active_view == view;
        let label_style = TextStyle { weight: if active { 600.0 } else { 500.0 }, ..theme.ui_sm };
        let mut label_row = row().gap(6.0).items_center().child(
            text(label.to_string())
                .font(label_style)
                .color(if active { theme.accent_soft } else { theme.text_muted }),
        );

        // Queue approval badge — semantic amber (working/approval telemetry),
        // mono tabular count, pill shape (web MorphTabs badge parity).
        if view == ActiveView::Queue {
            let n = state.pending_approvals().len();
            if n > 0 {
                label_row = label_row.child(
                    el().key("queue-approval-badge")
                        .min_w(16.0)
                        .h(16.0)
                        .px(4.0)
                        .radius(8.0)
                        .fill(theme.semantic_working.alpha(0.2))
                        .border(1.0, theme.semantic_working.alpha(0.4))
                        .items_center()
                        .justify_center()
                        .child(
                            text(n.to_string())
                                .font(TextStyle::new(9.5 * theme.font_scale, 500.0).lh(12.0).mono().features(crate::theme::MONO_FEATURES))
                                .color(theme.semantic_working_soft),
                        ),
                );
            }
        }

        let tab = el().key(tab_key(view))
            .role("tab")
            .checked(active)
            .shortcut_hint(format!("Ctrl+{}", i + 1))
            .px(10.0)
            .py(7.0)
            .radius(theme.radius_control.min(10.0))
            .motion(theme.spring())
            .transition(theme.stagger_transition())
            .enter_delay((theme.stagger_ms(i) / 1000.0) as f32)
            .on_press(move |s: &mut AppState| s.active_view = view)
            .on_layout(move |s: &mut AppState, r| s.nav_tab_rects[i] = Some(r))
            .child(label_row);
        tabs = tabs.child(tab);
    }

    tabs.on_layout(|s: &mut AppState, r| s.nav_container_rect = Some(r))
}

pub fn render_main_window(state: &AppState, cx: &mut Cx<AppState>) -> El {
    let mut theme = VoxcellTheme::from_config(&state.ui_config);
    // Honor both the customization switch and the OS reduced-motion setting;
    // propagate into rune so springs/entrances land at once (§6.3).
    theme.reduced_motion = theme.reduced_motion || cx.system_reduced_motion();
    cx.reduced_motion(theme.reduced_motion);
    cx.caption_color(theme.text_muted);
    cx.foreground(theme.text);
    // P5 native-chrome (spec P3-01 gap): declare the app-drawn titlebar band
    // to rune while the toggle is on. The frame swap itself is creation-time
    // (`App::titlebar` in main.rs, from the persisted flag); this keeps the
    // band height in sync at runtime (`Cx::set_titlebar`). Off = native frame
    // stays and nothing here changes.
    if state.native_chrome {
        cx.set_titlebar(rune::Titlebar::new(TITLEBAR_HEIGHT));
    }
    let k = &theme.kit;

    // 1. Custom titlebar: workspace wordmark, sidebar toggle, nav tabs,
    //    gateway telemetry readout and window controls.
    let mut top_bar = row()
        .key("top-header")
        .role("toolbar")
        .w_full()
        .h(TITLEBAR_HEIGHT)
        .px(14.0)
        .justify_between()
        .items_center()
        .fill(theme.glass_fill)
        .border(1.0, theme.border_subtle)
        .child(
            row()
                .gap(12.0)
                .items_center()
                .child(
                    text(state.ui_config.workspace_title.clone())
                        .key("workspace-title")
                        .font(TextStyle { size: 13.0 * theme.font_scale, weight: 700.0, ..theme.heading })
                        .color(theme.text),
                )
                .child(
                    Button::new(if state.ui_config.sidebar_collapsed {
                        "Show Sidebar"
                    } else {
                        "Hide Sidebar"
                    })
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Mini)
                    .on_press(|s: &mut AppState| {
                        s.ui_config.sidebar_collapsed = !s.ui_config.sidebar_collapsed;
                    })
                    .build(k),
                )
                .child(if state.ui_config.show_top_nav {
                    render_nav_tabs(state, &theme)
                } else {
                    el().key("top-nav-tabs-hidden")
                }),
        )
        .child(
            row()
                .gap(8.0)
                .items_center()
                .child(
                    row().key("gateway-status").gap(6.0).items_center()
                        .child(
                            el().key("gateway-status-dot")
                                .w(6.0)
                                .h(6.0)
                                .radius(3.0)
                                .fill(if state.gateway.online {
                                    theme.semantic_live
                                } else {
                                    theme.semantic_error.alpha(0.7)
                                }),
                        )
                        .child(
                            text(if state.gateway.online {
                                format!("{} · {}ms", state.gateway.model, state.gateway.latency_ms.unwrap_or(0))
                            } else {
                                "local standby".to_string()
                            })
                            .font(theme.mono_sm)
                            .color(if state.gateway.online {
                                theme.text_muted
                            } else {
                                theme.text_faint
                            }),
                        ),
                )
                .child(
                    Button::new(if state.voice.muted { "TTS Muted" } else { "TTS On" })
                        .variant(ButtonVariant::Neutral)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| {
                            let next = !s.voice.muted;
                            s.voice.muted = next;
                            s.pending_commands.push(UiCommand::ToggleMute(next));
                        })
                        .build(k),
                )
                .child(
                    Button::new("Phone HUD")
                        .variant(if state.phone_widget_open {
                            ButtonVariant::Submit
                        } else {
                            ButtonVariant::Neutral
                        })
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| {
                            let next = !s.phone_widget_open;
                            s.phone_widget_open = next;
                            s.pending_commands.push(UiCommand::TogglePhoneWindow(next));
                        })
                        .build(k),
                )
                .child(
                    Button::new("Settings")
                        .variant(ButtonVariant::Neutral)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| s.settings_open = true)
                        .build(k),
                ),
        );

    // P5 native-chrome: the titlebar row is the window drag region (rune keeps
    // presses on hit targets inside it) and clears the OS caption buttons'
    // trailing inset so Settings/Phone HUD never sit under them. Headless the
    // inset is 0, so the row keeps its normal 14px padding.
    if state.native_chrome {
        top_bar = top_bar
            .window_drag()
            .pr(14.0 + cx.chrome_insets().trailing);
    }

    // 2. Active Content Pane
    let main_pane = match state.active_view {
        ActiveView::Home => home::render_home(state, &theme, cx),
        ActiveView::Canvas => canvas::render_canvas(state, &theme, cx),
        ActiveView::Thread => thread::render_thread(state, &theme, cx),
        ActiveView::Queue => queue::render_queue(state, &theme, cx),
        ActiveView::Cli => cli::render_cli(state, &theme, cx),
    };

    let main_pane_wrap = el()
        .key("main-pane")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .h_full()
        .child(main_pane);

    // 3. Responsive Workspace Body (Sidebar Left or Right)
    let mut workspace = row().key("workspace-body").grow(1.0).min_h(0.0).w_full();
    let sidebar_shown = !state.ui_config.sidebar_collapsed && state.ui_config.sidebar_visible;
    if sidebar_shown {
        let sb = sidebar::render_sidebar(state, &theme, cx);
        match state.ui_config.sidebar_position {
            SidebarPosition::Left => {
                workspace = workspace.child(sb).child(main_pane_wrap);
            }
            SidebarPosition::Right => {
                workspace = workspace.child(main_pane_wrap).child(sb);
            }
        }
    } else {
        workspace = workspace.child(main_pane_wrap);
    }

    // 4. Overlay Layer (Settings Dialog, Add Dot Dialog, or Embedded Phone HUD Preview in phone scene)
    let mut overlay_layer = Vec::new();
    settings::render_settings_dialog(state, &theme, &mut overlay_layer);
    canvas::render_add_dot_dialog(state, &theme, &mut overlay_layer);

    // Embedded dock only while the widget is NOT detached into its own
    // always-on-top window (P4-02 phone-tray) — avoids double display.
    if state.phone_widget_open
        && !state.phone_detached
        && state.active_view == ActiveView::Home
        && !state.settings_open
    {
        let phone_card = el()
            .key("phone-floating-dock")
            .absolute(920.0, 320.0)
            .w(320.0)
            .h(420.0)
            .radius(theme.radius_card)
            .child(phone_widget::render_phone_widget(state, &theme, cx));
        overlay_layer.push(phone_card);
    }

    let base_shell = el()
        .key("voxcell-root")
        .role("window")
        .w_full()
        .h_full()
        .child(render_ambient_backdrop(
            &theme,
            &state.bg_settings,
            state.time_s,
            theme.reduced_motion,
        ))
        .child(
            col()
                .w_full()
                .h_full()
                .min_h(0.0)
                .child(top_bar)
                .child(workspace),
        );

    rune_kit::root(base_shell, overlay_layer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime, Spring};
    use voxcell_core::motion::compute_sliding_indicator_rect;

    fn boot(scene: &str) -> Runtime<AppState> {
        let mut s = AppState::new();
        s.apply_scene_preset(scene);
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);
        rt
    }

    fn settle(rt: &mut Runtime<AppState>, frames: usize) {
        for _ in 0..frames {
            rt.frame(0.016);
        }
    }

    fn click_text(rt: &mut Runtime<AppState>, label: &str) {
        let el = rt
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("element with text {label:?}"));
        let cx = el.target[0] + el.target[2] * 0.5;
        let cy = el.target[1] + el.target[3] * 0.5;
        rt.pointer_down(cx, cy);
        rt.pointer_up(cx, cy);
        rt.frame(0.016);
    }

    fn indicator_element(rt: &Runtime<AppState>) -> Option<rune::ElementInfo> {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("nav-tab-indicator"))
    }

    #[test]
    fn titlebar_tabs_switch_all_five_views() {
        let mut rt = boot("idle");
        for (view, label) in NAV_TABS {
            click_text(&mut rt, label);
            assert_eq!(rt.state.active_view, view, "tab {label}");
        }
        // Back to Home via its tab.
        click_text(&mut rt, "Home");
        assert_eq!(rt.state.active_view, ActiveView::Home);
    }

    #[test]
    fn tab_rects_measured_and_indicator_tracks_active_tab() {
        // Reduced motion makes the indicator land at once -> deterministic geometry.
        let mut s = AppState::new();
        s.apply_scene_preset("idle");
        s.ui_config.reduced_motion = true;
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        settle(&mut rt, 4);

        let container = rt.state.nav_container_rect.expect("container measured");
        assert!(container[2] > 0.0 && container[3] > 0.0);
        for (i, rect) in rt.state.nav_tab_rects.iter().enumerate() {
            let r = rect.unwrap_or_else(|| panic!("tab {i} measured"));
            assert!(r[2] > 0.0 && r[3] > 0.0, "tab {i} has size");
        }

        // Indicator rect equals core::motion output for the active tab.
        let active = nav_tab_index(rt.state.active_view);
        let c = rt.state.nav_container_rect.unwrap();
        let it = rt.state.nav_tab_rects[active].unwrap();
        let expected = compute_sliding_indicator_rect(
            Some(IndicatorContainerRect { left: c[0] as f64, top: c[1] as f64, scroll_left: None, scroll_top: None }),
            Some(IndicatorItemRect { left: it[0] as f64, top: it[1] as f64, width: it[2] as f64, height: it[3] as f64 }),
        );
        assert!(expected.ready);
        let ind = indicator_element(&rt).expect("indicator present");
        assert!((ind.rect[0] - (c[0] + expected.x as f32)).abs() < 1.0, "indicator x");
        assert!((ind.rect[1] - (c[1] + (expected.y + expected.height - 2.0) as f32)).abs() < 1.0, "indicator y");
        assert!((ind.rect[2] - expected.width as f32).abs() < 1.0, "indicator width");
        assert!((ind.rect[3] - 2.0).abs() < 0.5, "underline height");

        // Switch to Thread: under reduced motion the indicator lands on the
        // Thread tab rect within the very first frame (no glide).
        click_text(&mut rt, "Thread");
        assert_eq!(rt.state.active_view, ActiveView::Thread);
        let it2 = rt.state.nav_tab_rects[nav_tab_index(ActiveView::Thread)].unwrap();
        let ind2 = indicator_element(&rt).expect("indicator still present");
        assert!((ind2.rect[0] - it2[0]).abs() < 1.0, "indicator moved to Thread tab");
        assert!((ind2.rect[2] - it2[2]).abs() < 1.0, "indicator width matches Thread tab");
    }

    #[test]
    fn indicator_glides_with_spring_when_motion_enabled() {
        let mut rt = boot("idle");
        settle(&mut rt, 4);
        assert!(indicator_element(&rt).is_some());
        let theme = VoxcellTheme::from_config(&rt.state.ui_config);
        assert_ne!(theme.tab_spring(), Spring::NONE);
        click_text(&mut rt, "Queue");
        rt.frame(0.016);
        let ind = indicator_element(&rt).expect("indicator present");
        assert!(ind.moving, "indicator mid-glide after tab switch");
        // It settles on the Queue tab eventually.
        settle(&mut rt, 40);
        let q = rt.state.nav_tab_rects[nav_tab_index(ActiveView::Queue)].unwrap();
        let ind = indicator_element(&rt).unwrap();
        assert!((ind.rect[0] - q[0]).abs() < 1.5);
    }

    #[test]
    fn queue_approval_badge_counts_pending_and_clears() {
        let mut rt = boot("attention");
        settle(&mut rt, 2);
        assert_eq!(rt.state.pending_approvals().len(), 1);
        let badge = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("queue-approval-badge"))
            .expect("queue badge visible");
        // Badge count text lives inside the badge element.
        let count_text = rt
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref() == Some("1") && e.rect[0] >= badge.rect[0] - 1.0 && e.rect[0] <= badge.rect[0] + badge.rect[2] + 1.0)
            .expect("badge count text");
        assert!(count_text.rect[3] <= 16.0);

        // Mutate through Runtime::update so the view re-runs (frame() only
        // re-renders on notified state changes).
        rt.update(|s, _tx| s.resolve_approval("apr-deploy-1", "once"));
        assert_eq!(rt.state.pending_approvals().len(), 0);
        // The badge plays its inherited exit fade; let it finish.
        settle(&mut rt, 30);
        assert!(rt
            .elements()
            .into_iter()
            .all(|e| e.key.as_deref() != Some("queue-approval-badge")));
    }

    #[test]
    fn reduced_motion_zeroes_chrome_stagger() {
        let mut s = AppState::new();
        s.ui_config.reduced_motion = true;
        let theme = VoxcellTheme::from_config(&s.ui_config);
        for i in 0..NAV_TABS.len() {
            assert_eq!(theme.stagger_ms(i), 0.0);
        }
        assert_eq!(theme.spring(), Spring::NONE);
        // Chrome still renders and measures with reduced motion.
        s.apply_scene_preset("idle");
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        settle(&mut rt, 3);
        assert!(rt.state.nav_container_rect.is_some());
        assert!(indicator_element(&rt).is_some());
    }

    #[test]
    fn sidebar_toggle_button_collapses_and_restores() {
        let mut rt = boot("idle");
        assert!(!rt.state.ui_config.sidebar_collapsed);
        click_text(&mut rt, "Hide Sidebar");
        assert!(rt.state.ui_config.sidebar_collapsed);
        click_text(&mut rt, "Show Sidebar");
        assert!(!rt.state.ui_config.sidebar_collapsed);
    }

    #[test]
    fn show_top_nav_false_hides_tabs_but_keeps_chrome() {
        let mut s = AppState::new();
        s.apply_scene_preset("idle");
        s.ui_config.show_top_nav = false;
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        settle(&mut rt, 2);
        assert!(rt.find("top-header").is_some());
        assert!(rt.find("top-nav-tabs").is_none());
        assert!(rt.find("Settings").is_some() || rt.elements().iter().any(|e| e.text.as_deref() == Some("Settings")));
    }

    #[test]
    fn embedded_phone_dock_hidden_while_detached() {
        let mut s = AppState::new();
        s.apply_scene_preset("phone");
        assert!(s.phone_widget_open);
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        settle(&mut rt, 2);
        assert!(rt.find("phone-floating-dock").is_some(), "embedded dock while attached");
        // Detached into its own always-on-top window: no double display.
        rt.update(|s, _tx| s.phone_detached = true);
        settle(&mut rt, 2);
        assert!(rt.find("phone-floating-dock").is_none(), "dock hidden while detached");
        // Re-attaching brings the embedded dock back.
        rt.update(|s, _tx| s.phone_detached = false);
        settle(&mut rt, 2);
        assert!(rt.find("phone-floating-dock").is_some(), "dock returns when re-attached");
    }

    // ---- P5 native-chrome (headless chrome-branch tests) ----

    #[test]
    fn native_chrome_mode_renders_and_tabs_keep_working() {
        let mut s = AppState::new();
        s.apply_scene_preset("idle");
        s.native_chrome = true;
        s.ui_config.reduced_motion = true;
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        settle(&mut rt, 3);
        assert!(rt.find("top-header").is_some(), "titlebar renders frameless");
        // Hit targets inside the `.window_drag()` region keep their presses.
        click_text(&mut rt, "Canvas");
        assert_eq!(rt.state.active_view, ActiveView::Canvas);
        click_text(&mut rt, "Home");
        assert_eq!(rt.state.active_view, ActiveView::Home);
        // Settings dialog still opens over the frameless chrome.
        click_text(&mut rt, "Settings");
        assert!(rt.state.settings_open);
        assert!(rt.find("settings-dialog").is_some());
    }

    #[test]
    fn native_chrome_toggle_flips_between_modes_without_panic() {
        let mut rt = boot("idle");
        assert!(!rt.state.native_chrome, "default off = current behavior");
        assert!(rt.find("top-header").is_some());
        rt.update(|s, _tx| s.native_chrome = true);
        settle(&mut rt, 2);
        assert!(rt.find("top-header").is_some(), "chrome-on branch renders");
        assert!(rt.find("nav-tab-home").is_some());
        rt.update(|s, _tx| s.native_chrome = false);
        settle(&mut rt, 2);
        assert!(rt.find("top-header").is_some(), "chrome-off branch renders");
        // Default mode unchanged: tabs still switch views.
        click_text(&mut rt, "Thread");
        assert_eq!(rt.state.active_view, ActiveView::Thread);
    }
}
