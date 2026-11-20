use rune::{Pixels, TextEdit};
use std::collections::{HashMap, HashSet};
use voxcell_audio::AudioSnapshot;
use voxcell_core::canvas::{
    CanvasDotData, CanvasFormation, CanvasShapeKind, apply_formation, create_home_session_channel,
};
use voxcell_core::cli::{CliAction, autocomplete_slash_commands, parse_cli_input};
use voxcell_core::tasks::{
    apply_streaming_delta_to_steps, apply_tool_progress_to_steps, derive_turn_task_title,
};
use voxcell_core::ui_config::{
    apply_preset, apply_ui_customization_patch, parse_natural_language_ui_request,
};
use voxcell_core::sidebar_char::lead_running_task;
use voxcell_core::{
    ActiveView, ActivityKind, ApprovalKind, AsciiShaderMode, AvatarMode, BackgroundSettings,
    BackgroundTask, ChannelApprovalRequest, ChannelMeta, EntryRole, ExpressionMood, EyeState,
    GatewayCapabilities, GatewayProfile, GatewayTransport, HermesAgentSession, HermesCronJob,
    HermesGatewayInfo, HermesModelOption, HermesSkill, HermesToolset, MouthViseme,
    PhosphorTheme, SessionOrigin, SessionScopeTab, TaskLifecycleStatus, TaskStepItem,
    TerminalEntry, TerminalOutcome, UiCustomizationConfig, VoiceSettings,
};
use voxcell_halftone::{
    Ascii3DParams, HorizonAnimator, PortraitAnimator, PortraitAnimatorInput, PortraitRenderOptions,
    render_ascii3d_frame, render_ascii3d_rgba, render_canvas_dot_rgba, render_horizon_rgba,
    render_portrait_rgba,
};
use voxcell_hermes::{GatewayStreamEvent, StreamSignal};

#[derive(Clone, Debug, PartialEq)]
pub enum UiCommand {
    SendPrompt {
        channel_id: String,
        session_id: String,
        prompt: String,
    },
    ResolveApproval {
        run_id: Option<String>,
        request_id: String,
        execution_generation: u32,
        decision: String,
        is_clarify: bool,
    },
    StopRun {
        run_id: String,
        channel_id: String,
    },
    SteerRun {
        run_id: String,
        text: String,
    },
    ResetSession {
        channel_id: String,
        session_id: String,
    },
    SwitchProfile {
        profile: String,
    },
    SaveGatewaySettings {
        base_url: String,
        hermes_api_key: String,
        deepgram_api_key: String,
    },
    RefreshGateway,
    ManageCronJob {
        job_id: String,
        action: String,
    },
    ToggleMic(bool),
    ToggleMute(bool),
    SpeakText(String),
    TogglePhoneWindow(bool),
    PersistUiConfig,
    PersistBackgroundSettings,
    PersistVoiceSettings,
    PersistDots,
    DismissChannel(String),
    // P4-02 (phone-tray, append-only): end the phone call — stop active runs,
    // drop the mic and close the widget. Side effects live in app/bridge.rs.
    PhoneHangup,
    // P5 queue-forms (append-only): deferred Operations forms. Explicit bridge
    // arms live in app/bridge.rs before the wildcard fallback.
    /// New-Job form submit (web onCreateJob -> POST /api/jobs).
    CreateJob {
        name: String,
        schedule: String,
        prompt: String,
    },
    /// Add-Profile form submit. `api_key` is a transient payload for the bridge
    /// store seam only — it is NEVER retained in AppState (redacted summaries).
    RegisterProfile {
        id: String,
        mode: String,
        base_url: String,
        api_key: String,
    },
    /// Confirmed Delete-Profile (custom registrations only; default protected).
    DeleteProfile {
        id: String,
    },
    /// Transport-mode toggle (web onChangeTransportMode).
    SetTransportMode {
        mode: GatewayTransport,
    },
    /// Header session switcher (web onSelectChannel).
    SwitchSession {
        channel_id: String,
    },
}

pub struct AppState {
    pub active_view: ActiveView,
    pub avatar_mode: AvatarMode,
    pub shader_mode: AsciiShaderMode,
    pub phosphor_theme: PhosphorTheme,

    pub gateway: HermesGatewayInfo,
    pub gateway_base_url: String,
    pub gateway_profiles: Vec<GatewayProfile>,
    pub models: Vec<HermesModelOption>,
    pub selected_model: Option<String>,
    pub skills: Vec<HermesSkill>,
    pub toolsets: Vec<HermesToolset>,
    pub cron_jobs: Vec<HermesCronJob>,
    pub gateway_sessions: Vec<HermesAgentSession>,

    pub channels: Vec<ChannelMeta>,
    pub active_channel_id: String,
    pub session_scope: SessionScopeTab,
    pub entries: Vec<TerminalEntry>,
    pub thinking_by_channel: HashMap<String, bool>,
    pub active_run_ids: HashMap<String, String>,
    pub tasks: Vec<BackgroundTask>,
    pub approvals: Vec<ChannelApprovalRequest>,
    pub collapsed_traces: HashSet<String>,

    pub dots: Vec<CanvasDotData>,
    pub canvas_formation: CanvasFormation,
    pub selected_dot_id: Option<String>,

    pub ui_config: UiCustomizationConfig,
    pub bg_settings: BackgroundSettings,
    pub voice: VoiceSettings,
    pub audio: AudioSnapshot,

    pub settings_open: bool,
    pub add_dot_modal_open: bool,
    pub add_dot_shape: CanvasShapeKind,
    pub phone_widget_open: bool,
    pub cli_log: Vec<String>,
    pub status_banner: Option<String>,

    // TextEdit controls
    pub composer_edit: TextEdit,
    pub cli_edit: TextEdit,
    pub search_edit: TextEdit,
    pub canvas_prompt_edit: TextEdit,
    pub add_dot_label_edit: TextEdit,
    pub ui_prompt_edit: TextEdit,
    pub gateway_url_edit: TextEdit,
    pub hermes_key_edit: TextEdit,
    pub deepgram_key_edit: TextEdit,

    // Animation & pixel surfaces
    pub stage_mood: ExpressionMood,
    pub stage_viseme: MouthViseme,
    pub stage_energy: f32,
    pub tap_impulse: f32,
    pub pointer_stage: Option<(f32, f32)>,
    pub pointer_sidebar: Option<(f32, f32)>,
    pub pointer_horizon: (f32, f32),
    pub time_s: f32,
    pub now_ms: u64,
    pub session_counter: usize,
    pub entry_counter: usize,
    pub has_hermes_key: bool,
    pub has_deepgram_key: bool,

    pub stage_anim: PortraitAnimator,
    pub sidebar_anim: PortraitAnimator,
    pub phone_anim: PortraitAnimator,
    pub horizon_anim: HorizonAnimator,

    pub stage_pixels: Pixels,
    pub sidebar_pixels: Pixels,
    pub phone_pixels: Pixels,
    pub horizon_pixels: Pixels,
    pub add_dot_preview_pixels: Pixels,
    pub dot_pixels: HashMap<String, Pixels>,

    pub pending_commands: Vec<UiCommand>,

    /// P3-02 sidebar: parallel-profile badges per session. Maps channel_id or
    /// session_id -> gateway profile name (e.g. "ops"). Populated by the bridge
    /// from gateway session history; absent/empty/"default" = no badge.
    pub session_profiles: HashMap<String, String>,

    /// P3-05 CLI terminal: submitted command history (oldest first, capped).
    pub cli_history: Vec<String>,
    /// P3-05 CLI terminal: history browse cursor; None = editing a fresh line.
    pub cli_history_cursor: Option<usize>,
    /// P3-05 CLI terminal: stashed in-progress draft while browsing history.
    pub cli_history_draft: String,
    /// P3-05 CLI terminal: highlighted autocomplete row index.
    pub cli_suggestion_selected: usize,
    /// P3-05 CLI terminal: autocomplete dismissed via Esc until next edit.
    pub cli_suggestions_dismissed: bool,
    /// P3-05 CLI terminal: collapsed output trace blocks (mirrors thread traces).
    pub cli_collapsed_traces: HashSet<String>,

    /// P4-02/P4-03 (phone-tray): call timer anchor, set by `sync_phone_call_timer`
    /// when the widget opens; elapsed = now_ms - started.
    pub phone_call_started_ms: Option<u64>,
    /// P4-02 (phone-tray): true while the widget lives in its own always-on-top
    /// window (or `--scene phone` companion) instead of the embedded dock.
    pub phone_detached: bool,

    /// P3-01 theme-chrome: laid-out rect `[x, y, w, h]` of the titlebar tab
    /// container, reported by `El::on_layout`; feeds the sliding indicator via
    /// `core::motion::compute_sliding_indicator_rect`.
    pub nav_container_rect: Option<[f32; 4]>,
    /// P3-01 theme-chrome: laid-out rects of the five titlebar tabs in
    /// Home/Canvas/Thread/Queue/CLI order, reported by `El::on_layout`.
    pub nav_tab_rects: [Option<[f32; 4]>; 5],

    /// P3-04 thread: right-corner Task drawer visibility (hidden by default).
    pub thread_tasks_open: bool,
    /// P3-04 thread: trace blocks explicitly expanded by the user. Traces are
    /// collapsed by default (web parity) unless live or listed here;
    /// `collapsed_traces` keeps ids the user explicitly collapsed.
    pub thread_expanded_traces: HashSet<String>,
    /// P3-04 thread: trace blocks whose full coalesced step list is revealed
    /// ("+N earlier groups" pressed).
    pub thread_trace_show_all: HashSet<String>,
    /// P3-04 thread: bottom-anchor scroll pin state for the virtual transcript.
    pub thread_scroll: voxcell_core::thread::ScrollAnchorState,
    /// P3-04 thread: previous measured scroll offset (pin intent detection).
    pub thread_prev_scroll_top: f32,
    /// P3-04 thread: channel the scroll pin state belongs to (re-pin on visit).
    pub thread_scroll_channel: String,
    /// P3-04 thread: minimap turn anchor currently in view.
    pub thread_minimap_active: Option<String>,
    /// P3-04 thread: cached grouped timeline shared by virtual row builders.
    pub thread_timeline_cache: std::cell::RefCell<crate::views::thread::ThreadTimelineCache>,

    /// P3-05 queue: active Operations tab ("runtime" | "approvals" | "jobs").
    /// None = auto: approvals when a gate is pending, else runtime (web parity).
    pub queue_tab: Option<String>,
    /// P3-05 queue: toolset row expanded to reveal its tool list (web expandedToolset).
    pub queue_expanded_toolset: Option<String>,
    /// P3-05 queue: per-card clarify answer drafts (approval id -> edit).
    pub queue_clarify_edits: HashMap<String, TextEdit>,
    /// P3-05 queue: run_id targeted by the steer composer (None = closed).
    pub queue_steer_run: Option<String>,
    /// P3-05 queue: steer composer draft text.
    pub queue_steer_edit: TextEdit,

    /// P3-03 home: measured rect `[x, y, w, h]` of the Home view root, reported
    /// by `El::on_layout`; feeds responsive stage/composer clamping and the
    /// stage-header tier resolution (`core::layout::resolve_stage_header_size`).
    pub home_stage_rect: Option<[f32; 4]>,
    /// P3-03 home: true when the app attached `views::home::HomeHalftoneCompositor`
    /// (feature `gpu-halftone`) and the portrait should render through the GPU
    /// effect slot instead of the CPU `pixels_fit` surface. Default false keeps
    /// headless tests and GPU-less machines on the fallback path.
    pub home_gpu_halftone: bool,

    /// P3-06 settings: last UI studio patch summary (`UiPatchOutcome.summary`).
    pub ui_studio_feedback: Option<String>,
    /// P3-06 settings: Preferences Vault restore points, newest first (rolling
    /// 12 — mirrors `voxcell_store::vault::PreferencesVault::list_backups`).
    pub vault_backups: Vec<VaultBackupEntry>,
    /// P3-06 settings: vault storage label (`PreferencesVault::storage_label`).
    pub vault_storage_label: String,
    /// P3-06 settings: last vault sync/write timestamp in ms.
    pub vault_last_synced_ms: Option<i64>,
    /// P3-06 settings: a backup/restore/import roundtrip is in flight.
    pub vault_busy: bool,
    /// P3-06 settings: transient vault feedback (import/export result line).
    pub vault_feedback: Option<String>,
    /// P3-06 settings: paste-JSON import buffer (native file picker is a bridge seam).
    pub vault_import_edit: TextEdit,
    /// P3-06 settings: updater probe status (`voxcell_store::updater::AppUpdateStatus` mirror).
    pub update_status: Option<UpdaterStatus>,
    /// P3-06 settings: updater probe in flight.
    pub update_checking: bool,
    /// P3-06 settings: updater apply in flight.
    pub update_applying: bool,
    /// P3-06 settings: apply step results (`UpdateStepResult` mirror).
    pub update_steps: Vec<UpdaterStep>,
    /// P3-06 settings: last probe/apply error banner text.
    pub update_error: Option<String>,
    /// P3-06 settings: pending vault/updater actions for app/bridge.rs to drain
    /// through `voxcell_store::{vault, updater}`. Kept off `UiCommand` because
    /// bridge.rs matches it exhaustively (foreign file, phone-tray owns).
    pub settings_outbox: Vec<SettingsAction>,

    /// P4-01 canvas: GPU pan/zoom viewport (TS `viewport` parity, core::canvas).
    pub canvas_viewport: voxcell_core::canvas::CanvasViewport,
    /// P4-01 canvas: laid-out field rect [x,y,w,h] reported by `El::on_layout`.
    pub canvas_field_rect: Option<[f32; 4]>,
    /// P4-01 canvas: dot id whose edge-clamped popover is open (None = closed).
    pub canvas_popover_dot: Option<String>,
    /// P4-01 canvas: TS-parity percentage-space sessions (`duet.canvasSessions.v2`).
    pub canvas_sessions: Vec<voxcell_core::canvas::CanvasSession>,
    /// P4-01 canvas: spatial context pins (`duet.canvasPins.v1`).
    pub canvas_pins: Vec<voxcell_core::canvas::CanvasPin>,
    /// P4-01 canvas: spring layout transition record (rune-motion springs),
    /// sampled as a pure function of `time_s` — headless-deterministic.
    pub canvas_layout_anim: Option<CanvasLayoutAnim>,
    /// P4-01 canvas: true once the app attaches the GPU dot compositor;
    /// false (default) renders the headless-safe `pixels_fit` fallback.
    pub canvas_gpu_dots: bool,
    /// P4-01 canvas: toolbar pin composer visibility.
    pub canvas_pin_composer_open: bool,
    /// P4-01 canvas: pin composer title draft.
    pub canvas_pin_title_edit: TextEdit,
    /// P4-01 canvas: pin composer selected kind.
    pub canvas_pin_kind: voxcell_core::canvas::CanvasPinKind,
    /// P4-01 canvas: background stage-pan gesture anchor (TS panMetaRef).
    pub canvas_pan_drag: Option<CanvasPanDrag>,
    /// P4-01 canvas: dot currently being dragged (pan guard + sprite lift).
    pub canvas_dragging_dot: Option<String>,
    /// P4-01 canvas: dragged dot's design-space origin at gesture start.
    pub canvas_drag_origin: Option<(f32, f32)>,

