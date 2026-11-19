//! Acoustic Braille horizon — port of `src/utils/brailleFieldEngine.ts`.
//!
//! 12-band FFT-driven oscillographic wave field: high-Q dual-trace carrier harmonics,
//! interference nodes, quantized sample stems, optical graticule crosshairs, lower
//! spectrum needles, cursor phase-lock, and Silk/Peaks/Pulse/Ribbon styles.

use crate::portrait::{clamp01, smoothstep};
use crate::raster::{
    BAYER_8X8, BrailleFrameGrid, DotGeometry, TonalTier, grid_to_tonal_dots, mix_rgb,
    parse_hex_rgb, rasterize_tonal_dots_rgba, HORIZON_TIER_THRESHOLDS,
};
use voxcell_core::{DotProfile, HorizonMode};

#[derive(Clone, Debug)]
pub struct HorizonMotionState {
    pub energy: f32,
    pub pointer_x: f32,
    pub pointer_y: f32,
    pub bands: [f32; 12],
    pub pointer_vel_x: f32,
    pub wake_phase: f32,
}

impl Default for HorizonMotionState {
    fn default() -> Self {
        Self {
            energy: 0.08,
            pointer_x: 0.0,
            pointer_y: 0.0,
            bands: [0.0; 12],
            pointer_vel_x: 0.0,
            wake_phase: 0.0,
        }
    }
}

/// Stateful spring-damped filter for horizon energy, 12-band FFT levels, and pointer
/// interaction with directional wake tracking (matches `createHorizonAnimator`).
#[derive(Clone, Debug)]
pub struct HorizonAnimator {
    state: HorizonMotionState,
    energy_vel: f32,
    pointer_vel_x: f32,
    wake_phase: f32,
}

impl Default for HorizonAnimator {
    fn default() -> Self {
        Self::new()
    }
}

impl HorizonAnimator {
    pub fn new() -> Self {
        Self { state: HorizonMotionState::default(), energy_vel: 0.0, pointer_vel_x: 0.0, wake_phase: 0.0 }
    }

    pub fn state(&self) -> &HorizonMotionState {
        &self.state
    }

    pub fn step(
        &mut self,
        energy: f32,
        pointer_x: f32,
        pointer_y: f32,
        bands: &[f32; 12],
        dt: f32,
        reduced_motion: bool,
    ) -> &HorizonMotionState {
        let clamped_dt = dt.clamp(0.004, 0.06);
        if reduced_motion {
            self.state.energy = clamp01(energy * 0.4);
            self.state.pointer_x = pointer_x.clamp(-1.0, 1.0);
            self.state.pointer_y = pointer_y.clamp(-1.0, 1.0);
            self.state.pointer_vel_x = 0.0;
            for i in 0..12 {
                self.state.bands[i] = clamp01(bands[i] * 0.5);
            }
            return &self.state;
        }

        // Critically-damped 2nd-order energy filter: fluid takeoff & release.
        let target_energy = clamp01(energy);
        let (e_stiff, e_damp) = if target_energy > self.state.energy { (68.0, 17.5) } else { (42.0, 13.5) };
        self.energy_vel += ((target_energy - self.state.energy) * e_stiff - self.energy_vel * e_damp) * clamped_dt;
        self.state.energy = clamp01(self.state.energy + self.energy_vel * clamped_dt);

        // Exponential pointer smoothing + damped horizontal wake velocity.
        let prev_ptr_x = self.state.pointer_x;
        let ptr_factor = 1.0 - (-9.5 * clamped_dt).exp();
        self.state.pointer_x += (pointer_x.clamp(-1.0, 1.0) - self.state.pointer_x) * ptr_factor;
        self.state.pointer_y += (pointer_y.clamp(-1.0, 1.0) - self.state.pointer_y) * ptr_factor;

        let raw_vel_x = ((self.state.pointer_x - prev_ptr_x) / clamped_dt).clamp(-3.5, 3.5);
        let vel_factor = 1.0 - (-7.0 * clamped_dt).exp();
        self.pointer_vel_x += (raw_vel_x - self.pointer_vel_x) * vel_factor;
        self.wake_phase += clamped_dt * (1.2 + self.pointer_vel_x.abs() * 2.4 + self.state.energy * 1.4);
        self.state.pointer_vel_x = self.pointer_vel_x;
        self.state.wake_phase = self.wake_phase;

        // Fast-attack / graceful-release band filtering.
        let attack = 1.0 - (-9.5 * clamped_dt).exp();
        let release = 1.0 - (-6.0 * clamped_dt).exp();
        for i in 0..12 {
            let target = clamp01(bands[i]);
            let f = if target > self.state.bands[i] { attack } else { release };
            self.state.bands[i] += (target - self.state.bands[i]) * f;
        }

        &self.state
    }
}

