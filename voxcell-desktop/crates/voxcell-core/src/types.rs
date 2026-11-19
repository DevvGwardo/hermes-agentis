use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionMood {
    #[default]
    Calm,
    Focused,
    Listening,
    Thinking,
    Speaking,
    Happy,
    Alert,
}

impl ExpressionMood {
    pub fn label(self) -> &'static str {
        match self {
            Self::Calm => "Calm",
            Self::Focused => "Focused",
            Self::Listening => "Listening",
            Self::Thinking => "Thinking",
            Self::Speaking => "Speaking",
            Self::Happy => "Happy",
            Self::Alert => "Alert",
        }
    }

    pub fn next_cycle(self) -> Self {
        match self {
            Self::Calm => Self::Happy,
            Self::Happy => Self::Focused,
            Self::Focused => Self::Thinking,
            Self::Thinking => Self::Listening,
            Self::Listening => Self::Speaking,
            Self::Speaking => Self::Alert,
            Self::Alert => Self::Calm,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EyeState {
    #[default]
    Open,
    Half,
    Blink,
    Wide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MouthViseme {
    #[default]
    Closed,
    Rest,
    Mbp,
    Fv,
    Lth,
    Smile,
    Small,
    Mid,
    Round,
    Ee,
    Wide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActiveView {
    #[default]
    Home,
    Canvas,
    Thread,
    Queue,
    Cli,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AvatarMode {
    #[default]
    Halftone,
    Ascii3d,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AsciiShaderMode {
    #[default]
    Solid,
    Wire,
    Depth,
    Holo,
    Norm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PhosphorTheme {
    #[default]
    Emerald,
    Cyan,
    Gold,
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HorizonMode {
    #[default]
    Silk,
    Peaks,
    Pulse,
    Ribbon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DotProfile {
    Crisp,
    #[default]
    Balanced,
    Phosphor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DensityMode {
    Compact,
    #[default]
    Comfortable,
    Airy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SidebarPosition {
    #[default]
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionScopeTab {
    #[default]
    Sessions,
    Gateway,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EntryRole {
    User,
    #[default]
    Hermes,
    Activity,
    Command,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    Tool,
    Reasoning,
    Subagent,
    Commentary,
    Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalOutcome {
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    IterationBudgetExhausted,
    AdmissionConflict,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEntry {
    pub id: String,
    pub channel_id: String,
    pub role: EntryRole,
    pub text: String,
    pub timestamp: String,
    pub created_at_ms: Option<u64>,
    pub streaming: bool,
    pub activity_kind: Option<ActivityKind>,
    pub activity_status: Option<String>,
    pub reasoning_detail: Option<String>,
    pub error_code: Option<String>,
    pub terminal_outcome: Option<TerminalOutcome>,
    pub model: Option<String>,
}

impl TerminalEntry {
    pub fn new(
        id: impl Into<String>,
        channel_id: impl Into<String>,
        role: EntryRole,
        text: impl Into<String>,
        timestamp: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            channel_id: channel_id.into(),
            role,
            text: text.into(),
            timestamp: timestamp.into(),
            created_at_ms: None,
            streaming: false,
            activity_kind: None,
            activity_status: None,
            reasoning_detail: None,
            error_code: None,
            terminal_outcome: None,
            model: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskStepItem {
    pub text: String,
    pub done: bool,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskLifecycleStatus {
    #[default]
    Running,
    Paused,
    Completed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackgroundTask {
    pub id: String,
    pub channel_id: String,
    pub session_id: String,
    pub kind: Option<String>,
    pub background: Option<bool>,
    pub title: String,
    pub status: TaskLifecycleStatus,
    pub raw_status: Option<String>,
    pub progress: f32,
    pub progress_label: Option<String>,
    pub last_step: Option<String>,
    pub steps: Vec<TaskStepItem>,
    pub result: Option<String>,
    pub error_code: Option<String>,
    pub created_at_ms: Option<u64>,
    pub updated_at_ms: Option<u64>,
    pub agent_done_at_ms: Option<u64>,
    pub confirmed_at_ms: Option<u64>,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    #[default]
    Approval,
    Clarify,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelApprovalRequest {
    pub id: String,
    pub channel_id: String,
    pub run_id: Option<String>,
    pub request_id: String,
    pub execution_generation: u32,
    pub kind: ApprovalKind,
    pub title: String,
    pub tool_name: Option<String>,
    pub command: String,
    pub summary: String,
    pub destination: String,
    pub choices: Vec<String>,
    pub created_at: String,
    pub resolved: bool,
    pub resolution: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelMeta {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub session_id: String,
    pub unread: usize,
    pub origin: SessionOrigin,
    pub has_local_history: bool,
    pub model: Option<String>,
    pub updated_at_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionOrigin {
    #[default]
    Workspace,
    External,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    #[default]
    Idle,
    Thinking,
    Speaking,
    Running,
    Offline,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesAgentSession {
    pub channel_id: String,
    pub session_id: String,
    pub title: String,
    pub status: SessionStatus,
    pub model: String,
    pub source: String,
    pub last_message: String,
    pub updated_at_ms: Option<u64>,
    pub message_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GatewayTransport {
    Runs,
    SessionStream,
    #[default]
    ChatCompletions,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GatewayCapabilities {
    pub runs: bool,
    pub approvals: bool,
    pub clarify: bool,
    pub steering: bool,
    pub session_stream: bool,
    pub chat_completions: bool,
    pub skills: bool,
    pub toolsets: bool,
    pub cron_jobs: bool,
    pub profiles: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayProfile {
    pub name: String,
    pub base_url: String,
    pub active: bool,
    pub online: bool,
    pub model: Option<String>,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HermesGatewayInfo {
    pub online: bool,
    pub connected: bool,
    pub base_url: String,
    pub active_profile: String,
    pub model: String,
    pub active_transport: GatewayTransport,
    pub capabilities: GatewayCapabilities,
    pub version: Option<String>,
    pub latency_ms: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesSkill {
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesToolset {
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesCronJob {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub prompt: String,
    pub channel_id: Option<String>,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub last_status: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesModelOption {
    pub id: String,
    pub label: String,
    pub provider: Option<String>,
}

/// Legacy ambient backdrop identifiers kept for API compatibility with
/// `voxcell-ui::theme` (exhaustive match). Under the `slate-v1` epoch the
/// default `EmeraldAurora` variant renders the neutral **Aurora Drift** slate
/// wash (its primary color follows `theme.accent`, now Slate 400 `#94A3B8`);
/// emerald is no longer ambient chrome. Web-parity string ids live in
/// [`BackgroundPresetId::web_id`] / [`BackgroundSettings::active_id`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundPresetId {
    #[default]
    EmeraldAurora,
    AbyssGrid,
    CyberMist,
    SolarFlare,
    MidnightPulse,
    ObsidianSilk,
    PhosphorGrain,
}

impl BackgroundPresetId {
    pub fn label(self) -> &'static str {
        match self {
            Self::EmeraldAurora => "Aurora Drift",
            Self::AbyssGrid => "Abyss Grid",
            Self::CyberMist => "Cyber Mist",
            Self::SolarFlare => "Solar Flare",
            Self::MidnightPulse => "Midnight Pulse",
            Self::ObsidianSilk => "Obsidian Silk",
            Self::PhosphorGrain => "Phosphor Grain",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::EmeraldAurora,
            Self::AbyssGrid,
            Self::CyberMist,
            Self::SolarFlare,
            Self::MidnightPulse,
            Self::ObsidianSilk,
            Self::PhosphorGrain,
        ]
    }

    /// Crosswalk to the web `BackgroundPresetKind` ids used by
    /// `settingsSnapshot` / `backgrounds/persistence.ts`. The legacy scaffold
    /// variants map best-effort onto the 7 web presets (documented assumption;
    /// full shader parity is P2-03).
    pub fn web_id(self) -> &'static str {
        match self {
            Self::EmeraldAurora => "aurora-drift",
            Self::AbyssGrid => "mesh-gradient",
            Self::CyberMist => "nebula",
            Self::SolarFlare => "scanline-shimmer",
            Self::MidnightPulse => "constellation-drift",
            Self::ObsidianSilk => "horizon-tide",
            Self::PhosphorGrain => "braille-rain",
        }
    }

    pub fn from_web_id(id: &str) -> Option<Self> {
        Self::all().iter().copied().find(|p| p.web_id() == id)
    }
}

/// Whether an uploaded backdrop slot holds an image or a video.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundMediaType {
    #[default]
    Image,
    Video,
}

/// User-uploaded backdrop slot (port of `BackgroundCustomSlot` in
/// `src/backgrounds/types.ts`, sanitized by `backgrounds/persistence.ts`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundCustomSlot {
    pub id: String,
    pub label: String,
    pub media_type: BackgroundMediaType,
    pub src: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BackgroundSettings {
    /// Legacy scaffold preset selector (kept for `voxcell-ui` compatibility).
    #[serde(alias = "preset")]
    pub preset: BackgroundPresetId,
    #[serde(alias = "opacity")]
    pub opacity: f32,
    #[serde(alias = "motion_speed")]
    pub motion_speed: f32,
    #[serde(alias = "intensity")]
    pub intensity: f32,
    /// Web-parity active backdrop id (`aurora-drift` default, `custom-*` slots).
    #[serde(alias = "active_id")]
    pub active_id: String,
    #[serde(alias = "blur_px")]
    pub blur_px: f32,
    #[serde(alias = "custom_slots")]
    pub custom_slots: Vec<BackgroundCustomSlot>,
}

impl Default for BackgroundSettings {
    fn default() -> Self {
        Self {
            preset: BackgroundPresetId::EmeraldAurora,
            opacity: 0.64,
            motion_speed: 1.0,
            intensity: 0.8,
            active_id: "aurora-drift".to_string(),
            blur_px: 12.0,
            custom_slots: Vec::new(),
        }
    }
}

/// Accent swatch ids — port of `UiAccentPreset` (`src/utils/uiCustomization.ts`).
/// `slate` is the `slate-v1` default; emerald/amber/rose remain available as
/// opt-in accents while their hex values stay reserved for semantic telemetry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AccentPreset {
    #[default]
    Slate,
    Emerald,
    Cyan,
    Violet,
    Amber,
    Rose,
    Crimson,
    Mono,
    Custom,
}

impl AccentPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Slate => "slate",
            Self::Emerald => "emerald",
            Self::Cyan => "cyan",
            Self::Violet => "violet",
            Self::Amber => "amber",
            Self::Rose => "rose",
            Self::Crimson => "crimson",
            Self::Mono => "mono",
            Self::Custom => "custom",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "slate" => Some(Self::Slate),
            "emerald" => Some(Self::Emerald),
            "cyan" => Some(Self::Cyan),
            "violet" => Some(Self::Violet),
            "amber" => Some(Self::Amber),
            "rose" => Some(Self::Rose),
            "crimson" => Some(Self::Crimson),
            "mono" => Some(Self::Mono),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// Corner radius style — port of `UiCornerRadius` (sharp 4px / rounded 16px / pill 26px).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CornerRadius {
    Sharp,
    #[default]
    Rounded,
    Pill,
}

impl CornerRadius {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sharp => "sharp",
            Self::Rounded => "rounded",
            Self::Pill => "pill",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "sharp" => Some(Self::Sharp),
            "rounded" => Some(Self::Rounded),
            "pill" => Some(Self::Pill),
            _ => None,
        }
    }
}

/// Font stack preset — port of `UiFontPreset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FontPreset {
    #[default]
    Jakarta,
    Mono,
    Display,
    System,
}

impl FontPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jakarta => "jakarta",
            Self::Mono => "mono",
            Self::Display => "display",
            Self::System => "system",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "jakarta" => Some(Self::Jakarta),
            "mono" => Some(Self::Mono),
            "display" => Some(Self::Display),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

/// Composer dock style — port of `UiComposerStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ComposerStyle {
    #[default]
    Docked,
    Floating,
    Minimal,
}

impl ComposerStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Docked => "docked",
            Self::Floating => "floating",
            Self::Minimal => "minimal",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "docked" => Some(Self::Docked),
            "floating" => Some(Self::Floating),
            "minimal" => Some(Self::Minimal),
            _ => None,
        }
    }
}

/// UI customization state — `slate-v1` Charcoal-Slate default epoch.
///
/// Port of `UiCustomizationConfig` in `src/utils/uiCustomization.ts` plus the
/// legacy scaffold fields (`bg_hex`, `amber_hex`, `rose_hex`, `glass_opacity`,
/// `sidebar_collapsed`, `custom_css_note`, `preset_name`) kept so existing
/// `voxcell-ui` / `voxcell-store` consumers keep compiling. Emerald `#34D399`,
/// Amber `#F59E0B` and Rose `#F43F5E` are semantic telemetry colors only
/// (live/working/error) — never ambient chrome defaults.
///
/// Serde: camelCase primary keys (web snapshot parity) with snake_case aliases
/// so legacy desktop-persisted JSON still deserializes; missing fields fall
/// back to the slate-v1 defaults via struct-level `serde(default)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiCustomizationConfig {
    #[serde(alias = "accent_preset")]
    pub accent_preset: AccentPreset,
    #[serde(alias = "accent_hex")]
    pub accent_hex: String,
    #[serde(alias = "accent_soft_hex")]
    pub accent_soft_hex: String,
    #[serde(alias = "accent_bright_hex")]
    pub accent_bright_hex: String,
    /// Semantic "working/thinking" telemetry color (never ambient chrome).
    #[serde(alias = "amber_hex")]
    pub amber_hex: String,
    /// Semantic "error/offline/stop" telemetry color (never ambient chrome).
    #[serde(alias = "rose_hex")]
    pub rose_hex: String,
    #[serde(alias = "surface_hex")]
    pub surface_hex: String,
    #[serde(alias = "bg_hex")]
    pub bg_hex: String,
    #[serde(alias = "text_hex")]
    pub text_hex: String,
    #[serde(alias = "panel_alpha")]
    pub panel_alpha: f32,
    #[serde(alias = "border_opacity")]
    pub border_opacity: f32,
    #[serde(alias = "glow_intensity")]
    pub glow_intensity: f32,
    #[serde(alias = "corner_radius")]
    pub corner_radius: CornerRadius,
    #[serde(alias = "density")]
    pub density: DensityMode,
    #[serde(alias = "font_preset")]
    pub font_preset: FontPreset,
    #[serde(alias = "font_scale")]
    pub font_scale: f32,
    #[serde(alias = "sidebar_visible")]
    pub sidebar_visible: bool,
    #[serde(alias = "sidebar_position")]
    pub sidebar_position: SidebarPosition,
    #[serde(alias = "sidebar_width", alias = "sidebar_width_px")]
    pub sidebar_width: f32,
    #[serde(alias = "sidebar_queue_visible")]
    pub sidebar_queue_visible: bool,
    #[serde(alias = "sidebar_channels_visible")]
    pub sidebar_channels_visible: bool,
    #[serde(alias = "show_top_nav")]
    pub show_top_nav: bool,
    #[serde(alias = "composer_style")]
    pub composer_style: ComposerStyle,
    #[serde(alias = "workspace_title")]
    pub workspace_title: String,
    #[serde(alias = "portrait_visible")]
    pub portrait_visible: bool,
    #[serde(alias = "portrait_scale")]
    pub portrait_scale: f32,
    #[serde(alias = "portrait_tint_hex")]
    pub portrait_tint_hex: Option<String>,
    #[serde(alias = "dot_profile", alias = "braille_dot_profile")]
    pub dot_profile: DotProfile,
    #[serde(alias = "bloom", alias = "braille_bloom")]
    pub bloom: f32,
    #[serde(alias = "horizon_visible")]
    pub horizon_visible: bool,
    #[serde(alias = "horizon_opacity")]
    pub horizon_opacity: f32,
    #[serde(alias = "horizon_mode", alias = "horizon_style")]
    pub horizon_mode: HorizonMode,
    #[serde(alias = "status_readout_visible")]
    pub status_readout_visible: bool,
    #[serde(alias = "animation_speed")]
    pub animation_speed: f32,
    #[serde(alias = "reduced_motion")]
    pub reduced_motion: bool,
    // ---- Legacy scaffold fields (desktop-only, no web counterpart) ----
    /// Legacy glass fill alpha; kept in sync with `panel_alpha` by patch appliers.
    #[serde(alias = "glass_opacity")]
    pub glass_opacity: f32,
    #[serde(alias = "sidebar_collapsed")]
    pub sidebar_collapsed: bool,
    #[serde(alias = "preset_name")]
    pub preset_name: String,
    #[serde(alias = "custom_css_note")]
    pub custom_css_note: String,
}

impl Default for UiCustomizationConfig {
    fn default() -> Self {
        Self {
            accent_preset: AccentPreset::Slate,
            accent_hex: "#94A3B8".to_string(),
            accent_soft_hex: "#CBD5E1".to_string(),
            accent_bright_hex: "#E2E8F0".to_string(),
            amber_hex: "#F59E0B".to_string(),
            rose_hex: "#F43F5E".to_string(),
            surface_hex: "#11141B".to_string(),
            bg_hex: "#0B0D12".to_string(),
            text_hex: "#F1F5F9".to_string(),
            panel_alpha: 0.80,
            border_opacity: 0.22,
            glow_intensity: 0.30,
            corner_radius: CornerRadius::Rounded,
            density: DensityMode::Comfortable,
            font_preset: FontPreset::Jakarta,
            font_scale: 1.0,
            sidebar_visible: true,
            sidebar_position: SidebarPosition::Left,
            sidebar_width: 296.0,
            sidebar_queue_visible: true,
            sidebar_channels_visible: true,
            show_top_nav: true,
            composer_style: ComposerStyle::Docked,
            workspace_title: "Hermes".to_string(),
            portrait_visible: true,
            portrait_scale: 1.0,
            portrait_tint_hex: None,
            dot_profile: DotProfile::Balanced,
            bloom: 0.75,
            horizon_visible: true,
            horizon_opacity: 0.8,
            horizon_mode: HorizonMode::Silk,
            status_readout_visible: true,
            animation_speed: 1.0,
            reduced_motion: false,
            glass_opacity: 0.80,
            sidebar_collapsed: false,
            preset_name: "Slate Monochrome".to_string(),
            custom_css_note: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoiceSettings {
    pub hands_free_vad: bool,
    pub tts_enabled: bool,
    pub tts_voice: String,
    pub stt_language: String,
    pub muted: bool,
    pub push_to_talk_active: bool,
    pub input_level: f32,
    pub output_level: f32,
    pub bands: [f32; 12],
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            hands_free_vad: false,
            tts_enabled: true,
            tts_voice: "aura-2-thalia-en".to_string(),
            stt_language: "en-US".to_string(),
            muted: false,
            push_to_talk_active: false,
            input_level: 0.0,
            output_level: 0.0,
            bands: [0.0; 12],
        }
    }
}

pub type ChannelThinkMap = HashMap<String, bool>;