    /// P5 queue-forms: New-Job form visibility (web showNewJobForm).
    pub queue_new_job_open: bool,
    /// P5 queue-forms: New-Job optional name draft.
    pub queue_job_name_edit: TextEdit,
    /// P5 queue-forms: New-Job cron schedule draft (web default `0 9 * * *`).
    pub queue_job_schedule_edit: TextEdit,
    /// P5 queue-forms: New-Job prompt draft.
    pub queue_job_prompt_edit: TextEdit,
    /// P5 queue-forms: job create in flight (web jobBusyId === 'create').
    pub queue_job_busy: bool,
    /// P5 queue-forms: job form error banner (web jobError).
    pub queue_job_error: Option<String>,
    /// P5 queue-forms: Add-Profile form visibility (web showAddProfileForm).
    pub queue_add_profile_open: bool,
    /// P5 queue-forms: profile slug draft.
    pub queue_profile_slug_edit: TextEdit,
    /// P5 queue-forms: gateway mode draft ("multiplexed" | "standalone").
    pub queue_profile_mode: String,
    /// P5 queue-forms: standalone gateway URL draft.
    pub queue_profile_url_edit: TextEdit,
    /// P5 queue-forms: TRANSIENT profile API-key draft (masked input). Cleared
    /// on submit/cancel; the raw key travels only inside RegisterProfile to the
    /// bridge store seam — registered profiles are redacted summaries only.
    pub queue_profile_key_edit: TextEdit,
    /// P5 queue-forms: profile save/delete in flight (web profileBusy).
    pub queue_profile_busy: bool,
    /// P5 queue-forms: profile form error banner (web profileError).
    pub queue_profile_error: Option<String>,
    /// P5 queue-forms: UI-registered custom profiles — REDACTED summaries only
    /// (voxcell_hermes::profiles::HermesProfileSummary; never raw keys).
    pub queue_registered_profiles: Vec<voxcell_hermes::profiles::HermesProfileSummary>,
    /// P5 queue-forms: profile id awaiting delete confirmation (None = closed).
    pub queue_delete_confirm: Option<String>,

    /// P5 native-chrome (spec P3-01 gap): frameless OS-chrome switch. When
    /// true, main.rs declares `App::titlebar` at startup (rune replaces the
    /// native titlebar; Windows keeps resize edges, Aero Snap and caption
    /// buttons) and the in-app 46px titlebar becomes the `.window_drag()`
    /// region. Persisted via main.rs's local config seam; the frame swap
    /// applies at next launch (rune's titlebar declaration is creation-time).
    pub native_chrome: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// P4-02 (phone-tray, append-only): call-timer helpers for the phone widget.
// Kept in a separate impl block so existing items stay untouched.
impl AppState {
    /// Elapsed call time in ms; 0 when no call is active.
    pub fn phone_call_elapsed_ms(&self) -> u64 {
        match self.phone_call_started_ms {
            Some(started) => self.now_ms.saturating_sub(started),
            None => 0,
        }
    }

    /// `MM:SS` call timer label (minutes accumulate past 59 for long calls).
    pub fn phone_call_timer_label(&self) -> String {
        let total_s = self.phone_call_elapsed_ms() / 1000;
        format!("{:02}:{:02}", total_s / 60, total_s % 60)
    }

    /// Anchor the call timer when the widget opens, clear it when it closes.
    /// Idempotent: an already-anchored open call keeps its original start.
    pub fn sync_phone_call_timer(&mut self) {
        if self.phone_widget_open {
            if self.phone_call_started_ms.is_none() {
                self.phone_call_started_ms = Some(self.now_ms);
            }
        } else {
            self.phone_call_started_ms = None;
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        let home_channel = ChannelMeta {
            id: "home".to_string(),
            name: "Home Stage".to_string(),
            category: "workspace".to_string(),
            description: "Primary Voxcell companion session".to_string(),
            session_id: "voxcell-home".to_string(),
            unread: 0,
            origin: SessionOrigin::Workspace,
            has_local_history: true,
            model: Some("hermes-agent".to_string()),
            updated_at_ms: Some(1_000),
        };

        let mut state = Self {
            active_view: ActiveView::Home,
            avatar_mode: AvatarMode::Halftone,
            shader_mode: AsciiShaderMode::Holo,
            phosphor_theme: PhosphorTheme::Emerald,

            gateway: HermesGatewayInfo {
                online: false,
                connected: false,
                base_url: "http://127.0.0.1:8642".to_string(),
                active_profile: "default".to_string(),
                model: "hermes-agent".to_string(),
                active_transport: GatewayTransport::Runs,
                capabilities: GatewayCapabilities::default(),
                version: None,
                latency_ms: None,
                last_error: None,
            },
            gateway_base_url: "http://127.0.0.1:8642".to_string(),
            gateway_profiles: vec![GatewayProfile {
                name: "default".to_string(),
                base_url: "http://127.0.0.1:8642".to_string(),
                active: true,
                online: false,
                model: Some("hermes-agent".to_string()),
                source: "desktop".to_string(),
            }],
            models: vec![HermesModelOption {
                id: "hermes-agent".to_string(),
                label: "Hermes Agent (default)".to_string(),
                provider: Some("hermes".to_string()),
            }],
            selected_model: Some("hermes-agent".to_string()),
            skills: Vec::new(),
            toolsets: Vec::new(),
            cron_jobs: Vec::new(),
            gateway_sessions: Vec::new(),

            channels: vec![home_channel],
            active_channel_id: "home".to_string(),
            session_scope: SessionScopeTab::Sessions,
            entries: Vec::new(),
            thinking_by_channel: HashMap::new(),
            active_run_ids: HashMap::new(),
            tasks: Vec::new(),
            approvals: Vec::new(),
            collapsed_traces: HashSet::new(),

            dots: Vec::new(),
            canvas_formation: CanvasFormation::Constellation,
            selected_dot_id: None,

            ui_config: UiCustomizationConfig::default(),
            bg_settings: BackgroundSettings::default(),
            voice: VoiceSettings::default(),
            audio: AudioSnapshot {
                has_output_device: false,
                has_input_device: false,
                is_speaking: false,
                is_listening: false,
                input_level: 0.0,
                output_level: 0.0,
                bands: [0.0; 12],
            },

            settings_open: false,
            add_dot_modal_open: false,
            add_dot_shape: CanvasShapeKind::HermesOrb,
            phone_widget_open: false,
            cli_log: vec![
                "Voxcell Desktop // RUNE Native Runtime ready.".to_string(),
                "Type a prompt or /help for slash commands.".to_string(),
            ],
            status_banner: None,

            composer_edit: TextEdit::default(),
            cli_edit: TextEdit::default(),
            search_edit: TextEdit::default(),
            canvas_prompt_edit: TextEdit::default(),
            add_dot_label_edit: TextEdit::default(),
            ui_prompt_edit: TextEdit::default(),
            gateway_url_edit: TextEdit::new("http://127.0.0.1:8642"),
            hermes_key_edit: TextEdit::default(),
            deepgram_key_edit: TextEdit::default(),

            stage_mood: ExpressionMood::Calm,
            stage_viseme: MouthViseme::Closed,
            stage_energy: 0.14,
            tap_impulse: 0.0,
            pointer_stage: None,
            pointer_sidebar: None,
            pointer_horizon: (0.0, 0.0),
            time_s: 0.0,
            now_ms: 10_000,
            session_counter: 1,
            entry_counter: 1,
            has_hermes_key: false,
            has_deepgram_key: false,

            stage_anim: PortraitAnimator::new(),
            sidebar_anim: PortraitAnimator::new(),
            phone_anim: PortraitAnimator::new(),
            horizon_anim: HorizonAnimator::new(),

            stage_pixels: Pixels::new(),
            sidebar_pixels: Pixels::new(),
            phone_pixels: Pixels::new(),
            horizon_pixels: Pixels::new(),
            add_dot_preview_pixels: Pixels::new(),
            dot_pixels: HashMap::new(),

            pending_commands: Vec::new(),
            session_profiles: HashMap::new(),
            cli_history: Vec::new(),
            cli_history_cursor: None,
            cli_history_draft: String::new(),
            cli_suggestion_selected: 0,
            cli_suggestions_dismissed: false,
            cli_collapsed_traces: HashSet::new(),
            phone_call_started_ms: None,
            phone_detached: false,
            nav_container_rect: None,
            nav_tab_rects: [None; 5],
            thread_tasks_open: false,
            thread_expanded_traces: HashSet::new(),
            thread_trace_show_all: HashSet::new(),
            thread_scroll: voxcell_core::thread::ScrollAnchorState {
                scroll_y: 0.0,
                content_h: 0.0,
                viewport_h: 0.0,
                pinned_to_bottom: true,
            },
            thread_prev_scroll_top: 0.0,
            thread_scroll_channel: String::new(),
            thread_minimap_active: None,
            thread_timeline_cache: std::cell::RefCell::new(
                crate::views::thread::ThreadTimelineCache::default(),
            ),
            queue_tab: None,
            queue_expanded_toolset: None,
            queue_clarify_edits: HashMap::new(),
            queue_steer_run: None,
            queue_steer_edit: TextEdit::default(),
            home_stage_rect: None,
            home_gpu_halftone: false,
            ui_studio_feedback: None,
            vault_backups: Vec::new(),
            vault_storage_label: String::new(),
            vault_last_synced_ms: None,
            vault_busy: false,
            vault_feedback: None,
            vault_import_edit: TextEdit::default(),
            update_status: None,
            update_checking: false,
            update_applying: false,
            update_steps: Vec::new(),
            update_error: None,
            settings_outbox: Vec::new(),
            canvas_viewport: voxcell_core::canvas::DEFAULT_CANVAS_VIEWPORT,
            canvas_field_rect: None,
            canvas_popover_dot: None,
            canvas_sessions: Vec::new(),
            canvas_pins: Vec::new(),
            canvas_layout_anim: None,
            canvas_gpu_dots: false,
            canvas_pin_composer_open: false,
            canvas_pin_title_edit: TextEdit::default(),
            canvas_pin_kind: voxcell_core::canvas::CanvasPinKind::Note,
            canvas_pan_drag: None,
            canvas_dragging_dot: None,
            canvas_drag_origin: None,
            queue_new_job_open: false,
            queue_job_name_edit: TextEdit::default(),
            queue_job_schedule_edit: TextEdit::new("0 9 * * *"),
            queue_job_prompt_edit: TextEdit::default(),
            queue_job_busy: false,
            queue_job_error: None,
            queue_add_profile_open: false,
            queue_profile_slug_edit: TextEdit::default(),
            queue_profile_mode: "multiplexed".to_string(),
            queue_profile_url_edit: TextEdit::default(),
            queue_profile_key_edit: TextEdit::default(),
            queue_profile_busy: false,
            queue_profile_error: None,
            queue_registered_profiles: Vec::new(),
            queue_delete_confirm: None,
            native_chrome: false,
        };
        state.step_animations(0.016);
        state
    }

    pub fn next_id(&mut self, prefix: &str) -> String {
        self.entry_counter += 1;
        format!("{prefix}-{}", self.entry_counter)
    }

    pub fn active_channel(&self) -> Option<&ChannelMeta> {
        self.channels
            .iter()
            .find(|c| c.id == self.active_channel_id)
            .or_else(|| self.channels.first())
    }

    pub fn active_session_id(&self) -> String {
        self.active_channel()
            .map(|c| c.session_id.clone())
            .unwrap_or_else(|| "voxcell-home".to_string())
    }

    pub fn active_entries(&self) -> Vec<TerminalEntry> {
        self.entries
            .iter()
            .filter(|e| e.channel_id == self.active_channel_id)
            .cloned()
            .collect()
    }

    pub fn active_is_thinking(&self) -> bool {
        self.thinking_by_channel
            .get(&self.active_channel_id)
            .copied()
            .unwrap_or(false)
    }

    pub fn pending_approvals(&self) -> Vec<ChannelApprovalRequest> {
        self.approvals.iter().filter(|a| !a.resolved).cloned().collect()
    }

    pub fn toggle_stage_mood(&mut self) {
        self.stage_mood = self.stage_mood.next_cycle();
        self.tap_impulse = 1.0;
        self.step_animations(0.016);
    }

    pub fn submit_composer_prompt(&mut self) {
        let prompt = self.composer_edit.text.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        self.composer_edit.clear();
        if prompt.starts_with('/') {
            self.execute_cli_line(&prompt);
            return;
        }
        self.dispatch_user_prompt(&prompt, true);
    }

    pub fn dispatch_user_prompt(&mut self, prompt: &str, switch_to_thread: bool) {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            return;
        }
        self.now_ms += 1000;
        if self.active_channel_id == "home" && self.active_entries().is_empty() {
            let new_ch = create_home_session_channel(trimmed, self.session_counter, self.now_ms);
            self.session_counter += 1;
            let ch_id = new_ch.id.clone();
            self.channels.push(new_ch);
            self.active_channel_id = ch_id;
        }
        let channel_id = self.active_channel_id.clone();
        let session_id = self.active_session_id();
        let eid = self.next_id("usr");
        let mut user_entry = TerminalEntry::new(eid, &channel_id, EntryRole::User, trimmed, "now");
        user_entry.created_at_ms = Some(self.now_ms);
        self.entries.push(user_entry);

        if let Some(ch) = self.channels.iter_mut().find(|c| c.id == channel_id) {
            ch.updated_at_ms = Some(self.now_ms);
            ch.has_local_history = true;
            if ch.name.starts_with("Session ") || ch.name == "Home Stage" {
                ch.name = derive_turn_task_title(trimmed, Some("Hermes Session"));
            }
        }

        self.thinking_by_channel.insert(channel_id.clone(), true);
        self.stage_mood = ExpressionMood::Thinking;

        let task_id = format!("turn-{channel_id}-{}", self.now_ms);
        self.tasks.retain(|t| !(t.channel_id == channel_id && t.status == TaskLifecycleStatus::Running));
        self.tasks.push(BackgroundTask {
            id: task_id,
            channel_id: channel_id.clone(),
            session_id: session_id.clone(),
            kind: Some("Turn".to_string()),
            background: Some(false),
            title: derive_turn_task_title(trimmed, Some("Hermes turn")),
            status: TaskLifecycleStatus::Running,
            raw_status: Some("running".to_string()),
            progress: 12.0,
            progress_label: Some("0/1".to_string()),
            last_step: Some("Routing to Hermes gateway".to_string()),
            steps: vec![TaskStepItem {
                text: "Routing to Hermes gateway".to_string(),
                done: false,
                active: true,
            }],
            result: None,
            error_code: None,
            created_at_ms: Some(self.now_ms),
            updated_at_ms: Some(self.now_ms),
            agent_done_at_ms: None,
            confirmed_at_ms: None,
            updated_at: "now".to_string(),
        });

        if switch_to_thread {
            self.active_view = ActiveView::Thread;
        }
        self.pending_commands.push(UiCommand::SendPrompt {
            channel_id,
            session_id,
            prompt: trimmed.to_string(),
        });
    }

    pub fn create_new_workspace_session(&mut self, title: Option<&str>) {
        self.now_ms += 1000;
        let label = title
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("New Hermes session");
        let ch = create_home_session_channel(label, self.session_counter, self.now_ms);
        self.session_counter += 1;
        let id = ch.id.clone();
        self.channels.push(ch);
        self.active_channel_id = id;
        self.active_view = ActiveView::Thread;
    }

    pub fn pin_active_channel_to_canvas(&mut self) {
        let Some(ch) = self.active_channel().cloned() else {
            return;
        };
        if let Some(existing) = self.dots.iter_mut().find(|d| d.channel_id == ch.id) {
            existing.pinned = true;
            self.selected_dot_id = Some(existing.id.clone());
        } else {
            let idx = self.dots.len();
            let shape = CanvasShapeKind::all()[idx % CanvasShapeKind::all().len()];
            let id = format!("dot-{}", ch.id);
            self.dots.push(CanvasDotData {
                id: id.clone(),
                channel_id: ch.id,
                label: ch.name,
                shape,
                x: 420.0,
                y: 250.0,
                pinned: true,
                color_hex: self.ui_config.accent_hex.clone(),
            });
            apply_formation(&mut self.dots, self.canvas_formation, 860.0, 520.0);
            self.selected_dot_id = Some(id);
        }
        self.active_view = ActiveView::Canvas;
        self.pending_commands.push(UiCommand::PersistDots);
        self.step_animations(0.016);
    }

