use rune::{
    AppSpec, Cx, El, Key, KeyEvent, Shortcut, ShortcutError, Tray, TrayError, TrayEvent, TrayIcon,
    TrayItem, TraySpec, Tx, Waker, WindowHandle, WindowSpec, open_window, windows::WindowOptions,
};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::runtime::Runtime as TokioRuntime;
use tokio::sync::mpsc as tokio_mpsc;
use voxcell_audio::{AudioEngine, AudioSnapshot};
use voxcell_core::canvas::CanvasDotData;
use voxcell_core::{
    ActiveView, ChannelMeta, ExpressionMood, HermesAgentSession, HermesCronJob, HermesGatewayInfo,
    HermesModelOption, HermesSkill, HermesToolset, MouthViseme, SessionOrigin, SessionStatus,
    TerminalOutcome, UiCustomizationConfig, VoiceSettings,
};
use voxcell_hermes::{
    GatewaySessionRecord, GatewayStreamEvent, HermesClient, ParsedApprovalRequest,
    ProfileGatewayConfig, StreamSignal,
};
use voxcell_store::VoxcellStore;
use voxcell_store::updater::{
    self, AppUpdateStatus, GitRunner, LocalBuildMetadata, StepStatus, SystemRunner,
    UpdateStepResult, UpstreamReleaseInfo,
};
use voxcell_store::vault::{BackupReason, PreferenceBackupSummary, PreferencesVault, UpdateReceipt};
use voxcell_ui::state::{
    SettingsAction, UpdaterStatus, UpdaterStep, VaultBackupEntry, VaultUpdateReceipt,
    settings_mood_from_id,
};
use voxcell_ui::{AppState, UiCommand, VoxcellTheme};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub enum BridgeEvent {
    GatewayProbe {
        info: HermesGatewayInfo,
        models: Vec<HermesModelOption>,
        profiles: Vec<voxcell_core::GatewayProfile>,
    },
    GatewaySessions(Vec<GatewaySessionRecord>),
    Skills(Vec<HermesSkill>),
    Toolsets(Vec<HermesToolset>),
    CronJobs(Vec<HermesCronJob>),
    Stream(GatewayStreamEvent),
    StatusNote(String),
    /// P4-03: OS integration events (tray menu, global PTT hotkey, notification clicks).
    Os(OsAction),
    /// P3-06: async upstream release probe result for the settings updater.
    SettingsProbe(UpstreamReleaseInfo),
    /// P5 queue-forms: New-Job create outcome (offline-safe error surfacing).
    JobFormOutcome(Result<(), String>),
    /// P5 queue-forms: profile key persistence outcome (register/delete).
    ProfileFormOutcome(Result<(), String>),
}

/// P4-03: actions originating outside the view tree (tray, hotkey, toasts).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OsAction {
    TrayShow,
    TrayPtt,
    TrayPhone,
    TrayQuit,
    PttHotkey,
    NotifyClick {
        id: String,
        action: Option<String>,
        channel: Option<String>,
        request: Option<String>,
    },
}

pub struct DesktopBridge {
    tokio: TokioRuntime,
    store: Mutex<Option<VoxcellStore>>,
    client: Mutex<Arc<HermesClient>>,
    tx: std::sync::mpsc::Sender<BridgeEvent>,
    rx: Mutex<std::sync::mpsc::Receiver<BridgeEvent>>,
    waker: Waker,
    deepgram_key: Mutex<String>,
    /// P4-03 tray companion (live status dot + approval badge + quick menu).
    tray: Mutex<TraySlot>,
    /// P4-02 detached phone window / companion subprocess lifecycle.
    phone: Mutex<PhoneSlot>,
    /// P3-06 preferences vault (web-contract snapshots, backups, receipts).
    vault: Mutex<Option<PreferencesVault>>,
    /// P3-06 updater app root (package.json / .git location; TS process.cwd parity).
    app_root: PathBuf,
}

static BRIDGE: OnceLock<DesktopBridge> = OnceLock::new();

thread_local! {
    static AUDIO: std::cell::RefCell<Option<AudioEngine>> = const { std::cell::RefCell::new(None) };
}

// ---------------------------------------------------------------------------
// P4-02: detached phone window state plumbing
// ---------------------------------------------------------------------------

/// Snapshot of the main window's call state, published every `poll_bridge` and
/// applied by the detached phone window's poll. `phone_pixels` is a shared
/// `rune::Pixels` handle (Arc inside), so the phone window shows the main
/// window's live portrait without re-rendering it.
#[derive(Clone)]
struct PhoneSnapshot {
    open: bool,
    started_ms: Option<u64>,
    audio: AudioSnapshot,
    mood: ExpressionMood,
    viseme: MouthViseme,
    energy: f32,
    time_s: f32,
    now_ms: u64,
    voice: VoiceSettings,
    ui_config: UiCustomizationConfig,
    gateway_online: bool,
    phone_pixels: rune::Pixels,
}

static PHONE_SNAPSHOT: Mutex<Option<PhoneSnapshot>> = Mutex::new(None);
/// Commands raised in the phone window's own state, forwarded to the main
/// window's bridge poll (the main AppState is the single source of truth).
static PHONE_TO_MAIN: Mutex<Vec<UiCommand>> = Mutex::new(Vec::new());

/// How many polls (~33 ms each) to wait for the detached window's first frame
/// before declaring multi-window unavailable and spawning the `--scene phone`
/// companion subprocess instead.
const PHONE_WINDOW_GRACE_POLLS: u32 = 90;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhoneMode {
    Embedded,
    Detached,
    Subprocess,
}

#[derive(Default)]
struct PhoneSlot {
    handle: Option<WindowHandle>,
    wait_polls: u32,
    child: Option<std::process::Child>,
    mode: PhoneMode,
    spawn_failed: bool,
}

impl Default for PhoneMode {
    fn default() -> Self {
        PhoneMode::Embedded
    }
}

/// True when this process IS the phone companion (`--scene phone`), matching
/// the arg convention in `main.rs`.
fn is_phone_scene_process() -> bool {
    std::env::args().any(|a| a == "phone")
}

/// P4-02 window frame per `os_integration_probe.rs`: 320×420, always-on-top,
/// frameless, translucent, fixed size.
pub fn phone_window_options() -> WindowOptions {
    WindowOptions {
        transparent: true,
        borderless: true,
        always_on_top: true,
        resizable: false,
        visible: true,
        min_size: Some((320.0, 420.0)),
        max_size: Some((320.0, 420.0)),
        ..Default::default()
    }
}

/// Pure fallback decision: headless runs keep the embedded dock; a detached
/// window that never produced a frame within the grace period degrades to the
/// `--scene phone` companion subprocess (spec §3 / P0-03 fallback).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhoneWindowPlan {
    EmbeddedOnly,
    OpenOrKeepDetached,
    SpawnCompanion,
}

pub fn phone_window_plan(headless: bool, window_ready: bool, waited_polls: u32) -> PhoneWindowPlan {
    if headless {
        PhoneWindowPlan::EmbeddedOnly
    } else if window_ready || waited_polls < PHONE_WINDOW_GRACE_POLLS {
        PhoneWindowPlan::OpenOrKeepDetached
    } else {
        PhoneWindowPlan::SpawnCompanion
    }
}

fn spawn_phone_companion() -> std::io::Result<std::process::Child> {
    std::process::Command::new(std::env::current_exe()?)
        .args(["--scene", "phone"])
        .spawn()
}

fn phone_window_view(state: &AppState, cx: &mut Cx<AppState>) -> El {
    let theme = VoxcellTheme::from_config(&state.ui_config);
    voxcell_ui::render_phone_widget(state, &theme, cx)
}

fn phone_init() -> AppState {
    let mut s = AppState::new();
    s.phone_widget_open = true;
    s.sync_phone_call_timer();
    s
}

fn phone_app_spec() -> AppSpec<AppState> {
    AppSpec {
        min_size: Some((320.0, 420.0)),
        poll: Some(poll_phone_window),
        on_key: Some(phone_on_key),
        ..AppSpec::new(
            "Voxcell // Cyber Comms",
            (320.0, 420.0),
            phone_init,
            phone_window_view,
        )
    }
}

fn poll_phone_window(state: &mut AppState, t_seconds: f64, tx: &mut Tx) -> bool {
    let target_t = t_seconds as f32;
    let dt = if target_t > state.time_s {
        (target_t - state.time_s).clamp(0.008, 0.05)
    } else {
        0.016
    };
    let _ = step_phone_window(state, dt);
    tx.wake_after(std::time::Duration::from_millis(33));
    true
}

/// Testable core of the phone window poll: forward local commands to the main
/// bridge, then apply the latest main-window snapshot. No `Tx` needed.
fn step_phone_window(state: &mut AppState, _dt: f32) -> bool {
    let cmds: Vec<UiCommand> = state.pending_commands.drain(..).collect();
    let forwarded = !cmds.is_empty();
    if forwarded {
        PHONE_TO_MAIN.lock().unwrap().extend(cmds);
    }
    let applied = apply_phone_snapshot(state);
    forwarded || applied
}

/// Focused-window keys for the phone window: Escape hangs up, and
/// Cmd/Ctrl+Shift+Space is the PTT fallback when the global hotkey is
/// unavailable (Wayland / headless).
fn phone_on_key(state: &mut AppState, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    if ev.key == Key::Escape {
        state.phone_widget_open = false;
        state.phone_call_started_ms = None;
        state.pending_commands.push(UiCommand::TogglePhoneWindow(false));
        return true;
    }
    if ev.mods.cmd && ev.mods.shift {
        if let Key::Text(t) = &ev.key {
            if t == " " {
                toggle_ptt(state);
                return true;
            }
        }
    }
    false
}

fn publish_phone_snapshot(state: &AppState) {
    let snap = PhoneSnapshot {
        open: state.phone_widget_open,
        started_ms: state.phone_call_started_ms,
        audio: state.audio.clone(),
        mood: state.stage_mood,
        viseme: state.stage_viseme,
        energy: state.stage_energy,
        time_s: state.time_s,
        now_ms: state.now_ms,
        voice: state.voice.clone(),
        ui_config: state.ui_config.clone(),
        gateway_online: state.gateway.online,
        phone_pixels: state.phone_pixels.clone(),
    };
    *PHONE_SNAPSHOT.lock().unwrap() = Some(snap);
}

fn apply_phone_snapshot(state: &mut AppState) -> bool {
    let guard = PHONE_SNAPSHOT.lock().unwrap();
    let Some(snap) = guard.as_ref() else {
        return false;
    };
    state.phone_widget_open = snap.open;
    state.phone_call_started_ms = snap.started_ms;
    state.audio = snap.audio.clone();
    state.stage_mood = snap.mood;
    state.stage_viseme = snap.viseme;
    state.stage_energy = snap.energy;
    state.time_s = snap.time_s;
    state.now_ms = snap.now_ms;
    state.voice = snap.voice.clone();
    state.ui_config = snap.ui_config.clone();
    state.gateway.online = snap.gateway_online;
    state.phone_pixels = snap.phone_pixels.clone();
    true
}

/// Drives the detached window / companion lifecycle from `phone_widget_open`.
/// Runs on the UI thread each poll; `open_window` must be called there.
fn sync_phone_window(b: &DesktopBridge, state: &mut AppState) {
    if rune::is_headless() {
        state.phone_detached = false;
        return;
    }
    let mut p = b.phone.lock().unwrap();
    if state.phone_widget_open {
        // Reap a companion that exited on its own (user closed its window).
        if let Some(child) = p.child.as_mut() {
            if matches!(child.try_wait(), Ok(Some(_))) {
                p.child = None;
                p.mode = PhoneMode::Embedded;
                state.phone_widget_open = false;
                state.phone_detached = false;
                return;
            }
        }
        if p.handle.is_none() && p.child.is_none() && !p.spawn_failed {
            let spec =
                WindowSpec::new("voxcell-phone", phone_app_spec()).options(phone_window_options());
            p.handle = Some(open_window(spec));
            p.wait_polls = 0;
        }
        if p.handle.is_some() {
            let ready = p.handle.as_ref().map(|h| h.is_ready()).unwrap_or(false);
            p.wait_polls += 1;
            match phone_window_plan(false, ready, p.wait_polls) {
                PhoneWindowPlan::OpenOrKeepDetached if ready => {
                    p.mode = PhoneMode::Detached;
                    state.phone_detached = true;
                }
                PhoneWindowPlan::SpawnCompanion => {
                    // Multi-window unavailable: degrade to the --scene phone
                    // companion subprocess (never panic).
                    p.handle = None;
                    match spawn_phone_companion() {
                        Ok(child) => {
                            p.child = Some(child);
                            p.mode = PhoneMode::Subprocess;
                            state.phone_detached = true;
                            let _ = b.tx.send(BridgeEvent::StatusNote(
                                "phone widget: multi-window unavailable — launched --scene phone companion"
                                    .to_string(),
                            ));
                            (b.waker)();
                        }
                        Err(e) => {
                            p.spawn_failed = true;
                            p.mode = PhoneMode::Embedded;
                            let _ = b.tx.send(BridgeEvent::StatusNote(format!(
                                "phone widget: companion fallback failed ({e}) — embedded dock only"
                            )));
                            (b.waker)();
                        }
                    }
                }
                _ => {}
            }
        } else if p.child.is_some() {
            p.mode = PhoneMode::Subprocess;
            state.phone_detached = true;
        }
    } else {
        if let Some(handle) = p.handle.take() {
            handle.close();
        }
        if let Some(mut child) = p.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        p.mode = PhoneMode::Embedded;
        p.wait_polls = 0;
        p.spawn_failed = false;
        state.phone_detached = false;
    }
}

// ---------------------------------------------------------------------------
// P4-03: tray companion (status dot + approval badge + quick menu)
// ---------------------------------------------------------------------------

