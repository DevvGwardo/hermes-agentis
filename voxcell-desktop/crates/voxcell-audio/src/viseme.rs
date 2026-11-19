//! 120 Hz viseme emission from playback FFT bands + RMS energy.
//!
//! The emitter is clock-driven (`step(dt, ...)`) so it emits at most
//! 120 frames/second regardless of the render tick rate, with attack /
//! release smoothing on intensity. Mouth shapes use the shared
//! [`voxcell_core::MouthViseme`] vocabulary.

use voxcell_core::MouthViseme;

pub const VISEME_RATE_HZ: f32 = 120.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisemeFrame {
    pub viseme: MouthViseme,
    pub intensity: f32,
    pub bands: [f32; 12],
}

impl Default for VisemeFrame {
    fn default() -> Self {
        Self {
            viseme: MouthViseme::Closed,
            intensity: 0.0,
            bands: [0.0; 12],
        }
    }
}

/// Map RMS amplitude + 12-band spectrum to a mouth shape.
pub fn bands_to_viseme(amp: f32, bands: &[f32; 12]) -> MouthViseme {
    if amp < 0.06 {
        return MouthViseme::Closed;
    }
    let mean = |r: std::ops::Range<usize>| -> f32 {
        let n = r.len().max(1) as f32;
        bands[r].iter().sum::<f32>() / n
    };
    let low = mean(0..4);
    let mid = mean(4..8);
    let high = mean(8..12);
    if high > low * 1.6 && high > mid * 1.2 && high > 0.05 {
        return MouthViseme::Ee;
    }
    if low > mid * 1.5 && low > high * 1.5 && amp < 0.54 {
        return MouthViseme::Round;
    }
    if amp < 0.16 {
        if low < 0.08 {
            return MouthViseme::Mbp;
        }
        return MouthViseme::Small;
    }
    if amp < 0.32 {
        return MouthViseme::Small;
    }
    if amp < 0.54 {
        return MouthViseme::Mid;
    }
    if amp < 0.76 {
        return MouthViseme::Round;
    }
    MouthViseme::Wide
}

pub struct VisemeEmitter {
    interval: f32,
    acc: f32,
    smooth_amp: f32,
    last: VisemeFrame,
}

impl VisemeEmitter {
    pub fn new() -> Self {
        Self {
            interval: 1.0 / VISEME_RATE_HZ,
            acc: 0.0,
            smooth_amp: 0.0,
            last: VisemeFrame::default(),
        }
    }

    /// Advance the emitter clock. Returns `Some(frame)` when a 120 Hz
    /// viseme slot is due, `None` otherwise. At most one frame per call —
    /// large dt gaps drop backlog instead of bursting frames.
    pub fn step(&mut self, dt: f32, amp: f32, bands: &[f32; 12]) -> Option<VisemeFrame> {
        if dt.is_finite() && dt > 0.0 {
            self.acc += dt;
        }
        if self.acc < self.interval {
            return None;
        }
        self.acc = 0.0;
        // Attack ~30 ms, release ~80 ms exponential smoothing.
        let target = amp.clamp(0.0, 1.0);
        let tau = if target > self.smooth_amp { 0.030 } else { 0.080 };
        let k = 1.0 - (-self.interval / tau).exp();
        self.smooth_amp += (target - self.smooth_amp) * k;
        if self.smooth_amp < 1e-4 {
            self.smooth_amp = 0.0;
        }
        let frame = VisemeFrame {
            viseme: bands_to_viseme(self.smooth_amp, bands),
            intensity: self.smooth_amp,
            bands: *bands,
        };
        self.last = frame;
        Some(frame)
    }

    pub fn latest(&self) -> VisemeFrame {
        self.last
    }

    pub fn reset(&mut self) {
        self.acc = 0.0;
        self.smooth_amp = 0.0;
        self.last = VisemeFrame::default();
    }
}

impl Default for VisemeEmitter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bands_with(low: f32, mid: f32, high: f32) -> [f32; 12] {
        let mut b = [0.0; 12];
        for v in b.iter_mut().take(4) {
            *v = low;
        }
        for v in b.iter_mut().skip(4).take(4) {
            *v = mid;
        }
        for v in b.iter_mut().skip(8) {
            *v = high;
        }
        b
    }

    #[test]
    fn silence_maps_to_closed() {
        assert_eq!(bands_to_viseme(0.0, &[0.0; 12]), MouthViseme::Closed);
        assert_eq!(bands_to_viseme(0.05, &bands_with(0.1, 0.1, 0.1)), MouthViseme::Closed);
    }

    #[test]
    fn high_frequency_dominance_maps_to_ee() {
        assert_eq!(bands_to_viseme(0.5, &bands_with(0.05, 0.1, 0.6)), MouthViseme::Ee);
    }

    #[test]
    fn low_frequency_dominance_maps_to_round() {
        assert_eq!(bands_to_viseme(0.4, &bands_with(0.6, 0.1, 0.05)), MouthViseme::Round);
    }

    #[test]
    fn amplitude_ladder_without_spectral_bias() {
        let flat = bands_with(0.2, 0.2, 0.2);
        assert_eq!(bands_to_viseme(0.10, &flat), MouthViseme::Small);
        assert_eq!(bands_to_viseme(0.25, &flat), MouthViseme::Small);
        assert_eq!(bands_to_viseme(0.45, &flat), MouthViseme::Mid);
        assert_eq!(bands_to_viseme(0.65, &flat), MouthViseme::Round);
        assert_eq!(bands_to_viseme(0.9, &flat), MouthViseme::Wide);
    }

    #[test]
    fn quiet_low_band_closure_maps_to_mbp() {
        assert_eq!(bands_to_viseme(0.1, &bands_with(0.01, 0.02, 0.02)), MouthViseme::Mbp);
    }

    #[test]
    fn emits_at_120hz_when_ticked_at_240hz() {
        let mut e = VisemeEmitter::new();
        let mut emitted = 0;
        for _ in 0..240 {
            if e.step(1.0 / 240.0, 0.5, &bands_with(0.2, 0.2, 0.2)).is_some() {
                emitted += 1;
            }
        }
        assert!((119..=121).contains(&emitted), "emitted {emitted} in 1 s");
    }

    #[test]
    fn emits_every_tick_at_120hz_and_never_faster() {
        let mut e = VisemeEmitter::new();
        for _ in 0..120 {
            assert!(e.step(1.0 / 120.0, 0.5, &[0.2; 12]).is_some());
        }
        // A tiny dt must not emit.
        assert!(e.step(0.0001, 0.5, &[0.2; 12]).is_none());
    }

    #[test]
    fn intensity_smooths_toward_target_and_decays_to_closed() {
        let mut e = VisemeEmitter::new();
        let mut last_intensity = 0.0;
        for _ in 0..60 {
            if let Some(f) = e.step(1.0 / 120.0, 0.8, &bands_with(0.2, 0.2, 0.2)) {
                assert!(f.intensity >= last_intensity - 1e-6);
                last_intensity = f.intensity;
            }
        }
        assert!(last_intensity > 0.5, "attack should approach target");
        for _ in 0..240 {
            e.step(1.0 / 120.0, 0.0, &[0.0; 12]);
        }
        let f = e.latest();
        assert!(f.intensity < 0.06);
        assert_eq!(f.viseme, MouthViseme::Closed);
    }

    #[test]
    fn reset_returns_to_closed() {
        let mut e = VisemeEmitter::new();
        e.step(1.0 / 120.0, 0.9, &[0.5; 12]);
        e.reset();
        assert_eq!(e.latest(), VisemeFrame::default());
    }
}
