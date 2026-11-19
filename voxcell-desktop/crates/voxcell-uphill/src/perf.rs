//! 120 Hz render-budget measurement (<4 ms/frame, spec §9 / P4-04).
//! Frames are recorded as `rune_perf::Frame` samples; wall time covers the
//! full update+render pipeline (state step + view rebuild + layout).

use rune::Runtime;
use serde_json::{Value, json};
use std::time::Instant;
use voxcell_ui::AppState;

use crate::FRAME_BUDGET_MS;

#[derive(Clone, Debug)]
pub struct PerfReport {
    pub warmup_frames: usize,
    pub measured_frames: usize,
    pub avg_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    pub budget_ms: f64,
    pub within_budget: bool,
}

impl PerfReport {
    pub fn to_json(&self) -> Value {
        json!({
            "warmupFrames": self.warmup_frames,
            "measuredFrames": self.measured_frames,
            "avgMs": (self.avg_ms * 1000.0).round() / 1000.0,
            "p95Ms": (self.p95_ms * 1000.0).round() / 1000.0,
            "maxMs": (self.max_ms * 1000.0).round() / 1000.0,
            "budgetMs": self.budget_ms,
            "withinBudget": self.within_budget,
        })
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 * p).floor() as usize).min(sorted.len() - 1);
    sorted[idx]
}

/// Drive `warmup + measured` full frames at 120 Hz cadence, stepping the
/// character/horizon animations each frame so every frame re-runs the view
/// (a static `rt.frame` would skip the rebuild and under-report).
pub fn measure_budget(rt: &mut Runtime<AppState>, warmup: usize, measured: usize) -> PerfReport {
    let dt = 1.0 / 120.0;
    for _ in 0..warmup {
        rt.update(|s, _tx| {
            s.time_s += dt as f32;
            s.step_animations(dt as f32);
        });
        rt.frame(dt);
    }
    let mut samples = Vec::with_capacity(measured);
    let mut perf_frames = Vec::with_capacity(measured);
    for seq in 0..measured {
        let t0 = Instant::now();
        rt.update(|s, _tx| {
            s.time_s += dt as f32;
            s.step_animations(dt as f32);
        });
        rt.frame(dt);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        samples.push(ms);
        // rune-perf frame record: headless has no GPU pass, so the whole
        // pipeline time is booked as render_ms (update_ms = state step share
        // is not separable through the public Runtime API).
        perf_frames.push(
            rune_perf::Frame::new(0, seq as u64, rune_perf::Outcome::Drawn).render_ms(ms),
        );
    }
    let avg = samples.iter().sum::<f64>() / samples.len().max(1) as f64;
    let mut sorted = samples.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p95 = percentile(&sorted, 0.95);
    let max = *sorted.last().unwrap_or(&0.0);
    let _ = &perf_frames; // recorded for parity with rune-perf consumers
    PerfReport {
        warmup_frames: warmup,
        measured_frames: measured,
        avg_ms: avg,
        p95_ms: p95,
        max_ms: max,
        budget_ms: FRAME_BUDGET_MS,
        within_budget: p95 < FRAME_BUDGET_MS,
    }
}