/// Semantic tray status. Colors follow the brief: slate chrome, emerald = live,
/// amber = working, rose = error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayStatus {
    Offline,
    Idle,
    Listening,
    Speaking,
    Working,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayVisuals {
    pub status: TrayStatus,
    pub approvals: usize,
}

/// Pure state -> tray mapping (headless-testable).
pub fn tray_visuals(state: &AppState) -> TrayVisuals {
    let thinking = state.thinking_by_channel.values().any(|&t| t)
        || state
            .tasks
            .iter()
            .any(|t| t.status == voxcell_core::TaskLifecycleStatus::Running);
    let status = if state.gateway.last_error.is_some() {
        TrayStatus::Error
    } else if state.audio.is_speaking || state.stage_mood == ExpressionMood::Speaking {
        TrayStatus::Speaking
    } else if state.audio.is_listening || state.voice.hands_free_vad {
        TrayStatus::Listening
    } else if thinking {
        TrayStatus::Working
    } else if !state.gateway.online {
        TrayStatus::Offline
    } else {
        TrayStatus::Idle
    };
    TrayVisuals {
        status,
        approvals: state.pending_approvals().len(),
    }
}

fn status_rgb(status: TrayStatus) -> (u8, u8, u8) {
    match status {
        TrayStatus::Offline => (0x64, 0x74, 0x8B),
        TrayStatus::Idle => (0x94, 0xA3, 0xB8),
        TrayStatus::Listening => (0x34, 0xD3, 0x99),
        TrayStatus::Speaking => (0x34, 0xD3, 0x99),
        TrayStatus::Working => (0xF5, 0x9E, 0x0B),
        TrayStatus::Error => (0xF4, 0x3F, 0x5E),
    }
}

/// RGBA-bake workaround (documented in `os_integration_probe.rs` / gate1 §C.2):
/// RUNE's tray has no badge API and the icon is fixed at `TraySpec` time, so the
/// amber approval badge is baked into the icon pixels and live updates happen
/// by drop + recreate (`sync_tray`).
pub fn tray_icon(v: TrayVisuals) -> TrayIcon {
    TrayIcon::rgba(16, 16, tray_icon_pixels(v))
}

/// The baked 16×16 RGBA buffer (straight alpha), exposed for headless tests.
fn tray_icon_pixels(v: TrayVisuals) -> Vec<u8> {
    let (w, h) = (16u32, 16u32);
    let (r, g, b) = status_rgb(v.status);
    let badge = v.approvals > 0;
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let dx = x as f32 - 7.5;
            let dy = y as f32 - 7.5;
            if dx * dx + dy * dy <= 36.0 {
                px[i] = r;
                px[i + 1] = g;
                px[i + 2] = b;
                px[i + 3] = 0xFF;
            }
            if badge && x >= 9 && y <= 6 {
                px[i] = 0xF5;
                px[i + 1] = 0x9E;
                px[i + 2] = 0x0B;
                px[i + 3] = 0xFF;
            }
        }
    }
    px
}

pub fn tray_tooltip(v: TrayVisuals) -> String {
    let label = match v.status {
        TrayStatus::Offline => "offline",
        TrayStatus::Idle => "idle",
        TrayStatus::Listening => "listening",
        TrayStatus::Speaking => "speaking",
        TrayStatus::Working => "working",
        TrayStatus::Error => "error",
    };
    if v.approvals > 0 {
        format!(
            "Voxcell — {label} · {} approval{} pending",
            v.approvals,
            if v.approvals == 1 { "" } else { "s" }
        )
    } else {
        format!("Voxcell — {label}")
    }
}

