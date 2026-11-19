//! Voice activity detection: a WebRTC-style onset/hangover state machine
//! driven by a pluggable frame classifier.
//!
//! [`EnergyClassifier`] is the built-in RMS-energy classifier with an
//! adaptive noise floor. Silero (or any neural VAD) can plug in later by
//! implementing [`VadFrameClassifier`] — the state machine, utterance
//! buffering and event semantics stay identical.

use crate::dsp::rms;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VadState {
    Silence,
    Speech,
}

#[derive(Clone, Debug, PartialEq)]
pub enum VadEvent {
    SpeechStart,
    SpeechEnd {
        utterance: Vec<f32>,
        sample_rate: u32,
    },
}

#[derive(Clone, Debug)]
pub struct VadConfig {
    /// Analysis frame length in milliseconds (WebRTC VAD uses 10/20/30).
    pub frame_ms: u32,
    /// Consecutive voiced frames required to trigger SpeechStart.
    pub onset_frames: u32,
    /// Consecutive unvoiced frames required to trigger SpeechEnd.
    pub hangover_frames: u32,
    /// Minimum RMS energy treated as speech (noise-floor independent).
    pub min_energy: f32,
    /// Speech threshold = max(min_energy, noise_floor * noise_ratio).
    pub noise_ratio: f32,
    /// Hard cap on buffered utterance length (samples) to bound memory.
    pub max_utterance_samples: usize,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            frame_ms: 20,
            onset_frames: 3,
            hangover_frames: 15,
            min_energy: 0.02,
            noise_ratio: 2.5,
            max_utterance_samples: 30 * 48_000,
        }
    }
}

/// Per-frame speech probability source. Implement this to plug in Silero
/// or another neural VAD; frames are `frame_ms` long at `sample_rate`.
pub trait VadFrameClassifier: Send {
    /// Returns speech probability in [0, 1] for one frame.
    fn classify(&mut self, frame: &[f32], sample_rate: u32) -> f32;
    fn reset(&mut self) {}
}

/// RMS-energy classifier with adaptive noise floor (WebRTC-style).
pub struct EnergyClassifier {
    min_energy: f32,
    noise_ratio: f32,
    noise_floor: f32,
    floor_alpha: f32,
}

impl EnergyClassifier {
    pub fn new(min_energy: f32, noise_ratio: f32) -> Self {
        Self {
            min_energy,
            noise_ratio,
            noise_floor: min_energy * 0.25,
            floor_alpha: 0.05,
        }
    }

    fn threshold(&self) -> f32 {
        self.min_energy.max(self.noise_floor * self.noise_ratio)
    }
}

impl Default for EnergyClassifier {
    fn default() -> Self {
        Self::new(0.02, 2.5)
    }
}

impl VadFrameClassifier for EnergyClassifier {
    fn classify(&mut self, frame: &[f32], _sample_rate: u32) -> f32 {
        let energy = rms(frame);
        // Slow upward adaptation for steady near-threshold noise (so a loud
        // room raises the bar), but never for clearly-loud speech frames —
        // otherwise sustained speech would adapt itself into silence.
        let speech_guard = self.min_energy * 4.0;
        if energy > self.noise_floor && energy < speech_guard {
            self.noise_floor += (energy - self.noise_floor) * self.floor_alpha;
        }
        let thr = self.threshold();
        if energy >= thr {
            1.0
        } else {
            // Fast downward adaptation when the room gets quieter.
            if energy < self.noise_floor {
                self.noise_floor += (energy - self.noise_floor) * 0.2;
            }
            (energy / thr.max(1e-9)) * 0.4
        }
    }

    fn reset(&mut self) {
        self.noise_floor = self.min_energy * 0.25;
    }
}

/// Onset/hangover state machine over any [`VadFrameClassifier`].
pub struct VadStateMachine<C: VadFrameClassifier> {
    classifier: C,
    config: VadConfig,
    state: VadState,
    voiced_run: u32,
    silent_run: u32,
    utterance: Vec<f32>,
    partial: Vec<f32>,
    sample_rate: u32,
}

impl<C: VadFrameClassifier> VadStateMachine<C> {
    pub fn new(classifier: C, config: VadConfig) -> Self {
        Self {
            classifier,
            config,
            state: VadState::Silence,
            voiced_run: 0,
            silent_run: 0,
            utterance: Vec::new(),
            partial: Vec::new(),
            sample_rate: 16_000,
        }
    }

    pub fn state(&self) -> VadState {
        self.state
    }