/// Renders the technical 2x4 Braille oscillographic & telemetry wave field into a grid.
pub fn sample_horizon_grid(
    cols: usize,
    rows: usize,
    time: f32,
    motion: &HorizonMotionState,
    style: HorizonMode,
) -> BrailleFrameGrid {
    let sub_w = cols * 2;
    let sub_h = rows * 4;
    let mut sub_lum = vec![0.0_f32; sub_w * sub_h];

    let energy = motion.energy;
    let bands = &motion.bands;
    let band_len = bands.len();
    let px_target = (motion.pointer_x + 1.0) * 0.5;
    let py_boost = (1.0 - clamp01((motion.pointer_y + 1.0) * 0.5)) * 0.06;
    let voice_amp = 0.065 + energy * 0.19;
    let inv_sub_w = 1.0 / sub_w.max(1) as f32;
    let inv_sub_h = 1.0 / sub_h.max(1) as f32;
    let cursor_weight = 0.09 + py_boost;
    let wake_amp = (motion.pointer_vel_x.abs() * 0.048).clamp(0.0, 0.08);
    let wake_dir = if motion.pointer_vel_x >= 0.0 { -1.0 } else { 1.0 };
    let is_ribbon = style == HorizonMode::Ribbon;
    let is_peaks = style == HorizonMode::Peaks;
    let is_pulse = style == HorizonMode::Pulse;
    let c1_amp = if is_ribbon { 0.14 + energy * 0.24 } else { 0.072 + voice_amp * 0.22 };
    let c2_amp = if is_ribbon { 0.09 + energy * 0.17 } else { 0.064 + voice_amp * 0.18 };
    let c3_amp = 0.056 + voice_amp * 0.16;
    let c4_amp = 0.046 + voice_amp * 0.14;
    let c5_amp = 0.036 + voice_amp * 0.11;
    let shimmer_scale = 0.24 * (0.35 + energy * 0.65);
    let bar_count = ((cols as f32 * 0.28).round() as usize).max(18);

    let sample_step = if sub_w >= 360 { 12 } else { 8 };
    let grid_step_x = sample_step * 2;
    let grid_step_y = if sub_h >= 96 { 20 } else { 14 };
    let ptr_sub_x = (px_target * (sub_w.saturating_sub(1)) as f32).round() as isize;

    let mut crest1 = vec![0.0_f32; sub_w];
    let mut crest1_echo = vec![0.0_f32; sub_w];
    let mut crest2 = vec![0.0_f32; sub_w];
    let mut crest3 = vec![0.0_f32; sub_w];
    let mut crest4 = vec![0.0_f32; sub_w];
    let mut crest5 = vec![0.0_f32; sub_w];
    let mut spectrum_top = vec![0.0_f32; sub_w];
    let mut shimmer = vec![0.0_f32; sub_w];
    let mut mist = vec![0.0_f32; sub_w];
    let mut sin_u = vec![0.0_f32; sub_w];
    let mut cos_u = vec![0.0_f32; sub_w];
    let mut peak_top = vec![0.0_f32; sub_w];
    let mut bar_gate = vec![0.0_f32; sub_w];

    for sx in 0..sub_w {
        let ux = (sx as f32 + 0.5) * inv_sub_w;
        let dx_ptr = ux - px_target;
        let ptr_arg = dx_ptr * dx_ptr * 22.0;
        let cursor_wave = if ptr_arg < 8.0 { (-ptr_arg).exp() * cursor_weight } else { 0.0 };

        let mut wake_wave = 0.0_f32;
        if wake_amp > 0.004 {
            let behind = dx_ptr * wake_dir;
            if behind > -0.04 && behind < 0.42 {
                let wake_env = (-(behind * behind * 22.0)).exp();
                wake_wave = wake_env * (behind * 34.0 - motion.wake_phase * 6.5).sin() * wake_amp;
            }
        }

        let mut band_raw = 0.18 + 0.12 * (ux * 19.0 + time * 2.1).sin();
        let mut band_boost = 0.0_f32;
        if band_len > 0 {
            let f_idx = ux * (band_len - 1) as f32;
            let i0 = (f_idx as usize).min(band_len - 1);
            let i1 = (i0 + 1).min(band_len - 1);
            let frac = f_idx - i0 as f32;
            let smooth_frac = frac * frac * (3.0 - 2.0 * frac);
            let b0 = bands[i0];
            let b1 = bands[i1];
            band_raw = b0 + (b1 - b0) * smooth_frac;
            band_boost = band_raw * if is_ribbon { 0.11 } else { 0.105 };
        }

        let pulse_mod = if is_pulse {
            (dx_ptr.abs() * 34.0 - time * 5.2).sin() * (0.04 + energy * 0.065) * (-(dx_ptr * dx_ptr * 4.2)).exp()
        } else {
            0.0
        };

        let am_envelope = 0.55 + 0.45 * (ux * 9.5 - time * 1.15).cos();
        let carrier_ripple = (ux * 44.0 - time * 3.4).sin() * (0.008 + energy * 0.022) * am_envelope;

        if is_ribbon {
            crest1[sx] = 0.50
                - (ux * 6.4 + time * 1.45).sin() * c1_amp
                - (ux * 13.5 - time * 1.85).cos() * (0.052 + energy * 0.045)
                - cursor_wave * 0.76
                - wake_wave * 0.85
                - band_boost;
            crest1_echo[sx] = 0.50
                - (ux * 6.4 + time * 1.45 - 0.65).sin() * (c1_amp * 0.86)
                + (ux * 16.5 + time * 2.15).sin() * (0.032 + energy * 0.025);
            crest2[sx] = 0.52
                - (ux * 4.6 - time * 0.98 + 1.6).sin() * c2_amp
                - (ux * 18.0 + time * 2.25).sin() * 0.032
                - cursor_wave * 0.48
                - wake_wave * 0.55
                - band_boost * 0.55;
            crest3[sx] = 0.5;
            crest4[sx] = 0.5;
            crest5[sx] = 0.5;
            spectrum_top[sx] = 1.0;
        } else {
            crest1[sx] = (0.31
                - (ux * 6.6 + time * 0.88).sin() * c1_amp
                - (ux * 15.2 - time * 1.32).cos() * 0.028
                - carrier_ripple
                - cursor_wave * 0.76
                - wake_wave * 0.80
                - band_boost * 0.72
                - pulse_mod)
                .max(0.16);
            crest1_echo[sx] = (0.34
                - (ux * 6.6 + time * 0.88 - 0.55).sin() * (c1_amp * 0.88)
                + (ux * 19.0 + time * 1.75).sin() * 0.020
                - cursor_wave * 0.48
                - band_boost * 0.44)
                .max(0.17);
            crest2[sx] = 0.47
                - (ux * 5.2 - time * 0.68 + 1.45).sin() * c2_amp
                - (ux * 18.4 + time * 1.85).sin() * 0.022
                + carrier_ripple * 0.55
                - cursor_wave * 0.54
                - wake_wave * 0.48
                - band_boost * 0.50
                - pulse_mod * 0.5;
            crest3[sx] = 0.62
                - (ux * 8.4 + time * 1.12 + 0.6).cos() * c3_amp
                - (ux * 16.0 - time * 0.98).sin() * 0.018
                - cursor_wave * 0.32
                - band_boost * 0.36;
            crest4[sx] = 0.77
                - (ux * 6.0 - time * 0.78 + 2.35).sin() * c4_amp
                - (ux * 13.6 + time * 1.36).cos() * 0.018
                - band_boost * 0.26;
            crest5[sx] = (0.895
                - (ux * 7.4 + time * 0.94 + 1.1).cos() * c5_amp
                - (ux * 16.8 - time * 1.16).sin() * 0.016
                - band_boost * 0.22)
                .min(0.955);

            let needle_mod = 0.5 + 0.5 * (ux * 31.0 - time * 2.8).sin() * (ux * 13.0 + time * 1.5).cos();
            let needle_height = 0.045 + (band_raw * 0.55 + needle_mod * 0.45) * (0.055 + energy * 0.09);
            spectrum_top[sx] = 0.985 - needle_height;
        }

        if is_peaks {
            let bar_pos = ux * bar_count as f32;
            let bar_idx = bar_pos as usize;
            let bar_frac = bar_pos - bar_idx as f32 - 0.5;
            let bar_center_u = (bar_idx as f32 + 0.5) / bar_count as f32;
            bar_gate[sx] = if bar_frac > -0.34 && bar_frac < 0.34 { 1.0 } else { 0.0 };
            let mut bar_band = 0.14 + 0.12 * (bar_center_u * 11.0 + time * 2.4).sin();
            if band_len > 0 {
                let bi = ((bar_center_u * band_len as f32) as usize).min(band_len - 1);
                bar_band = bands[bi] * 0.42 + 0.08;
            }
            let ptr_bar = (-(bar_center_u - px_target).powi(2) * 20.0).exp() * (0.12 + energy * 0.14);
            peak_top[sx] = (0.88 - (bar_band * (0.55 + energy * 0.75) + ptr_bar)).clamp(0.18, 0.86);
        }

        let edge_taper = if is_ribbon {
            smoothstep(0.0, 0.075, ux) * (1.0 - smoothstep(0.925, 1.0, ux))
        } else {
            1.0
        };
        let harmonic_shimmer = 0.74 * (ux * 22.0 - time * 3.4).sin() + 0.26 * (ux * 11.0 + time * 1.85).cos();
        shimmer[sx] = (0.94 + shimmer_scale * harmonic_shimmer) * 0.98 * edge_taper;
        let sparkle_gate = (ux * 54.0 - time * 4.6).sin() * (ux * 31.0 + time * 2.9).cos();
        mist[sx] = if !is_ribbon && energy > 0.12 {
            clamp01(0.42 + 0.58 * sparkle_gate) * energy * 0.42
        } else {
            0.0
        };

        let ripple_u = if is_pulse { dx_ptr.abs() * 38.0 - time * 4.4 } else { ux * 28.0 - time * 1.75 };
        sin_u[sx] = ripple_u.sin();
        cos_u[sx] = ripple_u.cos();
    }

    for sy in 0..sub_h {
        let uy = (sy as f32 + 0.5) * inv_sub_h;
        let top_fade = if is_ribbon {
            smoothstep(0.04, 0.28, uy)
        } else if sy < 4 {
            0.0
        } else {
            smoothstep(0.08, 0.38, uy)
        };
        let bottom_taper = if is_ribbon {
            1.0 - smoothstep(0.76, 0.96, uy)
        } else {
            1.0 - 0.10 * smoothstep(0.93, 1.0, uy)
        };
        let fade_envelope = top_fade * bottom_taper;
        let row_off = sy * sub_w;
        if fade_envelope <= 0.001 {
            continue;
        }

        let is_grid_row = !is_ribbon && sy % grid_step_y == 0;
        let is_cross_arm_y = !is_ribbon && (sy % grid_step_y == 1 || sy % grid_step_y == grid_step_y - 1);
        let is_baseline_rule = !is_ribbon
            && ((uy - 0.52).abs() < inv_sub_h * 0.55 || (uy - 0.80).abs() < inv_sub_h * 0.55);
        let bottom_anchor_weight = if is_ribbon {
            0.0
        } else {
            smoothstep(0.35, 0.75, uy) * (1.0 - 0.15 * smoothstep(0.92, 1.0, uy)) * if is_pulse { 0.075 } else { 0.036 }
        };
        let peak_scan_mask = if is_peaks && sy % 3 != 2 { 1.0 } else { 0.42 };
        let stroke1 = if is_ribbon { 46.0 } else { 56.0 };
        let stroke_echo = 60.0;
        let stroke2 = if is_ribbon { 38.0 } else { 48.0 };

        for sx in 0..sub_w {
            let c1 = crest1[sx];
            let c1_echo = crest1_echo[sx];
            let c2 = crest2[sx];
            let c3 = crest3[sx];
            let c4 = crest4[sx];
            let c5 = crest5[sx];

            let d1 = (uy - c1).abs();
            let d2 = (uy - c2).abs();
            let s1 = (if uy > c1 { d1 * 0.84 } else { d1 }) * stroke1;
            let s2 = (if uy > c2 { d2 * 0.86 } else { d2 }) * stroke2;
            let line1 = if s1 < 2.83 { (-(s1 * s1)).exp() * (shimmer[sx] * 1.14).min(1.16) } else { 0.0 };

            if is_ribbon {
                let edge_mask = if shimmer[sx] > 0.0 { (shimmer[sx] / 0.94).min(1.0) } else { 0.0 };
                let ribbon_second_weight = 0.72 * edge_mask;
                let d_echo_r = (uy - c1_echo).abs();
                let s_echo_r = d_echo_r * 52.0;
                let line_echo_r = if s_echo_r < 2.83 { (-(s_echo_r * s_echo_r)).exp() * 0.48 * edge_mask } else { 0.0 };
                let line2 = if s2 < 2.83 { (-(s2 * s2)).exp() * ribbon_second_weight } else { 0.0 };
                let tick = if sx % sample_step == 0 && sy == sub_h / 2 { 0.48 * edge_mask } else { 0.0 };
                let max_line = line1.max(line2).max(line_echo_r).max(tick);
                sub_lum[row_off + sx] = clamp01(max_line * fade_envelope);
                continue;
            }

            let d_echo = (uy - c1_echo).abs();
            let s_echo = d_echo * stroke_echo;
            let line_echo = if s_echo < 2.83 { (-(s_echo * s_echo)).exp() * 0.54 } else { 0.0 };

            let cross_gap = (c1 - c1_echo).abs();
            let node_lock = if cross_gap < 0.018 && d1 < 0.020 {
                (1.0 - cross_gap / 0.018) * (1.0 - d1 / 0.020) * 0.48
            } else {
                0.0
            };

            let d3 = (uy - c3).abs();
            let d4 = (uy - c4).abs();
            let d5 = (uy - c5).abs();
            let s3 = (if uy > c3 { d3 * 0.88 } else { d3 }) * 44.0;
            let s4 = (if uy > c4 { d4 * 0.88 } else { d4 }) * 40.0;
            let s5 = (if uy > c5 { d5 * 0.90 } else { d5 }) * 38.0;
            let line2 = if s2 < 2.83 { (-(s2 * s2)).exp() * 0.80 } else { 0.0 };
            let line3 = if s3 < 2.83 { (-(s3 * s3)).exp() * 0.64 } else { 0.0 };
            let line4 = if s4 < 2.83 { (-(s4 * s4)).exp() * 0.54 } else { 0.0 };
            let line5 = if s5 < 2.83 { (-(s5 * s5)).exp() * 0.46 } else { 0.0 };

            let mut telemetry_stem = 0.0_f32;
            let mut sample_pin = 0.0_f32;
            if sx % sample_step == 0 {
                let upper_span = c1.min(c2);
                let lower_span = c1.max(c2);
                if uy >= upper_span && uy <= lower_span && sy % 3 == 0 {
                    telemetry_stem = 0.44;
                }
                if d1 < inv_sub_h * 0.9 || d2 < inv_sub_h * 0.8 {
                    sample_pin = 0.98;
                }
            }

            let mut graticule = 0.0_f32;
            let mod_x = sx % grid_step_x;
            if is_grid_row && mod_x == 0 {
                graticule = 0.46;
            } else if (is_grid_row && (mod_x == 1 || mod_x == grid_step_x - 1)) || (is_cross_arm_y && mod_x == 0) {
                graticule = 0.26;
            } else if is_baseline_rule && sx % 4 == 0 {
                graticule = 0.22;
            } else if sx % grid_step_x == 0 && sy % 4 == 0 {
                graticule = 0.20;
            }

            let mut cursor_tick = 0.0_f32;
            if (sx as isize - ptr_sub_x).abs() <= 0 && sy % 2 == 0 && uy >= (c1 - 0.10).max(0.14) && uy <= c2 + 0.08 {
                cursor_tick = 0.74;
            }

            let mut spectrum_needle = 0.0_f32;
            if sx % 6 == 0 && uy >= spectrum_top[sx] && uy <= 0.985 {
                let is_tip = uy - spectrum_top[sx] < inv_sub_h * 1.4;
                spectrum_needle = if is_tip { 0.86 } else if sy % 2 == 0 { 0.54 } else { 0.0 };
            }

            let mut mist_val = 0.0_f32;
            let mist_offset = c1 - uy;
            if mist_offset > 0.008 && mist_offset < 0.12 && mist[sx] > 0.02 {
                mist_val = mist[sx] * (-(mist_offset * mist_offset * 185.0)).exp();
            }

            let skirt1 = if uy > c1 && d1 < 0.10 { (-(d1 * d1 * 240.0)).exp() * 0.065 } else { 0.0 };
            let skirt2 = if uy > c2 && d2 < 0.09 { (-(d2 * d2 * 260.0)).exp() * 0.055 } else { 0.0 };
            let skirt4 = if uy > c4 && d4 < 0.08 { (-(d4 * d4 * 280.0)).exp() * 0.05 } else { 0.0 };

            let sin_ripple = if is_pulse { sin_u[sx] * (uy * 14.0).cos() + cos_u[sx] * (uy * 14.0).sin() } else { 0.0 };
            let fine_ripple = if is_pulse { (0.5 + 0.5 * sin_ripple) * bottom_anchor_weight } else { 0.0 };

            let mut peak_boost = 0.0_f32;
            if is_peaks && bar_gate[sx] > 0.5 {
                let ptop = peak_top[sx];
                if uy >= ptop && uy < 0.94 {
                    let cap_dist = uy - ptop;
                    let cap_glow = if cap_dist < 0.055 {
                        0.88
                    } else {
                        0.48 * (1.0 - smoothstep(ptop, 0.94, uy) * 0.42)
                    };
                    peak_boost = cap_glow * peak_scan_mask;
                }
            }

            let max_trace = line1
                .max(line_echo)
                .max(line2)
                .max(line3)
                .max(line4)
                .max(line5)
                .max(peak_boost)
                .max(sample_pin)
                .max(spectrum_needle);
            let infra = graticule.max(telemetry_stem).max(cursor_tick);
            let signal_base = max_trace.max(infra);
            sub_lum[row_off + sx] =
                clamp01((signal_base + node_lock + skirt1 + skirt2 + skirt4 + fine_ripple + mist_val) * fade_envelope);
        }
    }

    // Braille packing with peak-weighted cell brightness.
    let mut cell_bits = vec![0u8; cols * rows];
    let mut cell_lums = vec![0u8; cols * rows];
    for r in 0..rows {
        let sy_base = r * 4;
        let r0 = sy_base * sub_w;
        let r1 = r0 + sub_w;
        let r2 = r1 + sub_w;
        let r3 = r2 + sub_w;
        let b0 = (sy_base & 7) << 3;
        let b1 = ((sy_base + 1) & 7) << 3;
        let b2 = ((sy_base + 2) & 7) << 3;
        let b3 = ((sy_base + 3) & 7) << 3;
        let row_off = r * cols;
        for c in 0..cols {
            let c2 = c * 2;
            let l00 = sub_lum[r0 + c2];
            let l01 = sub_lum[r0 + c2 + 1];
            let l10 = sub_lum[r1 + c2];
            let l11 = sub_lum[r1 + c2 + 1];
            let l20 = sub_lum[r2 + c2];
            let l21 = sub_lum[r2 + c2 + 1];
            let l30 = sub_lum[r3 + c2];
            let l31 = sub_lum[r3 + c2 + 1];
            let lum_sum = l00 + l01 + l10 + l11 + l20 + l21 + l30 + l31;
            let max_sub = l00.max(l01).max(l10).max(l11).max(l20).max(l21).max(l30).max(l31);
            let bx0 = c2 & 7;
            let bx1 = (c2 + 1) & 7;
            let mut bits = 0u8;
            if l00 > 0.11 + BAYER_8X8[b0 + bx0] * 0.72 { bits |= 0x01; }
            if l01 > 0.11 + BAYER_8X8[b0 + bx1] * 0.72 { bits |= 0x08; }
            if l10 > 0.11 + BAYER_8X8[b1 + bx0] * 0.72 { bits |= 0x02; }
            if l11 > 0.11 + BAYER_8X8[b1 + bx1] * 0.72 { bits |= 0x10; }
            if l20 > 0.11 + BAYER_8X8[b2 + bx0] * 0.72 { bits |= 0x04; }
            if l21 > 0.11 + BAYER_8X8[b2 + bx1] * 0.72 { bits |= 0x20; }
            if l30 > 0.11 + BAYER_8X8[b3 + bx0] * 0.72 { bits |= 0x40; }
            if l31 > 0.11 + BAYER_8X8[b3 + bx1] * 0.72 { bits |= 0x80; }

            let cell_intensity = (lum_sum * 0.125) * 0.52 + max_sub * 0.48;
            cell_lums[row_off + c] = (cell_intensity * 255.0 + 0.5).min(255.0) as u8;
            cell_bits[row_off + c] = bits;
        }
    }

    BrailleFrameGrid { cols, rows, cell_bits, cell_lums }
}

