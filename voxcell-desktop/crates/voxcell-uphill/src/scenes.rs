//! Scene definitions + the headless scene runner. Each scene boots the real
//! `voxcell-ui` render fns through rune's headless `Runtime`, settles layout
//! flights, then runs the full audit battery.

use rune::{Cx, El, Env, Runtime};
use std::time::Instant;
use voxcell_ui::{AppState, VoxcellTheme, render_main_window, render_phone_widget};

use crate::{SceneReport, audit, motion, perf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneKind {
    Main,
    ThreadSplit,
    ThreadOverlay,
    Phone,
}

#[derive(Clone, Copy, Debug)]
pub struct SceneSpec {
    pub name: &'static str,
    pub preset: &'static str,
    pub size: (f32, f32),
    pub kind: SceneKind,
}

/// Spec §9 scene matrix: idle/working/attention/speaking/canvas, thread at
/// both sides of the 900px stage boundary, and the 320x420 phone widget.
pub const SCENES: &[SceneSpec] = &[
    SceneSpec { name: "idle", preset: "idle", size: (1280.0, 800.0), kind: SceneKind::Main },
    SceneSpec { name: "working", preset: "working", size: (1280.0, 800.0), kind: SceneKind::Main },
    SceneSpec { name: "attention", preset: "attention", size: (1280.0, 800.0), kind: SceneKind::Main },
    SceneSpec { name: "speaking", preset: "speaking", size: (1280.0, 800.0), kind: SceneKind::Main },
    SceneSpec { name: "canvas", preset: "canvas", size: (1280.0, 800.0), kind: SceneKind::Main },
    // 1280 - 380 sidebar = 900px stage exactly: split dock.
    SceneSpec { name: "thread-split", preset: "thread", size: (1280.0, 800.0), kind: SceneKind::ThreadSplit },
    // 1279 - 380 = 899px stage: one pixel below the boundary -> overlay.
    SceneSpec { name: "thread-overlay", preset: "thread", size: (1279.0, 800.0), kind: SceneKind::ThreadOverlay },
    SceneSpec { name: "phone", preset: "phone", size: (320.0, 420.0), kind: SceneKind::Phone },
];

pub fn spec_by_name(name: &str) -> Option<&'static SceneSpec> {
    SCENES.iter().find(|s| s.name == name)
}

/// Phone scenes render the dedicated widget window; wrap the 3-arg view fn
/// into the 2-arg root signature rune's Runtime expects (mirrors main.rs).
fn render_phone_root(state: &AppState, cx: &mut Cx<AppState>) -> El {
    let theme = VoxcellTheme::from_config(&state.ui_config);
    render_phone_widget(state, &theme, cx)
}

/// Boot a scene's Runtime (state preset + viewport applied, first frame run).
pub fn boot(spec: &SceneSpec) -> Runtime<AppState> {
    let mut s = AppState::new();
    s.apply_scene_preset(spec.preset);
    match spec.kind {
        SceneKind::ThreadSplit | SceneKind::ThreadOverlay => {
            s.ui_config.sidebar_width = 380.0;
        }
        _ => {}
    }
    let render: fn(&AppState, &mut Cx<AppState>) -> El = if spec.kind == SceneKind::Phone {
        render_phone_root
    } else {
        render_main_window
    };
    let mut rt = Runtime::new(s, render, spec.size, Env::new(true, 1.0));
    rt.frame(0.0);
    rt
}

fn settle(rt: &mut Runtime<AppState>, frames: usize) {
    for _ in 0..frames {
        rt.frame(0.016);
    }
}

/// Canvas is the second nav tab (NAV_TABS order).
pub fn nav_canvas_index() -> usize {
    1
}

fn rect_of(rt: &Runtime<AppState>, key: &str) -> Option<[f32; 4]> {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .map(|e| e.target)
}

const MAIN_A11Y_KEYS: &[&str] = &[
    "nav-tab-home", "nav-tab-canvas", "nav-tab-thread", "nav-tab-queue", "nav-tab-cli",
];
const PHONE_A11Y_KEYS: &[&str] = &["phone-header", "phone-controls"];