    pub fn add_canvas_dot(&mut self, label: &str, shape: CanvasShapeKind) {
        self.now_ms += 1000;
        let clean_label = if label.trim().is_empty() {
            format!("{} Node", shape.label())
        } else {
            label.trim().to_string()
        };
        let ch = create_home_session_channel(&clean_label, self.session_counter, self.now_ms);
        self.session_counter += 1;
        let ch_id = ch.id.clone();
        self.channels.push(ch);
        let dot_id = format!("dot-{ch_id}");
        self.dots.push(CanvasDotData {
            id: dot_id.clone(),
            channel_id: ch_id.clone(),
            label: clean_label,
            shape,
            x: 430.0,
            y: 260.0,
            pinned: true,
            color_hex: self.ui_config.accent_hex.clone(),
        });
        apply_formation(&mut self.dots, self.canvas_formation, 860.0, 520.0);
        self.selected_dot_id = Some(dot_id);
        self.active_channel_id = ch_id;
        self.add_dot_modal_open = false;
        self.add_dot_label_edit.clear();
        self.pending_commands.push(UiCommand::PersistDots);
        self.step_animations(0.016);
    }

    pub fn resolve_approval(&mut self, approval_id: &str, decision: &str) {
        if let Some(req) = self.approvals.iter_mut().find(|a| a.id == approval_id) {
            req.resolved = true;
            req.resolution = Some(decision.to_string());
            self.pending_commands.push(UiCommand::ResolveApproval {
                run_id: req.run_id.clone(),
                request_id: req.request_id.clone(),
                execution_generation: req.execution_generation,
                decision: decision.to_string(),
                is_clarify: req.kind == ApprovalKind::Clarify,
            });
            self.cli_log.push(format!(
                "[approval] {} -> {decision}",
                req.title
            ));
        }
    }

    pub fn execute_cli_line(&mut self, raw: &str) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        self.cli_log.push(format!("> {trimmed}"));
        match parse_cli_input(trimmed) {
            CliAction::PrintHelp(help) => {
                for line in help.lines() {
                    self.cli_log.push(line.to_string());
                }
            }
            CliAction::NewSession(title) => {
                self.create_new_workspace_session(title.as_deref());
                self.cli_log.push(format!("Created workspace session {}", self.active_channel_id));
            }
            CliAction::ResetActiveSession => {
                let ch_id = self.active_channel_id.clone();
                let sid = self.active_session_id();
                self.entries.retain(|e| e.channel_id != ch_id);
                self.pending_commands.push(UiCommand::ResetSession {
                    channel_id: ch_id,
                    session_id: sid,
                });
                self.cli_log.push("Reset active session history.".to_string());
            }
            CliAction::ListSessions => {
                for ch in &self.channels {
                    self.cli_log.push(format!(
                        "  {} ({:?}) — {}",
                        ch.id, ch.origin, ch.name
                    ));
                }
            }
            CliAction::ResumeSession(target) => {
                let needle = target.to_ascii_lowercase();
                if let Some(found) = self.channels.iter().find(|c| {
                    c.id.to_ascii_lowercase().contains(&needle)
                        || c.name.to_ascii_lowercase().contains(&needle)
                }) {
                    self.active_channel_id = found.id.clone();
                    self.active_view = ActiveView::Thread;
                    self.cli_log.push(format!("Switched to {}", found.name));
                } else {
                    self.cli_log.push(format!("No session matched '{target}'"));
                }
            }
            CliAction::SetModel(m) => {
                if let Some(model) = m {
                    self.selected_model = Some(model.clone());
                    self.gateway.model = model.clone();
                    self.cli_log.push(format!("Active model set to {model}"));
                } else {
                    self.cli_log.push(format!("Current model: {}", self.gateway.model));
                }
            }
            CliAction::ShowStatus => {
                self.cli_log.push(format!(
                    "Gateway: {} | online={} | profile={} | transport={:?} | latency={:?}ms",
                    self.gateway.base_url,
                    self.gateway.online,
                    self.gateway.active_profile,
                    self.gateway.active_transport,
                    self.gateway.latency_ms
                ));
            }
            CliAction::SetGateway(url) => {
                if let Some(u) = url {
                    self.gateway_base_url = u.clone();
                    self.gateway_url_edit.replace_text(&u);
                    self.pending_commands.push(UiCommand::SaveGatewaySettings {
                        base_url: u,
                        hermes_api_key: String::new(),
                        deepgram_api_key: String::new(),
                    });
                    self.pending_commands.push(UiCommand::RefreshGateway);
                    self.cli_log.push("Updated gateway URL and queued probe.".to_string());
                } else {
                    self.cli_log.push(format!("Gateway URL: {}", self.gateway_base_url));
                }
            }
            CliAction::SetProfile(p) => {
                if let Some(name) = p {
                    self.gateway.active_profile = name.clone();
                    self.pending_commands.push(UiCommand::SwitchProfile { profile: name.clone() });
                    self.cli_log.push(format!("Switched profile to {name}"));
                } else {
                    self.cli_log.push(format!("Active profile: {}", self.gateway.active_profile));
                }
            }
            CliAction::ListSkills => {
                if self.skills.is_empty() {
                    self.cli_log.push("No skills loaded yet from gateway.".to_string());
                } else {
                    for sk in &self.skills {
                        self.cli_log.push(format!("  {} — {}", sk.name, sk.description));
                    }
                }
            }
            CliAction::ListToolsets => {
                if self.toolsets.is_empty() {
                    self.cli_log.push("No toolsets loaded yet from gateway.".to_string());
                } else {
                    for ts in &self.toolsets {
                        self.cli_log.push(format!("  {} ({} tools)", ts.name, ts.tools.len()));
                    }
                }
            }
            CliAction::CronCommand(arg) => {
                if let Some(spec) = arg {
                    let parts: Vec<&str> = spec.split_whitespace().collect();
                    if parts.len() == 2 {
                        self.pending_commands.push(UiCommand::ManageCronJob {
                            job_id: parts[1].to_string(),
                            action: parts[0].to_string(),
                        });
                        self.cli_log.push(format!("Dispatched cron {} {}", parts[0], parts[1]));
                    } else {
                        self.cli_log.push("Usage: /cron [pause|resume|run|delete] <job-id>".to_string());
                    }
                } else {
                    self.cli_log.push(format!("Cron jobs tracked: {}", self.cron_jobs.len()));
                }
            }
            CliAction::ListTasks => {
                self.cli_log.push(format!("Tracked tasks: {}", self.tasks.len()));
                for t in &self.tasks {
                    self.cli_log.push(format!("  [{}] {} ({:?})", t.id, t.title, t.status));
                }
            }
            CliAction::SwitchView(v) => {
                self.active_view = v;
                self.cli_log.push(format!("Switched view to {v:?}"));
            }
            CliAction::ResolveApproval(decision) => {
                if let Some(first_id) = self.pending_approvals().first().map(|a| a.id.clone()) {
                    self.resolve_approval(&first_id, &decision);
                } else {
                    self.cli_log.push("No pending approval request.".to_string());
                }
            }
            CliAction::StopActiveRun => {
                let ch_id = self.active_channel_id.clone();
                if let Some(run_id) = self.active_run_ids.get(&ch_id).cloned() {
                    self.pending_commands.push(UiCommand::StopRun {
                        run_id,
                        channel_id: ch_id.clone(),
                    });
                }
                self.thinking_by_channel.insert(ch_id, false);
                self.cli_log.push("Stopped active run.".to_string());
            }
            CliAction::ClearTerminal => {
                self.cli_log.clear();
            }
            CliAction::VoiceControl(arg) => {
                let next = match arg.as_deref() {
                    Some("on") | Some("ptt") => true,
                    Some("off") => false,
                    _ => !self.voice.hands_free_vad,
                };
                self.voice.hands_free_vad = next;
                self.pending_commands.push(UiCommand::ToggleMic(next));
                self.cli_log.push(format!("Voice input {}", if next { "enabled" } else { "disabled" }));
            }
            CliAction::TtsControl(arg) => {
                let muted = match arg.as_deref() {
                    Some("off") | Some("mute") => true,
                    Some("on") => false,
                    _ => !self.voice.muted,
                };
                self.voice.muted = muted;
                self.pending_commands.push(UiCommand::ToggleMute(muted));
                self.cli_log.push(format!("TTS audio {}", if muted { "muted" } else { "unmuted" }));
            }
            CliAction::SetAvatarMode(mode) => {
                self.avatar_mode = mode.unwrap_or(match self.avatar_mode {
                    AvatarMode::Halftone => AvatarMode::Ascii3d,
                    AvatarMode::Ascii3d => AvatarMode::Halftone,
                });
                self.cli_log.push(format!("Avatar mode: {:?}", self.avatar_mode));
                self.step_animations(0.016);
            }
            CliAction::ApplyUiCustomization(req) => {
                if !req.is_empty() {
                    let patch = parse_natural_language_ui_request(&req);
                    apply_ui_customization_patch(&mut self.ui_config, &patch);
                    self.pending_commands.push(UiCommand::PersistUiConfig);
                    self.cli_log.push(format!(
                        "Applied UI style: accent={}, profile={:?}, horizon={:?}",
                        self.ui_config.accent_hex,
                        self.ui_config.dot_profile,
                        self.ui_config.horizon_mode
                    ));
                    self.step_animations(0.016);
                } else {
                    self.settings_open = true;
                }
            }
            CliAction::TogglePhoneWidget(open) => {
                let next = open.unwrap_or(!self.phone_widget_open);
                self.phone_widget_open = next;
                self.pending_commands.push(UiCommand::TogglePhoneWindow(next));
                self.cli_log.push(format!("Phone widget {}", if next { "opened" } else { "closed" }));
            }
            CliAction::SendPrompt(prompt) => {
                self.dispatch_user_prompt(&prompt, false);
            }
            CliAction::UnknownCommand(cmd) => {
                let suggestions = autocomplete_slash_commands(&cmd);
                if suggestions.is_empty() {
                    self.cli_log.push(format!("Unknown command: {cmd}. Try /help."));
                } else {
                    let names: Vec<&str> = suggestions.iter().map(|s| s.name).collect();
                    self.cli_log.push(format!("Unknown command {cmd}. Did you mean: {}", names.join(", ")));
                }
            }
        }
    }

    pub fn apply_ui_prompt(&mut self, prompt: &str) {
        if !apply_preset(&mut self.ui_config, prompt) {
            let patch = parse_natural_language_ui_request(prompt);
            apply_ui_customization_patch(&mut self.ui_config, &patch);
        }
        self.pending_commands.push(UiCommand::PersistUiConfig);
        self.step_animations(0.016);
    }

    pub fn apply_hermes_event(&mut self, ev: GatewayStreamEvent) {
        self.now_ms += 50;
        match ev {
            GatewayStreamEvent::RunStarted { channel_id, run_id } => {
                self.active_run_ids.insert(channel_id.clone(), run_id);
                self.thinking_by_channel.insert(channel_id, true);
                self.stage_mood = ExpressionMood::Thinking;
            }
            GatewayStreamEvent::Signal { channel_id, signal } => match signal {
                StreamSignal::ContentDelta { chunk, .. } => {
                    self.stage_mood = ExpressionMood::Speaking;
                    if let Some(last) = self.entries.iter_mut().rev().find(|e| {
                        e.channel_id == channel_id && e.role == EntryRole::Hermes && e.streaming
                    }) {
                        last.text.push_str(&chunk);
                    } else {
                        let id = self.next_id("hrm");
                        let mut entry = TerminalEntry::new(
                            id,
                            &channel_id,
                            EntryRole::Hermes,
                            chunk,
                            "live",
                        );
                        entry.streaming = true;
                        entry.created_at_ms = Some(self.now_ms);
                        self.entries.push(entry);
                    }
                    if let Some(task) = self.tasks.iter_mut().rev().find(|t| {
                        t.channel_id == channel_id && t.status == TaskLifecycleStatus::Running
                    }) {
                        let (steps, ratio, changed) = apply_streaming_delta_to_steps(&task.steps);
                        if changed {
                            task.steps = steps;
                            task.progress_label = Some(ratio);
                            task.last_step = Some("Composing response".to_string());
                        }
                    }
                }
                StreamSignal::ReasoningDelta { chunk, .. } => {
                    let id = self.next_id("rsn");
                    let mut entry = TerminalEntry::new(
                        id,
                        &channel_id,
                        EntryRole::Activity,
                        format!("Reasoning: {chunk}"),
                        "live",
                    );
                    entry.activity_kind = Some(ActivityKind::Reasoning);
                    entry.reasoning_detail = Some(chunk);
                    self.entries.push(entry);
                }
                StreamSignal::Interim { text, .. } => {
                    let id = self.next_id("note");
                    let mut entry = TerminalEntry::new(
                        id,
                        &channel_id,
                        EntryRole::Activity,
                        format!("Commentary: {text}"),
                        "live",
                    );
                    entry.activity_kind = Some(ActivityKind::Commentary);
                    self.entries.push(entry);
                }
                StreamSignal::ToolEvent(ev) => {
                    let is_running = ev.status.eq_ignore_ascii_case("running")
                        || ev.phase.eq_ignore_ascii_case("started");
                    if is_running {
                        self.stage_mood = ExpressionMood::Focused;
                    }
                    let detail = if !ev.summary.is_empty() {
                        ev.summary.clone()
                    } else if !ev.label.is_empty() {
                        ev.label.clone()
                    } else {
                        format!("{} ({})", ev.tool_name, ev.status)
                    };
                    let id = self.next_id("tool");
                    let mut entry = TerminalEntry::new(
                        id,
                        &channel_id,
                        EntryRole::Activity,
                        format!("{}: {detail}", ev.tool_name),
                        "live",
                    );
                    entry.activity_kind = Some(ActivityKind::Tool);
                    entry.activity_status = Some(ev.status.clone());
                    self.entries.push(entry);

                    if let Some(task) = self.tasks.iter_mut().rev().find(|t| {
                        t.channel_id == channel_id && t.status == TaskLifecycleStatus::Running
                    }) {
                        let (steps, ratio) = apply_tool_progress_to_steps(
                            &task.steps,
                            &ev.tool_name,
                            if ev.label.is_empty() { None } else { Some(ev.label.as_str()) },
                            &ev.status,
                            if ev.summary.is_empty() { None } else { Some(ev.summary.as_str()) },
                        );
                        task.steps = steps;
                        task.progress_label = Some(ratio);
                        task.last_step = Some(detail);
                    }
                }
                StreamSignal::ApprovalRequest(req) => {
                    self.stage_mood = ExpressionMood::Alert;
                    let run_id = self.active_run_ids.get(&channel_id).cloned();
                    self.approvals.push(ChannelApprovalRequest {
                        id: format!("apr-{}", req.request_id),
                        channel_id,
                        run_id,
                        request_id: req.request_id,
                        execution_generation: req.execution_generation,
                        kind: req.kind,
                        title: req.title,
                        tool_name: req.tool_name,
                        command: req.command,
                        summary: req.summary,
                        destination: req.destination,
                        choices: req.choices,
                        created_at: "now".to_string(),
                        resolved: false,
                        resolution: None,
                    });
                }
                StreamSignal::SubagentStart { summary, .. } => {
                    let id = self.next_id("sub");
                    let mut entry = TerminalEntry::new(
                        id,
                        &channel_id,
                        EntryRole::Activity,
                        format!("Subagent: {summary}"),
                        "live",
                    );
                    entry.activity_kind = Some(ActivityKind::Subagent);
                    entry.activity_status = Some("running".to_string());
                    self.entries.push(entry);
                }
                StreamSignal::SubagentComplete { status, summary, .. } => {
                    let id = self.next_id("sub");
                    let mut entry = TerminalEntry::new(
                        id,
                        &channel_id,
                        EntryRole::Activity,
                        format!("Subagent ({status}): {summary}"),
                        "live",
                    );
                    entry.activity_kind = Some(ActivityKind::Subagent);
                    entry.activity_status = Some(status);
                    self.entries.push(entry);
                }
                StreamSignal::Steered(txt) => {
                    let id = self.next_id("steer");
                    self.entries.push(TerminalEntry::new(
                        id,
                        &channel_id,
                        EntryRole::System,
                        format!("Steered: {txt}"),
                        "now",
                    ));
                }
            },
            GatewayStreamEvent::Completed { channel_id, result } => {
                self.thinking_by_channel.insert(channel_id.clone(), false);
                self.active_run_ids.remove(&channel_id);
                let mut spoken = None;
                if let Some(last) = self.entries.iter_mut().rev().find(|e| {
                    e.channel_id == channel_id && e.role == EntryRole::Hermes && e.streaming
                }) {
                    if last.text.trim().is_empty() && !result.output.is_empty() {
                        last.text = result.output.clone();
                    }
                    last.streaming = false;
                    spoken = Some(last.text.clone());
                } else if !result.output.is_empty() {
                    let id = self.next_id("hrm");
                    spoken = Some(result.output.clone());
                    self.entries.push(TerminalEntry::new(
                        id,
                        &channel_id,
                        EntryRole::Hermes,
                        result.output,
                        "now",
                    ));
                }
                let ok = result.outcome == TerminalOutcome::Completed;
                if let Some(task) = self.tasks.iter_mut().rev().find(|t| {
                    t.channel_id == channel_id && t.status == TaskLifecycleStatus::Running
                }) {
                    task.status = TaskLifecycleStatus::Completed;
                    task.raw_status = Some(if ok { "done".into() } else { "failed".into() });
                    task.progress = 100.0;
                    for s in &mut task.steps {
                        s.done = ok;
                        s.active = false;
                    }
                }
                if let Some(err_msg) = result.error {
                    let id = self.next_id("err");
                    let mut entry = TerminalEntry::new(
                        id,
                        &channel_id,
                        EntryRole::Activity,
                        err_msg,
                        "now",
                    );
                    entry.error_code = result.error_code;
                    entry.terminal_outcome = Some(result.outcome);
                    self.entries.push(entry);
                }
                self.stage_mood = if ok { ExpressionMood::Calm } else { ExpressionMood::Alert };
                if self.voice.tts_enabled && !self.voice.muted {
                    if let Some(txt) = spoken.filter(|s| !s.trim().is_empty()) {
                        self.pending_commands.push(UiCommand::SpeakText(txt));
                    }
                }
            }
            GatewayStreamEvent::Error { channel_id, message } => {
                self.thinking_by_channel.insert(channel_id.clone(), false);
                self.active_run_ids.remove(&channel_id);
                let id = self.next_id("err");
                let mut entry = TerminalEntry::new(
                    id,
                    &channel_id,
                    EntryRole::Activity,
                    message.clone(),
                    "now",
                );
                entry.error_code = Some("gateway_error".to_string());
                entry.terminal_outcome = Some(TerminalOutcome::Failed);
                self.entries.push(entry);
                self.cli_log.push(format!("[gateway error] {message}"));
                self.stage_mood = ExpressionMood::Alert;
            }
        }
    }

