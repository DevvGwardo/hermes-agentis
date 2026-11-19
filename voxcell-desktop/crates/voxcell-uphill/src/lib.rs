//! Headless uphill harness (spec §9, ticket P4-04).
//!
//! Runs the Voxcell scenes (idle, working, attention, speaking, canvas,
//! thread split/overlay, phone) through rune's headless Runtime and asserts:
//! AccessKit node completeness, zero clipped text (headless equivalent of
//! `scroll_w <= client_w`), edge-safe Canvas popovers, inter-frame character
//! motion deltas, and the <4 ms/frame 120 Hz render budget. Ports the
//! `tools/blueprint/` uphill frame-continuity / contrast scoring to Rust.
//!
//! Owned by the uphill workstream — see docs/RUNE_DESKTOP_SPEC.md.

pub mod audit;
pub mod mock_gateway;
pub mod motion;
pub mod perf;
pub mod scenes;
pub mod scoring;

use serde_json::{Value, json};

/// Hard render budget from spec §9 / P4-04: 120 Hz => <4 ms per frame.
pub const FRAME_BUDGET_MS: f64 = 4.0;

/// One audited scene run. `status` is "ok", or "blocked" with the panic /
/// gap reason when a sibling workstream's mid-edit view prevents the audit
/// (recorded, never silently skipped).
#[derive(Clone, Debug)]
pub struct SceneReport {
    pub scene: String,
    pub status: String,
    pub gap: Option<String>,
    pub viewport: (f32, f32),
    pub a11y: audit::A11yReport,
    pub clipping: audit::ClipReport,
    pub contrast: audit::ContrastReport,
    pub popover: Option<audit::PopoverReport>,
    pub motion: Option<motion::MotionReport>,
    pub perf: Option<perf::PerfReport>,
    pub copy: audit::CopyReport,
    pub reduced_motion_freeze: Option<bool>,
    /// Wall time to boot the scene (first frame), ms — headless `switchMs`.
    pub boot_ms: f64,
    /// Wall time of the whole audit pass, ms — headless `screenshotMs`.
    pub audit_ms: f64,
    /// Settings dialog (main) / task overlay (thread) opened cleanly.
    pub overlay_ok: Option<bool>,
    /// Phone scene: portrait box rendered at usable size.
    pub portrait_rendered: Option<bool>,
}

impl SceneReport {
    pub fn blocked(scene: &str, viewport: (f32, f32), reason: String) -> Self {
        Self {
            scene: scene.into(),
            status: "blocked".into(),
            gap: Some(reason),
            viewport,
            a11y: Default::default(),
            clipping: Default::default(),
            contrast: Default::default(),
            popover: None,
            motion: None,
            perf: None,
            copy: Default::default(),
            reduced_motion_freeze: None,
            boot_ms: 0.0,
            audit_ms: 0.0,
            overlay_ok: None,
            portrait_rendered: None,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "scene": self.scene,
            "status": self.status,
            "gap": self.gap,
            "viewport": [self.viewport.0, self.viewport.1],
            "a11y": self.a11y.to_json(),
            "clipping": self.clipping.to_json(),
            "contrast": self.contrast.to_json(),
            "popover": self.popover.as_ref().map(|p| p.to_json()),
            "motion": self.motion.as_ref().map(|m| m.to_json()),
            "perf": self.perf.as_ref().map(|p| p.to_json()),
            "copy": self.copy.to_json(),
            "reducedMotionFreeze": self.reduced_motion_freeze,
            "bootMs": (self.boot_ms * 100.0).round() / 100.0,
            "auditMs": (self.audit_ms * 100.0).round() / 100.0,
            "overlayOk": self.overlay_ok,
            "portraitRendered": self.portrait_rendered,
        })
    }
}

/// Full suite report: per-scene audits + the ported blueprint breakdown.
#[derive(Clone, Debug)]
pub struct SuiteReport {
    pub scenes: Vec<SceneReport>,
    pub breakdown: scoring::UiBreakdown,
    pub total_score: f64,
    pub max_score: f64,
}

impl SuiteReport {
    pub fn to_json(&self) -> Value {
        json!({
            "label": "voxcell-uphill",
            "totalScore": self.total_score,
            "maxScore": self.max_score,
            "breakdown": self.breakdown.to_json(),
            "scenes": self.scenes.iter().map(|s| s.to_json()).collect::<Vec<_>>(),
        })
    }

    /// Scenes that could not be audited (sibling mid-edit breakage etc.).
    pub fn gaps(&self) -> Vec<(String, String)> {
        self.scenes
            .iter()
            .filter(|s| s.status != "ok")
            .map(|s| (s.scene.clone(), s.gap.clone().unwrap_or_default()))
            .collect()
    }
}

/// Run every scene through the headless Runtime, auditing each. A panicking
/// scene (e.g. a sibling's mid-edit view) is caught and recorded as a gap so
/// the rest of the suite still produces evidence.
pub fn run_suite() -> SuiteReport {
    let mut scenes = Vec::new();
    for spec in scenes::SCENES {
        let report = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scenes::run_scene(spec)
        }))
        .unwrap_or_else(|e| {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown panic".into());
            SceneReport::blocked(spec.name, spec.size, msg)
        });
        scenes.push(report);
    }
    let breakdown = scoring::score_suite(&scenes);
    let total_score = breakdown.total();
    SuiteReport { scenes, breakdown, total_score, max_score: 100.0 }
}
