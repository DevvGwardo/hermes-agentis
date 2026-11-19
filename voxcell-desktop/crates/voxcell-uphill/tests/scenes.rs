//! P4-04 headless scene audits: AccessKit completeness, zero clipped text,
//! edge-safe canvas popovers, character motion deltas, 120 Hz frame budget,
//! and the ported blueprint scoring math.

use voxcell_uphill::{audit, motion, scenes, scoring};

fn run(name: &str) -> voxcell_uphill::SceneReport {
    let spec = scenes::spec_by_name(name).expect("scene exists");
    scenes::run_scene(spec)
}

fn assert_static_audits(r: &voxcell_uphill::SceneReport) {
    assert!(
        r.a11y.complete(),
        "{} a11y incomplete: unnamed={:?} missing={:?} unexposed={:?}",
        r.scene, r.a11y.unnamed_interactive, r.a11y.missing_keys, r.a11y.unexposed_focusable
    );
    assert!(
        r.clipping.clean(),
        "{} clipped text (scroll_w > client_w equivalent): {:?}",
        r.scene, r.clipping.clipped
    );
    assert!(
        r.contrast.below_30.is_empty(),
        "{} text below WCAG 3.0: {:?}",
        r.scene, r.contrast.below_30
    );
    assert!(!r.copy.stuck_calibrating, "{} stuck Calibrating copy", r.scene);
    assert!(!r.copy.singular_gate_text, "{} plural 'N gate' copy", r.scene);
    assert!(!r.copy.empty_main_pane, "{} empty main pane ({} chars)", r.scene, r.copy.main_pane_chars);
}

#[test]
fn idle_scene_passes_full_static_audit() {
    let r = run("idle");
    assert_static_audits(&r);
    assert!(r.overlay_ok == Some(true), "settings dialog opens");
    assert!(r.reduced_motion_freeze == Some(true), "reduced motion freezes chrome");
}

#[test]
fn working_scene_passes_full_static_audit() {
    let r = run("working");
    assert_static_audits(&r);
}

#[test]
fn attention_scene_passes_full_static_audit() {
    let r = run("attention");
    assert_static_audits(&r);
}

#[test]
fn speaking_scene_motion_deltas_and_expression_score() {
    let r = run("speaking");
    assert_static_audits(&r);
    let m = r.motion.expect("speaking trace");
    assert!(m.speaking);
    assert!(m.frames == 90);
    // Inter-frame character motion must be measurable and non-dead.
    assert!(m.cell_diffs.mean > 0.0, "speaking luminance field must move between frames");
    assert!(m.moving, "speaking scene shows live motion");
    let expr = m.expression_score.expect("expression score");
    assert!((0.0..=15.0).contains(&expr), "expression score in range: {expr}");
    assert!(m.visemes_hit >= 2, "speech drives multiple visemes, got {}", m.visemes_hit);
}

#[test]
fn idle_scene_continuity_score_port() {
    let r = run("idle");
    let m = r.motion.expect("idle trace");
    assert!(m.frames == 120);
    let cont = m.continuity_score.expect("continuity score from paired traces");
    assert!((0.0..=20.0).contains(&cont), "continuity score in range: {cont}");
    // Natural blink envelope must fire within the 4s idle window.
    assert!(m.blink_max > 0.5, "idle blink envelope peaks, got {}", m.blink_max);
}

#[test]
fn canvas_scene_popovers_are_edge_safe() {
    let r = run("canvas");
    assert_static_audits(&r);
    let p = r.popover.expect("popover audit");
    assert!(p.checks.len() == 3, "left/center/right checks: {:?}", p.checks);
    assert!(p.edge_safe(), "popover violations: {:?}", p.violations);
}

#[test]
fn thread_split_docks_drawer_at_900px_stage() {
    let spec = scenes::spec_by_name("thread-split").unwrap();
    let mut rt = scenes::boot(spec);
    rt.frame(0.016);
    rt.update(|s, _tx| s.thread_tasks_open = true);
    rt.frame(0.016);
    rt.settle_all();
    rt.frame(0.016);
    let els = rt.elements();
    let drawer = els.iter().find(|e| e.key.as_deref() == Some("thread-tasks-drawer")).expect("split drawer");
    assert!(rt.find("thread-tasks-overlay").is_none(), "no overlay wrapper in split mode");
    // Split drawer sits at the right edge of the window and shrinks the pane.
    assert!(drawer.target[0] + drawer.target[2] <= 1280.0 + 1.0);
    assert!(drawer.target[0] > 600.0, "drawer docked right, x={}", drawer.target[0]);
    let r = run("thread-split");
    assert_static_audits(&r);
}