    pub fn step_animations(&mut self, dt: f32) {
        let clamped_dt = dt.clamp(0.001, 0.1);
        self.time_s += clamped_dt;
        self.tap_impulse = (self.tap_impulse - clamped_dt * 2.4).max(0.0);

        let running_count = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskLifecycleStatus::Running)
            .count();
        let pending_approval_count = self.approvals.iter().filter(|a| !a.resolved).count();
        let any_thinking = self.thinking_by_channel.values().any(|&v| v);
        let active_channel_thinking = self.active_is_thinking();

        let is_speaking = self.audio.is_speaking || self.stage_mood == ExpressionMood::Speaking;
        let is_listening = self.audio.is_listening || self.voice.hands_free_vad;
        let voice_amp = self.audio.output_level.max(self.audio.input_level);

        let stage_effective_mood = if pending_approval_count > 0 {
            ExpressionMood::Alert
        } else if is_speaking {
            ExpressionMood::Speaking
        } else if is_listening {
            ExpressionMood::Listening
        } else if running_count > 0 || any_thinking {
            ExpressionMood::Thinking
        } else {
            self.stage_mood
        };

        let stage_eye = match stage_effective_mood {
            ExpressionMood::Alert => EyeState::Wide,
            _ => EyeState::Open,
        };
        let stage_viseme = if is_speaking {
            let amp = if voice_amp > 0.05 {
                voice_amp
            } else {
                ((self.time_s * 7.5).sin().abs() * 0.65 + 0.25).clamp(0.0, 1.0)
            };
            voxcell_core::sidebar_char::amplitude_to_viseme(amp)
        } else if is_listening {
            MouthViseme::Rest
        } else {
            self.stage_viseme
        };

        let stage_energy = if is_speaking {
            voice_amp.max(0.55)
        } else if running_count > 0 || any_thinking {
            0.55
        } else if is_listening {
            0.38
        } else {
            0.16
        };

        let tint = self.ui_config.portrait_tint_hex.as_deref();
        let dot_profile = self.ui_config.dot_profile;
        let bloom = self.ui_config.bloom;
        let reduced = self.ui_config.reduced_motion;

        // 1) Main stage avatar (Halftone or 3D ASCII SDF)
        let motion_stage = self.stage_anim.step(
            &PortraitAnimatorInput {
                time: self.time_s,
                mood: stage_effective_mood,
                eye_state: stage_eye,
                viseme: stage_viseme,
                energy: stage_energy,
                is_speaking,
                is_listening,
                pointer_x: self.pointer_stage.map(|p| p.0),
                pointer_y: self.pointer_stage.map(|p| p.1),
                tap_impulse: self.tap_impulse,
            },
            clamped_dt,
            reduced,
        );
        let stage_rgba = match self.avatar_mode {
            AvatarMode::Halftone => render_portrait_rgba(
                motion_stage,
                &PortraitRenderOptions {
                    pixel_w: 320,
                    pixel_h: 294,
                    cols: 58,
                    rows: 27,
                    time: self.time_s,
                    mood: stage_effective_mood,
                    viseme: stage_viseme,
                    energy: stage_energy,
                    is_speaking,
                    is_listening,
                    is_loading: false,
                    is_thumb: false,
                    dot_profile,
                    bloom,
                    tint_hex: tint,
                },
            ),
            AvatarMode::Ascii3d => {
                let frame = render_ascii3d_frame(&Ascii3DParams {
                    cols: 56,
                    rows: 27,
                    time: self.time_s,
                    yaw: self.pointer_stage.map(|p| p.0 * 0.45).unwrap_or(self.time_s * 0.25),
                    pitch: self.pointer_stage.map(|p| p.1 * 0.28).unwrap_or(0.06),
                    zoom: 1.0,
                    mood: stage_effective_mood,
                    viseme: stage_viseme,
                    eye_state: stage_eye,
                    energy: stage_energy,
                    is_speaking,
                    is_listening,
                    shader_mode: self.shader_mode,
                    bands: self.audio.bands,
                });
                render_ascii3d_rgba(320, 294, &frame, self.phosphor_theme)
            }
        };
        self.stage_pixels.set(320, 294, stage_rgba);

        // 2) Sidebar live 72x66 reactive avatar
        let lead_task = lead_running_task(&self.tasks);
        let sb_pres = voxcell_core::sidebar_char::derive_sidebar_character_state(
            &voxcell_core::sidebar_char::SidebarCharacterInput {
                connected: self.gateway.online,
                is_speaking,
                is_listening,
                active_channel_thinking,
                any_thinking,
                running_task_count: running_count,
                pending_approval_count,
                voice_amplitude: if is_speaking { stage_energy } else { voice_amp },
                active_task_progress: lead_task.map(|t| t.progress),
                active_task_title: lead_task.map(|t| t.title.clone()),
                look_x: self.pointer_sidebar.map(|p| p.0).unwrap_or(0.0),
                look_y: self.pointer_sidebar.map(|p| p.1).unwrap_or(0.0),
            },
        );
        let motion_sb = self.sidebar_anim.step(
            &PortraitAnimatorInput {
                time: self.time_s,
                mood: sb_pres.mood,
                eye_state: sb_pres.eye_state,
                viseme: sb_pres.viseme,
                energy: sb_pres.energy,
                is_speaking: sb_pres.is_speaking,
                is_listening: sb_pres.is_listening,
                pointer_x: self.pointer_sidebar.map(|p| p.0),
                pointer_y: self.pointer_sidebar.map(|p| p.1),
                tap_impulse: self.tap_impulse * 0.6,
            },
            clamped_dt,
            reduced,
        );
        let sb_rgba = render_portrait_rgba(
            motion_sb,
            &PortraitRenderOptions {
                pixel_w: 144,
                pixel_h: 132,
                cols: 32,
                rows: 16,
                time: self.time_s,
                mood: sb_pres.mood,
                viseme: sb_pres.viseme,
                energy: sb_pres.energy,
                is_speaking: sb_pres.is_speaking,
                is_listening: sb_pres.is_listening,
                is_loading: false,
                is_thumb: true,
                dot_profile,
                bloom: (bloom + 0.1).min(1.0),
                tint_hex: tint,
            },
        );
        self.sidebar_pixels.set(144, 132, sb_rgba);

        // 3) Horizon wave strip (Home view)
        let h_state = self.horizon_anim.step(
            stage_energy,
            self.pointer_horizon.0,
            self.pointer_horizon.1,
            &self.audio.bands,
            clamped_dt,
            reduced,
        );
        let h_rgba = render_horizon_rgba(
            680,
            88,
            136,
            12,
            self.time_s,
            h_state,
            self.ui_config.horizon_mode,
            &self.ui_config.accent_hex,
            dot_profile,
            bloom,
        );
        self.horizon_pixels.set(680, 88, h_rgba);

        // 4) Phone widget portrait (only update when open or small cost)
        if self.phone_widget_open {
            let phone_rgba = render_portrait_rgba(
                motion_stage,
                &PortraitRenderOptions {
                    pixel_w: 240,
                    pixel_h: 210,
                    cols: 48,
                    rows: 22,
                    time: self.time_s,
                    mood: stage_effective_mood,
                    viseme: stage_viseme,
                    energy: stage_energy,
                    is_speaking,
                    is_listening,
                    is_loading: false,
                    is_thumb: false,
                    dot_profile,
                    bloom,
                    tint_hex: tint,
                },
            );
            self.phone_pixels.set(240, 210, phone_rgba);
        }

        // 5) Canvas dots & Add-Dot modal preview
        if self.active_view == ActiveView::Canvas {
            for dot in &self.dots {
                let busy = self
                    .thinking_by_channel
                    .get(&dot.channel_id)
                    .copied()
                    .unwrap_or(false)
                    || self.tasks.iter().any(|t| {
                        t.channel_id == dot.channel_id && t.status == TaskLifecycleStatus::Running
                    });
                let dot_rgba = render_canvas_dot_rgba(
                    96,
                    96,
                    22,
                    11,
                    dot.shape,
                    self.time_s,
                    if busy { ExpressionMood::Thinking } else { ExpressionMood::Calm },
                    MouthViseme::Rest,
                    if busy { 0.72 } else { 0.2 },
                    false,
                    false,
                    busy,
                    Some(&dot.color_hex),
                    dot_profile,
                    bloom,
                );
                self.dot_pixels
                    .entry(dot.id.clone())
                    .or_default()
                    .set(96, 96, dot_rgba);
            }
        }
        if self.add_dot_modal_open {
            let prev_rgba = render_canvas_dot_rgba(
                96,
                96,
                22,
                11,
                self.add_dot_shape,
                self.time_s,
                ExpressionMood::Happy,
                MouthViseme::Smile,
                0.45,
                false,
                false,
                false,
                Some(&self.ui_config.accent_hex),
                dot_profile,
                bloom,
            );
            self.add_dot_preview_pixels.set(96, 96, prev_rgba);
        }
    }

    /// Deterministic scene presets for headless inspection & UI verification tests.
    pub fn apply_scene_preset(&mut self, scene: &str) {
        self.gateway.online = true;
        self.gateway.connected = true;
        self.gateway.latency_ms = Some(14);
        self.gateway.version = Some("2026.11-rune".to_string());
        self.gateway.capabilities = GatewayCapabilities {
            runs: true,
            approvals: true,
            clarify: true,
            steering: true,
            session_stream: true,
            chat_completions: true,
            skills: true,
            toolsets: true,
            cron_jobs: true,
            profiles: vec!["default".into(), "ops".into()],
        };
        self.gateway_profiles = vec![
            GatewayProfile {
                name: "default".into(),
                base_url: self.gateway_base_url.clone(),
                active: true,
                online: true,
                model: Some("hermes-agent".into()),
                source: "config".into(),
            },
            GatewayProfile {
                name: "ops".into(),
                base_url: self.gateway_base_url.clone(),
                active: false,
                online: true,
                model: Some("glm-5.3".into()),
                source: "profile".into(),
            },
        ];

        match scene {
            "idle" => {
                self.active_view = ActiveView::Home;
            }
            "working" => {
                self.create_new_workspace_session(Some("Audit RUNE shader pipeline"));
                let ch_id = self.active_channel_id.clone();
                self.thinking_by_channel.insert(ch_id.clone(), true);
                self.tasks.push(BackgroundTask {
                    id: "t-work".into(),
                    channel_id: ch_id.clone(),
                    session_id: "voxcell-work".into(),
                    kind: Some("Build".into()),
                    background: Some(false),
                    title: "Audit RUNE shader pipeline".into(),
                    status: TaskLifecycleStatus::Running,
                    raw_status: Some("running".into()),
                    progress: 62.0,
                    progress_label: Some("2/3".into()),
                    last_step: Some("Running cargo check --workspace".into()),
                    steps: vec![
                        TaskStepItem { text: "Inspect compositor passes".into(), done: true, active: false },
                        TaskStepItem { text: "Verify Bayer 8x8 LUT".into(), done: true, active: false },
                        TaskStepItem { text: "Run cargo test".into(), done: false, active: true },
                    ],
                    result: None,
                    error_code: None,
                    created_at_ms: Some(1_000),
                    updated_at_ms: Some(9_000),
                    agent_done_at_ms: None,
                    confirmed_at_ms: None,
                    updated_at: "14s".into(),
                });
                self.stage_mood = ExpressionMood::Thinking;
                self.active_view = ActiveView::Home;
            }
            "attention" => {
                self.create_new_workspace_session(Some("Deploy gateway config"));
                let ch_id = self.active_channel_id.clone();
                self.approvals.push(ChannelApprovalRequest {
                    id: "apr-deploy-1".into(),
                    channel_id: ch_id,
                    run_id: Some("run-77".into()),
                    request_id: "req-77".into(),
                    execution_generation: 1,
                    kind: ApprovalKind::Approval,
                    title: "Execute Shell Command".into(),
                    tool_name: Some("terminal".into()),
                    command: "systemctl restart hermes-gateway".into(),
                    summary: "Restart Hermes gateway daemon to apply TLS profile".into(),
                    destination: "ops".into(),
                    choices: vec!["once".into(), "always".into(), "deny".into()],
                    created_at: "just now".into(),
                    resolved: false,
                    resolution: None,
                });
                self.stage_mood = ExpressionMood::Alert;
                self.active_view = ActiveView::Home;
            }
            "speaking" => {
                self.stage_mood = ExpressionMood::Speaking;
                self.audio.is_speaking = true;
                self.audio.output_level = 0.72;
                self.audio.bands = [0.4, 0.7, 0.85, 0.75, 0.6, 0.5, 0.65, 0.45, 0.35, 0.25, 0.18, 0.12];
                self.active_view = ActiveView::Home;
            }
            "canvas" => {
                self.active_view = ActiveView::Canvas;
                let samples = [
                    ("Hermes Core", CanvasShapeKind::HermesOrb, "#34D399"),
                    ("Neko Scout", CanvasShapeKind::CatBot, "#22D3EE"),
                    ("Kitsune Trace", CanvasShapeKind::FoxSpirit, "#F59E0B"),
                    ("Seraph Guard", CanvasShapeKind::AngelBot, "#A78BFA"),
                    ("Phantom CLI", CanvasShapeKind::GhostShell, "#F43F5E"),
                ];
                for (i, (label, shape, color)) in samples.iter().enumerate() {
                    let ch = create_home_session_channel(label, i + 10, 5000 + i as u64);
                    let ch_id = ch.id.clone();
                    self.channels.push(ch);
                    self.dots.push(CanvasDotData {
                        id: format!("dot-{i}"),
                        channel_id: ch_id.clone(),
                        label: (*label).to_string(),
                        shape: *shape,
                        x: 0.0,
                        y: 0.0,
                        pinned: true,
                        color_hex: (*color).to_string(),
                    });
                    if i == 2 {
                        self.thinking_by_channel.insert(ch_id, true);
                    }
                }
                apply_formation(&mut self.dots, CanvasFormation::Constellation, 860.0, 520.0);
                self.selected_dot_id = Some("dot-0".into());
            }
            "thread" => {
                self.create_new_workspace_session(Some("RUNE GPU Atlas Inspection"));
                let ch_id = self.active_channel_id.clone();
                self.entries.push(TerminalEntry::new(
                    "e1",
                    &ch_id,
                    EntryRole::User,
                    "Inspect halftone atlas throughput and confirm tests pass.",
                    "15:40",
                ));
                let mut rsn = TerminalEntry::new(
                    "e2",
                    &ch_id,
                    EntryRole::Activity,
                    "Reasoning: Measuring RGBA buffer generation vs atlas dirty-row upload cost.",
                    "15:40",
                );
                rsn.activity_kind = Some(ActivityKind::Reasoning);
                self.entries.push(rsn);

                let mut t1 = TerminalEntry::new(
                    "e3",
                    &ch_id,
                    EntryRole::Activity,
                    "terminal: cargo test -p voxcell-halftone -> 4 passed",
                    "15:41",
                );
                t1.activity_kind = Some(ActivityKind::Tool);
                t1.activity_status = Some("done".into());
                self.entries.push(t1);

                self.entries.push(TerminalEntry::new(
                    "e4",
                    &ch_id,
                    EntryRole::Hermes,
                    "All 4 halftone engine tests passed. Portrait rendering averages 0.6 ms per frame at 320x294 with bilinear sprite bank blending and two-pass phosphor bloom.",
                    "15:41",
                ));
                self.active_view = ActiveView::Thread;
            }
            "queue" => {
                self.active_view = ActiveView::Queue;
                self.skills = vec![
                    HermesSkill { name: "rust-diagnostics".into(), description: "Analyze cargo errors and workspace targets".into(), category: Some("dev".into()), enabled: true },
                    HermesSkill { name: "ui-screenshot-audit".into(), description: "Headless visual frame critique".into(), category: Some("qa".into()), enabled: true },
                ];
                self.toolsets = vec![
                    HermesToolset { name: "terminal".into(), description: "Shell command execution with approval gates".into(), tools: vec!["exec".into(), "process".into()], enabled: true },
                    HermesToolset { name: "filesystem".into(), description: "Workspace file read/write".into(), tools: vec!["read_file".into(), "write_file".into()], enabled: true },
                ];
                self.cron_jobs = vec![
                    HermesCronJob {
                        id: "cron-health".into(),
                        name: "Gateway Heartbeat".into(),
                        schedule: "*/5 * * * *".into(),
                        prompt: "Check local gateway metrics".into(),
                        channel_id: None,
                        enabled: true,
                        last_run_at: Some("2m ago".into()),
                        next_run_at: Some("in 3m".into()),
                        last_status: Some("ok".into()),
                    },
                ];
                self.approvals.push(ChannelApprovalRequest {
                    id: "apr-q-1".into(),
                    channel_id: "home".into(),
                    run_id: Some("run-88".into()),
                    request_id: "req-88".into(),
                    execution_generation: 2,
                    kind: ApprovalKind::Approval,
                    title: "Run Integration Suite".into(),
                    tool_name: Some("terminal".into()),
                    command: "cargo test --workspace".into(),
                    summary: "Execute full desktop workspace tests".into(),
                    destination: "default".into(),
                    choices: vec!["once".into(), "always".into(), "deny".into()],
                    created_at: "1m ago".into(),
                    resolved: false,
                    resolution: None,
                });
            }
            "cli" => {
                self.active_view = ActiveView::Cli;
                self.cli_edit.replace_text("/ui");
                self.execute_cli_line("/status");
            }
            "settings" => {
                self.active_view = ActiveView::Home;
                self.settings_open = true;
            }
            "phone" => {
                self.phone_widget_open = true;
                self.stage_mood = ExpressionMood::Speaking;
                self.audio.is_speaking = true;
                self.audio.output_level = 0.65;
                self.audio.bands = [0.3, 0.55, 0.78, 0.66, 0.5, 0.42, 0.52, 0.38, 0.25, 0.18, 0.14, 0.1];
            }
            _ => {}
        }
        self.step_animations(0.016);
    }
}