    pub fn config(&self) -> &VadConfig {
        &self.config
    }

    /// Feed arbitrary-length PCM; returns events produced by complete
    /// frames. Leftover samples are buffered until the next call.
    pub fn process(&mut self, pcm: &[f32], sample_rate: u32) -> Vec<VadEvent> {
        let mut events = Vec::new();
        if pcm.is_empty() {
            return events;
        }
        self.sample_rate = sample_rate;
        self.partial.extend_from_slice(pcm);
        let frame_len = self.frame_len(sample_rate);
        let mut consumed = 0usize;
        while self.partial.len() - consumed >= frame_len {
            let frame = &self.partial[consumed..consumed + frame_len];
            let voiced = self.classifier.classify(frame, sample_rate) >= 0.5;
            consumed += frame_len;
            match self.state {
                VadState::Silence => {
                    if voiced {
                        self.voiced_run += 1;
                        self.utterance.extend_from_slice(frame);
                        if self.voiced_run >= self.config.onset_frames {
                            self.state = VadState::Speech;
                            self.silent_run = 0;
                            events.push(VadEvent::SpeechStart);
                        }
                    } else {
                        // Discard pre-onset audio (no speech confirmed yet).
                        self.voiced_run = 0;
                        self.utterance.clear();
                    }
                }
                VadState::Speech => {
                    self.utterance.extend_from_slice(frame);
                    if voiced {
                        self.silent_run = 0;
                    } else {
                        self.silent_run += 1;
                        if self.silent_run >= self.config.hangover_frames {
                            self.end_speech(&mut events);
                        }
                    }
                }
            }
            // Hard memory cap: force-close even mid-speech.
            if self.state == VadState::Speech
                && self.utterance.len() >= self.config.max_utterance_samples
            {
                self.end_speech(&mut events);
            }
        }
        self.partial.drain(..consumed);
        events
    }

    /// Force-close an in-progress utterance (e.g. push-to-talk release).
    pub fn flush(&mut self) -> Vec<VadEvent> {
        let mut events = Vec::new();
        if self.state == VadState::Speech {
            self.utterance.extend_from_slice(&self.partial);
            self.end_speech(&mut events);
        }
        self.partial.clear();
        self.voiced_run = 0;
        self.silent_run = 0;
        events
    }

    pub fn reset(&mut self) {
        self.state = VadState::Silence;
        self.voiced_run = 0;
        self.silent_run = 0;
        self.utterance.clear();
        self.partial.clear();
        self.classifier.reset();
    }

    fn frame_len(&self, sample_rate: u32) -> usize {
        ((sample_rate as u64 * self.config.frame_ms as u64) / 1000).max(1) as usize
    }