fn tray_menu(mic_on: bool, phone_open: bool) -> Vec<TrayItem> {
    vec![
        TrayItem::action("show", "Show Voxcell"),
        TrayItem::action("ptt", if mic_on { "Release PTT" } else { "Push to talk" }),
        TrayItem::action(
            "phone",
            if phone_open { "Close Phone HUD" } else { "Open Phone HUD" },
        ),
        TrayItem::Separator,
        TrayItem::action("quit", "Quit"),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TrayKey {
    visuals: TrayVisuals,
    mic_on: bool,
    phone_open: bool,
}

#[derive(Default)]
struct TraySlot {
    tray: Option<Tray>,
    last: Option<TrayKey>,
    failed: bool,
}

/// Spec §10 mitigation: every tray failure degrades to an in-app StatusNote,
/// never a panic. `TrayError` is `#[non_exhaustive]` — wildcard arm required.
pub fn tray_error_note(err: &TrayError) -> String {
    match err {
        TrayError::Unsupported => {
            "tray: unsupported on this platform — in-app status fallback".to_string()
        }
        TrayError::Backend(e) => format!("tray: backend error ({e}) — in-app status fallback"),
        other => format!("tray: {other} — in-app status fallback"),
    }
}

fn tray_spec(key: TrayKey) -> TraySpec {
    TraySpec::new("dev.voxcell.desktop", "Voxcell", tray_icon(key.visuals))
        .tooltip(tray_tooltip(key.visuals))
        .menu(tray_menu(key.mic_on, key.phone_open))
}

/// Install (or reinstall) the tray. Returns the platform `TrayError` on
/// failure so the caller can surface `tray_error_note` in-app; a missing tray
/// is marked failed once and never retried every frame.
fn try_install_tray(
    spec: TraySpec,
    tx: std::sync::mpsc::Sender<BridgeEvent>,
    waker: Waker,
) -> Result<Tray, TrayError> {
    Tray::new(spec, move |ev| {
        // Runs on the tray's own thread: keep it short, hand work to the app.
        let action = match ev {
            TrayEvent::Activate { .. } => OsAction::TrayShow,
            TrayEvent::Menu(id) => match id.as_str() {
                "show" => OsAction::TrayShow,
                "ptt" => OsAction::TrayPtt,
                "phone" => OsAction::TrayPhone,
                "quit" => OsAction::TrayQuit,
                _ => return,
            },
            _ => return, // future TrayEvent variants (non_exhaustive)
        };
        let _ = tx.send(BridgeEvent::Os(action));
        (waker)();
    })
}

fn sync_tray(b: &DesktopBridge, state: &AppState) {
    let mut t = b.tray.lock().unwrap();
    if t.failed {
        return;
    }
    let key = TrayKey {
        visuals: tray_visuals(state),
        mic_on: state.voice.hands_free_vad || state.audio.is_listening,
        phone_open: state.phone_widget_open,
    };
    if t.last.as_ref() == Some(&key) {
        return;
    }
    // Live icon/tooltip/menu update = drop + recreate (no badge API; probe §C.2).
    t.tray = None;
    let tx = b.tx.clone();
    let waker = b.waker.clone();
    match try_install_tray(tray_spec(key), tx, waker) {
        Ok(tray) => {
            t.tray = Some(tray);
            t.last = Some(key);
        }
        Err(err) => {
            t.failed = true;
            t.last = None;
            let _ = b.tx.send(BridgeEvent::StatusNote(tray_error_note(&err)));
            (b.waker)();
        }
    }
}

// ---------------------------------------------------------------------------
// P4-03: global PTT hotkey (Cmd/Ctrl+Shift+Space) with graceful degradation
// ---------------------------------------------------------------------------

pub fn ptt_accelerator() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd+Shift+Space"
    } else {
        "Ctrl+Shift+Space"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutOutcome {
    Registered,
    /// Wayland/headless: focused-window Cmd/Ctrl+Shift+Space remains available.
    FallbackFocusKey,
    PortalConsentDeclined,
    ConflictAskOwner,
    BadAccelerator,
    NotRunningYet,
    UnknownId,
    BackendFallback,
}

/// Pure classifier over the full `ShortcutError` taxonomy (probe §C.3); every
/// failure maps to a spec §10 mitigation, never a panic.
pub fn classify_shortcut_result(r: Result<rune::ShortcutId, ShortcutError>) -> ShortcutOutcome {
    match r {
        Ok(_) => ShortcutOutcome::Registered,
        Err(ShortcutError::Unsupported) => ShortcutOutcome::FallbackFocusKey,
        Err(ShortcutError::PermissionDenied) => ShortcutOutcome::PortalConsentDeclined,
        Err(ShortcutError::Conflict) => ShortcutOutcome::ConflictAskOwner,
        Err(ShortcutError::InvalidAccelerator(_)) => ShortcutOutcome::BadAccelerator,
        Err(ShortcutError::NotRunning) => ShortcutOutcome::NotRunningYet,
        Err(ShortcutError::UnknownId) => ShortcutOutcome::UnknownId,
        Err(ShortcutError::Backend(_)) => ShortcutOutcome::BackendFallback,
    }
}

fn toggle_ptt(state: &mut AppState) {
    let next = !state.voice.hands_free_vad;
    state.voice.hands_free_vad = next;
    state.pending_commands.push(UiCommand::ToggleMic(next));
}

/// Handler for `App::global_shortcut` / `AppHandle::register_shortcut`.
pub fn ptt_shortcut_handler(state: &mut AppState, _tx: &mut Tx) {
    toggle_ptt(state);
}

/// Register the global PTT hotkey on the running app. Call from startup once an
/// `AppHandle` exists (see report: needs one line in `main.rs`, owned by
/// theme-chrome). Every failure degrades to the focused-window PTT key already
/// handled in `main.rs::on_key` + an in-app StatusNote.
pub fn register_ptt_hotkey(handle: &rune::AppHandle<AppState>) -> ShortcutOutcome {
    let shortcut: Shortcut = match ptt_accelerator().parse() {
        Ok(s) => s,
        Err(_) => {
            if let Some(b) = BRIDGE.get() {
                let _ = b.tx.send(BridgeEvent::StatusNote(
                    "PTT hotkey: bad accelerator — focused-window PTT remains available".to_string(),
                ));
                (b.waker)();
            }
            return ShortcutOutcome::BadAccelerator;
        }
    };
    let outcome = classify_shortcut_result(handle.register_shortcut(shortcut, ptt_shortcut_handler));
    if outcome != ShortcutOutcome::Registered {
        if let Some(b) = BRIDGE.get() {
            let _ = b.tx.send(BridgeEvent::StatusNote(format!(
                "PTT hotkey: {outcome:?} — focused-window Cmd/Ctrl+Shift+Space remains available"
            )));
            (b.waker)();
        }
    }
    outcome
}

// ---------------------------------------------------------------------------
// P4-03: rune-notify approval/completion alerts with click-to-focus
// ---------------------------------------------------------------------------

/// Approval toast: Approve/Deny buttons + channel/request routing info.
pub fn approval_notification(req: &ParsedApprovalRequest, channel_id: &str) -> rune_notify::Notification {
    rune_notify::Notification::new(
        format!("voxcell-apr-{}", req.request_id),
        format!(
            "Approval Needed: {}",
            req.tool_name.as_deref().unwrap_or("Tool")
        ),
    )
    .body(req.summary.clone())
    .info("channel", channel_id)
    .info("request", req.request_id.clone())
    .action("approve", "Approve")
    .action("deny", "Deny")
}

/// Completion toast: click (or "Open Thread") focuses the originating thread.
pub fn completion_notification(channel_id: &str, preview: &str) -> rune_notify::Notification {
    rune_notify::Notification::new("voxcell-turn-done", "Hermes Turn Complete")
        .body(preview)
        .info("channel", channel_id)
        .action("open", "Open Thread")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotifyRoute {
    FocusApproval {
        channel: Option<String>,
        request: String,
    },
    ResolveApproval {
        channel: Option<String>,
        request: String,
        decision: &'static str,
    },
    FocusThread {
        channel: Option<String>,
    },
    Ignore,
}

/// Pure click routing (headless-testable): approval toasts focus the Queue card
/// or resolve directly from their buttons; completion toasts focus the thread.
pub fn notify_click_route(click: &rune_notify::Click) -> NotifyRoute {
    notify_click_route_parts(
        &click.id,
        click.action(),
        click.get("channel"),
        click.get("request"),
    )
}

fn notify_click_route_parts(
    id: &str,
    action: Option<&str>,
    channel: Option<&str>,
    request: Option<&str>,
) -> NotifyRoute {
    if let Some(req_from_id) = id.strip_prefix("voxcell-apr-") {
        let request = request.unwrap_or(req_from_id).to_string();
        let channel = channel.map(|c| c.to_string());
        return match action {
            Some("approve") => NotifyRoute::ResolveApproval {
                channel,
                request,
                decision: "once",
            },
            Some("deny") => NotifyRoute::ResolveApproval {
                channel,
                request,
                decision: "deny",
            },
            _ => NotifyRoute::FocusApproval { channel, request },
        };
    }
    if id == "voxcell-turn-done" || id.starts_with("voxcell-done-") {
        return NotifyRoute::FocusThread {
            channel: channel.map(|c| c.to_string()),
        };
    }
    NotifyRoute::Ignore
}

fn handle_os_action(state: &mut AppState, action: OsAction) {
    match action {
        OsAction::TrayShow => {
            rune_notify::bring_to_front();
        }
        OsAction::TrayPtt | OsAction::PttHotkey => toggle_ptt(state),
        OsAction::TrayPhone => {
            let next = !state.phone_widget_open;
            state.phone_widget_open = next;
            state.pending_commands.push(UiCommand::TogglePhoneWindow(next));
            state.sync_phone_call_timer();
        }
        OsAction::TrayQuit => {
            // Tray icons are removed by the OS when the process exits.
            std::process::exit(0);
        }
        OsAction::NotifyClick {
            id,
            action,
            channel,
            request,
        } => {
            let route = notify_click_route_parts(
                &id,
                action.as_deref(),
                channel.as_deref(),
                request.as_deref(),
            );
            match route {
                NotifyRoute::FocusApproval { channel, .. } => {
                    state.active_view = ActiveView::Queue;
                    if let Some(ch) = channel {
                        state.active_channel_id = ch;
                    }
                    rune_notify::bring_to_front();
                }
                NotifyRoute::ResolveApproval {
                    channel,
                    request,
                    decision,
                } => {
                    state.active_view = ActiveView::Queue;
                    if let Some(ch) = channel {
                        state.active_channel_id = ch;
                    }
                    if let Some(req) = state
                        .approvals
                        .iter()
                        .find(|a| !a.resolved && a.request_id == request)
                    {
                        state.pending_commands.push(UiCommand::ResolveApproval {
                            run_id: req.run_id.clone(),
                            request_id: req.request_id.clone(),
                            execution_generation: req.execution_generation,
                            decision: decision.to_string(),
                            is_clarify: false,
                        });
                    }
                    rune_notify::bring_to_front();
                }
                NotifyRoute::FocusThread { channel } => {
                    state.active_view = ActiveView::Thread;
                    if let Some(ch) = channel {
                        state.active_channel_id = ch;
                    }
                    rune_notify::bring_to_front();
                }
                NotifyRoute::Ignore => {}
            }
        }
    }
}

/// Register the toast identity + click router at startup. `register` is
/// Result-based; failure degrades to in-app alerts only (never panics).
fn init_notify(b: &DesktopBridge) {
    let identity = rune_notify::Identity::new("dev.voxcell.desktop", "Voxcell");
    if let Err(e) = rune_notify::register(&identity) {
        let _ = b.tx.send(BridgeEvent::StatusNote(format!(
            "notifications unavailable ({e}) — in-app alerts only"
        )));
    }
    let tx = b.tx.clone();
    let waker = b.waker.clone();
    rune_notify::on_click(move |click| {
        let action = OsAction::NotifyClick {
            id: click.id.clone(),
            action: click.action().map(|s| s.to_string()),
            channel: click.get("channel").map(|s| s.to_string()),
            request: click.get("request").map(|s| s.to_string()),
        };
        let _ = tx.send(BridgeEvent::Os(action));
        (waker)();
    });
}

// ---------------------------------------------------------------------------
// Bridge lifecycle
// ---------------------------------------------------------------------------

pub fn init_bridge(state: &mut AppState, waker: Waker) {
    if BRIDGE.get().is_some() {
        return;
    }

    let store = VoxcellStore::open_default().ok();
    if let Some(ref s) = store {
        state.ui_config = s.load_ui_config();
        state.bg_settings = s.load_background_settings();
        state.voice = s.load_voice_settings();
        if let Ok(dots) = s.load_canvas_dots() {
            if !dots.is_empty() {
                state.dots = dots;
            }
        }
        state.has_hermes_key = s.get_secret("HERMES_API_KEY").is_some();
        state.has_deepgram_key = s.get_secret("DEEPGRAM_API_KEY").is_some();
        if let Some(url) = s.get_secret("HERMES_API_URL") {
            state.gateway_base_url = url.clone();
            state.gateway.base_url = url.clone();
            state.gateway_url_edit.replace_text(&url);
        }
    }

    let mut client = HermesClient::from_env();
    if let Some(ref s) = store {
        if let Some(key) = s.get_secret("HERMES_API_KEY") {
            client = HermesClient::new(ProfileGatewayConfig {
                base_url: state.gateway_base_url.clone(),
                profile: client.config.profile.clone(),
                api_key: key,
                route_prefix: client.config.route_prefix.clone(),
            });
        }
    }
    let deepgram_key = store
        .as_ref()
        .and_then(|s| s.get_secret("DEEPGRAM_API_KEY"))
        .or_else(|| std::env::var("DEEPGRAM_API_KEY").ok())
        .unwrap_or_default();
    if !deepgram_key.is_empty() {
        state.has_deepgram_key = true;
    }

    // P3-06: preferences vault + updater app root. The vault lives in the
    // TS-parity data dir (VOXCELL_DATA_DIR / ~/.voxcell / app-root fallback);
    // failures degrade to "vault unavailable" feedback, never a panic.
    let app_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vault = updater::resolve_voxcell_data_dir(&app_root)
        .ok()
        .and_then(|dir| PreferencesVault::open(dir).ok());
    if let Some(ref v) = vault {
        state.vault_backups = v.list_backups().iter().map(vault_backup_entry).collect();
        state.vault_storage_label = v.storage_label();
        // Post-update receipt surfaces on next launch (TS parity) without
        // waiting for the first manual probe.
        if let Some(receipt) = v.read_receipt() {
            let local = updater::read_local_build_metadata(&app_root);
            let status =
                updater::build_app_update_status(&local, None, Some(receipt), None, None);
            state.update_status = Some(updater_status_mirror(&status));
        }
    }

    let tokio = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime");

    let audio = AudioEngine::init();
    audio.set_muted(state.voice.muted);
    AUDIO.with(|cell| {
        *cell.borrow_mut() = Some(audio);
    });

    let (tx, rx) = std::sync::mpsc::channel();
    let bridge = DesktopBridge {
        tokio,
        store: Mutex::new(store),
        client: Mutex::new(Arc::new(client)),
        tx,
        rx: Mutex::new(rx),
        waker,
        deepgram_key: Mutex::new(deepgram_key),
        tray: Mutex::new(TraySlot::default()),
        phone: Mutex::new(PhoneSlot::default()),
        vault: Mutex::new(vault),
        app_root,
    };

    let _ = BRIDGE.set(bridge);
    // The --scene phone companion is a bare widget window: no second tray icon
    // or toast identity (the main process owns OS integration).
    if !is_phone_scene_process() {
        if let Some(b) = BRIDGE.get() {
            init_notify(b);
        }
    }
    trigger_gateway_refresh();
}

pub fn trigger_gateway_refresh() {
    let Some(b) = BRIDGE.get() else { return };
    let client = Arc::clone(&b.client.lock().unwrap());
    let tx = b.tx.clone();
    let waker = b.waker.clone();

    b.tokio.spawn(async move {
        let (info, models, profiles) = client.probe_gateway().await;
        let online = info.online;
        let _ = tx.send(BridgeEvent::GatewayProbe {
            info,
            models,
            profiles,
        });
        (waker)();

        if online {
            if let Ok(sessions) = client.fetch_sessions().await {
                let _ = tx.send(BridgeEvent::GatewaySessions(sessions));
                (waker)();
            }
            if let Ok(skills) = client.fetch_skills().await {
                let _ = tx.send(BridgeEvent::Skills(skills));
                (waker)();
            }
            if let Ok(toolsets) = client.fetch_toolsets().await {
                let _ = tx.send(BridgeEvent::Toolsets(toolsets));
                (waker)();
            }
            if let Ok(jobs) = client.fetch_jobs().await {
                let _ = tx.send(BridgeEvent::CronJobs(jobs));
                (waker)();
            }
        }
    });
}

pub fn poll_bridge(state: &mut AppState, dt: f32) -> bool {
    let Some(b) = BRIDGE.get() else {
        return false;
    };
    let mut changed = false;

    // 1. Drain UI commands queued by button/input handlers (main window and
    //    the detached phone window, whose commands arrive via PHONE_TO_MAIN).
    let mut cmds: Vec<UiCommand> = state.pending_commands.drain(..).collect();
    cmds.extend(PHONE_TO_MAIN.lock().unwrap().drain(..));
    for cmd in cmds {
        changed = true;
        handle_command(b, state, cmd);
    }

    // 1.5 P3-06: drain the settings vault/updater outbox. The settings view
    //     dispatches through state.settings_outbox (no new UiCommand variants
    //     — see the P3-06 coordination note); vault ops run synchronously on
    //     the UI thread (local file IO), the updater probe fetches upstream
    //     async and finalizes via BridgeEvent::SettingsProbe.
    if poll_settings_outbox(b, state) {
        changed = true;
    }

    // 2. Step audio FFT engine
    AUDIO.with(|cell| {
        if let Some(audio) = cell.borrow().as_ref() {
            let snap = audio.step_snapshot(dt);
            if snap.is_speaking != state.audio.is_speaking
                || snap.is_listening != state.audio.is_listening
                || (snap.output_level - state.audio.output_level).abs() > 0.01
                || (snap.input_level - state.audio.input_level).abs() > 0.01
            {
                changed = true;
            }
            state.audio = snap;
        }
    });

    // 3. Drain incoming async Hermes/Gateway/OS events
    if let Ok(rx) = b.rx.lock() {
        while let Ok(ev) = rx.try_recv() {
            changed = true;
            match ev {
                BridgeEvent::GatewayProbe {
                    info,
                    models,
                    profiles,
                } => {
                    state.gateway = info;
                    if !models.is_empty() {
                        state.models = models;
                    }
                    if !profiles.is_empty() {
                        state.gateway_profiles = profiles;
                    }
                }
                BridgeEvent::GatewaySessions(records) => {
                    apply_gateway_sessions(state, records);
                }
                BridgeEvent::Skills(skills) => {
                    state.skills = skills;
                }
                BridgeEvent::Toolsets(toolsets) => {
                    state.toolsets = toolsets;
                }
                BridgeEvent::CronJobs(jobs) => {
                    state.cron_jobs = jobs;
                }
                BridgeEvent::Stream(stream_ev) => {
                    maybe_notify(&stream_ev);
                    state.apply_hermes_event(stream_ev);
                }
                BridgeEvent::StatusNote(note) => {
                    state.cli_log.push(note);
                }
                BridgeEvent::Os(action) => {
                    handle_os_action(state, action);
                }
                BridgeEvent::SettingsProbe(upstream) => {
                    let guard = b.vault.lock().unwrap();
                    let ctx = SettingsContext {
                        vault: guard.as_ref(),
                        app_root: &b.app_root,
                        runner: &SystemRunner,
                        upstream: Some(&upstream),
                        export_path: None,
                    };
                    finish_update_probe(state, &ctx);
                }
                BridgeEvent::JobFormOutcome(result) => {
                    state.queue_apply_job_outcome(result);
                }
                BridgeEvent::ProfileFormOutcome(result) => {
                    state.queue_apply_profile_outcome(result);
                }
            }
        }
    }

    // 4. P4-02/P4-03 OS integration upkeep (UI thread).
    state.sync_phone_call_timer();
    publish_phone_snapshot(state);
    if !is_phone_scene_process() {
        sync_tray(b, state);
    }
    sync_phone_window(b, state);

    changed
}

fn maybe_notify(ev: &GatewayStreamEvent) {
    if !rune_notify::available() {
        return;
    }
    match ev {
        GatewayStreamEvent::Signal {
            channel_id,
            signal: StreamSignal::ApprovalRequest(req),
        } => {
            rune_notify::post(&approval_notification(req, channel_id));
        }
        GatewayStreamEvent::Completed { channel_id, result }
            if result.outcome == TerminalOutcome::Completed =>
        {
            let preview: String = result.output.chars().take(120).collect();
            if !preview.trim().is_empty() {
                rune_notify::post(&completion_notification(channel_id, &preview));
            }
        }
        _ => {}
    }
}

fn apply_gateway_sessions(state: &mut AppState, records: Vec<GatewaySessionRecord>) {
    state.gateway_sessions = records
        .iter()
        .map(|r| HermesAgentSession {
            channel_id: format!("gw-{}", r.id),
            session_id: r.id.clone(),
            title: r.title.clone().unwrap_or_else(|| r.id.clone()),
            status: SessionStatus::Idle,
            model: r.model.clone().unwrap_or_else(|| state.gateway.model.clone()),
            source: r.source.clone().unwrap_or_else(|| "gateway".into()),
            last_message: r.last_message.clone().unwrap_or_default(),
            updated_at_ms: r.updated_at_ms,
            message_count: r.message_count,
        })
        .collect();

    for r in records {
        let ch_id = format!("gw-{}", r.id);
        if !state.channels.iter().any(|c| c.id == ch_id) {
            state.channels.push(ChannelMeta {
                id: ch_id,
                name: r.title.unwrap_or_else(|| r.id.clone()),
                category: "gateway".into(),
                description: r.last_message.unwrap_or_default(),
                session_id: r.id,
                unread: 0,
                origin: SessionOrigin::External,
                has_local_history: false,
                model: r.model,
                updated_at_ms: r.updated_at_ms,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// P5 queue-forms helpers: New-Job POST through the gateway client seam, plus
// pure mappings unit-tested without network.
// ---------------------------------------------------------------------------

/// Pure POST /api/jobs payload (web handleCreateJobSubmit): trimmed prompt +
/// schedule, optional name omitted when empty.
fn job_create_payload(name: &str, schedule: &str, prompt: &str) -> Value {
    let mut body = json!({
        "prompt": prompt.trim(),
        "schedule": schedule.trim(),
    });
    let name = name.trim();
    if !name.is_empty() {
        body["name"] = Value::String(name.to_string());
    }
    body
}

/// POST /api/jobs via the gateway client seam (`fetch_with_429_retry` +
/// `build_gateway_url`), mirroring the web server's /api/hermes/jobs proxy.
async fn create_gateway_job(
    client: &HermesClient,
    name: &str,
    schedule: &str,
    prompt: &str,
) -> Result<(), String> {
    let url = voxcell_hermes::build_gateway_url(&client.config, "/api/jobs", false);
    let body = job_create_payload(name, schedule, prompt);
    let api_key = client.config.api_key.clone();
    let http = reqwest::Client::new();
    let resp = client
        .fetch_with_429_retry(|| {
            let mut req = http
                .post(&url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .json(&body);
            if !api_key.is_empty() {
                req = req.header(
                    reqwest::header::AUTHORIZATION,
                    format!("Bearer {api_key}"),
                );
            }
            req
        })
        .await?;
    if !resp.status().is_success() {
        return Err(format!("job create HTTP {}", resp.status()));
    }
    Ok(())
}

/// Map the UI transport toggle onto the client's preferred transport.
fn preferred_for_transport(mode: voxcell_core::GatewayTransport) -> voxcell_hermes::PreferredTransport {
    match mode {
        voxcell_core::GatewayTransport::Runs => voxcell_hermes::PreferredTransport::Runs,
        voxcell_core::GatewayTransport::SessionStream => {
            voxcell_hermes::PreferredTransport::SessionStream
        }
        voxcell_core::GatewayTransport::ChatCompletions => {
            voxcell_hermes::PreferredTransport::ChatCompletions
        }
    }
}

/// Store secret name for a per-profile key — mirrors the profiles crate's
/// `HERMES_API_KEY_<SLUG>` env convention (non-alphanumeric runs -> one `_`,
/// uppercased).
fn profile_key_secret_name(id: &str) -> String {
    let mut slug = String::new();
    let mut prev_underscore = false;
    for c in id.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_uppercase());
            prev_underscore = false;
        } else if !prev_underscore {
            slug.push('_');
            prev_underscore = true;
        }
    }
    format!("HERMES_API_KEY_{slug}")
}

fn handle_command(b: &DesktopBridge, state: &AppState, cmd: UiCommand) {
    match cmd {
        UiCommand::SendPrompt {
            channel_id,
            session_id,
            prompt,
        } => {
            let client = Arc::clone(&b.client.lock().unwrap());
            let transport = state.gateway.active_transport;
            let model = state.selected_model.clone();
            let tx = b.tx.clone();
            let waker = b.waker.clone();

            b.tokio.spawn(async move {
                let (stx, mut srx) = tokio_mpsc::unbounded_channel();
                let waker_inner = waker.clone();
                let tx_fwd = tx.clone();
                let fwd = tokio::spawn(async move {
                    while let Some(ev) = srx.recv().await {
                        let _ = tx_fwd.send(BridgeEvent::Stream(ev));
                        (waker_inner)();
                    }
                });
                client
                    .stream_turn(
                        transport,
                        channel_id,
                        session_id.clone(),
                        format!("voxcell-{session_id}"),
                        prompt,
                        model,
                        stx,
                        move || (waker)(),
                    )
                    .await;
                let _ = fwd.await;
            });
        }
        UiCommand::ResolveApproval {
            run_id,
            request_id,
            execution_generation,
            decision,
            is_clarify,
        } => {
            let Some(rid) = run_id else { return };
            let client = Arc::clone(&b.client.lock().unwrap());
            let tx = b.tx.clone();
            let waker = b.waker.clone();
            b.tokio.spawn(async move {
                if let Err(e) = client
                    .submit_approval(&rid, &request_id, execution_generation, &decision, is_clarify)
                    .await
                {
                    let _ = tx.send(BridgeEvent::StatusNote(format!("[approval err] {e}")));
                    (waker)();
                }
            });
        }
        UiCommand::StopRun { run_id, .. } => {
            let client = Arc::clone(&b.client.lock().unwrap());
            b.tokio.spawn(async move {
                let _ = client.stop_run(&run_id).await;
            });
        }
        UiCommand::SteerRun { run_id, text } => {
            let client = Arc::clone(&b.client.lock().unwrap());
            b.tokio.spawn(async move {
                let _ = client.steer_run(&run_id, &text).await;
            });
        }
        UiCommand::ResetSession { session_id, .. } => {
            let client = Arc::clone(&b.client.lock().unwrap());
            b.tokio.spawn(async move {
                let _ = client.reset_session(&session_id).await;
            });
        }
        UiCommand::SwitchProfile { profile } => {
            let mut guard = b.client.lock().unwrap();
            let next = guard.with_profile(&profile, None);
            *guard = Arc::new(next);
            drop(guard);
            trigger_gateway_refresh();
        }
        UiCommand::SaveGatewaySettings {
            base_url,
            hermes_api_key,
            deepgram_api_key,
        } => {
            if let Ok(guard) = b.store.lock() {
                if let Some(ref s) = *guard {
                    if !base_url.is_empty() {
                        let _ = s.set_secret("HERMES_API_URL", &base_url);
                    }
                    if !hermes_api_key.is_empty() {
                        let _ = s.set_secret("HERMES_API_KEY", &hermes_api_key);
                    }
                    if !deepgram_api_key.is_empty() {
                        let _ = s.set_secret("DEEPGRAM_API_KEY", &deepgram_api_key);
                    }
                }
            }
            if !deepgram_api_key.is_empty() {
                *b.deepgram_key.lock().unwrap() = deepgram_api_key;
            }
            let mut cguard = b.client.lock().unwrap();
            let mut cfg = cguard.config.clone();
            if !base_url.is_empty() {
                cfg.base_url = base_url;
            }
            if !hermes_api_key.is_empty() {
                cfg.api_key = hermes_api_key;
            }
            *cguard = Arc::new(HermesClient::new(cfg));
        }
        UiCommand::RefreshGateway => {
            trigger_gateway_refresh();
        }
        UiCommand::ManageCronJob { job_id, action } => {
            let client = Arc::clone(&b.client.lock().unwrap());
            let tx = b.tx.clone();
            let waker = b.waker.clone();
            b.tokio.spawn(async move {
                let _ = client.job_action(&job_id, &action).await;
                if let Ok(jobs) = client.fetch_jobs().await {
                    let _ = tx.send(BridgeEvent::CronJobs(jobs));
                    (waker)();
                }
            });
        }
        UiCommand::ToggleMic(on) => {
            AUDIO.with(|cell| {
                if let Some(audio) = cell.borrow().as_ref() {
                    audio.set_listening(on);
                }
            });
        }
        UiCommand::ToggleMute(muted) => {
            AUDIO.with(|cell| {
                if let Some(audio) = cell.borrow().as_ref() {
                    audio.set_muted(muted);
                    if muted {
                        audio.stop_playback();
                    }
                }
            });
        }
        UiCommand::SpeakText(txt) => {
            let key = b.deepgram_key.lock().unwrap().clone();
            let model = state.voice.tts_voice.clone();
            let _guard = b.tokio.enter();
            AUDIO.with(|cell| {
                if let Some(audio) = cell.borrow().as_ref() {
                    audio.speak_text_async(key, model, txt);
                }
            });
        }
        UiCommand::TogglePhoneWindow(open) => {
            // The detached-window lifecycle is driven from state.phone_widget_open
            // in sync_phone_window (every poll, UI thread). The --scene phone
            // companion process IS the phone window: closing it ends the process.
            if !open && is_phone_scene_process() {
                std::process::exit(0);
            }
        }
        UiCommand::PhoneHangup => {
            // P4-02: drop the mic and stop every active run; the view already
            // closed the widget and cleared the call timer.
            AUDIO.with(|cell| {
                if let Some(audio) = cell.borrow().as_ref() {
                    audio.set_listening(false);
                }
            });
            let run_ids: Vec<String> = state.active_run_ids.values().cloned().collect();
            if !run_ids.is_empty() {
                let client = Arc::clone(&b.client.lock().unwrap());
                b.tokio.spawn(async move {
                    for rid in run_ids {
                        let _ = client.stop_run(&rid).await;
                    }
                });
            }
        }
        UiCommand::PersistUiConfig => {
            if let Ok(guard) = b.store.lock() {
                if let Some(ref s) = *guard {
                    let _ = s.save_ui_config(&state.ui_config);
                }
            }
        }
        UiCommand::PersistBackgroundSettings => {
            if let Ok(guard) = b.store.lock() {
                if let Some(ref s) = *guard {
                    let _ = s.save_background_settings(&state.bg_settings);
                }
            }
        }
        UiCommand::PersistVoiceSettings => {
            if let Ok(guard) = b.store.lock() {
                if let Some(ref s) = *guard {
                    let _ = s.save_voice_settings(&state.voice);
                }
            }
        }
        UiCommand::PersistDots => {
            if let Ok(mut guard) = b.store.lock() {
                if let Some(ref mut s) = *guard {
                    let dots: Vec<CanvasDotData> = state.dots.clone();
                    let _ = s.save_canvas_dots(&dots);
                }
            }
        }
        UiCommand::DismissChannel(ch_id) => {
            if let Ok(guard) = b.store.lock() {
                if let Some(ref s) = *guard {
                    let _ = s.dismiss_channel(&ch_id);
                }
            }
        }
        // ---- P5 queue-forms: explicit arms for the deferred Operations forms.
        UiCommand::CreateJob {
            name,
            schedule,
            prompt,
        } => {
            // Offline-safe: never attempt the network while the gateway is
            // down; surface the failure through the form error banner.
            if !state.gateway.online {
                let _ = b.tx.send(BridgeEvent::JobFormOutcome(Err(
                    "Gateway offline — job not created.".to_string(),
                )));
                (b.waker.clone())();
                return;
            }
            let client = Arc::clone(&b.client.lock().unwrap());
            let tx = b.tx.clone();
            let waker = b.waker.clone();
            b.tokio.spawn(async move {
                let outcome = create_gateway_job(&client, &name, &schedule, &prompt).await;
                if outcome.is_ok() {
                    if let Ok(jobs) = client.fetch_jobs().await {
                        let _ = tx.send(BridgeEvent::CronJobs(jobs));
                    }
                }
                let _ = tx.send(BridgeEvent::JobFormOutcome(outcome));
                (waker)();
            });
        }
        UiCommand::RegisterProfile {
            id,
            mode: _,
            base_url: _,
            api_key,
        } => {
            // The raw key is persisted ONLY through the store seam (the
            // server-side equivalent); AppState keeps redacted summaries.
            // Registration metadata (mode/base_url) already lives in the
            // state's redacted catalog entry.
            let mut outcome = Ok(());
            if !api_key.is_empty() {
                match b.store.lock() {
                    Ok(guard) => {
                        if let Some(ref s) = *guard {
                            if let Err(e) =
                                s.set_secret(&profile_key_secret_name(&id), &api_key)
                            {
                                outcome = Err(format!("profile key storage failed: {e}"));
                            }
                        }
                    }
                    Err(_) => {
                        outcome = Err("profile key storage unavailable".to_string());
                    }
                }
            }
            let _ = b.tx.send(BridgeEvent::ProfileFormOutcome(outcome));
            (b.waker.clone())();
        }
        UiCommand::DeleteProfile { id } => {
            // Clear any stored per-profile key (empty secret = inherit primary).
            let mut outcome = Ok(());
            if let Ok(guard) = b.store.lock() {
                if let Some(ref s) = *guard {
                    if let Err(e) = s.set_secret(&profile_key_secret_name(&id), "") {
                        outcome = Err(format!("profile key cleanup failed: {e}"));
                    }
                }
            }
            let _ = b.tx.send(BridgeEvent::ProfileFormOutcome(outcome));
            (b.waker.clone())();
        }
        UiCommand::SetTransportMode { mode } => {
            {
                let mut guard = b.client.lock().unwrap();
                let mut next = (**guard).clone();
                next.preferred_transport = preferred_for_transport(mode);
                *guard = Arc::new(next);
            }
            trigger_gateway_refresh();
        }
        UiCommand::SwitchSession { channel_id: _ } => {
            // AppState already focused the channel (queue_switch_session);
            // refresh gateway session telemetry so the header switcher and
            // the session rail reflect the gateway's view.
            trigger_gateway_refresh();
        }
        // P3-06 integration: wildcard fallback so future UiCommand variants
        // never break the app build. Settings vault/updater dispatch rides
        // state.settings_outbox (drained above), not new command variants.
        #[allow(unreachable_patterns)] // deliberate future-proofing seam
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// P3-06 settings integration: drain AppState::settings_outbox through
// voxcell_store::{vault, updater}. The settings view deliberately dispatches
// through the outbox (no new UiCommand variants — see the P3-06 coordination
// note), so this is the single bridge seam for backup/restore/import/export
// and the updater probe/apply/receipt lifecycle. All IO is injectable via
// SettingsContext: headless tests run against a tempdir vault, a scripted
// GitRunner and offline upstream info — no live network, no real git, no keys.
// ---------------------------------------------------------------------------

fn epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Injectable environment for one settings-outbox drain pass.
pub struct SettingsContext<'a> {
    pub vault: Option<&'a PreferencesVault>,
    /// package.json / .git location for the updater (TS process.cwd parity).
    pub app_root: &'a Path,
    pub runner: &'a dyn GitRunner,
    /// Pre-fetched upstream probe result; `None` = offline / not probed yet.
    pub upstream: Option<&'a UpstreamReleaseInfo>,
    /// File destination for VaultExport. RUNE's clipboard is Runtime-bound
    /// (no bridge-level setter), so the export seam is an atomic-ish file
    /// write + `vault_feedback` line — web "download snapshot" parity.
    pub export_path: Option<&'a Path>,
}

fn vault_backup_entry(b: &PreferenceBackupSummary) -> VaultBackupEntry {
    VaultBackupEntry {
        id: b.id.clone(),
        created_at: b.created_at,
        reason: b.reason.clone(),
        app_version: b.app_version.clone(),
        accent_hex: b.accent_hex.clone(),
        accent_preset: b.accent_preset.clone(),
        background_id: b.background_id.clone(),
        custom_slot_count: b.custom_slot_count,
        workspace_title: b.workspace_title.clone(),
    }
}

fn update_receipt_mirror(r: &UpdateReceipt) -> VaultUpdateReceipt {
    VaultUpdateReceipt {
        updated_at: r.updated_at,
        previous_version: r.previous_version.clone(),
        new_version: r.new_version.clone(),
        previous_commit: r.previous_commit.clone(),
        new_commit: r.new_commit.clone(),
        backup_id: r.backup_id.clone(),
        mode: r.mode.clone(),
        preserved_categories: r.preserved_categories.clone(),
    }
}

fn updater_step_mirror(s: &UpdateStepResult) -> UpdaterStep {
    UpdaterStep {
        id: s.id.clone(),
        label: s.label.clone(),
        status: match s.status {
            StepStatus::Ok => "ok",
            StepStatus::Skipped => "skipped",
            StepStatus::Failed => "failed",
        }
        .to_string(),
        detail: s.detail.clone(),
    }
}

fn updater_status_mirror(s: &AppUpdateStatus) -> UpdaterStatus {
    UpdaterStatus {
        current_version: s.current_version.clone(),
        current_commit: s.current_commit.clone(),
        branch: s.branch.clone(),
        latest_version: s.latest_version.clone(),
        latest_commit: s.latest_commit.clone(),
        update_available: s.update_available,
        update_mode: s.update_mode.as_str().to_string(),
        can_self_update: s.can_self_update,
        dirty_worktree: s.dirty_worktree,
        dirty_reason: s.dirty_reason.clone(),
        release_title: s.release_title.clone(),
        release_notes: s.release_notes.clone(),
        release_url: s.release_url.clone(),
        checked_at: s.checked_at,
        upstream_error: s.upstream_error.clone(),
        pending_receipt: s.pending_receipt.as_ref().map(update_receipt_mirror),
    }
}

/// Inverse of `voxcell_ui::state::settings_mood_from_id` for snapshot export.
fn settings_mood_id(mood: ExpressionMood) -> &'static str {
    match mood {
        ExpressionMood::Calm => "calm",
        ExpressionMood::Focused => "focused",
        ExpressionMood::Listening => "listening",
        ExpressionMood::Thinking => "thinking",
        ExpressionMood::Speaking => "speaking",
        ExpressionMood::Happy => "happy",
        ExpressionMood::Alert => "alert",
        #[allow(unreachable_patterns)] // future ExpressionMood variants
        _ => "happy",
    }
}

/// Build the web camelCase snapshot contract from live desktop state
/// (uiConfig / background / workspace / canvas). Serialized core types already
/// use the web key set; unknown desktop-only fields are ignored on re-import
/// by `core::snapshot::normalize_preferences_snapshot`.
pub fn settings_snapshot_json(state: &AppState) -> Value {
    json!({
        "schemaVersion": 1,
        "themeEpoch": "slate-v1",
        "capturedAt": epoch_ms(),
        "appVersion": env!("CARGO_PKG_VERSION"),
        "reason": "sync",
        "uiConfig": serde_json::to_value(&state.ui_config).unwrap_or_else(|_| json!({})),
        "background": serde_json::to_value(&state.bg_settings).unwrap_or_else(|_| json!({})),
        "workspace": json!({
            "showThinking": true,
            "voiceMuted": state.voice.muted,
            "mood": settings_mood_id(state.stage_mood),
            "draftProfile": state.gateway.active_profile,
            "onboardingCompleted": false,
        }),
        "canvas": json!({
            "sessions": serde_json::to_value(&state.canvas_sessions).unwrap_or_else(|_| json!([])),
            "pins": serde_json::to_value(&state.canvas_pins).unwrap_or_else(|_| json!([])),
            "dismissedIds": [],
        }),
    })
}

/// Reload ui/bg/voice prefs from a restored/imported vault snapshot. UI rides
/// the settings_apply_json_patch helper (sanitize + side effects + persist);
/// background/voice/mood come from the typed core normalizer. Persist commands
/// keep the SQLite store in sync so the next launch hydrates the same values.
fn hydrate_state_from_snapshot(state: &mut AppState, snapshot: &Value) -> bool {
    let Some(typed) = voxcell_core::snapshot::normalize_preferences_snapshot(snapshot) else {
        return false;
    };
    if let Some(ui) = snapshot.get("uiConfig") {
        state.settings_apply_json_patch(ui);
    }
    state.bg_settings = typed.background;
    state.pending_commands.push(UiCommand::PersistBackgroundSettings);
    state.voice.muted = typed.workspace.voice_muted;
    state.pending_commands.push(UiCommand::ToggleMute(state.voice.muted));
    state.pending_commands.push(UiCommand::PersistVoiceSettings);
    if let Some(mood) = settings_mood_from_id(typed.workspace.mood.as_str()) {
        state.stage_mood = mood;
    }
    true
}

/// Post-vault-op refresh per the P3-06 note: backups list, storage label,
/// last-synced stamp, and the busy flag always clears (success or failure).
fn refresh_vault_state(state: &mut AppState, vault: Option<&PreferencesVault>) {
    if let Some(v) = vault {
        state.vault_backups = v.list_backups().iter().map(vault_backup_entry).collect();
        state.vault_storage_label = v.storage_label();
        let captured = v.preferences_captured_at();
        state.vault_last_synced_ms = Some(if captured > 0 { captured } else { epoch_ms() });
    }
    state.vault_busy = false;
}

/// Ancestry check for the commit-divergence heuristic (only when both sides
/// have commits and a git repo exists; the scripted runner answers in tests).
fn upstream_ancestry(ctx: &SettingsContext, local: &LocalBuildMetadata) -> Option<bool> {
    let upstream_commit = ctx.upstream.and_then(|u| u.latest_commit.as_deref())?;
    if !local.has_git_repo || local.commit.is_none() {
        return None;
    }
    updater::is_commit_ancestor_of_head_with(ctx.app_root, upstream_commit, ctx.runner)
}

/// Finalize an updater probe: local metadata + (optional) upstream info ->
/// `build_app_update_status` -> state mirror; `update_checking` always clears.
pub fn finish_update_probe(state: &mut AppState, ctx: &SettingsContext) {
    let local = updater::read_local_build_metadata_with(ctx.app_root, ctx.runner);
    let receipt = ctx.vault.and_then(|v| v.read_receipt());
    let ancestry = upstream_ancestry(ctx, &local);
    let status = updater::build_app_update_status(&local, ctx.upstream, receipt, None, ancestry);
    state.update_error = status.upstream_error.clone();
    state.update_status = Some(updater_status_mirror(&status));
    state.update_checking = false;
}

/// Apply with the pre-update snapshot hook inside the store; map steps,
/// status and receipt onto state; `update_applying` always clears.
fn apply_update_action(state: &mut AppState, ctx: &SettingsContext) {
    let Some(vault) = ctx.vault else {
        state.update_error =
            Some("Preferences vault unavailable — cannot snapshot settings before update.".into());
        state.update_applying = false;
        return;
    };
    let snapshot = settings_snapshot_json(state);
    let local = updater::read_local_build_metadata_with(ctx.app_root, ctx.runner);
    let ancestry = upstream_ancestry(ctx, &local);
    let outcome = updater::apply_update_with_snapshot(
        vault,
        ctx.app_root,
        Some(&snapshot),
        ctx.upstream,
        ancestry,
        ctx.runner,
    );
    state.update_steps = outcome.steps.iter().map(updater_step_mirror).collect();
    state.update_error = outcome.error.clone();
    state.update_status = Some(updater_status_mirror(&outcome.status));
    state.update_applying = false;
    // The store took a pre-update backup internally — refresh the list.
    refresh_vault_state(state, Some(vault));
}

/// Drain every queued SettingsAction synchronously against `ctx`. Returns
/// false when the outbox was empty (no-op tick). Pure over state + injected
/// IO so the whole settings contract is headless-testable.
pub fn drain_settings_outbox(state: &mut AppState, ctx: &SettingsContext) -> bool {
    let actions: Vec<SettingsAction> = state.settings_outbox.drain(..).collect();
    if actions.is_empty() {
        return false;
    }
    for action in actions {
        match action {
            SettingsAction::VaultCreateBackup { reason } => {
                match ctx.vault {
                    Some(v) => {
                        let snapshot = settings_snapshot_json(state);
                        let backup_reason = if reason == "pre-update" {
                            BackupReason::PreUpdate
                        } else {
                            BackupReason::Manual
                        };
                        state.vault_feedback = Some(match v.create_backup(&snapshot, backup_reason) {
                            Ok(outcome) => format!("Backup created ({})", outcome.backup.id),
                            Err(e) => format!("Backup failed: {e}"),
                        });
                    }
                    None => {
                        state.vault_feedback =
                            Some("Backup failed: preferences vault unavailable".to_string());
                    }
                }
                refresh_vault_state(state, ctx.vault);
            }
            SettingsAction::VaultRestore { backup_id } => {
                match ctx.vault.and_then(|v| v.restore_backup(&backup_id)) {
                    Some(outcome) => {
                        state.vault_feedback = Some(
                            if hydrate_state_from_snapshot(state, &outcome.snapshot) {
                                format!("Restored backup {backup_id}")
                            } else {
                                format!("Restore failed: unreadable snapshot in {backup_id}")
                            },
                        );
                    }
                    None => {
                        state.vault_feedback =
                            Some(format!("Restore failed: unknown backup {backup_id}"));
                    }
                }
                refresh_vault_state(state, ctx.vault);
            }
            SettingsAction::VaultExport => {
                let snapshot = settings_snapshot_json(state);
                state.vault_feedback = Some(match (ctx.export_path, serde_json::to_string_pretty(&snapshot)) {
                    (Some(path), Ok(pretty)) => {
                        let written = path
                            .parent()
                            .map(std::fs::create_dir_all)
                            .unwrap_or(Ok(()))
                            .and_then(|_| std::fs::write(path, pretty));
                        match written {
                            Ok(()) => format!("Exported settings to {}", path.display()),
                            Err(e) => format!("Export failed: {e}"),
                        }
                    }
                    (None, _) => "Export failed: no destination available".to_string(),
                    (_, Err(e)) => format!("Export failed: {e}"),
                });
                refresh_vault_state(state, ctx.vault);
            }
            SettingsAction::VaultImport { json: raw_json } => {
                let parsed = serde_json::from_str::<Value>(&raw_json)
                    .ok()
                    .filter(|v| v.is_object());
                state.vault_feedback = Some(match (ctx.vault, parsed) {
                    (Some(v), Some(raw)) => match v.write_vault(&raw, Some("import")) {
                        Ok(stored) => {
                            if hydrate_state_from_snapshot(state, &stored) {
                                "Imported settings backup".to_string()
                            } else {
                                "Import failed: snapshot rejected by normalizer".to_string()
                            }
                        }
                        Err(e) => format!("Import failed: {e}"),
                    },
                    (None, Some(_)) => {
                        "Import failed: preferences vault unavailable".to_string()
                    }
                    (_, None) => "Import failed: invalid settings JSON".to_string(),
                });
                refresh_vault_state(state, ctx.vault);
            }
            SettingsAction::UpdaterProbe => finish_update_probe(state, ctx),
            SettingsAction::UpdaterApply => apply_update_action(state, ctx),
            SettingsAction::UpdaterDismissReceipt => {
                if let Some(v) = ctx.vault {
                    updater::acknowledge_update_receipt(v);
                }
                if let Some(status) = state.update_status.as_mut() {
                    status.pending_receipt = None;
                }
            }
        }
    }
    true
}

/// Reconstruct the last probe's upstream info from the state mirror so a
/// subsequent apply keeps release metadata without a second network roundtrip.
fn upstream_from_status(status: Option<&UpdaterStatus>) -> Option<UpstreamReleaseInfo> {
    let st = status?;
    if st.latest_version.is_none()
        && st.latest_commit.is_none()
        && st.release_title.is_none()
        && st.upstream_error.is_none()
    {
        return None;
    }
    Some(UpstreamReleaseInfo {
        latest_version: st.latest_version.clone(),
        latest_commit: st.latest_commit.clone(),
        release_title: st.release_title.clone(),
        release_notes: st.release_notes.clone(),
        release_url: st.release_url.clone(),
        error: st.upstream_error.clone(),
    })
}

/// Production wiring on the bridge tick: vault ops + apply + receipt dismiss
/// drain synchronously (local file IO); UpdaterProbe spawns the async GitHub
/// fetch and finalizes through BridgeEvent::SettingsProbe (checking stays true
/// until the result lands, matching the view's "Checking…" state).
fn poll_settings_outbox(b: &DesktopBridge, state: &mut AppState) -> bool {
    if state.settings_outbox.is_empty() {
        return false;
    }
    let actions: Vec<SettingsAction> = state.settings_outbox.drain(..).collect();
    let mut probe_requested = false;
    let mut sync_actions = Vec::new();
    for action in actions {
        if matches!(action, SettingsAction::UpdaterProbe) {
            probe_requested = true;
        } else {
            sync_actions.push(action);
        }
    }
    if !sync_actions.is_empty() {
        let guard = b.vault.lock().unwrap();
        let export_path = guard
            .as_ref()
            .map(|v| v.data_dir().join(format!("preferences-export-{}.json", epoch_ms())));
        let upstream = upstream_from_status(state.update_status.as_ref());
        let ctx = SettingsContext {
            vault: guard.as_ref(),
            app_root: &b.app_root,
            runner: &SystemRunner,
            upstream: upstream.as_ref(),
            export_path: export_path.as_deref(),
        };
        state.settings_outbox = sync_actions;
        drain_settings_outbox(state, &ctx);
    }
    if probe_requested {
        state.update_checking = true;
        let tx = b.tx.clone();
        let waker = b.waker.clone();
        b.tokio.spawn(async move {
            let upstream = updater::fetch_upstream_release_info_default().await;
            let _ = tx.send(BridgeEvent::SettingsProbe(upstream));
            (waker)();
        });
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxcell_core::{ApprovalKind, ChannelApprovalRequest};

    /// Tests sharing the PHONE_SNAPSHOT / PHONE_TO_MAIN statics run serially.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn approval(request_id: &str) -> ChannelApprovalRequest {
        ChannelApprovalRequest {
            id: format!("apr-{request_id}"),
            channel_id: "home".to_string(),
            run_id: Some("run-1".to_string()),
            request_id: request_id.to_string(),
            execution_generation: 1,
            kind: ApprovalKind::Approval,
            title: "Run tests".to_string(),
            tool_name: Some("terminal".to_string()),
            command: "cargo test".to_string(),
            summary: "Execute workspace tests".to_string(),
            destination: "default".to_string(),
            choices: vec!["once".to_string(), "deny".to_string()],
            created_at: "now".to_string(),
            resolved: false,
            resolution: None,
        }
    }

    fn parsed_approval(request_id: &str) -> ParsedApprovalRequest {
        ParsedApprovalRequest {
            request_id: request_id.to_string(),
            execution_generation: 1,
            kind: ApprovalKind::Approval,
            title: "Run tests".to_string(),
            tool_name: Some("terminal".to_string()),
            command: "cargo test".to_string(),
            summary: "Execute workspace tests".to_string(),
            destination: "default".to_string(),
            choices: vec!["once".to_string(), "deny".to_string()],
        }
    }

    // ---- P4-03 tray badge/status mapping ----

    #[test]
    fn tray_visuals_map_state_to_status_and_badge() {
        let mut s = AppState::new();
        // Fresh state: gateway offline, no audio, no work.
        assert_eq!(
            tray_visuals(&s),
            TrayVisuals { status: TrayStatus::Offline, approvals: 0 }
        );

        s.gateway.online = true;
        assert_eq!(tray_visuals(&s).status, TrayStatus::Idle);

        s.audio.is_listening = true;
        assert_eq!(tray_visuals(&s).status, TrayStatus::Listening);

        s.audio.is_speaking = true;
        assert_eq!(tray_visuals(&s).status, TrayStatus::Speaking);

        s.audio.is_speaking = false;
        s.audio.is_listening = false;
        s.thinking_by_channel.insert("home".to_string(), true);
        assert_eq!(tray_visuals(&s).status, TrayStatus::Working);

        s.gateway.last_error = Some("boom".to_string());
        assert_eq!(tray_visuals(&s).status, TrayStatus::Error);

        // Approval badge counts unresolved approvals only.
        s.gateway.last_error = None;
        s.thinking_by_channel.clear();
        s.approvals.push(approval("req-1"));
        let mut resolved = approval("req-2");
        resolved.resolved = true;
        s.approvals.push(resolved);
        let v = tray_visuals(&s);
        assert_eq!(v.approvals, 1);
        assert_eq!(v.status, TrayStatus::Idle);
        assert!(tray_tooltip(v).contains("1 approval pending"));
    }

    #[test]
    fn tray_icon_bakes_status_dot_and_approval_badge_into_rgba() {
        let plain = tray_icon_pixels(TrayVisuals { status: TrayStatus::Idle, approvals: 0 });
        let badged = tray_icon_pixels(TrayVisuals { status: TrayStatus::Idle, approvals: 2 });
        assert_eq!(plain.len(), 16 * 16 * 4);
        // Center dot: slate idle color, opaque.
        let c = ((7 * 16 + 7) * 4) as usize;
        assert_eq!(&plain[c..c + 4], &[0x94, 0xA3, 0xB8, 0xFF]);
        // Badge corner (12,3): transparent without approvals, amber with.
        let b = ((3 * 16 + 12) * 4) as usize;
        assert_eq!(plain[b + 3], 0);
        assert_eq!(&badged[b..b + 4], &[0xF5, 0x9E, 0x0B, 0xFF]);
        // Speaking status recolors the dot emerald (semantic live telemetry).
        let speak = tray_icon_pixels(TrayVisuals { status: TrayStatus::Speaking, approvals: 0 });
        assert_eq!(&speak[c..c + 4], &[0x34, 0xD3, 0x99, 0xFF]);
        // Error status is rose; working is amber.
        let err = tray_icon_pixels(TrayVisuals { status: TrayStatus::Error, approvals: 0 });
        assert_eq!(&err[c..c + 4], &[0xF4, 0x3F, 0x5E, 0xFF]);
        let work = tray_icon_pixels(TrayVisuals { status: TrayStatus::Working, approvals: 0 });
        assert_eq!(&work[c..c + 4], &[0xF5, 0x9E, 0x0B, 0xFF]);
    }

    #[test]
    fn tray_errors_degrade_to_status_notes_without_panicking() {
        assert!(tray_error_note(&TrayError::Unsupported).contains("fallback"));
        assert!(tray_error_note(&TrayError::Backend("dbus died".into())).contains("fallback"));

        // Real Tray::new failure path, deterministic on every platform: an icon
        // whose pixel buffer doesn't match its dimensions is rejected before
        // any OS call. try_install_tray must surface the error (no panic).
        let bad_spec = TraySpec::new(
            "dev.voxcell.desktop",
            "Voxcell",
            TrayIcon::rgba(16, 16, vec![0u8; 4]),
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let waker: Waker = Arc::new(|| {});
        let err = match try_install_tray(bad_spec, tx, waker) {
            Err(e) => e,
            Ok(_) => panic!("mismatched icon pixels must be rejected before any OS call"),
        };
        assert!(matches!(err, TrayError::Backend(_)));
        assert!(tray_error_note(&err).contains("fallback"));
        assert!(rx.try_recv().is_err()); // no events from a failed install

        // A valid spec installs or degrades gracefully depending on platform;
        // either way it must not panic. (Windows creates a live icon briefly,
        // removed on drop — same path the frozen probe exercises.)
        let (tx2, _rx2) = std::sync::mpsc::channel();
        let waker2: Waker = Arc::new(|| {});
        let good_spec = tray_spec(TrayKey {
            visuals: TrayVisuals { status: TrayStatus::Idle, approvals: 0 },
            mic_on: false,
            phone_open: false,
        });
        let installed = try_install_tray(good_spec, tx2, waker2);
        drop(installed);
    }

    // ---- P4-03 global PTT hotkey ----

    #[test]
    fn ptt_accelerator_parses_and_shortcut_errors_classify_to_fallbacks() {
        let parsed: Result<Shortcut, _> = ptt_accelerator().parse();
        assert!(parsed.is_ok(), "PTT accelerator must parse");

        assert_eq!(
            classify_shortcut_result(Err(ShortcutError::Unsupported)),
            ShortcutOutcome::FallbackFocusKey
        );
        assert_eq!(
            classify_shortcut_result(Err(ShortcutError::PermissionDenied)),
            ShortcutOutcome::PortalConsentDeclined
        );
        assert_eq!(
            classify_shortcut_result(Err(ShortcutError::Conflict)),
            ShortcutOutcome::ConflictAskOwner
        );
        assert_eq!(
            classify_shortcut_result(Err(ShortcutError::InvalidAccelerator("x".into()))),
            ShortcutOutcome::BadAccelerator
        );
        assert_eq!(
            classify_shortcut_result(Err(ShortcutError::NotRunning)),
            ShortcutOutcome::NotRunningYet
        );
        assert_eq!(
            classify_shortcut_result(Err(ShortcutError::UnknownId)),
            ShortcutOutcome::UnknownId
        );
        assert_eq!(
            classify_shortcut_result(Err(ShortcutError::Backend("x".into()))),
            ShortcutOutcome::BackendFallback
        );
    }

    #[test]
    fn ptt_handler_toggles_mic_and_dispatches_toggle_mic() {
        let mut s = AppState::new();
        assert!(!s.voice.hands_free_vad);
        toggle_ptt(&mut s);
        assert!(s.voice.hands_free_vad);
        assert!(s.pending_commands.iter().any(|c| matches!(c, UiCommand::ToggleMic(true))));
        toggle_ptt(&mut s);
        assert!(!s.voice.hands_free_vad);
        assert!(s.pending_commands.iter().any(|c| matches!(c, UiCommand::ToggleMic(false))));
    }

    // ---- P4-03 notify builder + click routing ----

    #[test]
    fn approval_notification_builder_wires_actions_and_routing_info() {
        let req = parsed_approval("req-77");
        let n = approval_notification(&req, "home");
        let expected = rune_notify::Notification::new("voxcell-apr-req-77", "Approval Needed: terminal")
            .body("Execute workspace tests")
            .info("channel", "home")
            .info("request", "req-77")
            .action("approve", "Approve")
            .action("deny", "Deny");
        assert_eq!(n, expected);

        let done = completion_notification("home", "All tests passed");
        let expected_done = rune_notify::Notification::new("voxcell-turn-done", "Hermes Turn Complete")
            .body("All tests passed")
            .info("channel", "home")
            .action("open", "Open Thread");
        assert_eq!(done, expected_done);
    }

    #[test]
    fn notify_clicks_route_to_queue_approval_or_thread_focus() {
        // Body click on an approval toast -> focus the Queue card.
        let click = rune_notify::Click {
            id: "voxcell-apr-req-77".to_string(),
            info: vec![
                ("channel".to_string(), "home".to_string()),
                ("request".to_string(), "req-77".to_string()),
            ],
        };
        assert_eq!(
            notify_click_route(&click),
            NotifyRoute::FocusApproval { channel: Some("home".into()), request: "req-77".into() }
        );

        // Approve button -> resolve with "once".
        let mut approve = click.clone();
        approve.info.push(("rune.action".to_string(), "approve".to_string()));
        assert_eq!(
            notify_click_route(&approve),
            NotifyRoute::ResolveApproval { channel: Some("home".into()), request: "req-77".into(), decision: "once" }
        );

        // Deny button -> resolve with "deny".
        let mut deny = click.clone();
        deny.info.push(("rune.action".to_string(), "deny".to_string()));
        assert_eq!(
            notify_click_route(&deny),
            NotifyRoute::ResolveApproval { channel: Some("home".into()), request: "req-77".into(), decision: "deny" }
        );

        // Completion toast -> focus the originating thread.
        let done = rune_notify::Click {
            id: "voxcell-turn-done".to_string(),
            info: vec![("channel".to_string(), "gw-42".to_string())],
        };
        assert_eq!(
            notify_click_route(&done),
            NotifyRoute::FocusThread { channel: Some("gw-42".into()) }
        );

        // Unknown ids are ignored.
        let unknown = rune_notify::Click { id: "other-app".to_string(), info: vec![] };
        assert_eq!(notify_click_route(&unknown), NotifyRoute::Ignore);
    }

    #[test]
    fn notify_post_is_a_graceful_noop_before_identity_registration() {
        // No test registers an identity, so available() is false and post()
        // must return false without panicking (probe §C.4 contract).
        if !rune_notify::available() {
            let n = approval_notification(&parsed_approval("req-1"), "home");
            assert!(!rune_notify::post(&n));
        }
    }

    #[test]
    fn os_action_resolve_approval_uses_pending_request_fields() {
        let mut s = AppState::new();
        s.approvals.push(approval("req-77"));
        handle_os_action(
            &mut s,
            OsAction::NotifyClick {
                id: "voxcell-apr-req-77".to_string(),
                action: Some("approve".to_string()),
                channel: Some("home".to_string()),
                request: Some("req-77".to_string()),
            },
        );
        assert_eq!(s.active_view, ActiveView::Queue);
        assert!(s.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::ResolveApproval { request_id, decision, run_id, execution_generation, .. }
                if request_id == "req-77" && decision == "once"
                    && run_id.as_deref() == Some("run-1") && *execution_generation == 1
        )));
    }

    // ---- P4-02 phone window options + fallback plan ----

    #[test]
    fn phone_window_options_match_the_probe_spec() {
        let o = phone_window_options();
        assert!(o.transparent);
        assert!(o.borderless);
        assert!(o.always_on_top);
        assert!(!o.resizable);
        assert!(o.visible);
        assert_eq!(o.min_size, Some((320.0, 420.0)));
        assert_eq!(o.max_size, Some((320.0, 420.0)));
    }

    #[test]
    fn phone_window_fallback_plan_covers_headless_wait_and_companion() {
        // Headless: embedded dock only, never a window or subprocess.
        assert_eq!(phone_window_plan(true, false, 0), PhoneWindowPlan::EmbeddedOnly);
        assert_eq!(phone_window_plan(true, true, 500), PhoneWindowPlan::EmbeddedOnly);
        // GUI: keep waiting during the grace period.
        assert_eq!(phone_window_plan(false, false, 0), PhoneWindowPlan::OpenOrKeepDetached);
        assert_eq!(
            phone_window_plan(false, false, PHONE_WINDOW_GRACE_POLLS - 1),
            PhoneWindowPlan::OpenOrKeepDetached
        );
        // Ready window stays detached regardless of wait count.
        assert_eq!(phone_window_plan(false, true, 500), PhoneWindowPlan::OpenOrKeepDetached);
        // Never ready within grace -> companion subprocess fallback.
        assert_eq!(
            phone_window_plan(false, false, PHONE_WINDOW_GRACE_POLLS),
            PhoneWindowPlan::SpawnCompanion
        );
    }

    // ---- P4-02 phone snapshot + command forwarding ----

    #[test]
    fn phone_snapshot_roundtrip_and_command_forwarding() {
        let _guard = TEST_LOCK.lock().unwrap();
        PHONE_TO_MAIN.lock().unwrap().clear();
        *PHONE_SNAPSHOT.lock().unwrap() = None;

        let mut main = AppState::new();
        main.apply_scene_preset("phone");
        main.now_ms = 120_000;
        main.phone_call_started_ms = Some(55_000);
        main.voice.muted = true;
        publish_phone_snapshot(&main);

        let mut phone = phone_init();
        phone.pending_commands.push(UiCommand::ToggleMute(false));
        phone.pending_commands.push(UiCommand::PhoneHangup);
        let changed = step_phone_window(&mut phone, 0.016);
        assert!(changed);

        // Commands were forwarded to the main bridge queue, not handled locally.
        let forwarded: Vec<UiCommand> = PHONE_TO_MAIN.lock().unwrap().drain(..).collect();
        assert_eq!(forwarded.len(), 2);
        assert!(matches!(forwarded[0], UiCommand::ToggleMute(false)));
        assert!(matches!(forwarded[1], UiCommand::PhoneHangup));

        // Snapshot applied: call state, timer anchor, audio and mood mirror main.
        assert!(phone.phone_widget_open);
        assert_eq!(phone.phone_call_started_ms, Some(55_000));
        assert_eq!(phone.now_ms, 120_000);
        assert!(phone.voice.muted);
        assert_eq!(phone.stage_mood, main.stage_mood);
        assert_eq!(phone.audio.bands, main.audio.bands);
        assert_eq!(phone.phone_call_timer_label(), "01:05");
        // Shared pixels handle: same size as the main window's render.
        assert_eq!(phone.phone_pixels.size(), main.phone_pixels.size());

        // No snapshot -> step is a no-op beyond forwarding.
        *PHONE_SNAPSHOT.lock().unwrap() = None;
        let mut bare = phone_init();
        assert!(!step_phone_window(&mut bare, 0.016));
    }

    #[test]
    fn tray_menu_reflects_mic_and_phone_state() {
        let idle = tray_menu(false, false);
        let active = tray_menu(true, true);
        let label = |items: &[TrayItem], id: &str| -> String {
            items
                .iter()
                .find_map(|it| match it {
                    TrayItem::Action { id: i, label, .. } if i == id => Some(label.clone()),
                    _ => None,
                })
                .unwrap_or_default()
        };
        assert_eq!(label(&idle, "ptt"), "Push to talk");
        assert_eq!(label(&active, "ptt"), "Release PTT");
        assert_eq!(label(&idle, "phone"), "Open Phone HUD");
        assert_eq!(label(&active, "phone"), "Close Phone HUD");
        assert_eq!(label(&idle, "quit"), "Quit");
        assert!(idle.iter().any(|it| matches!(it, TrayItem::Separator)));
    }

    // ---- P3-06 settings outbox drain (headless: tempdir vault, scripted
    // runner, offline upstream info — no network, no real git, no keys) ----

    use std::sync::atomic::{AtomicUsize, Ordering};

    static TMP_SEQ: AtomicUsize = AtomicUsize::new(0);

    /// Unique tempdir with best-effort cleanup (crate has no tempfile dev-dep).
    /// Declare BEFORE the vault so the SQLite connection drops first.
    struct TempRoot(PathBuf);
    impl TempRoot {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "voxcell-p306-{tag}-{}-{}",
                std::process::id(),
                TMP_SEQ.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&dir).expect("create test tempdir");
            TempRoot(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Offline GitRunner: answers from a script, unknown commands fail. Never
    /// spawns a real process, so updater tests cannot touch network or git.
    struct ScriptedRunner {
        responses: std::collections::HashMap<String, Result<String, updater::RunError>>,
    }
    impl ScriptedRunner {
        fn new() -> Self {
            Self { responses: std::collections::HashMap::new() }
        }
        fn ok(mut self, program: &str, args: &[&str], out: &str) -> Self {
            self.responses
                .insert(format!("{program} {}", args.join(" ")), Ok(out.to_string()));
            self
        }
        fn fail(mut self, program: &str, args: &[&str], msg: &str, code: Option<i32>) -> Self {
            self.responses.insert(
                format!("{program} {}", args.join(" ")),
                Err(updater::RunError { message: msg.to_string(), exit_code: code }),
            );
            self
        }
        fn clean_git(self, commit: &str) -> Self {
            self.ok("git", &["rev-parse", "--short", "HEAD"], commit)
                .ok("git", &["rev-parse", "--abbrev-ref", "HEAD"], "main")
                .ok("git", &["status", "--porcelain"], "")
        }
    }
    impl GitRunner for ScriptedRunner {
        fn run(
            &self,
            program: &str,
            args: &[&str],
            _cwd: &Path,
            _timeout_ms: u64,
        ) -> Result<String, updater::RunError> {
            let key = format!("{program} {}", args.join(" "));
            self.responses.get(&key).cloned().unwrap_or_else(|| {
                Err(updater::RunError {
                    message: format!("scripted runner: unexpected command {key}"),
                    exit_code: None,
                })
            })
        }
    }

    fn write_package_json(root: &Path, version: &str) {
        std::fs::write(root.join("package.json"), json!({ "version": version }).to_string())
            .expect("write package.json");
    }

    fn settings_ctx<'a>(
        vault: Option<&'a PreferencesVault>,
        app_root: &'a Path,
        runner: &'a dyn GitRunner,
        upstream: Option<&'a UpstreamReleaseInfo>,
        export_path: Option<&'a Path>,
    ) -> SettingsContext<'a> {
        SettingsContext { vault, app_root, runner, upstream, export_path }
    }

    fn feedback(state: &AppState) -> String {
        state.vault_feedback.clone().unwrap_or_default()
    }

    #[test]
    fn settings_outbox_empty_drain_is_noop() {
        let tmp = TempRoot::new("noop");
        let vault = PreferencesVault::open(tmp.path()).expect("open vault");
        let runner = ScriptedRunner::new();
        let mut state = AppState::new();
        state.vault_busy = true;
        let ctx = settings_ctx(Some(&vault), tmp.path(), &runner, None, None);
        assert!(!drain_settings_outbox(&mut state, &ctx));
        // No-op tick must not touch the busy flag or feedback.
        assert!(state.vault_busy);
        assert!(state.vault_feedback.is_none());
    }

    #[test]
    fn vault_create_backup_drain_refreshes_backups_label_and_busy() {
        let tmp = TempRoot::new("create");
        let vault = PreferencesVault::open(tmp.path()).expect("open vault");
        let runner = ScriptedRunner::new();
        let mut state = AppState::new();
        state.settings_apply_json_patch(&json!({ "accentHex": "#F59E0B" }));
        assert_eq!(state.ui_config.accent_hex, "#F59E0B");

        state.vault_busy = true;
        state.settings_outbox.push(SettingsAction::VaultCreateBackup { reason: "manual".into() });
        let ctx = settings_ctx(Some(&vault), tmp.path(), &runner, None, None);
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert!(state.settings_outbox.is_empty());
        assert!(!state.vault_busy);
        assert!(feedback(&state).contains("Backup created"));
        assert_eq!(state.vault_backups.len(), 1);
        assert_eq!(state.vault_backups[0].reason, "manual");
        assert_eq!(state.vault_backups[0].accent_hex, "#F59E0B");
        assert!(!state.vault_storage_label.is_empty());
        assert!(state.vault_last_synced_ms.is_some());
        assert!(vault.backups_dir().join(format!("{}.json", state.vault_backups[0].id)).exists());
        assert_eq!(vault.read_vault().unwrap()["reason"], "manual");

        // "pre-update" reason maps to BackupReason::PreUpdate (id prefix).
        state.vault_busy = true;
        state
            .settings_outbox
            .push(SettingsAction::VaultCreateBackup { reason: "pre-update".into() });
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert!(!state.vault_busy);
        assert_eq!(state.vault_backups.len(), 2);
        assert!(state.vault_backups.iter().any(|b| b.reason == "pre-update"));
        assert!(state.vault_backups.iter().any(|b| b.id.starts_with("pre-update-")));
    }

    #[test]
    fn vault_restore_drain_reloads_prefs_and_handles_unknown_id() {
        let tmp = TempRoot::new("restore");
        let vault = PreferencesVault::open(tmp.path()).expect("open vault");
        let runner = ScriptedRunner::new();
        let mut state = AppState::new();
        state.settings_apply_json_patch(&json!({ "accentHex": "#F59E0B" }));
        state.settings_outbox.push(SettingsAction::VaultCreateBackup { reason: "manual".into() });
        let ctx = settings_ctx(Some(&vault), tmp.path(), &runner, None, None);
        assert!(drain_settings_outbox(&mut state, &ctx));
        let backup_id = state.vault_backups[0].id.clone();

        // Drift the live prefs, then restore the backup through the outbox.
        state.settings_apply_json_patch(&json!({ "accentHex": "#38BDF8" }));
        state.voice.muted = true;
        state.vault_busy = true;
        state.settings_outbox.push(SettingsAction::VaultRestore { backup_id: backup_id.clone() });
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert_eq!(feedback(&state), format!("Restored backup {backup_id}"));
        assert_eq!(state.ui_config.accent_hex, "#F59E0B");
        assert!(!state.voice.muted);
        assert!(!state.vault_busy);
        assert_eq!(state.vault_backups.len(), 1);
        assert_eq!(vault.read_vault().unwrap()["reason"], "restore");

        // Unknown backup id: soft failure, busy still clears.
        state.vault_busy = true;
        state
            .settings_outbox
            .push(SettingsAction::VaultRestore { backup_id: "manual-does-not-exist".into() });
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert!(feedback(&state).contains("Restore failed: unknown backup"));
        assert!(!state.vault_busy);
        assert_eq!(state.vault_backups.len(), 1);
    }

    #[test]
    fn vault_export_drain_writes_snapshot_and_reports_missing_destination() {
        let tmp = TempRoot::new("export");
        let vault = PreferencesVault::open(tmp.path()).expect("open vault");
        let runner = ScriptedRunner::new();
        let mut state = AppState::new();
        state.settings_apply_json_patch(&json!({ "accentHex": "#F59E0B" }));
        let export_path = tmp.path().join("exports").join("snapshot.json");

        state.vault_busy = true;
        state.settings_outbox.push(SettingsAction::VaultExport);
        let ctx = settings_ctx(Some(&vault), tmp.path(), &runner, None, Some(&export_path));
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert!(feedback(&state).contains("Exported settings to"));
        assert!(!state.vault_busy);
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&export_path).expect("export file"))
                .expect("export JSON");
        assert_eq!(written["schemaVersion"], 1);
        assert_eq!(written["uiConfig"]["accentHex"], "#F59E0B");

        // No destination seam: soft failure, busy still clears.
        state.vault_busy = true;
        state.settings_outbox.push(SettingsAction::VaultExport);
        let ctx = settings_ctx(Some(&vault), tmp.path(), &runner, None, None);
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert_eq!(feedback(&state), "Export failed: no destination available");
        assert!(!state.vault_busy);
    }

    #[test]
    fn vault_import_drain_persists_and_reloads_state() {
        let tmp = TempRoot::new("import");
        let vault = PreferencesVault::open(tmp.path()).expect("open vault");
        let runner = ScriptedRunner::new();
        let mut source = AppState::new();
        source.settings_apply_json_patch(&json!({ "accentHex": "#38BDF8" }));
        source.voice.muted = true;
        source.stage_mood = ExpressionMood::Focused;
        let snapshot = settings_snapshot_json(&source);

        let mut state = AppState::new();
        assert_ne!(state.ui_config.accent_hex, "#38BDF8");
        state.vault_busy = true;
        state.settings_outbox.push(SettingsAction::VaultImport { json: snapshot.to_string() });
        let ctx = settings_ctx(Some(&vault), tmp.path(), &runner, None, None);
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert_eq!(feedback(&state), "Imported settings backup");
        assert_eq!(state.ui_config.accent_hex, "#38BDF8");
        assert!(state.voice.muted);
        assert_eq!(state.stage_mood, ExpressionMood::Focused);
        assert!(!state.vault_busy);
        assert_eq!(vault.read_vault().unwrap()["reason"], "import");
        // Hydration queues persist commands so SQLite stays in sync.
        assert!(state.pending_commands.iter().any(|c| matches!(c, UiCommand::PersistUiConfig)));

        // Malformed / non-object JSON: soft failure, busy still clears.
        for bad in ["{not json", "[1,2,3]"] {
            state.vault_busy = true;
            state.settings_outbox.push(SettingsAction::VaultImport { json: bad.into() });
            assert!(drain_settings_outbox(&mut state, &ctx));
            assert_eq!(feedback(&state), "Import failed: invalid settings JSON");
            assert!(!state.vault_busy);
        }
    }

    #[test]
    fn settings_ops_without_vault_fail_soft_and_clear_busy() {
        let tmp = TempRoot::new("novault");
        let runner = ScriptedRunner::new();
        let mut state = AppState::new();
        let ctx = settings_ctx(None, tmp.path(), &runner, None, None);

        state.vault_busy = true;
        state.settings_outbox.push(SettingsAction::VaultCreateBackup { reason: "manual".into() });
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert_eq!(feedback(&state), "Backup failed: preferences vault unavailable");
        assert!(!state.vault_busy);

        state.vault_busy = true;
        state.settings_outbox.push(SettingsAction::VaultImport { json: "{}".into() });
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert_eq!(feedback(&state), "Import failed: preferences vault unavailable");
        assert!(!state.vault_busy);

        state.vault_busy = true;
        state.settings_outbox.push(SettingsAction::VaultRestore { backup_id: "manual-x".into() });
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert!(feedback(&state).contains("Restore failed: unknown backup"));
        assert!(!state.vault_busy);

        state.update_applying = true;
        state.settings_outbox.push(SettingsAction::UpdaterApply);
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert!(state.update_error.as_deref().unwrap_or_default().contains("vault unavailable"));
        assert!(!state.update_applying);
    }

    #[test]
    fn updater_probe_drain_offline_available_and_error_paths() {
        let tmp = TempRoot::new("probe");
        write_package_json(tmp.path(), "0.1.0");
        let vault = PreferencesVault::open(tmp.path().join("data")).expect("open vault");
        let runner = ScriptedRunner::new();
        let mut state = AppState::new();

        // Offline (no upstream info): managed-refresh status, checking clears.
        state.update_checking = true;
        state.settings_outbox.push(SettingsAction::UpdaterProbe);
        let ctx = settings_ctx(Some(&vault), tmp.path(), &runner, None, None);
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert!(!state.update_checking);
        let status = state.update_status.as_ref().expect("probe status");
        assert_eq!(status.current_version, "0.1.0");
        assert!(!status.update_available);
        assert_eq!(status.update_mode, "managed-refresh");
        assert!(state.update_error.is_none());
        assert!(status.pending_receipt.is_none());

        // Pre-fetched upstream with a newer version: update flagged.
        let upstream = UpstreamReleaseInfo {
            latest_version: Some("9.9.9".into()),
            release_title: Some("Rune 9.9.9".into()),
            release_url: Some("https://example.invalid/rune".into()),
            ..Default::default()
        };
        state.update_checking = true;
        state.settings_outbox.push(SettingsAction::UpdaterProbe);
        let ctx = settings_ctx(Some(&vault), tmp.path(), &runner, Some(&upstream), None);
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert!(!state.update_checking);
        let status = state.update_status.as_ref().expect("probe status");
        assert!(status.update_available);
        assert_eq!(status.latest_version.as_deref(), Some("9.9.9"));
        assert_eq!(status.release_title.as_deref(), Some("Rune 9.9.9"));

        // Upstream fetch error surfaces on state.update_error, checking clears.
        let upstream_err = UpstreamReleaseInfo {
            error: Some("GitHub unreachable (offline test)".into()),
            ..Default::default()
        };
        state.update_checking = true;
        state.settings_outbox.push(SettingsAction::UpdaterProbe);
        let ctx = settings_ctx(Some(&vault), tmp.path(), &runner, Some(&upstream_err), None);
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert!(!state.update_checking);
        assert_eq!(state.update_error.as_deref(), Some("GitHub unreachable (offline test)"));
        let status = state.update_status.as_ref().expect("probe status");
        assert_eq!(status.upstream_error.as_deref(), Some("GitHub unreachable (offline test)"));
        assert!(!status.update_available);
    }

    #[test]
    fn updater_apply_drain_managed_refresh_offline_success() {
        let tmp = TempRoot::new("apply-managed");
        let app_root = tmp.path().join("app");
        std::fs::create_dir_all(&app_root).unwrap();
        write_package_json(&app_root, "0.1.0"); // no .git -> managed refresh
        let vault = PreferencesVault::open(tmp.path().join("data")).expect("open vault");
        let runner = ScriptedRunner::new();
        let mut state = AppState::new();
        state.update_applying = true;
        state.vault_busy = true;
        state.settings_outbox.push(SettingsAction::UpdaterApply);
        let ctx = settings_ctx(Some(&vault), &app_root, &runner, None, None);
        assert!(drain_settings_outbox(&mut state, &ctx));

        assert!(!state.update_applying);
        assert!(!state.vault_busy);
        assert!(state.update_error.is_none());
        let ids: Vec<&str> = state.update_steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["vault-backup", "vault-verify", "runtime-sync"]);
        assert!(state.update_steps.iter().all(|s| s.status == "ok"));
        let status = state.update_status.as_ref().expect("apply status");
        assert_eq!(status.update_mode, "managed-refresh");
        let receipt = status.pending_receipt.as_ref().expect("pending receipt");
        assert_eq!(receipt.mode, "managed-refresh");
        assert!(receipt.backup_id.starts_with("pre-update-"));
        assert!(vault.read_receipt().is_some());
        // Pre-update backup hook inside the store -> refreshed backups list.
        assert_eq!(state.vault_backups.len(), 1);
        assert_eq!(state.vault_backups[0].reason, "pre-update");
        assert!(!state.vault_storage_label.is_empty());
    }

    #[test]
    fn updater_apply_drain_git_fetch_failure_offline() {
        let tmp = TempRoot::new("apply-fetchfail");
        let app_root = tmp.path().join("app");
        std::fs::create_dir_all(app_root.join(".git")).unwrap();
        write_package_json(&app_root, "0.1.0");
        let vault = PreferencesVault::open(tmp.path().join("data")).expect("open vault");
        let runner = ScriptedRunner::new()
            .clean_git("abc1234")
            .fail("git", &["fetch", "--tags", "--prune"], "simulated offline failure", None);
        let mut state = AppState::new();
        state.update_applying = true;
        state.vault_busy = true;
        state.settings_outbox.push(SettingsAction::UpdaterApply);
        let ctx = settings_ctx(Some(&vault), &app_root, &runner, None, None);
        assert!(drain_settings_outbox(&mut state, &ctx));

        assert!(!state.update_applying);
        assert!(!state.vault_busy);
        assert!(state.update_error.as_deref().unwrap_or_default().contains("Unable to fetch updates"));
        assert_eq!(state.update_steps[0].id, "vault-backup");
        assert_eq!(state.update_steps[0].status, "ok");
        assert_eq!(state.update_steps[1].id, "git-fetch");
        assert_eq!(state.update_steps[1].status, "failed");
        let status = state.update_status.as_ref().expect("apply status");
        assert_eq!(status.update_mode, "git-pull");
        assert!(status.pending_receipt.is_none());
        // Safety backup survives the failed update.
        assert_eq!(state.vault_backups.len(), 1);
        assert_eq!(state.vault_backups[0].reason, "pre-update");
    }

    #[test]
    fn updater_apply_drain_dirty_worktree_pauses() {
        let tmp = TempRoot::new("apply-dirty");
        let app_root = tmp.path().join("app");
        std::fs::create_dir_all(app_root.join(".git")).unwrap();
        write_package_json(&app_root, "0.1.0");
        let vault = PreferencesVault::open(tmp.path().join("data")).expect("open vault");
        let runner = ScriptedRunner::new()
            .ok("git", &["rev-parse", "--short", "HEAD"], "abc1234")
            .ok("git", &["rev-parse", "--abbrev-ref", "HEAD"], "main")
            .ok("git", &["status", "--porcelain"], " M src/lib.rs");
        let mut state = AppState::new();
        state.update_applying = true;
        state.vault_busy = true;
        state.settings_outbox.push(SettingsAction::UpdaterApply);
        let ctx = settings_ctx(Some(&vault), &app_root, &runner, None, None);
        assert!(drain_settings_outbox(&mut state, &ctx));

        assert!(!state.update_applying);
        assert!(!state.vault_busy);
        assert!(state.update_error.as_deref().unwrap_or_default().contains("uncommitted edits"));
        assert!(state.update_steps.iter().any(|s| s.id == "git-check" && s.status == "failed"));
        let status = state.update_status.as_ref().expect("apply status");
        assert!(status.dirty_worktree);
        assert!(!status.can_self_update);
        assert_eq!(state.vault_backups.len(), 1);
    }

    #[test]
    fn updater_dismiss_receipt_drain_clears_file_and_mirror() {
        let tmp = TempRoot::new("dismiss");
        let app_root = tmp.path().join("app");
        std::fs::create_dir_all(&app_root).unwrap();
        write_package_json(&app_root, "0.1.0");
        let vault = PreferencesVault::open(tmp.path().join("data")).expect("open vault");
        let runner = ScriptedRunner::new();
        let mut state = AppState::new();
        let ctx = settings_ctx(Some(&vault), &app_root, &runner, None, None);

        // Managed apply writes a receipt; a later probe surfaces it.
        state.settings_outbox.push(SettingsAction::UpdaterApply);
        assert!(drain_settings_outbox(&mut state, &ctx));
        state.settings_outbox.push(SettingsAction::UpdaterProbe);
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert!(state.update_status.as_ref().unwrap().pending_receipt.is_some());
        assert!(vault.receipt_path().exists());

        state.settings_outbox.push(SettingsAction::UpdaterDismissReceipt);
        assert!(drain_settings_outbox(&mut state, &ctx));
        assert!(vault.read_receipt().is_none());
        assert!(!vault.receipt_path().exists());
        assert!(state.update_status.as_ref().unwrap().pending_receipt.is_none());

        // Dismiss with no probe status is a safe no-op.
        let mut bare = AppState::new();
        bare.settings_outbox.push(SettingsAction::UpdaterDismissReceipt);
        assert!(drain_settings_outbox(&mut bare, &ctx));
        assert!(bare.update_status.is_none());
    }

    // ---- P5 queue-forms: bridge arms + pure helpers (headless, no network).
    // The test bridge has no store and an unroutable base URL; only arms that
    // never dial the network are exercised (offline guard, sync store seam,
    // client preference). BRIDGE is unset in tests, so trigger_gateway_refresh
    // is a safe no-op. ----

    fn test_bridge() -> DesktopBridge {
        let (tx, rx) = std::sync::mpsc::channel();
        let cfg = ProfileGatewayConfig {
            base_url: "http://127.0.0.1:9".to_string(),
            profile: "default".to_string(),
            api_key: String::new(),
            route_prefix: String::new(),
        };
        DesktopBridge {
            tokio: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test tokio runtime"),
            store: Mutex::new(None),
            client: Mutex::new(Arc::new(HermesClient::new(cfg))),
            tx,
            rx: Mutex::new(rx),
            waker: Arc::new(|| {}),
            deepgram_key: Mutex::new(String::new()),
            tray: Mutex::new(TraySlot::default()),
            phone: Mutex::new(PhoneSlot::default()),
            vault: Mutex::new(None),
            app_root: PathBuf::from("."),
        }
    }

    #[test]
    fn job_create_payload_trims_and_omits_empty_name() {
        let body = job_create_payload("  ", " 0 9 * * * ", " Summarize runs ");
        assert_eq!(body["prompt"], "Summarize runs");
        assert_eq!(body["schedule"], "0 9 * * *");
        assert!(body.get("name").is_none());
        let named = job_create_payload(" Digest ", "0 9 * * *", "p");
        assert_eq!(named["name"], "Digest");
    }

    #[test]
    fn preferred_for_transport_covers_all_modes() {
        assert_eq!(
            preferred_for_transport(voxcell_core::GatewayTransport::Runs),
            voxcell_hermes::PreferredTransport::Runs
        );
        assert_eq!(
            preferred_for_transport(voxcell_core::GatewayTransport::SessionStream),
            voxcell_hermes::PreferredTransport::SessionStream
        );
        assert_eq!(
            preferred_for_transport(voxcell_core::GatewayTransport::ChatCompletions),
            voxcell_hermes::PreferredTransport::ChatCompletions
        );
    }

    #[test]
    fn profile_key_secret_name_matches_env_convention() {
        assert_eq!(
            profile_key_secret_name("coder-agent"),
            "HERMES_API_KEY_CODER_AGENT"
        );
        assert_eq!(
            profile_key_secret_name("research_v2"),
            "HERMES_API_KEY_RESEARCH_V2"
        );
        assert_eq!(profile_key_secret_name("a--b"), "HERMES_API_KEY_A_B");
    }

    #[test]
    fn create_job_offline_surfaces_form_error_without_network() {
        let b = test_bridge();
        let mut s = AppState::new();
        s.gateway.online = false;
        handle_command(
            &b,
            &s,
            UiCommand::CreateJob {
                name: "x".into(),
                schedule: "0 9 * * *".into(),
                prompt: "p".into(),
            },
        );
        let ev = b.rx.lock().unwrap().try_recv().expect("offline outcome event");
        match ev {
            BridgeEvent::JobFormOutcome(Err(e)) => assert!(e.contains("offline")),
            _ => panic!("expected offline JobFormOutcome Err"),
        }
        // Applying the outcome surfaces the banner and clears busy.
        s.queue_job_busy = true;
        s.queue_apply_job_outcome(Err("Gateway offline — job not created.".into()));
        assert!(!s.queue_job_busy);
        assert!(s.queue_job_error.unwrap().contains("offline"));
    }

    #[test]
    fn register_and_delete_profile_outcomes_without_store() {
        let b = test_bridge();
        let s = AppState::new();
        handle_command(
            &b,
            &s,
            UiCommand::RegisterProfile {
                id: "coder".into(),
                mode: "multiplexed".into(),
                base_url: String::new(),
                api_key: "sk-test".into(),
            },
        );
        assert!(matches!(
            b.rx.lock().unwrap().try_recv(),
            Ok(BridgeEvent::ProfileFormOutcome(Ok(())))
        ));
        handle_command(&b, &s, UiCommand::DeleteProfile { id: "coder".into() });
        assert!(matches!(
            b.rx.lock().unwrap().try_recv(),
            Ok(BridgeEvent::ProfileFormOutcome(Ok(())))
        ));
    }

    #[test]
    fn set_transport_mode_updates_client_preference() {
        let b = test_bridge();
        let s = AppState::new();
        handle_command(
            &b,
            &s,
            UiCommand::SetTransportMode {
                mode: voxcell_core::GatewayTransport::Runs,
            },
        );
        assert_eq!(
            b.client.lock().unwrap().preferred_transport,
            voxcell_hermes::PreferredTransport::Runs
        );
        handle_command(
            &b,
            &s,
            UiCommand::SetTransportMode {
                mode: voxcell_core::GatewayTransport::SessionStream,
            },
        );
        assert_eq!(
            b.client.lock().unwrap().preferred_transport,
            voxcell_hermes::PreferredTransport::SessionStream
        );
    }

    #[test]
    fn switch_session_arm_is_a_safe_refresh_noop_headless() {
        let b = test_bridge();
        let mut s = AppState::new();
        s.queue_switch_session("home");
        let cmd = s.pending_commands.pop().expect("SwitchSession queued");
        assert!(matches!(cmd, UiCommand::SwitchSession { .. }));
        handle_command(&b, &s, cmd);
        // No BRIDGE static in tests: the refresh is a no-op, no events emitted.
        assert!(b.rx.lock().unwrap().try_recv().is_err());
    }
}