/// Run one scene end-to-end and produce its audit report. Panics propagate
/// to the caller (`run_suite` catches them into `blocked` gap reports).
pub fn run_scene(spec: &SceneSpec) -> SceneReport {
    let t_boot = Instant::now();
    let mut rt = boot(spec);
    let boot_ms = t_boot.elapsed().as_secs_f64() * 1000.0;
    let t_audit = Instant::now();
    // Land every entrance stagger / motion flight before static audits so
    // opacity and rects are final (mid-fade text reads as false low contrast).
    rt.settle_all();
    settle(&mut rt, 3);

    let is_phone = spec.kind == SceneKind::Phone;
    let mut report = SceneReport {
        scene: spec.name.into(),
        status: "ok".into(),
        gap: None,
        viewport: spec.size,
        a11y: Default::default(),
        clipping: Default::default(),
        contrast: Default::default(),
        popover: None,
        motion: None,
        perf: None,
        copy: Default::default(),
        reduced_motion_freeze: None,
        boot_ms,
        audit_ms: 0.0,
        overlay_ok: None,
        portrait_rendered: None,
    };

    // Thread scenes: open the task drawer so split/overlay chrome is audited.
    if matches!(spec.kind, SceneKind::ThreadSplit | SceneKind::ThreadOverlay) {
        rt.update(|s, _tx| s.thread_tasks_open = true);
        settle(&mut rt, 2);
        rt.settle_all();
        rt.frame(0.016);
    }

    report.a11y = audit::audit_a11y(&rt, if is_phone { PHONE_A11Y_KEYS } else { MAIN_A11Y_KEYS });
    report.clipping = audit::audit_clipping(&rt.elements());
    report.contrast = audit::audit_contrast(&rt, spec.name);
    report.copy = audit::audit_copy(&rt, if is_phone { "phone-widget-window" } else { "main-pane" });

    // Canvas popover edge safety at left/center/right field positions.
    if spec.preset == "canvas" {
        let mut merged = audit::PopoverReport::default();
        for x in [10.0, 50.0, 90.0] {
            let r = audit::audit_popover_at(&mut rt, "dot-0", x);
            merged.checks.extend(r.checks);
            merged.violations.extend(r.violations);
        }
        report.popover = Some(merged);
    }

    // Character motion traces: full TS-length traces for idle/speaking,
    // short traces elsewhere for the moving-ratio score.
    if !is_phone {
        let (frames, speaking) = match spec.preset {
            "idle" => (120, false),
            "speaking" => (90, true),
            _ => (30, false),
        };
        let mut m = motion::collect_trace(&mut rt, frames, 1.0 / 30.0, speaking);
        if spec.preset == "idle" {
            // Continuity score needs the paired speaking trace; run it on a
            // fresh boot so the idle state stays pristine for later checks.
            let speak_spec = SceneSpec { preset: "speaking", ..*spec };
            let mut srt = boot(&speak_spec);
            settle(&mut srt, 2);
            let speak = motion::collect_trace(&mut srt, 90, 1.0 / 30.0, true);
            m.continuity_score = Some(motion::continuity_score(&m, &speak));
        }
        report.motion = Some(m);
    }

    // 120 Hz render budget (30 warmup + 120 measured frames).
    report.perf = Some(perf::measure_budget(&mut rt, 30, 120));

    // Reduced-motion freeze (web data-ui-motion="reduced" parity): with the
    // switch on, the gliding tab indicator must LAND on the target tab within
    // one frame — no spring glide. Position-based: rune's `.moving` flag also
    // tracks landed presence state, so it is not a freeze signal by itself.
    if !is_phone {
        rt.update(|s, _tx| s.ui_config.reduced_motion = true);
        // Land any in-flight springs from the animated trace/perf phases so
        // the check measures reduced-motion behavior, not leftover flights.
        rt.settle_all();
        rt.frame(0.016);
        if let Some((x, y)) = rt.find("nav-tab-canvas") {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        rt.frame(0.016);
        let indicator = rt.elements().into_iter()
            .find(|e| e.key.as_deref() == Some("nav-tab-indicator"))
            .map(|e| e.rect);
        let target = rt.state.nav_tab_rects[crate::scenes::nav_canvas_index()];
        let frozen = match (indicator, target) {
            (Some(ind), Some(t)) => (ind[0] - t[0]).abs() < 1.0 && (ind[2] - t[2]).abs() < 1.0,
            _ => false,
        };
        if !frozen {
            eprintln!("[uphill] {} reduced-motion indicator did not land: ind={indicator:?} target={target:?}", spec.name);
        }
        report.reduced_motion_freeze = Some(frozen);
        rt.update(|s, _tx| {
            s.ui_config.reduced_motion = false;
            s.active_view = voxcell_core::ActiveView::Home;
        });
        rt.frame(0.016);
    }

    // Overlay fidelity: settings dialog (main) / task overlay (thread).
    match spec.kind {
        SceneKind::ThreadOverlay => {
            report.overlay_ok = Some(rect_of(&rt, "thread-tasks-overlay").is_some());
        }
        SceneKind::Main => {
            rt.update(|s, _tx| s.settings_open = true);
            settle(&mut rt, 2);
            let ok = rect_of(&rt, "settings-dialog").is_some();
            rt.update(|s, _tx| s.settings_open = false);
            settle(&mut rt, 2);
            report.overlay_ok = Some(ok);
        }
        _ => {}
    }

    if is_phone {
        report.portrait_rendered = Some(
            rect_of(&rt, "phone-portrait-box")
                .map(|r| r[2] > 100.0 && r[3] > 100.0)
                .unwrap_or(false),
        );
    }

    report.audit_ms = t_audit.elapsed().as_secs_f64() * 1000.0;
    report
}