// ---------------------------------------------------------------------------
// P3-05 CLI workstream additions (append-only, workstream 6 'cli').
// Consumes voxcell_core::cli (TS-parity engine) + voxcell_core::ui_config.
// ---------------------------------------------------------------------------

use voxcell_core::cli::{
    ApprovalCard as CliApprovalCard, ApprovalStatus as CliApprovalStatus, CliCapabilities,
    CliCapabilityFeatures, CliContext, CliCronJob, CliExecutionResult, CliHandledResult,
    CliProfileMode, CliProfileSummary, CliSkill, CliToolset, CliTransportMode, CliUiPatch,
    execute_cli_input,
};
use voxcell_core::ui_config::{UiCustomizationPatch, curated_ui_presets};
use voxcell_core::{AccentPreset, CornerRadius, DensityMode, FontPreset, SidebarPosition};

/// One autocomplete row for the CLI slash palette (P3-05).
#[derive(Clone, Debug, PartialEq)]
pub struct CliSuggestion {
    /// Text placed into the CLI input when the row is accepted.
    pub insert: String,
    /// Primary label (command usage or @profile slug).
    pub label: String,
    /// Secondary description line.
    pub detail: String,
}

impl AppState {
    /// Profile slugs offered as `/profile` subcommands (registry + capabilities).
    pub fn cli_profile_slugs(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in &self.gateway_profiles {
            if !p.name.is_empty() && !out.contains(&p.name) {
                out.push(p.name.clone());
            }
        }
        for slug in &self.gateway.capabilities.profiles {
            if !slug.is_empty() && !out.contains(slug) {
                out.push(slug.clone());
            }
        }
        out
    }

    /// Autocomplete rows for the current CLI input: slash-set filtering from
    /// core::cli, plus `/profile` slug, `/resume` session and `/ui` preset
    /// subcommands. Empty when the input is not a slash prefix or was dismissed.
    pub fn cli_suggestions(&self) -> Vec<CliSuggestion> {
        if self.cli_suggestions_dismissed {
            return Vec::new();
        }
        // While browsing history the palette stays closed so Up/Down keep
        // walking history; typing again re-opens it.
        if self.cli_history_cursor.is_some() {
            return Vec::new();
        }
        let text = self.cli_edit.text.trim_start();
        if !text.starts_with('/') {
            return Vec::new();
        }
        let lower = text.to_ascii_lowercase();
        let has_arg_sep = lower.contains(char::is_whitespace);
        let mut parts = lower.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim();
        if has_arg_sep {
            match cmd {
                "/profile" | "/profiles" | "/p" => {
                    return self
                        .cli_profile_slugs()
                        .into_iter()
                        .filter(|slug| slug.starts_with(arg))
                        .map(|slug| CliSuggestion {
                            insert: format!("/profile {slug}"),
                            label: format!("@{slug}"),
                            detail: "Set default profile for new sessions".to_string(),
                        })
                        .collect();
                }
                "/resume" | "/switch" => {
                    return self
                        .channels
                        .iter()
                        .filter(|c| {
                            arg.is_empty()
                                || c.id.to_ascii_lowercase().contains(arg)
                                || c.name.to_ascii_lowercase().contains(arg)
                        })
                        .map(|c| CliSuggestion {
                            insert: format!("/resume {}", c.id),
                            label: c.name.clone(),
                            detail: format!("#{}", c.id),
                        })
                        .collect();
                }
                "/ui" | "/theme" => {
                    let mut out: Vec<CliSuggestion> = curated_ui_presets()
                        .iter()
                        .filter(|p| p.id.starts_with(arg))
                        .map(|p| CliSuggestion {
                            insert: format!("/ui {}", p.id),
                            label: p.id.to_string(),
                            detail: format!("{} — {}", p.name, p.subtitle),
                        })
                        .collect();
                    for extra in ["reset", "studio"] {
                        if extra.starts_with(arg) {
                            out.push(CliSuggestion {
                                insert: format!("/ui {extra}"),
                                label: extra.to_string(),
                                detail: if extra == "reset" {
                                    "Restore default Slate Monochrome".to_string()
                                } else {
                                    "Open the UI customization studio".to_string()
                                },
                            });
                        }
                    }
                    return out;
                }
                _ => return Vec::new(),
            }
        }
        voxcell_core::cli::autocomplete_slash_commands(&lower)
            .into_iter()
            .map(|spec| CliSuggestion {
                insert: format!("{} ", spec.name),
                label: spec.usage.to_string(),
                detail: spec.description.to_string(),
            })
            .collect()
    }

    /// Move the autocomplete highlight (wraps like the web palette).
    pub fn cli_suggestion_move(&mut self, delta: i32) {
        let n = self.cli_suggestions().len();
        if n == 0 {
            self.cli_suggestion_selected = 0;
            return;
        }
        let cur = self.cli_suggestion_selected.min(n - 1) as i32;
        self.cli_suggestion_selected = (cur + delta).rem_euclid(n as i32) as usize;
    }

    /// Accept suggestion `idx` into the CLI input. Returns false when absent.
    pub fn cli_accept_suggestion(&mut self, idx: usize) -> bool {
        let items = self.cli_suggestions();
        let Some(item) = items.get(idx) else {
            return false;
        };
        let insert = item.insert.clone();
        self.cli_edit.replace_text(insert);
        self.cli_suggestion_selected = 0;
        true
    }

    /// Record a submitted line in history (dedup consecutive, cap 200).
    pub fn cli_push_history(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.cli_history.last().map(String::as_str) != Some(trimmed) {
            self.cli_history.push(trimmed.to_string());
            if self.cli_history.len() > 200 {
                self.cli_history.remove(0);
            }
        }
        self.cli_history_cursor = None;
        self.cli_history_draft.clear();
    }

    /// Up-arrow: recall older history, stashing the live draft on first step.
    pub fn cli_history_prev(&mut self) -> bool {
        if self.cli_history.is_empty() {
            return false;
        }
        let next = match self.cli_history_cursor {
            None => {
                self.cli_history_draft = self.cli_edit.text.clone();
                self.cli_history.len() - 1
            }
            Some(0) => return true,
            Some(i) => i - 1,
        };
        self.cli_history_cursor = Some(next);
        let line = self.cli_history[next].clone();
        self.cli_edit.replace_text(line);
        self.cli_suggestion_selected = 0;
        true
    }

    /// Down-arrow: newer history, restoring the stashed draft past the end.
    pub fn cli_history_next(&mut self) -> bool {
        let Some(i) = self.cli_history_cursor else {
            return false;
        };
        if i + 1 < self.cli_history.len() {
            self.cli_history_cursor = Some(i + 1);
            let line = self.cli_history[i + 1].clone();
            self.cli_edit.replace_text(line);
        } else {
            self.cli_history_cursor = None;
            let draft = self.cli_history_draft.clone();
            self.cli_edit.replace_text(draft);
        }
        self.cli_suggestion_selected = 0;
        true
    }

    /// Build the TS-parity executor context from live desktop state.
    pub fn cli_context(&self) -> CliContext {
        let approvals = self
            .approvals
            .iter()
            .map(|a| CliApprovalCard {
                id: a.id.clone(),
                channel_id: a.channel_id.clone(),
                title: a.title.clone(),
                action_type: "approval".to_string(),
                mcp_tool: a.tool_name.clone(),
                run_id: a.run_id.clone(),
                request_id: Some(a.request_id.clone()),
                execution_generation: Some(a.execution_generation),
                kind: Some(
                    if a.kind == ApprovalKind::Clarify { "clarify" } else { "approval" }
                        .to_string(),
                ),
                choices: a.choices.clone(),
                command: Some(a.command.clone()),
                destination: a.destination.clone(),
                summary: a.summary.clone(),
                draft: a.command.clone(),
                status: if a.resolved {
                    if a.resolution.as_deref() == Some("deny") {
                        CliApprovalStatus::Rejected
                    } else {
                        CliApprovalStatus::Approved
                    }
                } else {
                    CliApprovalStatus::Pending
                },
                created_at: a.created_at.clone(),
            })
            .collect();
        let skills = self
            .skills
            .iter()
            .map(|s| CliSkill {
                name: s.name.clone(),
                source: s.category.clone().unwrap_or_else(|| "gateway".to_string()),
                summary: Some(s.description.clone()),
                enabled: s.enabled,
            })
            .collect();
        let toolsets = self
            .toolsets
            .iter()
            .map(|t| CliToolset {
                name: t.name.clone(),
                label: t.name.clone(),
                description: t.description.clone(),
                enabled: t.enabled,
                configured: Some(true),
                tools: t.tools.clone(),
            })
            .collect();
        let cron_jobs = self
            .cron_jobs
            .iter()
            .map(|c| CliCronJob {
                id: c.id.clone(),
                name: c.name.clone(),
                schedule: c.schedule.clone(),
                mode: if c.enabled { "agent" } else { "paused" }.to_string(),
                status: c.last_status.clone(),
                summary: c.prompt.clone(),
                channel_id: c.channel_id.clone(),
            })
            .collect();
        let profiles = self
            .gateway_profiles
            .iter()
            .map(|p| {
                let multiplexed = p.source == "profile";
                CliProfileSummary {
                    id: p.name.clone(),
                    label: p.name.clone(),
                    mode: if multiplexed {
                        CliProfileMode::Multiplexed
                    } else {
                        CliProfileMode::Standalone
                    },
                    base_url: p.base_url.clone(),
                    route_prefix: if multiplexed {
                        format!("/p/{}", p.name)
                    } else {
                        String::new()
                    },
                    connected: p.online,
                }
            })
            .collect();
        let caps = &self.gateway.capabilities;
        CliContext {
            active_channel: self.active_channel_id.clone(),
            available_channels: self
                .channels
                .iter()
                .map(|c| c.id.to_ascii_lowercase())
                .collect(),
            approvals,
            tasks: self.tasks.clone(),
            brief: Vec::new(),
            is_muted: self.voice.muted,
            skills,
            toolsets,
            cron_jobs,
            gateway_connected: self.gateway.online,
            gateway_url: Some(self.gateway_base_url.clone()),
            active_model: self.selected_model.clone(),
            gateway_profile: Some(self.gateway.active_profile.clone()),
            profiles,
            draft_profile: Some(self.gateway.active_profile.clone()),
            route_prefix: Some(String::new()),
            health_status: if self.gateway.online {
                Some("ok".to_string())
            } else {
                None
            },
            session_key_header: Some("X-Hermes-Session-Key".to_string()),
            capabilities: Some(CliCapabilities {
                session_key_header: Some("X-Hermes-Session-Key".to_string()),
                features: CliCapabilityFeatures {
                    chat_completions: caps.chat_completions,
                    reasoning_streaming: caps.runs,
                    run_submission: caps.runs,
                    run_events_sse: caps.runs,
                    run_approval: caps.approvals,
                    session_chat_stream: caps.session_stream,
                },
            }),
            transport_mode: Some(match self.gateway.active_transport {
                GatewayTransport::Runs => CliTransportMode::Runs,
                GatewayTransport::SessionStream => CliTransportMode::SessionStream,
                GatewayTransport::ChatCompletions => CliTransportMode::ChatCompletions,
            }),
            resolved_transport: Some(
                match self.gateway.active_transport {
                    GatewayTransport::Runs => "runs",
                    GatewayTransport::SessionStream => "session_stream",
                    GatewayTransport::ChatCompletions => "chat_completions",
                }
                .to_string(),
            ),
            gateway_error: self.gateway.last_error.clone(),
        }
    }

    /// P3-05 submit path: history + echo, then dispatch through the core
    /// TS-parity CLI engine (`execute_cli_input`). Desktop-only surfaces
    /// (view switching, avatar mode, voice/tts, phone, reset, model) and
    /// arg-bearing `/gateway <url>` / `/cron <action> <id>` mutations keep the
    /// existing draft handler so no desktop capability is lost.
    pub fn cli_submit_line(&mut self, raw: &str) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        self.cli_push_history(trimmed);
        self.cli_suggestions_dismissed = false;
        self.cli_suggestion_selected = 0;