    fn end_speech(&mut self, events: &mut Vec<VadEvent>) {
        self.state = VadState::Silence;
        self.voiced_run = 0;
        self.silent_run = 0;
        let utterance = std::mem::take(&mut self.utterance);
        events.push(VadEvent::SpeechEnd {
            utterance,
            sample_rate: self.sample_rate,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn tone(seconds: f32, amp: f32, sr: u32) -> Vec<f32> {
        let n = (seconds * sr as f32) as usize;
        (0..n)
            .map(|i| (TAU * 220.0 * i as f32 / sr as f32).sin() * amp)
            .collect()
    }

    fn machine() -> VadStateMachine<EnergyClassifier> {
        VadStateMachine::new(EnergyClassifier::default(), VadConfig::default())
    }

    #[test]
    fn silence_produces_no_events() {
        let mut m = machine();
        let events = m.process(&vec![0.001; 16_000], 16_000);
        assert!(events.is_empty());
        assert_eq!(m.state(), VadState::Silence);
    }

    #[test]
    fn sustained_speech_triggers_single_speech_start() {
        let mut m = machine();
        let events = m.process(&tone(0.5, 0.3, 16_000), 16_000);
        let starts = events
            .iter()
            .filter(|e| matches!(e, VadEvent::SpeechStart))
            .count();
        assert_eq!(starts, 1);
        assert_eq!(m.state(), VadState::Speech);
        // Continued speech adds no further starts.
        let more = m.process(&tone(0.5, 0.3, 16_000), 16_000);
        assert!(more.is_empty());
    }

    #[test]
    fn speech_then_silence_ends_after_hangover_with_utterance() {
        let mut m = machine();
        m.process(&tone(0.4, 0.3, 16_000), 16_000);
        // 200 ms silence < 300 ms hangover: still speech.
        let early = m.process(&vec![0.0; 3200], 16_000);
        assert!(early.is_empty());
        assert_eq!(m.state(), VadState::Speech);
        // Another 200 ms crosses the 300 ms hangover.
        let late = m.process(&vec![0.0; 3200], 16_000);
        let ends: Vec<_> = late
            .iter()
            .filter_map(|e| match e {
                VadEvent::SpeechEnd {
                    utterance,
                    sample_rate,
                } => Some((utterance.len(), *sample_rate)),
                _ => None,
            })
            .collect();
        assert_eq!(ends.len(), 1);
        assert!(ends[0].0 >= 6400, "utterance should cover the speech");
        assert_eq!(ends[0].1, 16_000);
        assert_eq!(m.state(), VadState::Silence);
    }

    #[test]
    fn short_blip_below_onset_does_not_trigger() {
        let mut m = machine();
        // 40 ms = 2 frames < onset_frames (3).
        let events = m.process(&tone(0.04, 0.4, 16_000), 16_000);
        assert!(events.is_empty());
        assert_eq!(m.state(), VadState::Silence);
        let tail = m.process(&vec![0.0; 16_000], 16_000);
        assert!(tail.is_empty());
    }

    #[test]
    fn max_utterance_cap_forces_speech_end() {
        let cfg = VadConfig {
            max_utterance_samples: 16_000, // 1 s
            ..VadConfig::default()
        };
        let mut m = VadStateMachine::new(EnergyClassifier::default(), cfg);
        let events = m.process(&tone(2.0, 0.3, 16_000), 16_000);
        assert!(events
            .iter()
            .any(|e| matches!(e, VadEvent::SpeechEnd { .. })));
    }

    #[test]
    fn flush_closes_in_progress_utterance() {
        let mut m = machine();
        m.process(&tone(0.3, 0.3, 16_000), 16_000);
        assert_eq!(m.state(), VadState::Speech);
        let events = m.flush();
        assert_eq!(events.len(), 1);
        match &events[0] {
            VadEvent::SpeechEnd { utterance, .. } => assert!(!utterance.is_empty()),
            _ => panic!("expected SpeechEnd"),
        }
        assert_eq!(m.state(), VadState::Silence);
        // Flushing silence is a no-op.
        assert!(m.flush().is_empty());
    }

    #[test]
    fn reset_clears_state_and_buffers() {
        let mut m = machine();
        m.process(&tone(0.3, 0.3, 16_000), 16_000);
        m.reset();
        assert_eq!(m.state(), VadState::Silence);
        assert!(m.flush().is_empty());
    }

    #[test]
    fn partial_frames_are_buffered_across_calls() {
        let mut m = machine();
        // 10 ms chunks (half a frame): speech must still be detected.
        let speech = tone(0.3, 0.3, 16_000);
        let mut events = Vec::new();
        for chunk in speech.chunks(160) {
            events.extend(m.process(chunk, 16_000));
        }
        assert!(events.iter().any(|e| matches!(e, VadEvent::SpeechStart)));
    }

    /// Silero-style plug-in proof: a fixed-probability classifier drives
    /// the exact same state machine semantics.
    struct ScriptedClassifier {
        prob: f32,
    }
    impl VadFrameClassifier for ScriptedClassifier {
        fn classify(&mut self, _frame: &[f32], _sr: u32) -> f32 {
            self.prob
        }
    }

    #[test]
    fn custom_classifier_plugs_into_state_machine() {
        let mut m = VadStateMachine::new(ScriptedClassifier { prob: 0.9 }, VadConfig::default());
        let events = m.process(&vec![0.0; 16_000], 16_000);
        assert!(events.iter().any(|e| matches!(e, VadEvent::SpeechStart)));
        m.reset();
        let mut quiet =
            VadStateMachine::new(ScriptedClassifier { prob: 0.1 }, VadConfig::default());
        assert!(quiet.process(&vec![0.5; 16_000], 16_000).is_empty());
    }

    #[test]
    fn adaptive_noise_floor_raises_threshold_in_steady_noise() {
        let mut c = EnergyClassifier::new(0.01, 2.5);
        // Steady 0.02 noise: floor adapts upward over ~200 frames.
        for _ in 0..200 {
            c.classify(&vec![0.02; 320], 16_000);
        }
        // A 0.03 signal is below floor*2.5 (~0.05) now → not speech.
        assert!(c.classify(&vec![0.03; 320], 16_000) < 0.5);
        // A clearly louder signal still is.
        assert!(c.classify(&vec![0.2; 320], 16_000) >= 0.5);
    }
}
