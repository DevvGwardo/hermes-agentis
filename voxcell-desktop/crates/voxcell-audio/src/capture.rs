//! Mic capture: `cpal` input streams (16/48 kHz or any device rate) feeding
//! the lock-free [`MicRing`], plus a supervisor that recovers from stream
//! errors and device hot-swaps, and reports OS mic-permission state.
//!
//! The audio callback never locks and never allocates: it downmixes into a
//! pre-allocated scratch buffer owned by the closure and pushes into the
//! ring. All recovery logic lives in [`MicSupervisor::poll`], which the
//! engine ticks — so tests drive it with a mock backend and a fake clock,
//! no audio device required.

use crate::ring::MicRing;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Desired pipeline rate for VAD/STT. Device capture runs at its native
/// rate (typically 16 or 48 kHz); the engine resamples to this on consume.
pub const PIPELINE_SAMPLE_RATE: u32 = 16_000;

const INITIAL_BACKOFF_MS: u64 = 250;
const MAX_BACKOFF_MS: u64 = 5_000;
const DEVICE_CHECK_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureState {
    /// Not started yet.
    Idle,
    /// Stream open and healthy.
    Running,
    /// Stream lost; retrying with exponential backoff.
    Recovering,
    /// No input device present.
    NoDevice,
    /// OS reports microphone access denied.
    PermissionDenied,
    /// Explicitly stopped (listening disabled).
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicPermission {
    Granted,
    Denied,
    /// Platform does not expose a queryable consent state; capture is
    /// attempted anyway and a denial surfaces as a stream error.
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicPermissionSource {
    WindowsConsentStore,
    MacOsTcc,
    PlatformDefault,
    QueryFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    NoDevice,
    PermissionDenied,
    Build(String),
    Stream(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDevice => write!(f, "no input device"),
            Self::PermissionDenied => write!(f, "microphone permission denied"),
            Self::Build(e) => write!(f, "stream build failed: {e}"),
            Self::Stream(e) => write!(f, "stream error: {e}"),
        }
    }
}

impl std::error::Error for CaptureError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputStreamInfo {
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Clone, Debug)]
pub struct CaptureStatus {
    pub state: CaptureState,
    pub device: Option<String>,
    pub sample_rate: u32,
    pub channels: u16,
    pub dropped_samples: u64,
    pub recoveries: u32,
    pub last_error: Option<String>,
    pub permission: MicPermission,
    pub permission_source: MicPermissionSource,
}

/// An open capture stream. Dropping it closes the stream.
pub trait CaptureStreamHandle: Send {
    /// False once the backend's error callback has fired.
    fn is_alive(&self) -> bool;
}

/// Device enumeration + stream construction abstraction. `CpalBackend` is
/// the real implementation; tests inject mocks.
pub trait CaptureBackend: Send {
    fn default_input(&self) -> Result<InputStreamInfo, CaptureError>;
    fn open(
        &self,
        info: &InputStreamInfo,
        ring: Arc<MicRing>,
        error_flag: Arc<AtomicBool>,
    ) -> Result<Box<dyn CaptureStreamHandle>, CaptureError>;
    fn permission(&self) -> (MicPermission, MicPermissionSource) {
        query_mic_permission()
    }
}

// ---------------------------------------------------------------------------
// OS mic permission
// ---------------------------------------------------------------------------

/// Parse the Windows ConsentStore `Value` string.
pub fn parse_consent_value(value: &str) -> MicPermission {
    match value.trim() {
        "Allow" => MicPermission::Granted,
        "Deny" => MicPermission::Denied,
        _ => MicPermission::Unknown,
    }
}

#[cfg(windows)]
pub fn query_mic_permission() -> (MicPermission, MicPermissionSource) {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    match hkcu.open_subkey(path) {
        Ok(key) => match key.get_value::<String, _>("Value") {
            Ok(v) => (parse_consent_value(&v), MicPermissionSource::WindowsConsentStore),
            Err(_) => (MicPermission::Unknown, MicPermissionSource::QueryFailed),
        },
        Err(_) => (MicPermission::Unknown, MicPermissionSource::QueryFailed),
    }
}

