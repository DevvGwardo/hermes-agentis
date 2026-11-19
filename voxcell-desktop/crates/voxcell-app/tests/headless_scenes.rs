//! P4-04 app-level integration: the real voxcell-ui render fns boot every
//! spec §9 scene headlessly through rune's Runtime without panics, expose a
//! named AccessKit chrome, and honor the 900px thread drawer boundary.
//! Deep audits (clipping/contrast/motion/budget) live in voxcell-uphill.

use rune::{Cx, El, Env, Runtime};
use voxcell_core::ActiveView;
use voxcell_ui::{AppState, VoxcellTheme, render_main_window, render_phone_widget};

/// Wrap the 3-arg phone view fn into rune's 2-arg root signature.
fn render_phone_root(state: &AppState, cx: &mut Cx<AppState>) -> El {
    let theme = VoxcellTheme::from_config(&state.ui_config);
    render_phone_widget(state, &theme, cx)
}

fn boot_main(preset: &str, size: (f32, f32)) -> Runtime<AppState> {
    let mut s = AppState::new();
    s.apply_scene_preset(preset);
    let mut rt = Runtime::new(s, render_main_window, size, Env::new(true, 1.0));
    rt.frame(0.0);
    rt.frame(0.016);
    rt
}

#[test]
fn all_main_scenes_boot_and_expose_named_navigation() {
    for preset in ["idle", "working", "attention", "speaking", "canvas", "thread", "queue", "cli", "settings"] {
        let mut rt = boot_main(preset, (1280.0, 800.0));
        if preset == "settings" {
            // The preset opens the modal dialog: background chrome is inert
            // (correct modal a11y), so assert the dialog itself is exposed,
            // then close it and check the nav chrome underneath.
            let a11y = rt.accessibility_json().to_string();
            assert!(a11y.contains("Settings"), "settings dialog exposed in AccessKit tree");
            assert!(rt.find("settings-dialog").is_some());
            rt.update(|s, _tx| s.settings_open = false);
            rt.frame(0.016);
            rt.settle_all();
            rt.frame(0.016);
        }
        let a11y = rt.accessibility_json().to_string();
        for tab in ["Home", "Canvas", "Thread", "Queue", "CLI"] {
            assert!(a11y.contains(tab), "{preset}: nav tab {tab} missing from AccessKit tree");
        }
        assert!(rt.find("top-header").is_some(), "{preset}: titlebar present");
        assert!(rt.find("main-pane").is_some(), "{preset}: main pane present");
    }
}

#[test]
fn phone_widget_scene_boots_at_320x420() {
    let mut s = AppState::new();
    s.apply_scene_preset("phone");
    let mut rt = Runtime::new(s, render_phone_root, (320.0, 420.0), Env::new(true, 1.0));
    rt.frame(0.0);
    rt.frame(0.016);
    assert!(rt.find("phone-widget-window").is_some());
    assert!(rt.find("phone-controls").is_some());
    let portrait = rt.elements().into_iter()
        .find(|e| e.key.as_deref() == Some("phone-portrait-box"))
        .expect("phone portrait box");
    assert!(portrait.target[2] > 100.0 && portrait.target[3] > 100.0);
}

#[test]
fn thread_drawer_split_and_overlay_boundary_integration() {
    // >=900px stage: split dock shrinks the transcript.
    let mut s = AppState::new();
    s.apply_scene_preset("thread");
    s.ui_config.sidebar_width = 380.0; // 1280 - 380 = 900
    let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
    rt.frame(0.0);
    rt.frame(0.016);
    let (tx, ty) = rt.find("thread-tasks-toggle").expect("tasks toggle");
    rt.pointer_down(tx, ty);
    rt.pointer_up(tx, ty);
    rt.frame(0.016);
    rt.settle_all();
    rt.frame(0.016);
    assert!(rt.state.thread_tasks_open);
    assert!(rt.find("thread-tasks-drawer").is_some());
    assert!(rt.find("thread-tasks-overlay").is_none(), "split mode has no overlay wrapper");

    // <900px stage: overlay floats without resizing the transcript.
    let mut s2 = AppState::new();
    s2.apply_scene_preset("thread");
    s2.ui_config.sidebar_width = 380.0; // 1279 - 380 = 899
    let mut rt2 = Runtime::new(s2, render_main_window, (1279.0, 800.0), Env::new(true, 1.0));
    rt2.frame(0.0);
    rt2.frame(0.016);
    let pane_before = rt2.elements().into_iter()
        .find(|e| e.key.as_deref() == Some("main-pane")).unwrap().target[2];
    let (tx2, ty2) = rt2.find("thread-tasks-toggle").expect("tasks toggle");
    rt2.pointer_down(tx2, ty2);
    rt2.pointer_up(tx2, ty2);
    rt2.frame(0.016);
    rt2.settle_all();
    rt2.frame(0.016);
    assert!(rt2.find("thread-tasks-overlay").is_some(), "overlay wrapper below 900px");
    let pane_after = rt2.elements().into_iter()
        .find(|e| e.key.as_deref() == Some("main-pane")).unwrap().target[2];
    assert!((pane_after - pane_before).abs() < 2.0, "overlay must not resize transcript");
}

#[test]
fn scene_switching_via_nav_tabs_reaches_every_view() {
    let mut rt = boot_main("idle", (1280.0, 800.0));
    for (key, view) in [
        ("nav-tab-canvas", ActiveView::Canvas),
        ("nav-tab-thread", ActiveView::Thread),
        ("nav-tab-queue", ActiveView::Queue),
        ("nav-tab-cli", ActiveView::Cli),
        ("nav-tab-home", ActiveView::Home),
    ] {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("{key} present"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.016);
        assert_eq!(rt.state.active_view, view, "{key} switches view");
    }
}
