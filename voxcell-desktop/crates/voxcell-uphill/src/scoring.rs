//! Port of `tools/blueprint/eval-ui-uphill.ts::scoreUiReport` to headless
//! scene reports. Input mapping (documented deviations from the DOM audit):
//! - `switchMs`      -> scene boot wall time (first frame incl. layout).
//! - `screenshotMs`  -> full audit pass wall time.
//! - `clippedButtons`/`wrappedNavItems` -> horizontally clipped text nodes
//!   and nav tabs taller than 44pt.
//! - `canvasMoved`   -> character/braille/rect motion detected in the trace.
//! - `backdrop-blur-2xl` -> elements with heavy blur culling margin (>20pt).
//! - console/page errors -> blocked (panicking) scenes.

use serde_json::{Value, json};

use crate::SceneReport;

#[derive(Clone, Copy, Debug, Default)]
pub struct UiBreakdown {
    pub layout_integrity_25: f64,
    pub paint_and_latency_20: f64,
    pub status_and_copy_15: f64,
    pub live_motion_and_reduced_20: f64,
    pub mobile_and_overlay_fidelity_15: f64,
    pub zero_console_errors_5: f64,
}

fn clamp(v: f64, min: f64, max: f64) -> f64 {
    v.clamp(min, max)
}
fn clamp01(v: f64) -> f64 {
    clamp(v, 0.0, 1.0)
}
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

impl UiBreakdown {
    pub fn total(&self) -> f64 {
        round2(
            self.layout_integrity_25
                + self.paint_and_latency_20
                + self.status_and_copy_15
                + self.live_motion_and_reduced_20
                + self.mobile_and_overlay_fidelity_15
                + self.zero_console_errors_5,
        )
    }
    pub fn to_json(&self) -> Value {
        json!({
            "layoutIntegrity_25": self.layout_integrity_25,
            "paintAndLatency_20": self.paint_and_latency_20,
            "statusAndCopy_15": self.status_and_copy_15,
            "liveMotionAndReduced_20": self.live_motion_and_reduced_20,
            "mobileAndOverlayFidelity_15": self.mobile_and_overlay_fidelity_15,
            "zeroConsoleErrors_5": self.zero_console_errors_5,
        })
    }
}

pub fn score_suite(scenes: &[SceneReport]) -> UiBreakdown {
    let ok: Vec<&SceneReport> = scenes.iter().filter(|s| s.status == "ok").collect();

    // layoutIntegrity (max 25): overflow px, clipped text, empty panes.
    let max_overflow = ok
        .iter()
        .map(|s| {
            s.clipping
                .clipped
                .iter()
                .filter_map(|c| c.split("rect_w=").nth(1))
                .filter_map(|r| r.split(' ').next()?.parse::<f64>().ok())
                .fold(0.0_f64, f64::max)
        })
        .fold(0.0_f64, f64::max);
    let total_clipped: usize = ok.iter().map(|s| s.clipping.clipped.len()).sum();
    let empty_count = ok.iter().filter(|s| s.copy.empty_main_pane).count();
    let layout_integrity_25 = round2(clamp(
        25.0 - (max_overflow * 0.5).min(12.0) - (total_clipped as f64 * 2.5).min(8.0)
            - empty_count as f64 * 6.0,
        0.0,
        25.0,
    ));

    // paintAndLatency (max 20): boot + audit wall times, TS curves.
    let avg_switch = ok.iter().map(|s| s.boot_ms).sum::<f64>() / ok.len().max(1) as f64;
    let max_shot = ok.iter().map(|s| s.audit_ms).fold(0.0_f64, f64::max);
    let switch_score = clamp01((900.0 - avg_switch) / (900.0 - 300.0)) * 10.0;
    let shot_score = clamp01((10000.0 - max_shot) / (10000.0 - 1500.0)) * 10.0;
    let paint_and_latency_20 = round2(clamp(switch_score + shot_score, 0.0, 20.0));

    // statusAndCopy (max 15).
    let calibrating_hits = ok.iter().filter(|s| s.copy.stuck_calibrating).count();
    let singular_hits = ok.iter().filter(|s| s.copy.singular_gate_text).count();
    let status_and_copy_15 = round2(clamp(
        15.0 - calibrating_hits as f64 * 3.5 - singular_hits as f64 * 3.5,
        0.0,
        15.0,
    ));

    // liveMotionAndReduced (max 20).
    let traced: Vec<&&SceneReport> = ok.iter().filter(|s| s.motion.is_some()).collect();
    let moving_ratio = if traced.is_empty() {
        1.0
    } else {
        traced.iter().filter(|s| s.motion.as_ref().map(|m| m.moving).unwrap_or(false)).count() as f64
            / traced.len() as f64
    };
    let reduced_bonus = if ok.iter().any(|s| s.reduced_motion_freeze == Some(true)) { 4.0 } else { 0.0 };
    let live_motion_and_reduced_20 =
        round2(clamp(moving_ratio * 16.0 + reduced_bonus, 0.0, 20.0));

    // mobileAndOverlayFidelity (max 15): studio/dialog open, overlay drawer,
    // phone cleanliness (overflow, truncation, portrait).
    let studio_ok = ok.iter().any(|s| s.overlay_ok == Some(true));
    let phone = ok.iter().find(|s| s.scene == "phone");
    let mobile_clean = match phone {
        Some(p) => {
            (if p.clipping.clean() { 2.0 } else { 0.0 })
                + (if p.clipping.clean() { 2.0 } else { 0.0 })
                + (if p.portrait_rendered == Some(true) { 2.0 } else { 0.0 })
        }
        None => 0.0,
    };
    let mobile_and_overlay_fidelity_15 = round2(clamp(
        (if studio_ok { 6.0 } else { 0.0 }) + (if studio_ok { 3.0 } else { 0.0 }) + mobile_clean,
        0.0,
        15.0,
    ));

    // zeroConsoleErrors (max 5): blocked scenes are the headless panics.
    let err_count = scenes.iter().filter(|s| s.status != "ok").count();
    let zero_console_errors_5 = if err_count == 0 {
        5.0
    } else {
        round2((5.0 - err_count as f64 * 2.0).max(0.0))
    };

    UiBreakdown {
        layout_integrity_25,
        paint_and_latency_20,
        status_and_copy_15,
        live_motion_and_reduced_20,
        mobile_and_overlay_fidelity_15,
        zero_console_errors_5,
    }
}
