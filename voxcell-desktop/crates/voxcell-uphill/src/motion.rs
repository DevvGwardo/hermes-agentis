//! Inter-frame character motion deltas + the ported `tools/blueprint/
//! eval-uphill.ts` frame-continuity / expression scoring (jerk metrics,
//! blink envelope, hair lag, gaze fixation, viseme coverage).

use rune::Runtime;
use serde_json::{Value, json};
use voxcell_halftone::{portrait_assets, sample_portrait_luminance_field};
use voxcell_ui::AppState;
use voxcell_ui::views::home::stage_expression;

// ── metric ports (eval-uphill.ts) ──────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default)]
pub struct JerkMetrics {
    pub mean: f64,
    pub max: f64,
    pub p95: f64,
    pub jerk_ratio: f64,
    pub dead_ratio: f64,
    pub spike_ratio: f64,
}

impl JerkMetrics {
    pub fn to_json(&self) -> Value {
        json!({
            "mean": round3(self.mean), "max": round3(self.max), "p95": round3(self.p95),
            "jerkRatio": round3(self.jerk_ratio), "deadRatio": round3(self.dead_ratio),
            "spikeRatio": round3(self.spike_ratio),
        })
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Port of `computeJerkMetrics`: RMS second difference / mean, plus the
/// dead-frame (<0.08) and spike (>max(8, mean*3.2)) shares.
pub fn jerk_metrics(series: &[f64]) -> JerkMetrics {
    if series.len() < 3 {
        return JerkMetrics { dead_ratio: 1.0, ..Default::default() };
    }
    let mut sorted = series.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = series.iter().sum::<f64>() / series.len() as f64;
    let max = *sorted.last().unwrap();
    let p95 = sorted[((series.len() as f64 * 0.95).floor() as usize).min(sorted.len() - 1)];
    let mut second_diff_sq = 0.0;
    for i in 2..series.len() {
        let d2 = series[i] - 2.0 * series[i - 1] + series[i - 2];
        second_diff_sq += d2 * d2;
    }
    let rms_jerk = (second_diff_sq / (series.len() - 2) as f64).sqrt();
    let jerk_ratio = if mean > 1e-4 { rms_jerk / mean } else { 10.0 };
    let dead = series.iter().filter(|&&v| v < 0.08).count();
    let spike = series.iter().filter(|&&v| v > 8.0_f64.max(mean * 3.2)).count();
    JerkMetrics {
        mean,
        max,
        p95,
        jerk_ratio,
        dead_ratio: dead as f64 / series.len() as f64,
        spike_ratio: spike as f64 / series.len() as f64,
    }
}

/// Port of `lineSignatureDiff`: share of differing character cells.
pub fn line_signature_diff(a: &str, b: &str) -> f64 {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let mut diff = 0usize;
    let mut total = 0usize;
    for r in 0..ac.len().max(bc.len()) {
        let la = ac.get(r).copied().unwrap_or(' ');
        let lb = bc.get(r).copied().unwrap_or(' ');
        if la != ' ' || lb != ' ' {
            total += 1;
            if la != lb {
                diff += 1;
            }
        }
    }
    diff as f64 / total.max(1) as f64
}

/// Port of `cellsMeanAbsDiff` over the luminance field (normalized to the
/// TS 0..255 byte scale when the field arrives as 0..1 floats).
pub fn cells_mean_abs_diff(a: &[f32], b: &[f32]) -> f64 {
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }
    let scale = if a.iter().chain(b.iter()).all(|&v| v <= 1.0) { 255.0 } else { 1.0 };
    let sum: f64 = (0..len).map(|i| ((a[i] - b[i]) * scale).abs() as f64).sum();
    sum / len as f64
}

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

// ── character trace collection ─────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct MotionReport {
    pub frames: usize,
    pub speaking: bool,
    pub cell_diffs: JerkMetrics,
    pub braille_diff_mean: f64,
    pub braille_diff_max: f64,
    pub rect_delta_mean: f64,
    pub rect_delta_max: f64,
    pub blink_max: f32,
    pub half_lid_frames: usize,
    pub blink_asymmetric: bool,
    pub hair_range: f32,
    pub max_hair_lag_delta: f32,
    pub max_mouth_jump: f32,
    pub visemes_hit: usize,
    pub gaze_fixation_share: f64,
    pub gaze_kurtosis_proxy: f64,
    /// Any live character motion detected (cells, braille text, or rects).
    pub moving: bool,
    /// Ported continuity sub-score (max 20) — idle+speak traces combined.
    pub continuity_score: Option<f64>,
    /// Ported expression sub-score (max 15).
    pub expression_score: Option<f64>,
}