#[cfg(not(windows))]
pub fn query_mic_permission() -> (MicPermission, MicPermissionSource) {
    // macOS TCC / Linux PipeWire consent are not queryable without platform
    // APIs; report Unknown and let stream errors surface denials.
    (MicPermission::Unknown, MicPermissionSource::PlatformDefault)
}

// ---------------------------------------------------------------------------
// cpal backend
// ---------------------------------------------------------------------------

pub struct CpalBackend;

struct CpalStream {
    _stream: cpal::Stream,
    error_flag: Arc<AtomicBool>,
}

// SAFETY: cpal::Stream is conservatively !Send (raw platform handle). The
// stream is created, kept alive, and dropped on the engine's owning thread;
// the audio callback runs on cpal's own thread and only touches the
// Arc-shared ring/error flag. Moving the handle between threads never calls
// into the stream concurrently.
unsafe impl Send for CpalStream {}

impl CaptureStreamHandle for CpalStream {
    fn is_alive(&self) -> bool {
        !self.error_flag.load(Ordering::Relaxed)
    }
}

/// Build an input stream for any cpal sample format. The callback downmixes
/// to mono into a closure-owned scratch buffer (no allocation, no locks)
/// and pushes into the ring; overflow drops newest samples.
macro_rules! build_input {
    ($dev:expr, $cfg:expr, $ring:expr, $flag:expr, $t:ty, $conv:expr) => {{
        let channels = $cfg.channels() as usize;
        let ring = Arc::clone(&$ring);
        let flag = Arc::clone(&$flag);
        let mut scratch: Vec<f32> = vec![0.0; 8192];
        let convert = $conv;
        $dev.build_input_stream(
            &$cfg.clone().into(),
            move |data: &[$t], _| {
                let ch = channels.max(1);
                let mut filled = 0usize;
                for frame in data.chunks(ch) {
                    if frame.len() < ch {
                        break;
                    }
                    let mono = frame.iter().map(|&s| convert(s)).sum::<f32>() / ch as f32;
                    scratch[filled] = mono;
                    filled += 1;
                    if filled == scratch.len() {
                        ring.push(&scratch);
                        filled = 0;
                    }
                }
                if filled > 0 {
                    ring.push(&scratch[..filled]);
                }
            },
            move |err| {
                flag.store(true, Ordering::Relaxed);
                let _ = err;
            },
            None,
        )
    }};
}

impl CaptureBackend for CpalBackend {
    fn default_input(&self) -> Result<InputStreamInfo, CaptureError> {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let dev = host.default_input_device().ok_or(CaptureError::NoDevice)?;
        let cfg = dev
            .default_input_config()
            .map_err(|e| CaptureError::Build(e.to_string()))?;
        let name = dev.name().unwrap_or_else(|_| "unknown input".into());
        Ok(InputStreamInfo {
            name,
            sample_rate: cfg.sample_rate().0,
            channels: cfg.channels(),
        })
    }

    fn open(
        &self,
        _info: &InputStreamInfo,
        ring: Arc<MicRing>,
        error_flag: Arc<AtomicBool>,
    ) -> Result<Box<dyn CaptureStreamHandle>, CaptureError> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let dev = host.default_input_device().ok_or(CaptureError::NoDevice)?;
        let cfg = dev
            .default_input_config()
            .map_err(|e| CaptureError::Build(e.to_string()))?;
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => build_input!(dev, cfg, ring, error_flag, f32, |s: f32| s),
            cpal::SampleFormat::I16 => {
                build_input!(dev, cfg, ring, error_flag, i16, |s: i16| s as f32 / 32768.0)
            }
            cpal::SampleFormat::U16 => {
                build_input!(dev, cfg, ring, error_flag, u16, |s: u16| (s as f32 - 32768.0)
                    / 32768.0)
            }
            other => {
                return Err(CaptureError::Build(format!(
                    "unsupported sample format: {other:?}"
                )));
            }
        };
        let stream = stream.map_err(|e| CaptureError::Build(e.to_string()))?;
        stream.play().map_err(|e| CaptureError::Stream(e.to_string()))?;
        Ok(Box::new(CpalStream {
            _stream: stream,
            error_flag,
        }))
    }
}