        let cmd_token = trimmed
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let has_arg = trimmed
            .splitn(2, char::is_whitespace)
            .nth(1)
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let action = parse_cli_input(trimmed);
        let desktop_only = matches!(
            action,
            CliAction::ResetActiveSession
                | CliAction::SetModel(_)
                | CliAction::SwitchView(_)
                | CliAction::SetAvatarMode(_)
                | CliAction::VoiceControl(_)
                | CliAction::TtsControl(_)
                | CliAction::TogglePhoneWidget(_)
        );
        let desktop_mutating =
            has_arg && matches!(cmd_token.as_str(), "/gateway" | "/gw" | "/cron" | "/jobs");
        if desktop_only || desktop_mutating {
            self.execute_cli_line(trimmed);
            return;
        }

        self.cli_log.push(format!("> {trimmed}"));
        // `/theme` aliases `/ui` in the desktop slash set; the TS-parity
        // engine only knows `/ui`, so normalize before dispatch.
        let engine_input = if cmd_token == "/theme" {
            format!("/ui{}", &trimmed["/theme".len()..])
        } else {
            trimmed.to_string()
        };
        let ctx = self.cli_context();
        match execute_cli_input(&engine_input, &ctx) {
            CliExecutionResult::Chat(prompt) => self.dispatch_user_prompt(&prompt, false),
            CliExecutionResult::Handled(res) => self.cli_apply_handled(res),
        }
    }

    /// Apply a TS-parity `CliHandledResult` to desktop state + terminal log.
    pub fn cli_apply_handled(&mut self, res: CliHandledResult) {
        if res.clear_terminal {
            self.cli_log.clear();
        } else {
            for line in res.output.lines() {
                self.cli_log.push(line.to_string());
            }
        }
        if let Some(channel) = res.switch_channel {
            if let Some(found) = self
                .channels
                .iter()
                .find(|c| c.id.eq_ignore_ascii_case(&channel))
            {
                self.active_channel_id = found.id.clone();
                self.active_view = ActiveView::Thread;
            }
        }
        if let Some(profile) = res.switch_profile {
            self.gateway.active_profile = profile.clone();
            self.pending_commands
                .push(UiCommand::SwitchProfile { profile });
        }
        if res.start_new_session {
            self.create_new_workspace_session(res.new_session_prompt.as_deref());
            if let Some(prompt) = res.new_session_prompt {
                self.dispatch_user_prompt(&prompt, false);
            }
        }
        if let Some(card_id) = res.approve_card_id {
            let choice = res.approve_choice.map(|c| c.as_str()).unwrap_or("once");
            self.resolve_approval(&card_id, choice);
        }
        if let Some(card_id) = res.reject_card_id {
            self.resolve_approval(&card_id, "deny");
        }
        if res.cancel_active_run {
            let ch_id = self.active_channel_id.clone();
            if let Some(run_id) = self.active_run_ids.get(&ch_id).cloned() {
                self.pending_commands.push(UiCommand::StopRun {
                    run_id,
                    channel_id: ch_id.clone(),
                });
            }
            self.thinking_by_channel.insert(ch_id, false);
        }
        if let Some(mode) = res.set_transport_mode {
            self.gateway.active_transport = match mode {
                CliTransportMode::Runs => GatewayTransport::Runs,
                CliTransportMode::SessionStream => GatewayTransport::SessionStream,
                CliTransportMode::ChatCompletions | CliTransportMode::Auto => {
                    GatewayTransport::ChatCompletions
                }
            };
        }
        if res.toggle_mute {
            self.voice.muted = !self.voice.muted;
            self.pending_commands
                .push(UiCommand::ToggleMute(self.voice.muted));
        }
        if let Some(mood) = res.set_mood {
            self.stage_mood = mood;
        }
        if let Some(shader) = res.set_shader {
            self.shader_mode = shader;
        }
        if let Some(patch) = res.ui_patch {
            self.cli_apply_ui_patch(patch);
        }
        self.step_animations(0.016);
    }

    /// Route a CLI `/ui` / `/theme` patch through `ui_config` (P1-03) so the
    /// whole workspace theme reacts, then persist.
    pub fn cli_apply_ui_patch(&mut self, patch: CliUiPatch) {
        let mut p = UiCustomizationPatch::default();
        if patch.open_customizer {
            p.open_customizer = Some(true);
            self.settings_open = true;
        }
        p.reset_all = patch.reset_all;
        p.preset_id = patch.preset_id.clone();
        p.accent_preset = patch.accent_preset.as_deref().and_then(AccentPreset::from_id);
        p.accent_hex = patch.accent_hex.clone();
        p.portrait_tint_hex = patch.portrait_tint_hex.clone().map(Some);
        p.background_preset_id = patch.background_preset_id.clone();
        p.sidebar_visible = patch.sidebar_visible;
        p.sidebar_position = patch.sidebar_position.as_deref().map(|s| {
            if s == "right" {
                SidebarPosition::Right
            } else {
                SidebarPosition::Left
            }
        });
        p.corner_radius = patch
            .corner_radius
            .as_deref()
            .and_then(CornerRadius::from_id);
        p.density = patch.density.as_deref().and_then(|s| match s {
            "compact" => Some(DensityMode::Compact),
            "airy" => Some(DensityMode::Airy),
            "comfortable" => Some(DensityMode::Comfortable),
            _ => None,
        });
        p.font_preset = patch.font_preset.as_deref().and_then(FontPreset::from_id);
        p.reduced_motion = patch.reduced_motion;
        apply_ui_customization_patch(&mut self.ui_config, &p);
        self.pending_commands.push(UiCommand::PersistUiConfig);
    }
}

// ---------------------------------------------------------------------------
// P3-05 Queue workstream additions (append-only, workstream 5 'queue').
// Consumes voxcell_hermes::profiles redacted summaries (never raw keys) and
// dispatches through the existing UiCommand set — no new variants, because
// app/bridge.rs matches UiCommand exhaustively (foreign file, phone-tray owns).
// ---------------------------------------------------------------------------
impl AppState {
    /// Effective Operations tab: explicit choice wins, else auto-switch to
    /// approvals while any gate is pending (web HermesOperationsPanel).
    pub fn queue_active_tab(&self) -> &str {
        match self.queue_tab.as_deref() {
            Some(t @ ("runtime" | "approvals" | "jobs")) => t,
            _ => {
                if self.approvals.iter().any(|a| !a.resolved) {
                    "approvals"
                } else {
                    "runtime"
                }
            }
        }
    }

    /// Redacted profile health cards (TS `HermesProfileSummary` shape). Only
    /// `has_api_key: bool` ever surfaces — raw bearer tokens never reach the UI.
    pub fn queue_profile_summaries(&self) -> Vec<voxcell_hermes::profiles::HermesProfileSummary> {
        use voxcell_hermes::profiles::{HermesProfileMode, HermesProfileSource, HermesProfileSummary};
        let mut out: Vec<HermesProfileSummary> = self.gateway_profiles
            .iter()
            .map(|p| {
                let standalone = p.source == "standalone";
                let route_prefix = if standalone || p.name == "default" {
                    String::new()
                } else {
                    format!("/p/{}", p.name)
                };
                let is_default = p.active || p.name == self.gateway.active_profile;
                HermesProfileSummary {
                    id: p.name.clone(),
                    label: p.name.clone(),
                    mode: if standalone {
                        HermesProfileMode::Standalone
                    } else {
                        HermesProfileMode::Multiplexed
                    },
                    base_url: p.base_url.clone(),
                    route_prefix,
                    source: match p.source.as_str() {
                        "custom" => HermesProfileSource::Custom,
                        "env" => HermesProfileSource::Env,
                        _ => HermesProfileSource::Discovered,
                    },
                    // Desktop only knows the primary key's presence; per-profile
                    // keys stay server-side (redacted summary contract).
                    has_api_key: is_default && self.has_hermes_key,
                    is_default,
                    description: None,
                    connected: Some(p.online),
                    active_model: p.model.clone(),
                    health_status: if p.online { Some("ok".to_string()) } else { None },
                    error: if p.online { None } else { self.gateway.last_error.clone() },
                }
            })
            .collect();
        // P5 queue-forms: append UI-registered custom profiles (redacted
        // summaries only). Gateway-discovered ids win over local registrations.
        for reg in &self.queue_registered_profiles {
            if !out.iter().any(|p| p.id == reg.id) {
                out.push(reg.clone());
            }
        }
        out
    }

    /// Parallel lane count for a profile from the sidebar session badges.
    pub fn queue_profile_lanes(&self, profile: &str) -> usize {
        self.session_profiles
            .values()
            .filter(|p| p.as_str() == profile)
            .count()
    }

    /// Queue switcher: make `name` the default profile and dispatch the switch.
    pub fn queue_switch_profile(&mut self, name: &str) {
        if name.is_empty() {
            return;
        }
        self.gateway.active_profile = name.to_string();
        for p in &mut self.gateway_profiles {
            p.active = p.name == name;
        }
        self.pending_commands.push(UiCommand::SwitchProfile {
            profile: name.to_string(),
        });
    }

    /// Stop the run owned by `channel_id` (existing StopRun command) and drop
    /// the local thinking flag; the bridge clears active_run_ids on completion.
    pub fn queue_stop_run(&mut self, channel_id: &str) {
        if let Some(run_id) = self.active_run_ids.get(channel_id).cloned() {
            self.pending_commands.push(UiCommand::StopRun {
                run_id,
                channel_id: channel_id.to_string(),
            });
        }
        self.thinking_by_channel.insert(channel_id.to_string(), false);
    }

    /// Send the steer composer draft to the targeted run (existing SteerRun).
    /// Empty drafts are ignored; a sent draft clears the composer.
    pub fn queue_send_steer(&mut self) {
        let Some(run_id) = self.queue_steer_run.clone() else {
            return;
        };
        let text = self.queue_steer_edit.text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.pending_commands.push(UiCommand::SteerRun { run_id, text });
        self.queue_steer_edit.clear();
        self.queue_steer_run = None;
    }

    /// Reply to a clarify gate with the card's draft answer. Routes through
    /// `resolve_approval` so the command carries request_id + generation and
    /// `is_clarify = true`. Empty answers are ignored (web disables Reply).
    pub fn queue_reply_clarify(&mut self, approval_id: &str) {
        let answer = self
            .queue_clarify_edits
            .get(approval_id)
            .map(|e| e.text.trim().to_string())
            .unwrap_or_default();
        if answer.is_empty() {
            return;
        }
        self.resolve_approval(approval_id, &answer);
        self.queue_clarify_edits.remove(approval_id);
    }
}

// ---------------------------------------------------------------------------
// P3-06 Settings workstream additions (append-only, workstream 7 'settings').
// Data mirrors of the voxcell_store::{vault, updater} contracts — voxcell-ui
// has no voxcell-store dependency (ui/Cargo.toml is theme-chrome-owned), so
// field names/semantics match the store types 1:1 and app/bridge.rs can map
// them directly when it drains `settings_outbox`.
// ---------------------------------------------------------------------------

/// Mirrors `voxcell_store::vault::PreferenceBackupSummary` (camelCase contract).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VaultBackupEntry {
    pub id: String,
    pub created_at: i64,
    /// "manual" | "pre-update" (TS `createBackup` reason union).
    pub reason: String,
    pub app_version: String,
    pub accent_hex: String,
    pub accent_preset: String,
    pub background_id: String,
    pub custom_slot_count: usize,
    pub workspace_title: String,
}

/// Mirrors `voxcell_store::vault::UpdateReceipt`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VaultUpdateReceipt {
    pub updated_at: i64,
    pub previous_version: String,
    pub new_version: String,
    pub previous_commit: Option<String>,
    pub new_commit: Option<String>,
    pub backup_id: String,
    /// "git-pull" | "managed-refresh".
    pub mode: String,
    pub preserved_categories: Vec<String>,
}

/// Mirrors `voxcell_store::updater::UpdateStepResult` (status as TS string union).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdaterStep {
    pub id: String,
    pub label: String,
    /// "ok" | "skipped" | "failed".
    pub status: String,
    pub detail: Option<String>,
}

/// Mirrors `voxcell_store::updater::AppUpdateStatus`.
#[derive(Clone, Debug, PartialEq)]
pub struct UpdaterStatus {
    pub current_version: String,
    pub current_commit: Option<String>,
    pub branch: Option<String>,
    pub latest_version: Option<String>,
    pub latest_commit: Option<String>,
    pub update_available: bool,
    /// "git-pull" | "managed-refresh".
    pub update_mode: String,
    pub can_self_update: bool,
    pub dirty_worktree: bool,
    pub dirty_reason: Option<String>,
    pub release_title: Option<String>,
    pub release_notes: Option<String>,
    pub release_url: Option<String>,
    pub checked_at: i64,
    pub upstream_error: Option<String>,
    pub pending_receipt: Option<VaultUpdateReceipt>,
}

impl Default for UpdaterStatus {
    fn default() -> Self {
        Self {
            current_version: "0.1.0".to_string(),
            current_commit: None,
            branch: None,
            latest_version: None,
            latest_commit: None,
            update_available: false,
            update_mode: "git-pull".to_string(),
            can_self_update: false,
            dirty_worktree: false,
            dirty_reason: None,
            release_title: None,
            release_notes: None,
            release_url: None,
            checked_at: 0,
            upstream_error: None,
            pending_receipt: None,
        }
    }
}

/// P3-06 vault/updater dispatch. Deliberately NOT `UiCommand` variants:
/// app/bridge.rs matches `UiCommand` exhaustively and is phone-tray-owned, so
/// new variants would break the app build until arms land there. The bridge
/// drains this outbox through `voxcell_store::{vault, updater}` instead.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    /// `PreferencesVault::create_backup` — reason "manual" | "pre-update".
    VaultCreateBackup { reason: String },
    /// `PreferencesVault::restore_backup`.
    VaultRestore { backup_id: String },
    /// Export the current snapshot JSON (web download parity).
    VaultExport,
    /// `PreferencesVault::write_vault` with reason "import".
    VaultImport { json: String },
    /// `updater::build_app_update_status` probe (GitHub release/commit).
    UpdaterProbe,
    /// `updater::apply_update_with_snapshot` (pre-update backup hook inside).
    UpdaterApply,
    /// `updater::acknowledge_update_receipt`.
    UpdaterDismissReceipt,
}

impl AppState {
    /// P3-06: apply a raw web-contract JSON patch through
    /// `core::ui_config::apply_ui_patch` (sanitize + apply), then bridge the
    /// non-config side effects (backdrop, mood, voice, view) onto desktop
    /// state like App.tsx does. Returns the outcome summary.
    pub fn settings_apply_json_patch(&mut self, raw: &serde_json::Value) -> String {
        let outcome = voxcell_core::ui_config::apply_ui_patch(&self.ui_config, raw);
        self.ui_config = outcome.next;
        self.settings_bridge_patch_side_effects(&outcome.sanitized);
        self.pending_commands.push(UiCommand::PersistUiConfig);
        self.step_animations(0.016);
        outcome.summary
    }

    /// P3-06: natural-language UI intent (web `parseNaturalUiIntent` parity).
    /// Returns false when no intent was recognized.
    pub fn settings_apply_intent(&mut self, text: &str) -> bool {
        use voxcell_core::ui_config::{apply_sanitized_patch, parse_natural_ui_intent};
        match parse_natural_ui_intent(text, &self.ui_config) {
            Some(patch) => {
                let outcome = apply_sanitized_patch(&self.ui_config, &patch);
                self.ui_config = outcome.next;
                self.settings_bridge_patch_side_effects(&outcome.sanitized);
                self.ui_studio_feedback = Some(outcome.summary);
                self.pending_commands.push(UiCommand::PersistUiConfig);
                self.step_animations(0.016);
                true
            }
            None => {
                self.ui_studio_feedback = Some(
                    "No UI intent recognized — try \"solaris-amber\", \"zen-mono\" or \"move sidebar to the right\"."
                        .to_string(),
                );
                false
            }
        }
    }