#[test]
fn thread_overlay_floats_below_900px_without_resizing() {
    let spec = scenes::spec_by_name("thread-overlay").unwrap();
    let mut rt = scenes::boot(spec);
    rt.frame(0.016);
    let pane_before = rt.elements().iter()
        .find(|e| e.key.as_deref() == Some("main-pane")).map(|e| e.target[2]).unwrap();
    rt.update(|s, _tx| s.thread_tasks_open = true);
    rt.frame(0.016);
    rt.settle_all();
    rt.frame(0.016);
    assert!(rt.find("thread-tasks-overlay").is_some(), "overlay wrapper present");
    assert!(rt.find("thread-tasks-drawer").is_some(), "overlay drawer present");
    let pane_after = rt.elements().iter()
        .find(|e| e.key.as_deref() == Some("main-pane")).map(|e| e.target[2]).unwrap();
    assert!((pane_after - pane_before).abs() < 2.0, "overlay must not resize transcript: {pane_before} -> {pane_after}");
    let r = run("thread-overlay");
    assert_static_audits(&r);
    assert!(r.overlay_ok == Some(true));
}

#[test]
fn phone_widget_scene_passes_full_static_audit() {
    let r = run("phone");
    assert_static_audits(&r);
    assert!(r.portrait_rendered == Some(true), "phone portrait box rendered");
}

#[test]
fn frame_budget_120hz_measured_on_idle_scene() {
    let r = run("idle");
    let p = r.perf.expect("perf report");
    assert!(p.measured_frames == 120 && p.warmup_frames == 30);
    assert!(p.avg_ms > 0.0);
    if cfg!(debug_assertions) {
        // Debug builds are not the shipping artifact; hold the 60 Hz floor
        // and report the 4 ms budget status. Release enforces the budget.
        assert!(p.p95_ms < 16.0, "debug frame p95 {}ms exceeds 60Hz floor", p.p95_ms);
        eprintln!("[uphill] debug frame p95={:.2}ms avg={:.2}ms within 4ms budget={}", p.p95_ms, p.avg_ms, p.within_budget);
    } else {
        assert!(p.within_budget, "release frame p95 {}ms exceeds 4ms 120Hz budget", p.p95_ms);
    }
}

#[test]
fn full_suite_reports_no_blocked_scenes() {
    let suite = voxcell_uphill::run_suite();
    let gaps = suite.gaps();
    assert!(gaps.is_empty(), "blocked scenes (sibling mid-edit views?): {gaps:?}");
    assert!(suite.scenes.len() == 8);
    assert!((0.0..=100.0).contains(&suite.total_score));
    eprintln!("[uphill] suite score {:.2}/100 breakdown={}", suite.total_score, suite.breakdown.to_json());
    for s in &suite.scenes {
        if let Some(p) = &s.perf {
            eprintln!("[uphill] {} frame p95={:.2}ms budget-ok={}", s.scene, p.p95_ms, p.within_budget);
        }
        if !s.contrast.below_45.is_empty() {
            eprintln!("[uphill] {} contrast<4.5 (AA normal) count={}", s.scene, s.contrast.below_45.len());
        }
        for g in &s.a11y.known_gaps {
            eprintln!("[uphill] {} KNOWN-GAP {g}", s.scene);
        }
        for g in &s.contrast.known_gaps {
            eprintln!("[uphill] {} KNOWN-CONTRAST {g}", s.scene);
        }
    }
}

// ── ported scoring math (golden values computed from the TS formulas) ──────