// ---------------------------------------------------------------------------
// Supervisor: hot-swap + stream-error recovery loop
// ---------------------------------------------------------------------------

struct SupervisorInner {
    handle: Option<Box<dyn CaptureStreamHandle>>,
    state: CaptureState,
    info: Option<InputStreamInfo>,
    enabled: bool,
    /// None = attempt on the next poll (start / hot-swap). Keeping this
    /// optional avoids reading the wall clock in `start()`, which would
    /// break the injectable-clock contract of `poll(now)`.
    next_retry: Option<Instant>,
    backoff_ms: u64,
    recoveries: u32,
    last_error: Option<String>,
    /// None = never checked; the first poll always checks. Optional so the
    /// supervisor never depends on the wall clock under an injected `now`.
    last_device_check: Option<Instant>,
    permission: MicPermission,
    permission_source: MicPermissionSource,
    last_permission_check: Option<Instant>,
}

pub struct MicSupervisor<B: CaptureBackend> {
    backend: B,
    ring: Arc<MicRing>,
    error_flag: Arc<AtomicBool>,
    inner: Mutex<SupervisorInner>,
}

impl<B: CaptureBackend> MicSupervisor<B> {
    /// Ring holds ~2 s of audio at 48 kHz by default.
    pub fn new(backend: B, ring_capacity: usize) -> Self {
        Self {
            backend,
            ring: Arc::new(MicRing::new(ring_capacity)),
            error_flag: Arc::new(AtomicBool::new(false)),
            inner: Mutex::new(SupervisorInner {
                handle: None,
                state: CaptureState::Idle,
                info: None,
                enabled: false,
                next_retry: None,
                backoff_ms: INITIAL_BACKOFF_MS,
                recoveries: 0,
                last_error: None,
                last_device_check: None,
                permission: MicPermission::Unknown,
                permission_source: MicPermissionSource::PlatformDefault,
                last_permission_check: None,
            }),
        }
    }

    pub fn ring(&self) -> Arc<MicRing> {
        Arc::clone(&self.ring)
    }