    /// P3-06: accent swatch apply (8 web swatches) via the sanitize/apply pipeline.
    pub fn settings_apply_swatch(&mut self, id: &str) -> bool {
        let Some(sw) = voxcell_core::ui_config::get_accent_swatch(id) else {
            return false;
        };
        let raw = serde_json::json!({
            "accentPreset": sw.id,
            "accentHex": sw.accent_hex,
            "accentSoftHex": sw.accent_soft_hex,
            "accentBrightHex": sw.accent_bright_hex,
            "surfaceHex": sw.surface_hex,
        });
        self.ui_studio_feedback = Some(self.settings_apply_json_patch(&raw));
        true
    }

    /// P3-06: curated preset apply (7 web presets) — config + backdrop + mood.
    pub fn settings_apply_curated_preset(&mut self, id: &str) -> bool {
        if voxcell_core::ui_config::get_curated_preset(id).is_none() {
            return false;
        }
        let raw = serde_json::json!({ "presetId": id });
        self.ui_studio_feedback = Some(self.settings_apply_json_patch(&raw));
        true
    }

    /// P3-06: bridge sanitized-patch side effects that live outside ui_config.
    fn settings_bridge_patch_side_effects(
        &mut self,
        p: &voxcell_core::ui_config::UiCustomizationPatch,
    ) {
        let mut bg_changed = false;
        if let Some(bg) = &p.background_preset_id {
            if let Some(id) = voxcell_core::BackgroundPresetId::from_web_id(bg) {
                self.bg_settings.preset = id;
                self.bg_settings.active_id = bg.clone();
                bg_changed = true;
            } else if bg.starts_with("custom-") {
                self.bg_settings.active_id = bg.clone();
                bg_changed = true;
            }
        }
        if let Some(op) = p.background_opacity {
            self.bg_settings.opacity = op;
            bg_changed = true;
        }
        if let Some(blur) = p.glass_blur_px {
            self.bg_settings.blur_px = blur;
            bg_changed = true;
        }
        if bg_changed {
            self.pending_commands
                .push(UiCommand::PersistBackgroundSettings);
        }
        if let Some(mood) = &p.mood {
            if let Some(m) = settings_mood_from_id(mood) {
                self.stage_mood = m;
            }
        }
        if let Some(muted) = p.voice_muted {
            self.voice.muted = muted;
            self.pending_commands.push(UiCommand::ToggleMute(muted));
        }
        if let Some(view) = &p.active_view {
            if let Some(v) = settings_view_from_id(view) {
                self.active_view = v;
            }
        }
        if p.open_customizer == Some(true) {
            self.settings_open = true;
        }
        if p.close_customizer == Some(true) {
            self.settings_open = false;
        }
    }
}

/// P3-06: web mood id -> desktop ExpressionMood (best-effort TS parity).
pub fn settings_mood_from_id(id: &str) -> Option<ExpressionMood> {
    match id.trim().to_ascii_lowercase().as_str() {
        "calm" => Some(ExpressionMood::Calm),
        "focused" => Some(ExpressionMood::Focused),
        "listening" => Some(ExpressionMood::Listening),
        "thinking" => Some(ExpressionMood::Thinking),
        "speaking" => Some(ExpressionMood::Speaking),
        "happy" | "ecstatic" => Some(ExpressionMood::Happy),
        "alert" => Some(ExpressionMood::Alert),
        _ => None,
    }
}

/// P3-06: web view id -> desktop ActiveView.
pub fn settings_view_from_id(id: &str) -> Option<ActiveView> {
    match id.trim().to_ascii_lowercase().as_str() {
        "home" => Some(ActiveView::Home),
        "canvas" => Some(ActiveView::Canvas),
        "thread" => Some(ActiveView::Thread),
        "queue" => Some(ActiveView::Queue),
        "cli" => Some(ActiveView::Cli),
        _ => None,
    }
}


// ---------------------------------------------------------------------------
// P4-01 Canvas workstream additions (append-only, workstream 8 'canvas').
// Consumes voxcell_core::canvas (duet.canvasSessions.v2 / canvasPins.v1 parity),
// voxcell_core::layout::compute_canvas_popover_alignment, voxcell_core::motion
// stagger math and rune-motion springs for Constellation/Orbit/Grid transitions.
// No new UiCommand variants: persistence rides the existing PersistDots, voice
// rides ToggleMic, prompts ride dispatch_user_prompt -> SendPrompt.
// ---------------------------------------------------------------------------

use voxcell_core::canvas::{
    BRAILLE_SHAPE_CATALOG, BrailleShapeKind, CanvasDotOrigin, CanvasMood, CanvasPin,
    CanvasPinKind, CanvasSession, CanvasViewport, DEFAULT_CANVAS_VIEWPORT, CANVAS_ZOOM_STEP,
    DotAutonomyMode, DotSandboxEnv, arrange_sessions_formation, clamp_canvas_pan,
    clamp_canvas_position, create_hermes_session_id, find_open_pin_spot, get_shape_meta,
    load_canvas_pins_from_json, load_canvas_sessions_from_json, save_canvas_pins_json,
    save_canvas_sessions_json, zoom_viewport_around_point,
};
use voxcell_core::layout::{CanvasPopoverAlign, compute_canvas_popover_alignment};
use voxcell_core::motion::{StaggerOptions, compute_stagger_delay_ms};

/// Design-space size of the drafted dot field. Percentage space (TS parity)
/// maps onto it: `x_pct = x / 860 * 100`, `y_pct = y / 520 * 100`.
pub const CANVAS_DESIGN_W: f32 = 860.0;
pub const CANVAS_DESIGN_H: f32 = 520.0;

/// Spring layout transition record: `dots[i].x/y` already hold the TARGET;
/// `from` keeps the pre-transition positions so the view samples rune-motion
/// springs as a pure function of `time_s` (no per-frame integration needed).
/// Per-dot delays come from `core::motion::compute_stagger_delay_ms`.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasLayoutAnim {
    pub start_s: f32,
    pub from: Vec<(f32, f32)>,
    pub delays_ms: Vec<f64>,
}

/// Stage-pan gesture anchor (background drag): original clamped pan plus the
/// TS 4px move-threshold flag (`panMetaRef`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasPanDrag {
    pub orig_x: f32,
    pub orig_y: f32,
    pub moved: bool,
}

impl AppState {
    /// Live telemetry for a dot's channel: thinking flag or a running task.
    pub fn canvas_dot_busy(&self, channel_id: &str) -> bool {
        self.thinking_by_channel.get(channel_id).copied().unwrap_or(false)
            || self.tasks.iter().any(|t| {
                t.channel_id == channel_id && t.status == TaskLifecycleStatus::Running
            })
    }

    /// Desktop SDF engine kind -> TS braille vault kind (catalog-index parity;
    /// both catalogs hold 9 shapes — documented mapping assumption).
    pub fn canvas_shape_to_braille(shape: CanvasShapeKind) -> BrailleShapeKind {
        let idx = CanvasShapeKind::all().iter().position(|s| *s == shape).unwrap_or(0);
        BRAILLE_SHAPE_CATALOG[idx % BRAILLE_SHAPE_CATALOG.len()].kind
    }

    /// TS braille vault kind -> desktop SDF engine kind (inverse mapping).
    pub fn canvas_braille_to_shape(kind: BrailleShapeKind) -> CanvasShapeKind {
        let idx = BRAILLE_SHAPE_CATALOG.iter().position(|m| m.kind == kind).unwrap_or(0);
        CanvasShapeKind::all()[idx % CanvasShapeKind::all().len()]
    }

    fn canvas_session_from_dot(&self, dot: &CanvasDotData) -> CanvasSession {
        let (x_pct, y_pct) = clamp_canvas_position(
            dot.x / CANVAS_DESIGN_W * 100.0,
            dot.y / CANVAS_DESIGN_H * 100.0,
        );
        let shape = Self::canvas_shape_to_braille(dot.shape);
        let meta = get_shape_meta(shape);
        let busy = self.canvas_dot_busy(&dot.channel_id);
        CanvasSession {
            id: format!("session-{}", dot.channel_id),
            channel_id: dot.channel_id.clone(),
            hermes_session_id: create_hermes_session_id(&dot.channel_id),
            profile: self
                .session_profiles
                .get(&dot.channel_id)
                .cloned()
                .filter(|p| !p.is_empty() && p != "default"),
            agent_persona: meta.default_agent_persona.to_string(),
            name: dot.label.clone(),
            topic: format!("{} spatial node", dot.label),
            shape,
            tint: meta.default_tint,
            x_pct,
            y_pct,
            mood: if busy { CanvasMood::Thinking } else { CanvasMood::Neutral },
            eye_style: meta.default_eye_style,
            is_core: Some(false),
            origin: Some(CanvasDotOrigin::Workspace),
            autonomy_mode: DotAutonomyMode::Manual,
            schedule_label: "On-demand".to_string(),
            autonomy_paused: Some(false),
            sandbox_env: DotSandboxEnv::CloudSandbox,
            workspace_dir: format!("~/hermes/dots/{}", dot.channel_id),
            skills: vec!["files".to_string(), "browser".to_string()],
            connected_to: None,
        }
    }

    /// Rebuild the TS-parity session mirror from the live dot list, preserving
    /// richer metadata (persona/autonomy/skills/tint) of already-known channels.
    pub fn canvas_sync_sessions(&mut self) {
        let mut next = Vec::with_capacity(self.dots.len());
        for dot in &self.dots {
            let mut s = self.canvas_session_from_dot(dot);
            if let Some(old) = self.canvas_sessions.iter().find(|o| o.channel_id == dot.channel_id) {
                s.agent_persona = old.agent_persona.clone();
                s.topic = old.topic.clone();
                s.tint = old.tint;
                s.eye_style = old.eye_style;
                s.autonomy_mode = old.autonomy_mode;
                s.schedule_label = old.schedule_label.clone();
                s.autonomy_paused = old.autonomy_paused;
                s.sandbox_env = old.sandbox_env;
                s.workspace_dir = old.workspace_dir.clone();
                s.skills = old.skills.clone();
                s.connected_to = old.connected_to.clone();
                s.profile = old.profile.clone().or(s.profile);
            }
            next.push(s);
        }
        self.canvas_sessions = next;
    }

    /// `duet.canvasSessions.v2` payload for the current dots (syncs first).
    pub fn canvas_sessions_json(&mut self) -> String {
        self.canvas_sync_sessions();
        save_canvas_sessions_json(&self.canvas_sessions)
    }

    /// `duet.canvasPins.v1` payload for the current pins.
    pub fn canvas_pins_json(&self) -> String {
        save_canvas_pins_json(&self.canvas_pins)
    }

    /// Hydrate sessions + dots from a `duet.canvasSessions.v2` payload
    /// (core normalize + prune/migrate). Returns the hydrated session count.
    pub fn canvas_hydrate_sessions_json(&mut self, raw: &str) -> usize {
        let sessions = load_canvas_sessions_from_json(raw);
        let count = sessions.len();
        self.dots = sessions
            .iter()
            .map(|s| CanvasDotData {
                id: format!("dot-{}", s.channel_id),
                channel_id: s.channel_id.clone(),
                label: s.name.clone(),
                shape: Self::canvas_braille_to_shape(s.shape),
                x: s.x_pct / 100.0 * CANVAS_DESIGN_W,
                y: s.y_pct / 100.0 * CANVAS_DESIGN_H,
                pinned: true,
                color_hex: self.ui_config.accent_hex.clone(),
            })
            .collect();
        self.canvas_sessions = sessions;
        self.canvas_layout_anim = None;
        count
    }

    /// Hydrate pins from a `duet.canvasPins.v1` payload. Returns the count.
    pub fn canvas_hydrate_pins_json(&mut self, raw: &str) -> usize {
        self.canvas_pins = load_canvas_pins_from_json(raw);
        self.canvas_pins.len()
    }

    /// Switch formation with a rune-motion spring transition: sessions are
    /// re-arranged in percentage space (TS `arrangeSessionsFormation`), dots
    /// snap to the TARGET immediately (persistence saves targets) and the
    /// view samples springs from `from` -> target until settled.
    pub fn canvas_set_formation(&mut self, formation: CanvasFormation) {
        self.canvas_sync_sessions();
        let arranged = arrange_sessions_formation(&self.canvas_sessions, formation);
        let from: Vec<(f32, f32)> = self.dots.iter().map(|d| (d.x, d.y)).collect();
        let delays_ms: Vec<f64> = (0..self.dots.len())
            .map(|i| {
                compute_stagger_delay_ms(
                    i as f64,
                    StaggerOptions {
                        reduced_motion: Some(self.ui_config.reduced_motion),
                        ..StaggerOptions::default()
                    },
                )
            })
            .collect();
        for (dot, s) in self.dots.iter_mut().zip(arranged.iter()) {
            dot.x = s.x_pct / 100.0 * CANVAS_DESIGN_W;
            dot.y = s.y_pct / 100.0 * CANVAS_DESIGN_H;
        }
        self.canvas_sessions = arranged;
        self.canvas_formation = formation;
        self.canvas_layout_anim = if self.ui_config.reduced_motion || from.is_empty() {
            None
        } else {
            Some(CanvasLayoutAnim { start_s: self.time_s, from, delays_ms })
        };
        self.pending_commands.push(UiCommand::PersistDots);
    }

    /// Display position (design space) of dot `index`: spring-sampled while a
    /// layout transition is in flight, target once settled. Pure over state.
    pub fn canvas_layout_display_pos(&self, index: usize) -> (f32, f32) {
        let Some(dot) = self.dots.get(index) else { return (0.0, 0.0); };
        let target = (dot.x, dot.y);
        let Some(anim) = &self.canvas_layout_anim else { return target; };
        let Some(&from) = anim.from.get(index) else { return target; };
        let delay_s = anim.delays_ms.get(index).copied().unwrap_or(0.0) as f32 / 1000.0;
        let t = self.time_s - anim.start_s - delay_s;
        if t <= 0.0 {
            return from;
        }
        let spring = rune_motion::Spring::SMOOTH;
        if t >= spring.settling_duration(0.5) {
            return target;
        }
        let (x, _) = spring.sample(from.0, 0.0, target.0, t);
        let (y, _) = spring.sample(from.1, 0.0, target.1, t);
        (x, y)
    }

    /// True when no layout transition is in flight (all springs settled).
    pub fn canvas_layout_settled(&self) -> bool {
        match &self.canvas_layout_anim {
            None => true,
            Some(a) => {
                let max_delay = a.delays_ms.iter().cloned().fold(0.0_f64, f64::max) as f32 / 1000.0;
                self.time_s - a.start_s
                    >= max_delay + rune_motion::Spring::SMOOTH.settling_duration(0.5)
            }
        }
    }

    /// Measured stage size (field rect) with design-space fallback.
    pub fn canvas_stage_size(&self) -> (f32, f32) {
        match self.canvas_field_rect {
            Some(r) if r[2] > 0.0 && r[3] > 0.0 => (r[2], r[3]),
            _ => (CANVAS_DESIGN_W, CANVAS_DESIGN_H),
        }
    }

    /// Pan by a screen-px delta, clamped like TS `clampCanvasPan`.
    pub fn canvas_pan_by(&mut self, dx: f32, dy: f32) {
        let v = self.canvas_viewport;
        let (pan_x, pan_y) = clamp_canvas_pan(v.pan_x + dx, v.pan_y + dy, v.zoom);
        self.canvas_viewport = CanvasViewport { pan_x, pan_y, zoom: v.zoom };
    }

    /// Zoom around a field-local cursor point (TS `zoomViewportAroundPoint`).
    pub fn canvas_zoom_at(&mut self, next_zoom: f32, cursor_local_x: f32, cursor_local_y: f32) {
        let (w, h) = self.canvas_stage_size();
        self.canvas_viewport = zoom_viewport_around_point(
            self.canvas_viewport, next_zoom, cursor_local_x, cursor_local_y, w, h,
        );
    }

    /// Toolbar +/- zoom step around the stage center.
    pub fn canvas_zoom_step(&mut self, direction: i32) {
        let (w, h) = self.canvas_stage_size();
        let next = self.canvas_viewport.zoom + direction as f32 * CANVAS_ZOOM_STEP;
        self.canvas_zoom_at(next, w * 0.5, h * 0.5);
    }

