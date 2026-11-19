//! Native voice pipeline for Voxcell Desktop (spec §7.2, tickets P2-04/P2-05).
//!
//! `cpal` mic capture → lock-free ring → 12-band FFT + RMS → VAD → STT
//! (Deepgram Flux / whisper.cpp / OS fallback) → sentence-chunked Aura-2 TTS
//! playback with barge-in ducking → 120 Hz visemes.
//!
//! Framework-free: no RUNE/wgpu imports. All tests run on mocks/fixtures —
//! no audio device, network or API key required.

pub mod capture;
pub mod dsp;
pub mod engine;
pub mod fft;
pub mod ring;
pub mod stt;
pub mod tts;
pub mod vad;
pub mod viseme;

pub use capture::{
    CaptureBackend, CaptureError, CaptureState, CaptureStatus, MicPermission, MicPermissionSource,
    MicSupervisor,
};
pub use dsp::{decode_linear16, encode_linear16, mix_to_mono, resample_linear, rms};
pub use engine::*;
pub use fft::*;
pub use ring::MicRing;
pub use stt::{AudioChunk, SttBackend, SttBackendKind, SttConfig, SttError, SttEvent, SttRouter};
pub use tts::*;
pub use vad::{EnergyClassifier, VadConfig, VadEvent, VadFrameClassifier, VadState, VadStateMachine};
pub use viseme::{VisemeEmitter, VisemeFrame};