    pub fn error_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.error_flag)
    }

    pub fn start(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.enabled = true;
            g.next_retry = None; // first poll attempts immediately
            g.backoff_ms = INITIAL_BACKOFF_MS;
        }
    }

    pub fn stop(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.enabled = false;
            g.handle = None; // dropping closes the stream
            g.state = CaptureState::Stopped;
            g.info = None;
            self.ring.clear();
        }
    }

    /// Recovery loop tick. Call from the engine processing thread (never
    /// from the audio callback). `now` is injectable for deterministic tests.
    pub fn poll(&self, now: Instant) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        if !g.enabled {
            return;
        }

        // Permission re-check (cheap, throttled to the device-check cadence).
        let perm_due = g
            .last_permission_check
            .is_none_or(|t| now.duration_since(t) >= DEVICE_CHECK_INTERVAL);
        if perm_due {
            let (perm, src) = self.backend.permission();
            g.permission = perm;
            g.permission_source = src;
            g.last_permission_check = Some(now);
            if perm == MicPermission::Denied {
                g.handle = None;
                g.state = CaptureState::PermissionDenied;
                g.last_error = Some(CaptureError::PermissionDenied.to_string());
                return;
            }
        }
        if g.permission == MicPermission::Denied {
            g.state = CaptureState::PermissionDenied;
            return;
        }

        let stream_lost = match &g.handle {
            Some(h) => !h.is_alive() || self.error_flag.load(Ordering::Relaxed),
            None => true,
        };

        // Hot-swap: default device changed while the stream looks alive.
        let device_due = g
            .last_device_check
            .is_none_or(|t| now.duration_since(t) >= DEVICE_CHECK_INTERVAL);
        if !stream_lost && device_due {
            g.last_device_check = Some(now);
            if let Ok(info) = self.backend.default_input() {
                let changed = g
                    .info
                    .as_ref()
                    .map(|cur| cur.name != info.name || cur.sample_rate != info.sample_rate)
                    .unwrap_or(true);
                if changed {
                    g.handle = None;
                    g.info = None;
                    g.recoveries += 1;
                    g.last_error = Some("input device changed (hot-swap)".into());
                    // Reopen immediately — no backoff for a clean swap.
                    g.next_retry = Some(now);
                }
            }
        }

        let stream_lost = match &g.handle {
            Some(h) => !h.is_alive() || self.error_flag.load(Ordering::Relaxed),
            None => true,
        };
        if !stream_lost {
            g.state = CaptureState::Running;
            return;
        }

        // Lost (or never opened): retry with exponential backoff.
        let just_lost = g.handle.is_some();
        if just_lost {
            // A running stream died: schedule the first retry after backoff.
            g.handle = None;
            g.state = CaptureState::Recovering;
            g.last_error = Some(CaptureError::Stream("device stream error".into()).to_string());
            g.next_retry = Some(now + Duration::from_millis(g.backoff_ms));
            g.backoff_ms = (g.backoff_ms * 2).min(MAX_BACKOFF_MS);
            self.error_flag.store(false, Ordering::Relaxed);
            self.ring.clear();
            return;
        }
        if g.next_retry.is_some_and(|nr| now < nr) {
            if g.state != CaptureState::NoDevice {
                g.state = CaptureState::Recovering;
            }
            return;
        }
        self.error_flag.store(false, Ordering::Relaxed);
        // Discard stale samples from the dead stream before reopening.
        self.ring.clear();

        let attempt = self
            .backend
            .default_input()
            .and_then(|info| self.backend.open(&info, Arc::clone(&self.ring), Arc::clone(&self.error_flag)).map(|h| (info, h)));
        match attempt {
            Ok((info, handle)) => {
                // Only count recovery from a down state — the initial open
                // from Idle/Stopped is not a recovery.
                let was_down = matches!(
                    g.state,
                    CaptureState::Recovering
                        | CaptureState::NoDevice
                        | CaptureState::PermissionDenied
                );
                g.handle = Some(handle);
                g.info = Some(info);
                g.state = CaptureState::Running;
                g.last_error = None;
                g.backoff_ms = INITIAL_BACKOFF_MS;
                g.next_retry = Some(now);
                g.last_device_check = Some(now);
                if was_down {
                    g.recoveries += 1;
                }
            }
            Err(e) => {
                g.last_error = Some(e.to_string());
                g.state = match e {
                    CaptureError::NoDevice => CaptureState::NoDevice,
                    CaptureError::PermissionDenied => CaptureState::PermissionDenied,
                    _ => CaptureState::Recovering,
                };
                g.next_retry = Some(now + Duration::from_millis(g.backoff_ms));
                g.backoff_ms = (g.backoff_ms * 2).min(MAX_BACKOFF_MS);
            }
        }
    }

    pub fn status(&self) -> CaptureStatus {
        let g = self.inner.lock().expect("supervisor lock");
        CaptureStatus {
            state: g.state,
            device: g.info.as_ref().map(|i| i.name.clone()),
            sample_rate: g.info.as_ref().map(|i| i.sample_rate).unwrap_or(0),
            channels: g.info.as_ref().map(|i| i.channels).unwrap_or(0),
            dropped_samples: self.ring.dropped(),
            recoveries: g.recoveries,
            last_error: g.last_error.clone(),
            permission: g.permission,
            permission_source: g.permission_source,
        }
    }
}

