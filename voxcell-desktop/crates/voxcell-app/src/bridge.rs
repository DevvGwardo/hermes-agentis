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
use voxcell_ui::{AppState, UiCommand, VoxcellTheme};

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
    }
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
}