#[test]
fn jerk_metrics_port_matches_ts_formula() {
    let series = [0.5, 1.0, 1.5, 2.0, 1.0, 0.5, 1.0, 1.5, 0.0, 0.05, 12.0, 1.0];
    let m = motion::jerk_metrics(&series);
    assert!((m.mean - 1.838).abs() < 0.002, "mean {}", m.mean);
    assert!((m.max - 12.0).abs() < 1e-9);
    assert!((m.p95 - 12.0).abs() < 1e-9);
    assert!((m.dead_ratio - 0.167).abs() < 0.002, "dead {}", m.dead_ratio);
    assert!((m.spike_ratio - 0.083).abs() < 0.002, "spike {}", m.spike_ratio);
    assert!((m.jerk_ratio - 4.482).abs() < 0.01, "jerk ratio {}", m.jerk_ratio);
    // Short series degrade exactly like the TS helper.
    let short = motion::jerk_metrics(&[1.0, 2.0]);
    assert_eq!(short.dead_ratio, 1.0);
    assert_eq!(short.mean, 0.0);
}

#[test]
fn signature_and_cell_diff_ports() {
    assert_eq!(motion::line_signature_diff("abc", "abc"), 0.0);
    assert!((motion::line_signature_diff("abc", "abd") - 1.0 / 3.0).abs() < 1e-9);
    assert_eq!(motion::line_signature_diff("", ""), 0.0);
    let a = vec![0.0_f32, 0.5, 1.0];
    let b = vec![0.0_f32, 0.5, 0.9];
    // 0..1 fields normalize to the TS byte scale: |0.1|*255/3 = 8.5
    assert!((motion::cells_mean_abs_diff(&a, &b) - 8.5).abs() < 1e-4);
    assert_eq!(motion::cells_mean_abs_diff(&a, &a), 0.0);
}

#[test]
fn continuity_and_breakdown_ranges() {
    let idle = motion::MotionReport {
        frames: 120, speaking: false,
        cell_diffs: motion::jerk_metrics(&[1.0; 10]),
        braille_diff_mean: 0.0, braille_diff_max: 0.0,
        rect_delta_mean: 0.0, rect_delta_max: 0.0,
        blink_max: 0.9, half_lid_frames: 6, blink_asymmetric: true,
        hair_range: 0.03, max_hair_lag_delta: 0.01, max_mouth_jump: 0.05,
        visemes_hit: 0, gaze_fixation_share: 0.5, gaze_kurtosis_proxy: 3.0,
        moving: true, continuity_score: None, expression_score: None,
    };
    let speak = motion::MotionReport { speaking: true, hair_range: 0.04, max_hair_lag_delta: 0.01, ..idle.clone() };
    let c = motion::continuity_score(&idle, &speak);
    assert!((0.0..=20.0).contains(&c));
    // Perfect metrics land near the top of the range.
    assert!(c > 14.0, "clean traces score high: {c}");

    let clean = voxcell_uphill::SceneReport {
        scene: "idle".into(), status: "ok".into(), gap: None, viewport: (1280.0, 800.0),
        a11y: Default::default(), clipping: Default::default(), contrast: Default::default(),
        popover: None, motion: None, perf: None, copy: Default::default(),
        reduced_motion_freeze: Some(true), boot_ms: 120.0, audit_ms: 900.0,
        overlay_ok: Some(true), portrait_rendered: None,
    };
    let b = scoring::score_suite(&[clean.clone()]);
    assert!(b.layout_integrity_25 == 25.0);
    assert!(b.status_and_copy_15 == 15.0);
    assert!(b.zero_console_errors_5 == 5.0);
    assert!(b.total() <= 100.0 && b.total() > 0.0);
    // A blocked scene costs console-error points exactly like the TS port.
    let blocked = voxcell_uphill::SceneReport::blocked("thread-split", (1280.0, 800.0), "panic".into());
    let b2 = scoring::score_suite(&[clean, blocked]);
    assert!(b2.zero_console_errors_5 == 3.0);
}

#[test]
fn popover_audit_reports_missing_dot_as_violation() {
    let spec = scenes::spec_by_name("canvas").unwrap();
    let mut rt = scenes::boot(spec);
    rt.frame(0.016);
    let r = audit::audit_popover_at(&mut rt, "dot-does-not-exist", 50.0);
    assert!(!r.edge_safe());
    assert!(!r.violations.is_empty());
}