/// 5-tier horizon palette derived from the voice accent (BrailleHorizonCanvas buckets).
pub fn resolve_horizon_tone_palette(accent_hex: &str, is_active: bool) -> [TonalTier; 5] {
    let accent = parse_hex_rgb(accent_hex);
    let base_low = mix_rgb(parse_hex_rgb("#64748B"), accent, 0.35);
    let mid = mix_rgb(base_low, accent, 0.52);
    let high = mix_rgb(accent, [255, 255, 255], 0.42);
    let high_glow = mix_rgb(accent, [255, 255, 255], 0.32);
    let top = if is_active { [255, 255, 255] } else { parse_hex_rgb("#F8FAFC") };
    let alphas = if is_active {
        [0.46, 0.62, 0.74, 0.88, 0.98]
    } else {
        [0.40, 0.54, 0.64, 0.78, 0.92]
    };
    let colors = [base_low, mid, accent, high_glow, top];
    let diameters = [0.68, 0.80, 0.90, 1.00, 1.08];
    let blooms = [0.0, 0.0, 0.0, 1.0, 1.0];
    let _ = high;
    std::array::from_fn(|i| TonalTier {
        rgb: colors[i],
        alpha: alphas[i],
        diameter_scale: diameters[i],
        bloom_weight: blooms[i],
    })
}

pub fn render_horizon_rgba(
    pixel_w: u32,
    pixel_h: u32,
    cols: usize,
    rows: usize,
    time: f32,
    motion: &HorizonMotionState,
    style: HorizonMode,
    accent_hex: &str,
    dot_profile: DotProfile,
    bloom: f32,
) -> Vec<u8> {
    let grid = sample_horizon_grid(cols, rows, time, motion, style);
    let is_active = motion.energy > 0.14;
    let palette = resolve_horizon_tone_palette(accent_hex, is_active);
    let w = pixel_w.max(1) as usize;
    let h = pixel_h.max(1) as usize;
    let mut bytes = vec![0u8; w * h * 4];
    if grid.cols == 0 || grid.rows == 0 {
        return bytes;
    }
    let geom = DotGeometry::horizon(w as f32, h as f32, grid.cols, grid.rows);
    let dots = grid_to_tonal_dots(&grid, &palette, &geom, dot_profile, &HORIZON_TIER_THRESHOLDS);
    // Canvas applies bloom * 0.34 for the horizon.
    rasterize_tonal_dots_rgba(&mut bytes, w, h, &dots, &palette, dot_profile, bloom * 0.34);
    bytes
}