impl MotionReport {
    pub fn to_json(&self) -> Value {
        json!({
            "frames": self.frames,
            "speaking": self.speaking,
            "cellDiffs": self.cell_diffs.to_json(),
            "brailleDiffMean": round3(self.braille_diff_mean),
            "brailleDiffMax": round3(self.braille_diff_max),
            "rectDeltaMean": round3(self.rect_delta_mean),
            "rectDeltaMax": round3(self.rect_delta_max),
            "blinkMax": self.blink_max,
            "halfLidFrames": self.half_lid_frames,
            "blinkAsymmetric": self.blink_asymmetric,
            "hairRange": self.hair_range,
            "maxHairLagDelta": self.max_hair_lag_delta,
            "maxMouthJump": self.max_mouth_jump,
            "visemesHit": self.visemes_hit,
            "gazeFixationShare": round3(self.gaze_fixation_share),
            "gazeKurtosisProxy": round3(self.gaze_kurtosis_proxy),
            "moving": self.moving,
            "continuityScore": self.continuity_score,
            "expressionScore": self.expression_score,
        })
    }
}

const PORTRAIT_KEYS: [&str; 3] =
    ["home-portrait-stage", "home-portrait-pixels", "home-portrait-gpu"];

fn braille_signature(rt: &Runtime<AppState>) -> String {
    let mut parts: Vec<(String, String)> = Vec::new();
    for e in rt.elements() {
        if let Some(t) = e.text.as_deref() {
            if t.chars().any(|c| ('\u{2800}'..='\u{28FF}').contains(&c)) {
                parts.push((e.key.clone().unwrap_or_else(|| e.path.clone()), t.to_string()));
            }
        }
    }
    parts.sort();
    parts.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("\n")
}

fn portrait_rects(rt: &Runtime<AppState>) -> Vec<[f32; 4]> {
    rt.elements()
        .into_iter()
        .filter(|e| {
            e.key
                .as_deref()
                .map(|k| PORTRAIT_KEYS.contains(&k) || k.starts_with("sidebar-char"))
                .unwrap_or(false)
        })
        .map(|e| e.target)
        .collect()
}