    /// Reset pan/zoom to the TS default viewport.
    pub fn canvas_reset_viewport(&mut self) {
        self.canvas_viewport = DEFAULT_CANVAS_VIEWPORT;
    }

    /// TS `viewportActive`: zoom/pan deviates enough to show the reset chip.
    pub fn canvas_viewport_active(&self) -> bool {
        (self.canvas_viewport.zoom - 1.0).abs() > 0.01
            || self.canvas_viewport.pan_x.abs() > 2.0
            || self.canvas_viewport.pan_y.abs() > 2.0
    }

    /// Design-space point -> field-local screen px under the current viewport
    /// (zoom about the stage center + pan, matching the TS CSS transform).
    pub fn canvas_world_to_screen(&self, x: f32, y: f32) -> (f32, f32) {
        let (w, h) = self.canvas_stage_size();
        let v = self.canvas_viewport;
        let wx = x / CANVAS_DESIGN_W * w;
        let wy = y / CANVAS_DESIGN_H * h;
        (
            wx * v.zoom + v.pan_x + w * 0.5 * (1.0 - v.zoom),
            wy * v.zoom + v.pan_y + h * 0.5 * (1.0 - v.zoom),
        )
    }

    /// Field-local screen px -> design-space point (inverse of the above).
    pub fn canvas_screen_to_world(&self, sx: f32, sy: f32) -> (f32, f32) {
        let (w, h) = self.canvas_stage_size();
        let v = self.canvas_viewport;
        let wx = (sx - v.pan_x - w * 0.5 * (1.0 - v.zoom)) / v.zoom.max(0.001);
        let wy = (sy - v.pan_y - h * 0.5 * (1.0 - v.zoom)) / v.zoom.max(0.001);
        (wx / w.max(0.001) * CANVAS_DESIGN_W, wy / h.max(0.001) * CANVAS_DESIGN_H)
    }

    /// Edge-clamped popover alignment for a dot (TS computeCanvasPopoverAlignment:
    /// left <24%, right >76%, center otherwise).
    pub fn canvas_popover_align(&self, dot_id: &str) -> CanvasPopoverAlign {
        match self.dots.iter().find(|d| d.id == dot_id) {
            Some(d) => compute_canvas_popover_alignment((d.x / CANVAS_DESIGN_W * 100.0) as f64),
            None => CanvasPopoverAlign::Center,
        }
    }

    /// Open a dot's popover: select it and focus its channel.
    pub fn canvas_open_popover(&mut self, dot_id: &str) {
        self.selected_dot_id = Some(dot_id.to_string());
        self.canvas_popover_dot = Some(dot_id.to_string());
        if let Some(d) = self.dots.iter().find(|d| d.id == dot_id) {
            self.active_channel_id = d.channel_id.clone();
        }
    }

    pub fn canvas_close_popover(&mut self) {
        self.canvas_popover_dot = None;
    }

    /// Submit the inline dot prompt draft to the popover/selected dot.
    pub fn canvas_submit_prompt_edit(&mut self) {
        let target = self.canvas_popover_dot.clone().or_else(|| self.selected_dot_id.clone());
        let Some(dot_id) = target else { return; };
        let text = self.canvas_prompt_edit.text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.canvas_prompt_edit.clear();
        self.canvas_dispatch_dot_prompt(&dot_id, &text);
    }

    /// Dispatch a prompt to a specific dot's channel (no view switch — the
    /// canvas stays put; SendPrompt rides the existing bridge path).
    pub fn canvas_dispatch_dot_prompt(&mut self, dot_id: &str, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        let Some(dot) = self.dots.iter().find(|d| d.id == dot_id) else { return; };
        self.active_channel_id = dot.channel_id.clone();
        self.dispatch_user_prompt(trimmed, false);
    }

    /// Inline voice bar: toggle hands-free VAD through the existing ToggleMic.
    pub fn canvas_toggle_voice(&mut self) {
        let next = !self.voice.hands_free_vad;
        self.voice.hands_free_vad = next;
        self.pending_commands.push(UiCommand::ToggleMic(next));
    }

    /// Add a spatial pin at the TS open-spot finder position; returns its id.
    pub fn canvas_add_pin(&mut self, kind: CanvasPinKind, title: &str, detail: &str) -> String {
        self.canvas_sync_sessions();
        let sess: Vec<(f32, f32)> = self.canvas_sessions.iter().map(|s| (s.x_pct, s.y_pct)).collect();
        let pins: Vec<(f32, f32)> = self.canvas_pins.iter().map(|p| (p.x_pct, p.y_pct)).collect();
        let (x_pct, y_pct) = find_open_pin_spot(&sess, &pins);
        self.now_ms += 1;
        let id = format!("pin-{}", self.now_ms);
        let clean_title: String = if title.trim().is_empty() {
            "Workspace Note".to_string()
        } else {
            title.trim().chars().take(48).collect()
        };
        self.canvas_pins.push(CanvasPin {
            id: id.clone(),
            kind,
            title: clean_title,
            detail: detail.trim().chars().take(180).collect(),
            meta: "Canvas pin".to_string(),
            x_pct,
            y_pct,
            linked_channel_id: None,
        });
        self.canvas_pin_composer_open = false;
        self.canvas_pin_title_edit.clear();
        self.pending_commands.push(UiCommand::PersistDots);
        id
    }

    pub fn canvas_remove_pin(&mut self, pin_id: &str) {
        self.canvas_pins.retain(|p| p.id != pin_id);
        self.pending_commands.push(UiCommand::PersistDots);
    }

    /// Dot drag start: snap any in-flight layout spring, anchor the origin.
    pub fn canvas_begin_dot_drag(&mut self, dot_id: &str) {
        self.canvas_layout_anim = None;
        self.canvas_dragging_dot = Some(dot_id.to_string());
        if let Some(d) = self.dots.iter().find(|d| d.id == dot_id) {
            self.canvas_drag_origin = Some((d.x, d.y));
        }
    }

    /// Dot drag move: screen-px delta -> design space (undo zoom/pan scale),
    /// clamped to the TS canvas bounds in percentage space.
    pub fn canvas_move_dot_drag(&mut self, dot_id: &str, dx_screen: f32, dy_screen: f32) {
        let Some((ox, oy)) = self.canvas_drag_origin else { return; };
        let (w, h) = self.canvas_stage_size();
        let scale_x = (self.canvas_viewport.zoom * (w / CANVAS_DESIGN_W)).max(0.001);
        let scale_y = (self.canvas_viewport.zoom * (h / CANVAS_DESIGN_H)).max(0.001);
        let nx = ox + dx_screen / scale_x;
        let ny = oy + dy_screen / scale_y;
        let (x_pct, y_pct) = clamp_canvas_position(
            nx / CANVAS_DESIGN_W * 100.0,
            ny / CANVAS_DESIGN_H * 100.0,
        );
        let channel_id = {
            let Some(d) = self.dots.iter_mut().find(|d| d.id == dot_id) else { return; };
            d.x = x_pct / 100.0 * CANVAS_DESIGN_W;
            d.y = y_pct / 100.0 * CANVAS_DESIGN_H;
            d.channel_id.clone()
        };
        if let Some(s) = self.canvas_sessions.iter_mut().find(|s| s.channel_id == channel_id) {
            s.x_pct = x_pct;
            s.y_pct = y_pct;
        }
    }

    /// Dot drag end: sync the session mirror and persist.
    pub fn canvas_end_dot_drag(&mut self) {
        self.canvas_dragging_dot = None;
        self.canvas_drag_origin = None;
        self.canvas_sync_sessions();
        self.pending_commands.push(UiCommand::PersistDots);
    }
}

// ---------------------------------------------------------------------------
// P5 queue-forms (append-only, workstream 4 'queue-forms'): deferred Operations
// forms — New-Job, Add/Delete-Profile, transport toggle, header session switch.
// Raw profile keys live only in the transient key draft and the RegisterProfile
// command payload; registered profiles are stored as redacted summaries built
// by the voxcell_hermes::profiles catalog pipeline (never raw keys in state).
// ---------------------------------------------------------------------------
impl AppState {
    /// Toggle the New-Job form (web showNewJobForm); clears stale errors.
    pub fn queue_toggle_new_job_form(&mut self) {
        self.queue_new_job_open = !self.queue_new_job_open;
        self.queue_job_error = None;
    }

    /// Close the New-Job form without submitting (web Cancel).
    pub fn queue_cancel_new_job(&mut self) {
        self.queue_new_job_open = false;
        self.queue_job_error = None;
    }

    /// Validate + dispatch CreateJob. Empty prompt/schedule mirror the web
    /// disabled-submit guard; an offline gateway is rejected locally with a
    /// form error (no command queued, no network attempt).
    pub fn queue_submit_new_job(&mut self) {
        if self.queue_job_busy {
            return;
        }
        let prompt = self.queue_job_prompt_edit.text.trim().to_string();
        let schedule = self.queue_job_schedule_edit.text.trim().to_string();
        if prompt.is_empty() || schedule.is_empty() {
            self.queue_job_error =
                Some("Enter a job prompt and a cron schedule.".to_string());
            return;
        }
        if !self.gateway.online {
            self.queue_job_error =
                Some("Gateway offline — connect before creating jobs.".to_string());
            return;
        }
        let name = self.queue_job_name_edit.text.trim().to_string();
        self.queue_job_error = None;
        self.queue_job_busy = true;
        self.pending_commands.push(UiCommand::CreateJob {
            name,
            schedule,
            prompt,
        });
    }

    /// Apply the bridge CreateJob outcome (BridgeEvent::JobFormOutcome):
    /// success clears name/prompt drafts and closes the form (web parity,
    /// schedule kept); failure surfaces the error banner and re-enables submit.
    pub fn queue_apply_job_outcome(&mut self, result: Result<(), String>) {
        self.queue_job_busy = false;
        match result {
            Ok(()) => {
                self.queue_job_error = None;
                self.queue_job_name_edit.clear();
                self.queue_job_prompt_edit.clear();
                self.queue_new_job_open = false;
            }
            Err(e) => self.queue_job_error = Some(e),
        }
    }

    /// Toggle the Add-Profile form (web showAddProfileForm).
    pub fn queue_toggle_add_profile_form(&mut self) {
        self.queue_add_profile_open = !self.queue_add_profile_open;
        self.queue_profile_error = None;
    }

    /// Close the Add-Profile form, dropping the transient key draft.
    pub fn queue_cancel_add_profile(&mut self) {
        self.queue_add_profile_open = false;
        self.queue_profile_error = None;
        self.queue_profile_key_edit.clear();
    }

    /// Register a custom profile through voxcell_hermes::profiles: reserved-slug
    /// guard + slug normalization, then a redacted summary from the real catalog
    /// pipeline. The raw key travels only in the RegisterProfile command payload
    /// (bridge store seam) and is cleared from the draft immediately.
    pub fn queue_submit_profile(&mut self) {
        use voxcell_hermes::profiles::{
            HermesProfileDefinition, HermesProfileMode, HermesProfileSource,
            build_profile_catalog, is_valid_custom_profile_slug, is_valid_http_url,
            normalize_profile_id,
        };
        if self.queue_profile_busy {
            return;
        }
        let raw = self.queue_profile_slug_edit.text.trim().to_lowercase();
        if raw.is_empty() {
            self.queue_profile_error =
                Some("Enter a profile slug (e.g. coder, research).".to_string());
            return;
        }
        // Normalize FIRST (TS normalizeProfileId semantics): "Coder Agent!" ->
        // "coder-agent"; the reserved/validity guard then runs on the normalized
        // slug, and inputs that collapse to empty/"default" are still rejected.
        let id = normalize_profile_id(Some(&raw));
        if !is_valid_custom_profile_slug(Some(&id)) {
            self.queue_profile_error =
                Some(format!("Invalid or reserved profile slug: {id}"));
            return;
        }
        if self.queue_registered_profiles.iter().any(|p| p.id == id)
            || self.gateway_profiles.iter().any(|p| p.name == id)
        {
            self.queue_profile_error = Some(format!("Profile @{id} already exists."));
            return;
        }
        let standalone = self.queue_profile_mode == "standalone";
        let base_url = self.queue_profile_url_edit.text.trim().to_string();
        if standalone && base_url.is_empty() {
            self.queue_profile_error = Some(
                "Standalone profiles need a gateway URL (e.g. http://127.0.0.1:8643)."
                    .to_string(),
            );
            return;
        }
        if standalone && !is_valid_http_url(Some(&base_url)) {
            self.queue_profile_error = Some(format!("Invalid gateway URL: {base_url}"));
            return;
        }
        let api_key = self.queue_profile_key_edit.text.trim().to_string();
        let mode = if standalone {
            HermesProfileMode::Standalone
        } else {
            HermesProfileMode::Multiplexed
        };
        let mut def = HermesProfileDefinition::new(id.clone(), mode, HermesProfileSource::Custom)
            .with_label(id.clone());
        if standalone {
            def = def.with_base_url(base_url.clone());
        }
        if !api_key.is_empty() {
            def = def.with_api_key(api_key.clone());
        }
        // Multiplexed profiles inherit the primary gateway URL (web parity).
        let mut env = HashMap::new();
        if !self.gateway.base_url.is_empty() {
            env.insert(
                "HERMES_API_URL".to_string(),
                self.gateway.base_url.clone(),
            );
        }
        let catalog = build_profile_catalog(&env, &[], &[def]);
        let Some(summary) = catalog.runtime(&id).map(|r| r.to_redacted_summary(None)) else {
            self.queue_profile_error =
                Some(format!("Profile @{id} could not be registered."));
            return;
        };
        self.queue_profile_error = None;
        self.queue_profile_busy = true;
        self.queue_registered_profiles.push(summary);
        self.pending_commands.push(UiCommand::RegisterProfile {
            id,
            mode: self.queue_profile_mode.clone(),
            base_url,
            api_key,
        });
        self.queue_profile_slug_edit.clear();
        self.queue_profile_url_edit.clear();
        self.queue_profile_key_edit.clear();
        self.queue_add_profile_open = false;
    }

    /// Open the delete-confirm step for a custom profile (web confirm dialog).
    /// The default profile can never be deleted.
    pub fn queue_request_delete_profile(&mut self, id: &str) {
        if id.is_empty() || id == "default" {
            return;
        }
        self.queue_delete_confirm = Some(id.to_string());
        self.queue_profile_error = None;
    }

    /// Dismiss the delete-confirm step without deleting.
    pub fn queue_cancel_delete_profile(&mut self) {
        self.queue_delete_confirm = None;
    }

    /// Confirmed delete: drop the redacted registration and dispatch the bridge
    /// command (store key cleanup). Default profile is protected.
    pub fn queue_confirm_delete_profile(&mut self) {
        let Some(id) = self.queue_delete_confirm.take() else {
            return;
        };
        if id == "default" {
            self.queue_profile_error =
                Some("The default profile cannot be deleted.".to_string());
            return;
        }
        self.queue_registered_profiles.retain(|p| p.id != id);
        self.queue_profile_busy = true;
        self.queue_profile_error = None;
        self.pending_commands.push(UiCommand::DeleteProfile { id });
    }

    /// Apply the bridge RegisterProfile/DeleteProfile persistence outcome.
    pub fn queue_apply_profile_outcome(&mut self, result: Result<(), String>) {
        self.queue_profile_busy = false;
        if let Err(e) = result {
            self.queue_profile_error = Some(e);
        }
    }

    /// Transport-mode toggle (web onChangeTransportMode): update the transport
    /// consumed by SendPrompt and dispatch the bridge client sync.
    pub fn queue_set_transport(&mut self, mode: GatewayTransport) {
        self.gateway.active_transport = mode;
        self.pending_commands.push(UiCommand::SetTransportMode { mode });
    }

    /// Header session switcher (web onSelectChannel): focus another channel and
    /// notify the bridge. Unknown/empty ids are ignored.
    pub fn queue_switch_session(&mut self, channel_id: &str) {
        if channel_id.is_empty()
            || !self.channels.iter().any(|c| c.id == channel_id)
        {
            return;
        }
        self.active_channel_id = channel_id.to_string();
        self.pending_commands.push(UiCommand::SwitchSession {
            channel_id: channel_id.to_string(),
        });
    }
}