// ---------------------------------------------------------------------------
// Mock backend (shared with engine tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod mock {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    pub struct MockStream {
        pub flag: Arc<AtomicBool>,
    }
    impl CaptureStreamHandle for MockStream {
        fn is_alive(&self) -> bool {
            !self.flag.load(Ordering::Relaxed)
        }
    }

    pub struct MockBackend {
        pub device: Mutex<Result<InputStreamInfo, CaptureError>>,
        /// Number of upcoming `open` calls that fail before success.
        pub open_failures: AtomicUsize,
        pub open_count: AtomicUsize,
        pub permission: Mutex<(MicPermission, MicPermissionSource)>,
    }

    impl MockBackend {
        pub fn new(sample_rate: u32) -> Self {
            Self {
                device: Mutex::new(Ok(InputStreamInfo {
                    name: "Mock Mic".into(),
                    sample_rate,
                    channels: 1,
                })),
                open_failures: AtomicUsize::new(0),
                open_count: AtomicUsize::new(0),
                permission: Mutex::new((MicPermission::Granted, MicPermissionSource::PlatformDefault)),
            }
        }
    }

    impl CaptureBackend for MockBackend {
        fn default_input(&self) -> Result<InputStreamInfo, CaptureError> {
            self.device.lock().unwrap().clone()
        }
        fn open(
            &self,
            _info: &InputStreamInfo,
            _ring: Arc<MicRing>,
            error_flag: Arc<AtomicBool>,
        ) -> Result<Box<dyn CaptureStreamHandle>, CaptureError> {
            self.open_count.fetch_add(1, Ordering::Relaxed);
            if self.open_failures.load(Ordering::Relaxed) > 0 {
                self.open_failures.fetch_sub(1, Ordering::Relaxed);
                return Err(CaptureError::Build("mock open failure".into()));
            }
            Ok(Box::new(MockStream { flag: error_flag }))
        }
        fn permission(&self) -> (MicPermission, MicPermissionSource) {
            *self.permission.lock().unwrap()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mock::MockBackend;

    fn base() -> Instant {
        Instant::now()
    }

    #[test]
    fn consent_value_parsing() {
        assert_eq!(parse_consent_value("Allow"), MicPermission::Granted);
        assert_eq!(parse_consent_value("Deny"), MicPermission::Denied);
        assert_eq!(parse_consent_value("  Allow "), MicPermission::Granted);
        assert_eq!(parse_consent_value("whatever"), MicPermission::Unknown);
    }

    #[test]
    fn supervisor_opens_and_reports_running() {
        let sup = MicSupervisor::new(MockBackend::new(48_000), 4096);
        let t0 = base();
        sup.start();
        sup.poll(t0);
        let s = sup.status();
        assert_eq!(s.state, CaptureState::Running);
        assert_eq!(s.device.as_deref(), Some("Mock Mic"));
        assert_eq!(s.sample_rate, 48_000);
        assert_eq!(s.permission, MicPermission::Granted);
    }

    #[test]
    fn stream_error_triggers_backoff_recovery() {
        let sup = MicSupervisor::new(MockBackend::new(16_000), 4096);
        let t0 = base();
        sup.start();
        sup.poll(t0);
        assert_eq!(sup.status().state, CaptureState::Running);
        // Simulate the cpal error callback firing.
        sup.error_flag().store(true, Ordering::Relaxed);
        sup.poll(t0 + Duration::from_millis(10));
        assert_eq!(sup.status().state, CaptureState::Recovering);
        // Within the 250 ms backoff window: no reopen yet.
        sup.poll(t0 + Duration::from_millis(100));
        assert_eq!(sup.status().state, CaptureState::Recovering);
        // After backoff: recovered.
        sup.poll(t0 + Duration::from_millis(300));
        let s = sup.status();
        assert_eq!(s.state, CaptureState::Running);
        assert_eq!(s.recoveries, 1);
        assert!(s.last_error.is_none());
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let backend = MockBackend::new(16_000);
        backend.open_failures.store(6, Ordering::Relaxed);
        let sup = MicSupervisor::new(backend, 4096);
        let t0 = base();
        sup.start();
        sup.poll(t0); // attempt 1 fails -> next retry +250
        assert_eq!(sup.status().state, CaptureState::Recovering);
        sup.poll(t0 + Duration::from_millis(249));
        assert_eq!(sup.status().state, CaptureState::Recovering);
        sup.poll(t0 + Duration::from_millis(250)); // attempt 2 fails -> +500
        sup.poll(t0 + Duration::from_millis(749));
        assert_eq!(sup.status().state, CaptureState::Recovering);
        sup.poll(t0 + Duration::from_millis(750)); // attempt 3 fails -> +1000
        sup.poll(t0 + Duration::from_millis(1750)); // attempt 4 fails -> +2000
        sup.poll(t0 + Duration::from_millis(3750)); // attempt 5 fails -> +4000
        sup.poll(t0 + Duration::from_millis(7750)); // attempt 6 fails -> +5000 (cap)
        sup.poll(t0 + Duration::from_millis(12749));
        assert_eq!(sup.status().state, CaptureState::Recovering);
        sup.poll(t0 + Duration::from_millis(12750)); // attempt 7 succeeds
        assert_eq!(sup.status().state, CaptureState::Running);
    }

    #[test]
    fn device_hot_swap_reopens_on_new_default_device() {
        let t0 = base();
        let sup = MicSupervisor::new(
            SwapBackend {
                inner: MockBackend::new(48_000),
                swapped: AtomicBool::new(false),
            },
            4096,
        );
        sup.start();
        sup.poll(t0);
        assert_eq!(sup.status().device.as_deref(), Some("Mock Mic"));
        // After the device-check interval the OS reports a different
        // default input (unplug/replug, Bluetooth switch...): the
        // supervisor must close the old stream and open the new device.
        sup.poll(t0 + DEVICE_CHECK_INTERVAL);
        let s = sup.status();
        assert_eq!(s.device.as_deref(), Some("USB Mic"));
        assert_eq!(s.sample_rate, 16_000);
        assert_eq!(s.state, CaptureState::Running);
        assert_eq!(s.recoveries, 1);
    }

    /// Wrapper that flips the reported default device after the first check.
    struct SwapBackend {
        inner: MockBackend,
        swapped: AtomicBool,
    }
    impl CaptureBackend for SwapBackend {
        fn default_input(&self) -> Result<InputStreamInfo, CaptureError> {
            if self.swapped.load(Ordering::Relaxed) {
                Ok(InputStreamInfo {
                    name: "USB Mic".into(),
                    sample_rate: 16_000,
                    channels: 1,
                })
            } else {
                self.inner.default_input()
            }
        }
        fn open(
            &self,
            info: &InputStreamInfo,
            ring: Arc<MicRing>,
            flag: Arc<AtomicBool>,
        ) -> Result<Box<dyn CaptureStreamHandle>, CaptureError> {
            self.swapped.store(true, Ordering::Relaxed);
            self.inner.open(info, ring, flag)
        }
        fn permission(&self) -> (MicPermission, MicPermissionSource) {
            self.inner.permission()
        }
    }

    #[test]
    fn no_device_and_permission_denied_states() {
        let backend = MockBackend::new(16_000);
        *backend.device.lock().unwrap() = Err(CaptureError::NoDevice);
        let sup = MicSupervisor::new(backend, 4096);
        let t0 = base();
        sup.start();
        sup.poll(t0);
        assert_eq!(sup.status().state, CaptureState::NoDevice);
        drop(sup);

        let backend = MockBackend::new(16_000);
        *backend.permission.lock().unwrap() =
            (MicPermission::Denied, MicPermissionSource::WindowsConsentStore);
        let sup = MicSupervisor::new(backend, 4096);
        sup.start();
        sup.poll(t0);
        let s = sup.status();
        assert_eq!(s.state, CaptureState::PermissionDenied);
        assert_eq!(s.permission, MicPermission::Denied);
        assert_eq!(s.permission_source, MicPermissionSource::WindowsConsentStore);
    }

    #[test]
    fn stop_disables_and_clears_ring() {
        let sup = MicSupervisor::new(MockBackend::new(48_000), 4096);
        let t0 = base();
        sup.start();
        sup.poll(t0);
        sup.ring().push(&[0.5; 100]);
        sup.stop();
        assert_eq!(sup.status().state, CaptureState::Stopped);
        assert!(sup.ring().is_empty());
        // Polls while stopped are no-ops.
        sup.poll(t0 + Duration::from_secs(10));
        assert_eq!(sup.status().state, CaptureState::Stopped);
    }

    #[test]
    fn idle_supervisor_does_not_open() {
        let sup = MicSupervisor::new(MockBackend::new(16_000), 4096);
        sup.poll(base());
        assert_eq!(sup.status().state, CaptureState::Idle);
    }
}