/// Drive `frames` frames at `dt`, stepping the real animation pipeline and
/// (for speaking traces) the TS energy curve, collecting inter-frame deltas.
pub fn collect_trace(rt: &mut Runtime<AppState>, frames: usize, dt: f64, speaking: bool) -> MotionReport {
    let mut cell_diffs: Vec<f64> = Vec::new();
    let mut braille_diffs: Vec<f64> = Vec::new();
    let mut rect_deltas: Vec<f64> = Vec::new();
    let mut blinks: Vec<f32> = Vec::new();
    let mut gaze: Vec<(f32, f32)> = Vec::new();
    let mut hair: Vec<f32> = Vec::new();
    let mut mouth: Vec<f32> = Vec::new();
    let mut visemes: Vec<voxcell_core::MouthViseme> = Vec::new();
    let mut prev_cells: Option<Vec<f32>> = None;
    let mut prev_braille = String::new();
    let mut prev_rects: Vec<[f32; 4]> = Vec::new();

    for i in 0..frames {
        let t = i as f64 * dt;
        rt.update(|s, _tx| {
            s.time_s = t as f32;
            if speaking {
                // TS speech energy curve (eval-uphill.ts evaluateMainPortrait).
                let amp = 0.24 + 0.66 * (t * 7.4).sin().abs() * (0.65 + 0.35 * (t * 3.1).cos());
                s.audio.is_speaking = i >= 6 && i < frames.saturating_sub(18);
                s.audio.output_level = amp as f32;
            }
            s.step_animations(dt as f32);
        });
        rt.frame(dt);

        let expr = stage_expression(&rt.state);
        let cells = sample_portrait_luminance_field(
            62, 28, s_time(&rt.state), expr.mood, expr.viseme, expr.energy,
            expr.is_speaking, expr.is_listening, rt.state.stage_anim.state(), portrait_assets(),
        );
        if let Some(prev) = &prev_cells {
            cell_diffs.push(cells_mean_abs_diff(prev, &cells));
        }
        prev_cells = Some(cells);

        let sig = braille_signature(rt);
        if i > 0 {
            braille_diffs.push(line_signature_diff(&prev_braille, &sig));
        }
        prev_braille = sig;

        let rects = portrait_rects(rt);
        if i > 0 && !rects.is_empty() && rects.len() == prev_rects.len() {
            let d: f64 = rects
                .iter()
                .zip(prev_rects.iter())
                .map(|(a, b)| {
                    ((a[0] - b[0]).abs() + (a[1] - b[1]).abs()
                        + (a[2] - b[2]).abs() + (a[3] - b[3]).abs()) as f64
                })
                .sum::<f64>()
                / rects.len() as f64;
            rect_deltas.push(d);
        }
        prev_rects = rects;

        let m = rt.state.stage_anim.state();
        blinks.push(m.blink);
        gaze.push((m.gaze_x, m.gaze_y));
        hair.push(m.hair_lag_x);
        mouth.push(m.mouth_open);
        if expr.is_speaking {
            visemes.push(expr.viseme);
        }
    }

    // Blink envelope & half-lid continuity (TS port).
    let half_lid_frames = blinks.iter().filter(|&&b| b > 0.14 && b < 0.86).count();
    let blink_max = blinks.iter().copied().fold(0.0_f32, f32::max);
    let mut close_slope = 0.0_f32;
    let mut open_slope = 0.0_f32;
    for w in blinks.windows(2) {
        let d = w[1] - w[0];
        if d > close_slope { close_slope = d; }
        if -d > open_slope { open_slope = -d; }
    }
    let blink_asymmetric = close_slope > open_slope * 1.08;

    let hair_range = hair.iter().copied().fold(f32::NEG_INFINITY, f32::max)
        - hair.iter().copied().fold(f32::INFINITY, f32::min);
    let max_hair_lag_delta = hair.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0_f32, f32::max);
    let max_mouth_jump = mouth.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0_f32, f32::max);

    // Gaze fixation vs saccade kurtosis (TS port).
    let gaze_speeds: Vec<f64> = gaze
        .windows(2)
        .map(|w| (((w[1].0 - w[0].0) as f64).powi(2) + ((w[1].1 - w[0].1) as f64).powi(2)).sqrt() / dt)
        .collect();
    let mut sorted_gaze = gaze_speeds.clone();
    sorted_gaze.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted_gaze.get(sorted_gaze.len() / 2).copied().unwrap_or(0.001);
    let max_speed = sorted_gaze.last().copied().unwrap_or(0.0);
    let gaze_kurtosis_proxy = max_speed / 0.02_f64.max(median);
    let gaze_fixation_share = if gaze_speeds.is_empty() {
        0.0
    } else {
        gaze_speeds.iter().filter(|&&v| v < 0.14).count() as f64 / gaze_speeds.len() as f64
    };

    visemes.sort_by_key(|v| format!("{v:?}"));
    visemes.dedup_by_key(|v| format!("{v:?}"));

    let cell_jerk = jerk_metrics(&cell_diffs);
    let braille_mean = braille_diffs.iter().sum::<f64>() / braille_diffs.len().max(1) as f64;
    let braille_max = braille_diffs.iter().copied().fold(0.0_f64, f64::max);
    let rect_mean = rect_deltas.iter().sum::<f64>() / rect_deltas.len().max(1) as f64;
    let rect_max = rect_deltas.iter().copied().fold(0.0_f64, f64::max);
    let moving = cell_jerk.mean > 0.01 || braille_max > 0.0 || rect_max > 0.01;

    // Expression sub-score (max 15) — only meaningful for speaking traces.
    let expression_score = if speaking {
        let viseme_range = clamp01(visemes.len() as f64 / 6.0) * 5.0;
        let mouth_smooth = clamp01(1.0 - ((max_mouth_jump as f64 - 0.12).max(0.0)) / 0.25) * 4.0;
        let gaze_fix = clamp01(gaze_fixation_share / 0.35) * 3.0
            + clamp01((gaze_kurtosis_proxy - 1.5) / 1.8) * 3.0;
        Some((viseme_range + mouth_smooth + gaze_fix).clamp(0.0, 15.0))
    } else {
        None
    };

    MotionReport {
        frames,
        speaking,
        cell_diffs: cell_jerk,
        braille_diff_mean: braille_mean,
        braille_diff_max: braille_max,
        rect_delta_mean: rect_mean,
        rect_delta_max: rect_max,
        blink_max,
        half_lid_frames,
        blink_asymmetric,
        hair_range: if hair_range.is_finite() { hair_range } else { 0.0 },
        max_hair_lag_delta,
        max_mouth_jump,
        visemes_hit: visemes.len(),
        gaze_fixation_share,
        gaze_kurtosis_proxy,
        moving,
        continuity_score: None,
        expression_score,
    }
}

fn s_time(s: &AppState) -> f32 {
    s.time_s
}

/// Ported continuity sub-score (max 20) from an idle + speaking trace pair:
/// idleSmooth + speakSmooth + blink + hair (eval-uphill.ts formulas).
pub fn continuity_score(idle: &MotionReport, speak: &MotionReport) -> f64 {
    let ij = &idle.cell_diffs;
    let sj = &speak.cell_diffs;
    let idle_smooth = clamp01(1.0 - (ij.jerk_ratio - 0.35).max(0.0) / 1.1) * 5.0
        * (1.0 - ij.dead_ratio * 0.8);
    let speak_smooth = clamp01(1.0 - (sj.jerk_ratio - 0.35).max(0.0) / 1.1) * 5.0
        * (1.0 - sj.spike_ratio * 2.0);
    let blink = (if idle.blink_max > 0.75 { 2.0 } else { 0.5 })
        + clamp01(idle.half_lid_frames as f64 / 4.0) * 2.0
        + (if idle.blink_asymmetric { 1.5 } else { 0.3 });
    let hair = clamp01(speak.hair_range as f64 / 0.025) * 2.5
        + clamp01(1.0 - ((speak.max_hair_lag_delta as f64 - 0.015).max(0.0)) / 0.03) * 2.0;
    (idle_smooth + speak_smooth + blink + hair).clamp(0.0, 20.0)
}
