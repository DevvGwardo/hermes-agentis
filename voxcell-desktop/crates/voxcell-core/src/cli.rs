//! Port of `src/utils/cliEngine.ts` (P1-06) plus the desktop slash-command
//! parser from the original draft.
//!
//! Two layers:
//! 1. `parse_cli_input` / `SLASH_COMMANDS` — the desktop CLI surface (kept for
//!    voxcell-ui compatibility, includes `/profile` alongside the full set).
//! 2. `execute_cli_input` — the TS-parity pure executor with context: `/profile`
//!    & `/profiles` registry listing/switching, approvals, gateway doctor,
//!    transport, skills/toolsets/cron/memory readouts, `/ui` presets + NL edits.

use crate::types::{ActiveView, AsciiShaderMode, AvatarMode, BackgroundTask, ExpressionMood, TaskLifecycleStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashCommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub description: &'static str,
}

pub const SLASH_COMMANDS: &[SlashCommandSpec] = &[
    SlashCommandSpec { name: "/help", aliases: &["/?", "/h"], usage: "/help", description: "Show all slash commands and desktop shortcuts" },
    SlashCommandSpec { name: "/new", aliases: &["/n"], usage: "/new [title]", description: "Create a new Hermes workspace session" },
    SlashCommandSpec { name: "/reset", aliases: &[], usage: "/reset", description: "Reset conversation history for the active session" },
    SlashCommandSpec { name: "/sessions", aliases: &["/ls"], usage: "/sessions", description: "List workspace and gateway Hermes sessions" },
    SlashCommandSpec { name: "/resume", aliases: &["/switch"], usage: "/resume <id-or-name>", description: "Switch active thread to a matching session" },
    SlashCommandSpec { name: "/model", aliases: &["/m"], usage: "/model [id]", description: "Show or select active Hermes model" },
    SlashCommandSpec { name: "/status", aliases: &[], usage: "/status", description: "Print gateway status, transport, latency, and active profile" },
    SlashCommandSpec { name: "/gateway", aliases: &["/gw"], usage: "/gateway [url]", description: "Inspect or change Hermes gateway base URL" },
    SlashCommandSpec { name: "/profile", aliases: &["/p"], usage: "/profile [name]", description: "Show or switch active ~/.hermes profile" },
    SlashCommandSpec { name: "/skills", aliases: &[], usage: "/skills", description: "List available Hermes skills and enable state" },
    SlashCommandSpec { name: "/toolsets", aliases: &[], usage: "/toolsets", description: "List Hermes toolsets and exposed tools" },
    SlashCommandSpec { name: "/cron", aliases: &["/jobs"], usage: "/cron [pause|resume|run <id>]", description: "Inspect or control Hermes cron jobs" },
    SlashCommandSpec { name: "/tasks", aliases: &["/bg"], usage: "/tasks", description: "Show active, paused, and completed turn/background tasks" },
    SlashCommandSpec { name: "/queue", aliases: &["/approvals"], usage: "/queue", description: "Open Queue view for approvals, skills, and cron jobs" },
    SlashCommandSpec { name: "/approve", aliases: &[], usage: "/approve [once|always|deny]", description: "Resolve the latest pending approval gate" },
    SlashCommandSpec { name: "/stop", aliases: &["/cancel"], usage: "/stop", description: "Cancel the currently streaming Hermes run" },
    SlashCommandSpec { name: "/clear", aliases: &["/cls"], usage: "/clear", description: "Clear local terminal scrollback output" },
    SlashCommandSpec { name: "/voice", aliases: &["/mic"], usage: "/voice [on|off|ptt]", description: "Toggle hands-free VAD or push-to-talk audio" },
    SlashCommandSpec { name: "/tts", aliases: &["/mute"], usage: "/tts [on|off]", description: "Toggle Deepgram Aura-2 voice synthesis" },
    SlashCommandSpec { name: "/home", aliases: &[], usage: "/home", description: "Switch to Home stage view" },
    SlashCommandSpec { name: "/canvas", aliases: &["/dots"], usage: "/canvas", description: "Switch to spatial Canvas view" },
    SlashCommandSpec { name: "/thread", aliases: &["/chat"], usage: "/thread", description: "Switch to Thread view" },
    SlashCommandSpec { name: "/mode", aliases: &["/3d"], usage: "/mode [halftone|3d]", description: "Switch avatar renderer between Halftone and 3D ASCII" },
    SlashCommandSpec { name: "/ui", aliases: &["/theme"], usage: "/ui <preset or natural-language edit>", description: "Apply a theme preset or natural-language UI tweak" },
    SlashCommandSpec { name: "/phone", aliases: &["/widget"], usage: "/phone [open|close]", description: "Toggle the floating Phone Video Widget" },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CliAction {
    PrintHelp(String),
    NewSession(Option<String>),
    ResetActiveSession,
    ListSessions,
    ResumeSession(String),
    SetModel(Option<String>),
    ShowStatus,
    SetGateway(Option<String>),
    SetProfile(Option<String>),
    ListSkills,
    ListToolsets,
    CronCommand(Option<String>),
    ListTasks,
    SwitchView(ActiveView),
    ResolveApproval(String),
    StopActiveRun,
    ClearTerminal,
    VoiceControl(Option<String>),
    TtsControl(Option<String>),
    SetAvatarMode(Option<AvatarMode>),
    ApplyUiCustomization(String),
    TogglePhoneWidget(Option<bool>),
    SendPrompt(String),
    UnknownCommand(String),
}

pub fn parse_cli_input(raw: &str) -> CliAction {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return CliAction::PrintHelp(render_help_text());
    }
    if !trimmed.starts_with('/') {
        return CliAction::SendPrompt(trimmed.to_string());
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").to_ascii_lowercase();
    let arg = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    match cmd.as_str() {
        "/help" | "/?" | "/h" => CliAction::PrintHelp(render_help_text()),
        "/new" | "/n" => CliAction::NewSession(arg),
        "/reset" => CliAction::ResetActiveSession,
        "/sessions" | "/ls" => CliAction::ListSessions,
        "/resume" | "/switch" => match arg {
            Some(target) => CliAction::ResumeSession(target),
            None => CliAction::ListSessions,
        },
        "/model" | "/m" => CliAction::SetModel(arg),
        "/status" => CliAction::ShowStatus,
        "/gateway" | "/gw" => CliAction::SetGateway(arg),
        "/profile" | "/p" => CliAction::SetProfile(arg),
        "/skills" => CliAction::ListSkills,
        "/toolsets" => CliAction::ListToolsets,
        "/cron" | "/jobs" => CliAction::CronCommand(arg),
        "/tasks" | "/bg" => CliAction::ListTasks,
        "/queue" | "/approvals" => CliAction::SwitchView(ActiveView::Queue),
        "/approve" => CliAction::ResolveApproval(arg.unwrap_or_else(|| "once".to_string())),
        "/stop" | "/cancel" => CliAction::StopActiveRun,
        "/clear" | "/cls" => CliAction::ClearTerminal,
        "/voice" | "/mic" => CliAction::VoiceControl(arg),
        "/tts" | "/mute" => CliAction::TtsControl(arg),
        "/home" => CliAction::SwitchView(ActiveView::Home),
        "/canvas" | "/dots" => CliAction::SwitchView(ActiveView::Canvas),
        "/thread" | "/chat" => CliAction::SwitchView(ActiveView::Thread),
        "/mode" | "/3d" => {
            let m = match arg.as_deref().map(str::to_ascii_lowercase).as_deref() {
                Some("3d") | Some("ascii3d") => Some(AvatarMode::Ascii3d),
                Some("halftone") | Some("2d") => Some(AvatarMode::Halftone),
                _ => None,
            };
            CliAction::SetAvatarMode(m)
        }
        "/ui" | "/theme" => CliAction::ApplyUiCustomization(arg.unwrap_or_default()),
        "/phone" | "/widget" => {
            let toggle = match arg.as_deref().map(str::to_ascii_lowercase).as_deref() {
                Some("open") | Some("on") | Some("show") => Some(true),
                Some("close") | Some("off") | Some("hide") => Some(false),
                _ => None,
            };
            CliAction::TogglePhoneWidget(toggle)
        }
        _ => CliAction::UnknownCommand(cmd),
    }
}

pub fn autocomplete_slash_commands(prefix: &str) -> Vec<&'static SlashCommandSpec> {
    let trimmed = prefix.trim().to_ascii_lowercase();
    if !trimmed.starts_with('/') {
        return Vec::new();
    }
    SLASH_COMMANDS
        .iter()
        .filter(|spec| {
            spec.name.starts_with(&trimmed)
                || spec.aliases.iter().any(|a| a.starts_with(&trimmed))
        })
        .collect()
}

pub fn render_help_text() -> String {
    let mut out = String::from("Hermes Duet CLI Commands:\n");
    for spec in SLASH_COMMANDS {
        out.push_str(&format!("  {:<22} {}\n", spec.usage, spec.description));
    }
    out
}

// ---------------------------------------------------------------------------
// TS-parity executor (cliEngine.ts)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
}

impl ApprovalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}

/// Mirrors TS `ApprovalCard` (shared with `sidebar_char`).
#[derive(Clone, Debug, PartialEq)]
pub struct ApprovalCard {
    pub id: String,
    pub channel_id: String,
    pub title: String,
    pub action_type: String,
    pub mcp_tool: Option<String>,
    pub run_id: Option<String>,
    pub request_id: Option<String>,
    pub execution_generation: Option<u32>,
    pub kind: Option<String>,
    pub choices: Vec<String>,
    pub command: Option<String>,
    pub destination: String,
    pub summary: String,
    pub draft: String,
    pub status: ApprovalStatus,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BriefDecision {
    pub id: String,
    pub priority: String,
    pub title: String,
    pub detail: String,
    pub resolved: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CliSkill {
    pub name: String,
    pub source: String,
    pub summary: Option<String>,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CliToolset {
    pub name: String,
    pub label: String,
    pub description: String,
    pub enabled: bool,
    pub configured: Option<bool>,
    pub tools: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CliCronJob {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub mode: String,
    pub status: Option<String>,
    pub summary: String,
    pub channel_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliProfileMode {
    Multiplexed,
    Standalone,
}

impl CliProfileMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Multiplexed => "multiplexed",
            Self::Standalone => "standalone",
        }
    }
}

/// Redacted profile summary (mirrors TS `HermesProfileSummary` CLI surface).
#[derive(Clone, Debug, PartialEq)]
pub struct CliProfileSummary {
    pub id: String,
    pub label: String,
    pub mode: CliProfileMode,
    pub base_url: String,
    pub route_prefix: String,
    pub connected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliTransportMode {
    Auto,
    Runs,
    ChatCompletions,
    SessionStream,
}

impl CliTransportMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Runs => "runs",
            Self::ChatCompletions => "chat_completions",
            Self::SessionStream => "session_stream",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct CliCapabilityFeatures {
    pub chat_completions: bool,
    pub reasoning_streaming: bool,
    pub run_submission: bool,
    pub run_events_sse: bool,
    pub run_approval: bool,
    pub session_chat_stream: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct CliCapabilities {
    pub session_key_header: Option<String>,
    pub features: CliCapabilityFeatures,
}

/// Executor context mirroring TS `CliContext`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CliContext {
    pub active_channel: String,
    pub available_channels: Vec<String>,
    pub approvals: Vec<ApprovalCard>,
    pub tasks: Vec<BackgroundTask>,
    pub brief: Vec<BriefDecision>,
    pub is_muted: bool,
    pub skills: Vec<CliSkill>,
    pub toolsets: Vec<CliToolset>,
    pub cron_jobs: Vec<CliCronJob>,
    pub gateway_connected: bool,
    pub gateway_url: Option<String>,
    pub active_model: Option<String>,
    pub gateway_profile: Option<String>,
    pub profiles: Vec<CliProfileSummary>,
    pub draft_profile: Option<String>,
    pub route_prefix: Option<String>,
    pub health_status: Option<String>,
    pub session_key_header: Option<String>,
    pub capabilities: Option<CliCapabilities>,
    pub transport_mode: Option<CliTransportMode>,
    pub resolved_transport: Option<String>,
    pub gateway_error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalChoice {
    Once,
    Always,
    Deny,
}

impl ApprovalChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Always => "always",
            Self::Deny => "deny",
        }
    }
}

/// Focused UI patch surface produced by `/ui` and `/customize`. The full
/// sanitized patch application lives in `voxcell-core::ui_config` (P1-03);
/// this local type keeps the CLI executor dependency-free.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CliUiPatch {
    pub open_customizer: bool,
    pub reset_all: bool,
    pub preset_id: Option<String>,
    pub accent_preset: Option<String>,
    pub accent_hex: Option<String>,
    pub portrait_tint_hex: Option<String>,
    pub background_preset_id: Option<String>,
    pub sidebar_visible: Option<bool>,
    pub sidebar_position: Option<String>,
    pub corner_radius: Option<String>,
    pub density: Option<String>,
    pub font_preset: Option<String>,
    pub reduced_motion: Option<bool>,
}

impl CliUiPatch {
    fn touched(&self) -> bool {
        self.open_customizer
            || self.reset_all
            || self.preset_id.is_some()
            || self.accent_preset.is_some()
            || self.accent_hex.is_some()
            || self.portrait_tint_hex.is_some()
            || self.background_preset_id.is_some()
            || self.sidebar_visible.is_some()
            || self.sidebar_position.is_some()
            || self.corner_radius.is_some()
            || self.density.is_some()
            || self.font_preset.is_some()
            || self.reduced_motion.is_some()
    }

    /// Key names in TS patch insertion order (for `UI UPDATED // …` output).
    fn key_names(&self) -> Vec<&'static str> {
        let mut keys = Vec::new();
        if self.open_customizer {
            keys.push("openCustomizer");
        }
        if self.reset_all {
            keys.push("resetAll");
        }
        if self.preset_id.is_some() {
            keys.push("presetId");
        }
        if self.accent_preset.is_some() {
            keys.push("accentPreset");
        }
        if self.accent_hex.is_some() {
            keys.push("accentHex");
        }
        if self.portrait_tint_hex.is_some() {
            keys.push("portraitTintHex");
        }
        if self.background_preset_id.is_some() {
            keys.push("backgroundPresetId");
        }
        if self.sidebar_visible.is_some() {
            keys.push("sidebarVisible");
        }
        if self.sidebar_position.is_some() {
            keys.push("sidebarPosition");
        }
        if self.corner_radius.is_some() {
            keys.push("cornerRadius");
        }
        if self.density.is_some() {
            keys.push("density");
        }
        if self.font_preset.is_some() {
            keys.push("fontPreset");
        }
        if self.reduced_motion.is_some() {
            keys.push("reducedMotion");
        }
        keys
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CliHandledResult {
    pub output: String,
    pub mood: ExpressionMood,
    pub spoken_text: String,
    pub switch_channel: Option<String>,
    pub switch_profile: Option<String>,
    pub start_new_session: bool,
    pub new_session_prompt: Option<String>,
    pub approve_card_id: Option<String>,
    pub approve_choice: Option<ApprovalChoice>,
    pub reject_card_id: Option<String>,
    pub cancel_active_run: bool,
    pub set_transport_mode: Option<CliTransportMode>,
    pub toggle_mute: bool,
    pub clear_terminal: bool,
    pub set_mood: Option<ExpressionMood>,
    pub set_shader: Option<AsciiShaderMode>,
    pub set_dither_style: Option<&'static str>,
    pub ui_patch: Option<CliUiPatch>,
}

impl CliHandledResult {
    fn new(output: impl Into<String>, mood: ExpressionMood, spoken_text: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            mood,
            spoken_text: spoken_text.into(),
            switch_channel: None,
            switch_profile: None,
            start_new_session: false,
            new_session_prompt: None,
            approve_card_id: None,
            approve_choice: None,
            reject_card_id: None,
            cancel_active_run: false,
            set_transport_mode: None,
            toggle_mute: false,
            clear_terminal: false,
            set_mood: None,
            set_shader: None,
            set_dither_style: None,
            ui_patch: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CliExecutionResult {
    Handled(CliHandledResult),
    Chat(String),
}

pub const AVAILABLE_COMMANDS: &[&str] = &[
    "/help",
    "/new",
    "/profiles",
    "/profile",
    "/brief",
    "/tasks",
    "/skills",
    "/toolsets",
    "/memory",
    "/cron",
    "/sessions",
    "/gateway",
    "/doctor",
    "/transport",
    "/channels",
    "/switch",
    "/resume",
    "/approve",
    "/reject",
    "/stop",
    "/ui",
    "/customize",
    "/mood",
    "/style",
    "/mute",
    "/status",
    "/clear",
];

const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:8642";

fn pad_end(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

fn format_progress_number(p: f32) -> String {
    if p.fract() == 0.0 {
        format!("{}", p as i64)
    } else {
        format!("{p}")
    }
}

fn gateway_url(ctx: &CliContext) -> String {
    ctx.gateway_url.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_GATEWAY_URL.to_string())
}

fn offline_detail(ctx: &CliContext) -> String {
    ctx.gateway_error
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| ctx.gateway_url.clone().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| DEFAULT_GATEWAY_URL.to_string())
}

fn non_empty(value: Option<&String>) -> Option<String> {
    value.filter(|s| !s.is_empty()).cloned()
}

/// Pure parser + local executor for Hermes Agent slash commands (TS parity).
/// Natural-language prompts (not starting with `/`) route to the active session.
pub fn execute_cli_input(raw_input: &str, ctx: &CliContext) -> CliExecutionResult {
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return CliExecutionResult::Handled(CliHandledResult::new(
            "Empty input. Type /help for Hermes Agent commands or speak naturally.",
            ExpressionMood::Calm,
            "",
        ));
    }
    if !trimmed.starts_with('/') {
        return CliExecutionResult::Chat(trimmed.to_string());
    }

    let mut words = trimmed.split_whitespace();
    let cmd = words.next().unwrap_or("").to_ascii_lowercase();
    let args: Vec<&str> = words.collect();
    let arg_str = args.join(" ").trim().to_string();

    match cmd.as_str() {
        "/help" => CliExecutionResult::Handled(CliHandledResult::new(
            [
                "HERMES AGENT // COMMAND REFERENCE",
                "  /new [prompt]          Start a fresh isolated Hermes Agent session",
                "  /profiles              List Hermes profiles for parallel sessions",
                "  /profile <slug>        Set default Hermes profile for new Home/Dot launches",
                "  /tasks                 List run & turn tasks per session",
                "  /skills                List skills from GET /v1/skills",
                "  /toolsets              List toolsets & tools from GET /v1/toolsets",
                "  /memory                Inspect X-Hermes-Session-Key memory scoping header",
                "  /cron                  List scheduled jobs from GET /api/jobs",
                "  /sessions              Show synced sessions from GET /api/sessions",
                "  /gateway | /doctor     Inspect /v1/capabilities, /api/model/options & /health",
                "  /transport [mode]      Switch transport (auto | runs | session-stream | completions)",
                "  /switch <session>      Switch active session lane",
                "  /resume <session>      Resume session lane by slug or session id",
                "  /approve [id] [choice] Approve pending run card (once | always | deny)",
                "  /reject [id]           Deny a pending run approval card",
                "  /stop                  Stop active Hermes Agent run (/v1/runs/{id}/stop)",
                "  /ui <spec|preset>      Customize client theme, layout, character & backdrop",
                "  /customize             Open the client UI Studio drawer",
                "  /mood <state>          Shift portrait (neutral|happy|focused|alert|thinking)",
                "  /mute                  Toggle Hermes voice audio output",
                "  /status                Report gateway link, transport, and active tasks",
                "  /clear                 Clear terminal feed",
            ]
            .join("\n"),
            ExpressionMood::Happy,
            "Hermes Agent command reference ready. Inspect capabilities, skills, toolsets, scheduled jobs, or active sessions.",
        )),

        "/new" => {
            let mut res = CliHandledResult::new(
                if arg_str.is_empty() {
                    "New Hermes Agent session armed on Home. Prompt anything to start.".to_string()
                } else {
                    format!("Starting fresh Hermes Agent session for: \"{arg_str}\"")
                },
                ExpressionMood::Happy,
                if arg_str.is_empty() { "New Hermes Agent session armed on Home." } else { "" },
            );
            res.start_new_session = true;
            res.new_session_prompt = if arg_str.is_empty() { None } else { Some(arg_str) };
            CliExecutionResult::Handled(res)
        }

        "/profiles" | "/profile" => execute_profile_command(&arg_str, ctx),

        "/brief" => {
            let pending: Vec<&ApprovalCard> = ctx.approvals.iter().filter(|a| a.status == ApprovalStatus::Pending).collect();
            let open_brief: Vec<&BriefDecision> = ctx.brief.iter().filter(|b| !b.resolved).collect();
            if pending.is_empty() && open_brief.is_empty() {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    "HERMES AGENT // ACTION QUEUE\n  All action gates clear. Zero pending run approvals.",
                    ExpressionMood::Calm,
                    "All action gates clear. Zero pending approvals.",
                ));
            }
            let mut lines: Vec<String> = Vec::new();
            for (i, a) in pending.iter().enumerate() {
                lines.push(format!(
                    "  {}. [APPROVAL] {} ({}) · run {} -> /approve {} once",
                    i + 1,
                    a.title,
                    a.mcp_tool.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "tool".to_string()),
                    a.run_id.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| a.id.clone()),
                    a.id
                ));
            }
            for (i, b) in open_brief.iter().enumerate() {
                lines.push(format!(
                    "  {}. [{}] {}",
                    pending.len() + i + 1,
                    b.priority.to_ascii_uppercase(),
                    b.title
                ));
            }
            let total = pending.len() + open_brief.len();
            let mut out = String::from("HERMES AGENT // ACTION QUEUE");
            for line in lines {
                out.push('\n');
                out.push_str(&line);
            }
            CliExecutionResult::Handled(CliHandledResult::new(
                out,
                ExpressionMood::Focused,
                format!("{total} pending items in queue."),
            ))
        }

        "/tasks" => {
            if ctx.tasks.is_empty() {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    "HERMES AGENT // SESSION TASKS\n  No tasks recorded yet. Send a prompt to start a run.",
                    ExpressionMood::Calm,
                    "No turn tasks recorded yet.",
                ));
            }
            let mut out = String::from("HERMES AGENT // SESSION & RUN TASKS");
            for t in &ctx.tasks {
                let status = match t.status {
                    TaskLifecycleStatus::Running => "RUNNING",
                    TaskLifecycleStatus::Paused => "PAUSED",
                    TaskLifecycleStatus::Completed => "COMPLETED",
                };
                let session_id = if t.session_id.is_empty() {
                    format!("hermes-{}", t.channel_id)
                } else {
                    t.session_id.clone()
                };
                out.push_str(&format!(
                    "\n  [{}] {} {}% · #{} ({}) · {}",
                    t.id,
                    pad_end(status, 9),
                    format!("{:>3}", format_progress_number(t.progress)),
                    t.channel_id,
                    session_id,
                    t.title
                ));
            }
            CliExecutionResult::Handled(CliHandledResult::new(
                out,
                ExpressionMood::Focused,
                format!("Tracking {} Hermes Agent runs across active sessions.", ctx.tasks.len()),
            ))
        }

        "/skills" => {
            if !ctx.gateway_connected && ctx.skills.is_empty() {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    format!("HERMES AGENT // GET /v1/skills\n  Gateway offline ({}).", offline_detail(ctx)),
                    ExpressionMood::Alert,
                    "Hermes Gateway is offline. Cannot query /v1/skills.",
                ));
            }
            if ctx.skills.is_empty() {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    "HERMES AGENT // GET /v1/skills\n  0 skills reported by gateway.",
                    ExpressionMood::Calm,
                    "Gateway reported zero loaded skills.",
                ));
            }
            let mut out = String::from("HERMES AGENT // GET /v1/skills");
            for s in &ctx.skills {
                out.push_str(&format!(
                    "\n  {} {} {}",
                    if s.enabled { "[ON]" } else { "[--]" },
                    pad_end(&s.name, 20),
                    s.source
                ));
                if let Some(summary) = s.summary.as_deref().filter(|v| !v.is_empty()) {
                    out.push_str(&format!("\n       {summary}"));
                }
            }
            let enabled = ctx.skills.iter().filter(|s| s.enabled).count();
            CliExecutionResult::Handled(CliHandledResult::new(
                out,
                ExpressionMood::Focused,
                format!("{enabled} skills loaded from GET /v1/skills."),
            ))
        }

        "/toolsets" => {
            if !ctx.gateway_connected && ctx.toolsets.is_empty() {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    format!("HERMES AGENT // GET /v1/toolsets\n  Gateway offline ({}).", offline_detail(ctx)),
                    ExpressionMood::Alert,
                    "Hermes Gateway is offline. Cannot query /v1/toolsets.",
                ));
            }
            if ctx.toolsets.is_empty() {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    "HERMES AGENT // GET /v1/toolsets\n  0 toolsets reported by gateway.",
                    ExpressionMood::Calm,
                    "Gateway reported zero toolsets.",
                ));
            }
            let mut out = String::from("HERMES AGENT // GET /v1/toolsets");
            for ts in &ctx.toolsets {
                let badge = if ts.enabled {
                    if ts.configured == Some(false) { "[UNCFG]" } else { "[ON]   " }
                } else {
                    "[OFF]  "
                };
                let preview = if ts.tools.is_empty() {
                    "none".to_string()
                } else {
                    ts.tools.iter().take(6).cloned().collect::<Vec<_>>().join(", ")
                };
                out.push_str(&format!(
                    "\n  {} {} ({} tools: {})",
                    badge,
                    pad_end(&ts.label, 20),
                    ts.tools.len(),
                    preview
                ));
            }
            let enabled = ctx.toolsets.iter().filter(|t| t.enabled).count();
            CliExecutionResult::Handled(CliHandledResult::new(
                out,
                ExpressionMood::Focused,
                format!("{enabled} active toolsets on gateway."),
            ))
        }

        "/memory" => {
            let key_header = ctx
                .capabilities
                .as_ref()
                .and_then(|c| non_empty(c.session_key_header.as_ref()))
                .or_else(|| non_empty(ctx.session_key_header.as_ref()))
                .unwrap_or_else(|| "X-Hermes-Session-Key".to_string());
            let has_caps = ctx.capabilities.is_some();
            let not_advertised = has_caps
                && ctx
                    .capabilities
                    .as_ref()
                    .and_then(|c| c.session_key_header.as_deref())
                    .map(str::is_empty)
                    .unwrap_or(true);
            let out = [
                "HERMES AGENT // MEMORY SCOPING".to_string(),
                format!("  Continuity Header : X-Hermes-Session-Id (active channel: #{})", ctx.active_channel),
                format!(
                    "  Scoping Header    : {key_header}{}",
                    if not_advertised { " (not advertised in /v1/capabilities)" } else { "" }
                ),
                "  REST Endpoint     : Hermes API Server has no /v1/memory endpoint; persistent memory is managed inside each session.".to_string(),
            ]
            .join("\n");
            CliExecutionResult::Handled(CliHandledResult::new(
                out,
                ExpressionMood::Focused,
                format!("Memory isolation scopes per session via {key_header}."),
            ))
        }

        "/cron" => {
            if !ctx.gateway_connected && ctx.cron_jobs.is_empty() {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    format!("HERMES AGENT // GET /api/jobs\n  Gateway offline ({}).", offline_detail(ctx)),
                    ExpressionMood::Alert,
                    "Hermes Gateway is offline. Cannot query /api/jobs.",
                ));
            }
            if ctx.cron_jobs.is_empty() {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    "HERMES AGENT // GET /api/jobs\n  No scheduled jobs on gateway.",
                    ExpressionMood::Calm,
                    "Zero scheduled jobs configured on gateway.",
                ));
            }
            let mut out = String::from("HERMES AGENT // GET /api/jobs");
            for c in &ctx.cron_jobs {
                let state = c.status.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| c.mode.clone());
                out.push_str(&format!(
                    "\n  [{}] {} ({}) · {}",
                    pad_end(&c.schedule, 15),
                    pad_end(&state, 8),
                    c.id,
                    c.summary
                ));
            }
            CliExecutionResult::Handled(CliHandledResult::new(
                out,
                ExpressionMood::Focused,
                format!("{} scheduled jobs returned by /api/jobs.", ctx.cron_jobs.len()),
            ))
        }

        "/channels" | "/sessions" | "/lanes" => {
            if ctx.available_channels.is_empty() {
                let (mood, spoken, body) = if ctx.gateway_connected {
                    (
                        ExpressionMood::Calm,
                        "No active gateway sessions found.",
                        "  No sessions yet. Use /new <prompt> to create one.".to_string(),
                    )
                } else {
                    (
                        ExpressionMood::Alert,
                        "Gateway offline; no sessions synced.",
                        format!("  Gateway offline ({}).", offline_detail(ctx)),
                    )
                };
                return CliExecutionResult::Handled(CliHandledResult::new(
                    format!("HERMES AGENT // GET /api/sessions\n{body}"),
                    mood,
                    spoken,
                ));
            }
            let mut out = format!(
                "ACTIVE SESSION LANE: #{} ({})",
                ctx.active_channel,
                non_empty(ctx.session_key_header.as_ref()).unwrap_or_else(|| "X-Hermes-Session-Key".to_string())
            );
            for id in &ctx.available_channels {
                out.push_str(&format!(
                    "\n  #{} {}",
                    pad_end(id, 18),
                    if *id == ctx.active_channel { "[ACTIVE]" } else { "" }
                ));
            }
            CliExecutionResult::Handled(CliHandledResult::new(
                out,
                ExpressionMood::Calm,
                format!(
                    "{} synced Hermes sessions. Active lane is #{}.",
                    ctx.available_channels.len(),
                    ctx.active_channel
                ),
            ))
        }

        "/gateway" | "/doctor" => {
            let url = gateway_url(ctx);
            let connected = ctx.gateway_connected;
            let model = non_empty(ctx.active_model.as_ref()).unwrap_or_else(|| "hermes-agent".to_string());
            let cap_flags = match &ctx.capabilities {
                Some(caps) => {
                    let f = &caps.features;
                    let flags: Vec<&str> = [
                        f.run_submission.then_some("runs"),
                        f.run_approval.then_some("approvals"),
                        f.session_chat_stream.then_some("session-stream"),
                        f.reasoning_streaming.then_some("reasoning"),
                    ]
                    .into_iter()
                    .flatten()
                    .collect();
                    if flags.is_empty() { "basic completions".to_string() } else { flags.join(", ") }
                }
                None => "unavailable".to_string(),
            };
            let mut lines = vec![
                "HERMES AGENT DOCTOR // GATEWAY CHECK".to_string(),
                format!(
                    "  Gateway URL       : {}{} ({})",
                    url,
                    ctx.route_prefix.clone().unwrap_or_default(),
                    if connected { "CONNECTED" } else { "OFFLINE" }
                ),
                format!(
                    "  Profile           : {} · Health (/health): {}",
                    non_empty(ctx.gateway_profile.as_ref()).unwrap_or_else(|| "default".to_string()),
                    non_empty(ctx.health_status.as_ref())
                        .unwrap_or_else(|| if connected { "ok".to_string() } else { "unreachable".to_string() })
                ),
                format!("  Capabilities      : {cap_flags} (GET /v1/capabilities)"),
                format!("  Active Model      : {model} (GET /api/model/options)"),
                format!(
                    "  Transport Mode    : {} -> resolved: {}",
                    ctx.transport_mode.map(|m| m.as_str()).unwrap_or("auto"),
                    non_empty(ctx.resolved_transport.as_ref()).unwrap_or_else(|| "chat_completions".to_string())
                ),
                format!(
                    "  Session Headers   : X-Hermes-Session-Id + {}",
                    non_empty(ctx.session_key_header.as_ref()).unwrap_or_else(|| "X-Hermes-Session-Key".to_string())
                ),
            ];
            if let Some(err) = ctx.gateway_error.as_deref().filter(|_| !connected) {
                lines.push(format!("  Error             : {err}"));
            }
            CliExecutionResult::Handled(CliHandledResult::new(
                lines.join("\n"),
                if connected { ExpressionMood::Happy } else { ExpressionMood::Alert },
                if connected {
                    format!(
                        "Hermes Gateway connected using {} transport.",
                        non_empty(ctx.resolved_transport.as_ref()).unwrap_or_else(|| "chat_completions".to_string())
                    )
                } else {
                    "Hermes Gateway is offline. Start hermes gateway with API_SERVER_ENABLED=true.".to_string()
                },
            ))
        }

        "/transport" => {
            let arg = arg_str.to_ascii_lowercase();
            if arg.is_empty() {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    format!(
                        "TRANSPORT MODE // current={}, resolved={}\n  Usage: /transport <auto|runs|session-stream|completions>",
                        ctx.transport_mode.map(|m| m.as_str()).unwrap_or("auto"),
                        non_empty(ctx.resolved_transport.as_ref()).unwrap_or_else(|| "chat_completions".to_string())
                    ),
                    ExpressionMood::Focused,
                    format!("Active transport mode is {}.", ctx.transport_mode.map(|m| m.as_str()).unwrap_or("auto")),
                ));
            }
            let mapped = match arg.as_str() {
                "auto" => Some(CliTransportMode::Auto),
                "runs" | "run" => Some(CliTransportMode::Runs),
                "completions" | "chat_completions" => Some(CliTransportMode::ChatCompletions),
                "session-stream" | "session_stream" => Some(CliTransportMode::SessionStream),
                _ => None,
            };
            let Some(mode) = mapped else {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    "Usage: /transport <auto|runs|session-stream|completions>",
                    ExpressionMood::Alert,
                    "Invalid transport mode.",
                ));
            };
            let mut res = CliHandledResult::new(
                format!("TRANSPORT MODE // Preference updated to \"{}\".", mode.as_str()),
                ExpressionMood::Happy,
                format!("Gateway transport preference set to {}.", mode.as_str()),
            );
            res.set_transport_mode = Some(mode);
            CliExecutionResult::Handled(res)
        }

        "/switch" | "/resume" => {
            let stripped = arg_str.strip_prefix('#').unwrap_or(&arg_str);
            let stripped = stripped
                .strip_prefix("hermes-")
                .or_else(|| stripped.strip_prefix("duet-"))
                .unwrap_or(stripped);
            let normalized = stripped.to_ascii_lowercase();
            if ctx.available_channels.is_empty() {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    "No sessions synced from gateway yet. Use /new <prompt> to start one.",
                    ExpressionMood::Alert,
                    "No synced sessions available yet.",
                ));
            }
            if normalized.is_empty() || !ctx.available_channels.iter().any(|c| *c == normalized) {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    format!(
                        "Unknown session \"{arg_str}\". Synced sessions: {}.",
                        ctx.available_channels.join(", ")
                    ),
                    ExpressionMood::Alert,
                    "Unknown Hermes session.",
                ));
            }
            let mut res = CliHandledResult::new(
                format!("Resumed session #{normalized}."),
                ExpressionMood::Happy,
                format!("Switched to session #{normalized}."),
            );
            res.switch_channel = Some(normalized);
            CliExecutionResult::Handled(res)
        }

        "/approve" => {
            let first_pending = ctx.approvals.iter().find(|c| c.status == ApprovalStatus::Pending);
            let raw_choice = args.last().map(|s| s.to_ascii_lowercase());
            let mut choice = ApprovalChoice::Once;
            let mut target_arg = arg_str.clone();
            if let Some(c) = raw_choice.as_deref() {
                match c {
                    "once" => choice = ApprovalChoice::Once,
                    "always" => choice = ApprovalChoice::Always,
                    "deny" => choice = ApprovalChoice::Deny,
                    _ => {}
                }
                if matches!(c, "once" | "always" | "deny") {
                    target_arg = args[..args.len() - 1].join(" ").trim().to_string();
                }
            }
            let target_id = if !target_arg.is_empty() {
                target_arg.clone()
            } else {
                first_pending.map(|c| c.id.clone()).unwrap_or_default()
            };
            let card = ctx
                .approvals
                .iter()
                .find(|c| c.id == target_id || c.request_id.as_deref() == Some(target_id.as_str()));
            let Some(card) = card else {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    format!(
                        "No pending approval found matching \"{}\".",
                        if target_arg.is_empty() { "pending" } else { &target_arg }
                    ),
                    ExpressionMood::Alert,
                    "No pending run approval found.",
                ));
            };
            if card.status != ApprovalStatus::Pending {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    format!("Approval {} is already {}.", card.id, card.status.as_str()),
                    ExpressionMood::Calm,
                    format!("Approval {} is already {}.", card.id, card.status.as_str()),
                ));
            }
            let mut res = CliHandledResult::new(
                format!("RUN APPROVAL ({}) [{}]: {}", choice.as_str().to_ascii_uppercase(), card.id, card.title),
                if choice == ApprovalChoice::Deny { ExpressionMood::Calm } else { ExpressionMood::Happy },
                if choice == ApprovalChoice::Deny {
                    format!("Denied {}.", card.title)
                } else {
                    format!("Approved {} ({}).", card.title, choice.as_str())
                },
            );
            res.approve_card_id = Some(card.id.clone());
            res.approve_choice = Some(choice);
            CliExecutionResult::Handled(res)
        }

        "/reject" => {
            let target_id = if !arg_str.is_empty() {
                arg_str.clone()
            } else {
                ctx.approvals
                    .iter()
                    .find(|c| c.status == ApprovalStatus::Pending)
                    .map(|c| c.id.clone())
                    .unwrap_or_default()
            };
            let card = ctx
                .approvals
                .iter()
                .find(|c| c.id == target_id || c.request_id.as_deref() == Some(target_id.as_str()));
            let Some(card) = card else {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    format!(
                        "No pending approval found matching \"{}\".",
                        if arg_str.is_empty() { "pending" } else { &arg_str }
                    ),
                    ExpressionMood::Alert,
                    "No matching approval found.",
                ));
            };
            let mut res = CliHandledResult::new(
                format!("DENIED [{}]: {}", card.id, card.title),
                ExpressionMood::Calm,
                format!("Denied {}.", card.title),
            );
            res.reject_card_id = Some(card.id.clone());
            res.approve_choice = Some(ApprovalChoice::Deny);
            CliExecutionResult::Handled(res)
        }

        "/stop" => {
            let mut res = CliHandledResult::new(
                format!("STOP // Requesting cancellation for active run in #{}.", ctx.active_channel),
                ExpressionMood::Calm,
                "Stopping active run.",
            );
            res.cancel_active_run = true;
            CliExecutionResult::Handled(res)
        }

        "/mood" => {
            let next = parse_mood(&arg_str.to_ascii_lowercase());
            let Some(mood) = next else {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    "Valid moods: neutral, listening, thinking, speaking, happy, focused, alert.",
                    ExpressionMood::Alert,
                    "Unsupported mood state.",
                ));
            };
            let mut res = CliHandledResult::new(
                format!("Braille portrait expression set to \"{}\".", mood_id(mood)),
                mood,
                format!("Expression matrix shifted to {}.", mood_id(mood)),
            );
            res.set_mood = Some(mood);
            CliExecutionResult::Handled(res)
        }

        "/style" => {
            let mode = arg_str.to_ascii_lowercase();
            if !mode.is_empty() && mode != "braille" {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    "Valid halftone portrait style: braille.",
                    ExpressionMood::Alert,
                    "Only braille halftone portrait mode is active.",
                ));
            }
            let mut res = CliHandledResult::new(
                "Monochrome halftone portrait mode: \"BRAILLE\".",
                ExpressionMood::Focused,
                "Braille halftone portrait mode is active.",
            );
            res.set_dither_style = Some("braille");
            CliExecutionResult::Handled(res)
        }

        "/shader" => {
            let mode = arg_str.to_ascii_lowercase();
            let shader = match mode.as_str() {
                "solid" => Some(AsciiShaderMode::Solid),
                "wire" => Some(AsciiShaderMode::Wire),
                "depth" => Some(AsciiShaderMode::Depth),
                "holo" => Some(AsciiShaderMode::Holo),
                "norm" => Some(AsciiShaderMode::Norm),
                _ => None,
            };
            let Some(shader) = shader else {
                return CliExecutionResult::Handled(CliHandledResult::new(
                    "Valid 3D ASCII shaders: solid, wire, depth, holo, norm.",
                    ExpressionMood::Alert,
                    "Invalid shader. Choose solid, wire, depth, holo, or norm.",
                ));
            };
            let mut res = CliHandledResult::new(
                format!("3D SDF ASCII shader mode set to \"{}\".", mode.to_ascii_uppercase()),
                ExpressionMood::Focused,
                format!("3D ASCII shader switched to {mode} mode."),
            );
            res.set_shader = Some(shader);
            CliExecutionResult::Handled(res)
        }

        "/mute" => {
            let mut res = CliHandledResult::new(
                if ctx.is_muted { "Audio output UNMUTED." } else { "Audio output MUTED." },
                ExpressionMood::Calm,
                if ctx.is_muted { "Audio output unmuted." } else { "" },
            );
            res.toggle_mute = true;
            CliExecutionResult::Handled(res)
        }

        "/status" => {
            let pending_count = ctx.approvals.iter().filter(|a| a.status == ApprovalStatus::Pending).count();
            let running_count = ctx.tasks.iter().filter(|t| t.status == TaskLifecycleStatus::Running).count();
            let gateway_line = if ctx.gateway_connected {
                format!(
                    "CONNECTED ({}{})",
                    gateway_url(ctx),
                    ctx.route_prefix.clone().unwrap_or_default()
                )
            } else {
                format!("OFFLINE ({})", offline_detail(ctx))
            };
            let out = [
                format!("SESSION LANE: #{}", ctx.active_channel),
                format!("GATEWAY: {gateway_line}"),
                format!(
                    "TRANSPORT: {} -> {}",
                    ctx.transport_mode.map(|m| m.as_str()).unwrap_or("auto"),
                    non_empty(ctx.resolved_transport.as_ref()).unwrap_or_else(|| "chat_completions".to_string())
                ),
                format!("PENDING APPROVALS: {pending_count}"),
                format!("RUNNING TASKS: {running_count}"),
                format!("VOICE AUDIO: {}", if ctx.is_muted { "MUTED" } else { "ACTIVE" }),
            ]
            .join("\n");
            CliExecutionResult::Handled(CliHandledResult::new(
                out,
                if ctx.gateway_connected { ExpressionMood::Focused } else { ExpressionMood::Alert },
                if ctx.gateway_connected {
                    format!("Connected to Hermes Gateway. {pending_count} pending approvals, {running_count} active tasks.")
                } else {
                    "Hermes Gateway is offline.".to_string()
                },
            ))
        }

        "/customize" => {
            let patch = if arg_str.is_empty() {
                CliUiPatch { open_customizer: true, ..Default::default() }
            } else {
                parse_local_ui_intent(&format!("make ui {arg_str}")).unwrap_or(CliUiPatch {
                    open_customizer: true,
                    ..Default::default()
                })
            };
            let mut res = CliHandledResult::new(
                "UI CUSTOMIZATION STUDIO // Opening client theme, layout, character & backdrop controls.",
                ExpressionMood::Happy,
                if arg_str.is_empty() {
                    "UI Customization Studio open. Adjust any theme, layout, or stage element live."
                } else {
                    "Updated UI customization and opened the live studio."
                },
            );
            res.ui_patch = Some(patch);
            CliExecutionResult::Handled(res)
        }

        "/ui" => execute_ui_command(&arg_str),

        "/clear" => {
            let mut res = CliHandledResult::new("Terminal buffer cleared.", ExpressionMood::Calm, "");
            res.clear_terminal = true;
            CliExecutionResult::Handled(res)
        }

        _ => CliExecutionResult::Handled(CliHandledResult::new(
            format!("Unknown command \"{cmd}\". Type /help to list available Hermes Agent commands."),
            ExpressionMood::Alert,
            format!("Unrecognized command {cmd}. Type slash help for Hermes Agent commands."),
        )),
    }
}

fn execute_profile_command(arg_str: &str, ctx: &CliContext) -> CliExecutionResult {
    let current_draft = non_empty(ctx.draft_profile.as_ref())
        .or_else(|| non_empty(ctx.gateway_profile.as_ref()))
        .unwrap_or_else(|| "default".to_string());

    if arg_str.is_empty() {
        if ctx.profiles.is_empty() {
            return CliExecutionResult::Handled(CliHandledResult::new(
                format!(
                    "HERMES PROFILES // active default=\"{current_draft}\"\n  Add parallel profiles in Queue -> Gateway & Tools or set HERMES_PROFILES."
                ),
                ExpressionMood::Calm,
                format!("Active profile is {current_draft}. Configure extra profiles in Queue -> Gateway & Tools."),
            ));
        }
        let mut lines = vec!["HERMES AGENT // PARALLEL PROFILES".to_string()];
        for p in &ctx.profiles {
            let route = if p.mode == CliProfileMode::Multiplexed {
                format!("{}{}", p.base_url, p.route_prefix)
            } else {
                p.base_url.clone()
            };
            let marker = if p.id == current_draft { "[DRAFT]" } else { "       " };
            let state = if p.connected { "READY" } else { "OFFLINE" };
            lines.push(format!(
                "  {marker} @{} {} {} · {route}",
                pad_end(&p.id, 14),
                pad_end(p.mode.as_str(), 11),
                pad_end(state, 7)
            ));
        }
        lines.push("  Usage: /profile <slug>".to_string());
        return CliExecutionResult::Handled(CliHandledResult::new(
            lines.join("\n"),
            ExpressionMood::Focused,
            format!(
                "{} Hermes profiles registered. Default launch profile is {current_draft}.",
                ctx.profiles.len()
            ),
        ));
    }

    // Normalize: strip @, trim, lowercase, collapse non-[a-z0-9_-] runs to '-'.
    let lowered = arg_str.strip_prefix('@').unwrap_or(arg_str).trim().to_ascii_lowercase();
    let mut normalized = String::with_capacity(lowered.len());
    let mut prev_dash = false;
    for c in lowered.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            normalized.push(c);
            prev_dash = c == '-';
        } else if !prev_dash {
            normalized.push('-');
            prev_dash = true;
        }
    }

    let found = ctx.profiles.iter().find(|p| p.id == normalized);
    if !ctx.profiles.is_empty() && found.is_none() {
        return CliExecutionResult::Handled(CliHandledResult::new(
            format!(
                "Unknown profile \"{normalized}\". Available: {}.\nAdd custom profiles in Queue -> Gateway & Tools.",
                ctx.profiles.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(", ")
            ),
            ExpressionMood::Alert,
            format!("Profile {normalized} is not registered."),
        ));
    }

    let target = if normalized.is_empty() { "default".to_string() } else { normalized };
    let mut res = CliHandledResult::new(
        format!("HERMES PROFILE // New Home & Dot sessions will launch on @{target}."),
        ExpressionMood::Happy,
        format!("Default profile for new parallel sessions set to {target}."),
    );
    res.switch_profile = Some(target);
    CliExecutionResult::Handled(res)
}

fn parse_mood(id: &str) -> Option<ExpressionMood> {
    Some(match id {
        "neutral" => ExpressionMood::Calm,
        "listening" => ExpressionMood::Listening,
        "thinking" => ExpressionMood::Thinking,
        "speaking" => ExpressionMood::Speaking,
        "happy" => ExpressionMood::Happy,
        "focused" => ExpressionMood::Focused,
        "alert" => ExpressionMood::Alert,
        _ => return None,
    })
}

fn mood_id(mood: ExpressionMood) -> &'static str {
    match mood {
        ExpressionMood::Calm => "neutral",
        ExpressionMood::Listening => "listening",
        ExpressionMood::Thinking => "thinking",
        ExpressionMood::Speaking => "speaking",
        ExpressionMood::Happy => "happy",
        ExpressionMood::Focused => "focused",
        ExpressionMood::Alert => "alert",
    }
}

struct CuratedPreset {
    id: &'static str,
    name: &'static str,
    subtitle: &'static str,
    mood: ExpressionMood,
}

const CURATED_PRESETS: &[CuratedPreset] = &[
    CuratedPreset { id: "slate-terminal", name: "Slate Monochrome", subtitle: "Modern graphite glass & neutral telemetry", mood: ExpressionMood::Focused },
    CuratedPreset { id: "emerald-terminal", name: "Emerald Terminal", subtitle: "Signature smoked-emerald Hermes cockpit", mood: ExpressionMood::Happy },
    CuratedPreset { id: "cyber-violet", name: "Cyber Violet", subtitle: "Neon plasma nebula & ultraviolet glass", mood: ExpressionMood::Focused },
    CuratedPreset { id: "solaris-amber", name: "Solaris Amber", subtitle: "Retro phosphor CRT monospace telemetry", mood: ExpressionMood::Focused },
    CuratedPreset { id: "glacier-cyan", name: "Glacier Ice", subtitle: "Frosted Arctic blue glass & orbital nodes", mood: ExpressionMood::Calm },
    CuratedPreset { id: "sakura-rose", name: "Sakura Nocturne", subtitle: "Soft petal pills & bioluminescent tide", mood: ExpressionMood::Happy },
    CuratedPreset { id: "zen-mono", name: "Stealth Monograph", subtitle: "Zero-clutter minimalist focus shell", mood: ExpressionMood::Calm },
];

fn get_curated_preset(id: &str) -> Option<&'static CuratedPreset> {
    CURATED_PRESETS.iter().find(|p| p.id == id)
}

fn execute_ui_command(arg_str: &str) -> CliExecutionResult {
    let lower_arg = arg_str.to_ascii_lowercase();
    if lower_arg.is_empty() || lower_arg == "studio" || lower_arg == "open" {
        let mut res = CliHandledResult::new(
            "UI STUDIO // Opened customization drawer. Try /ui cyber-violet, /ui reset, or natural language.",
            ExpressionMood::Happy,
            "UI Studio opened. Every theme, layout, and stage token is live.",
        );
        res.ui_patch = Some(CliUiPatch { open_customizer: true, ..Default::default() });
        return CliExecutionResult::Handled(res);
    }
    if lower_arg == "reset" || lower_arg == "default" || lower_arg == "restore" {
        let mut res = CliHandledResult::new(
            "UI RESET // Restored default Slate Monochrome theme, layout, character & backdrop.",
            ExpressionMood::Happy,
            "Restored default Slate Monochrome layout and backdrop.",
        );
        res.ui_patch = Some(CliUiPatch { reset_all: true, ..Default::default() });
        return CliExecutionResult::Handled(res);
    }
    if let Some(preset) = get_curated_preset(&lower_arg) {
        let mut res = CliHandledResult::new(
            format!("UI PRESET // Applied \"{}\" ({}).", preset.name, preset.subtitle),
            preset.mood,
            format!("Applied {} UI preset across the workspace.", preset.name),
        );
        res.ui_patch = Some(CliUiPatch { preset_id: Some(preset.id.to_string()), ..Default::default() });
        return CliExecutionResult::Handled(res);
    }
    let parsed = parse_local_ui_intent(arg_str).or_else(|| parse_local_ui_intent(&format!("make ui {arg_str}")));
    if let Some(patch) = parsed {
        let mut res = CliHandledResult::new(
            format!("UI UPDATED // {}", patch.key_names().join(", ")),
            ExpressionMood::Happy,
            "Applied your requested UI customization across the workspace.",
        );
        res.ui_patch = Some(patch);
        return CliExecutionResult::Handled(res);
    }
    let preset_ids = CURATED_PRESETS.iter().map(|p| p.id).collect::<Vec<_>>().join(", ");
    CliExecutionResult::Handled(CliHandledResult::new(
        format!(
            "Usage: /ui <preset|reset|instructions>\nPresets: {preset_ids}\nExample: /ui violet mono sharp right sidebar braille rain"
        ),
        ExpressionMood::Alert,
        "Specify a UI preset, reset, or customization instructions.",
    ))
}

fn has_word(lower: &str, words: &[&str]) -> bool {
    lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| words.contains(&token))
}

/// Focused port of `parseNaturalUiIntent` covering the CLI-relevant patch
/// fields. Full NL parsing/application lives in `ui_config` (P1-03).
fn parse_local_ui_intent(raw_text: &str) -> Option<CliUiPatch> {
    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();

    let has_ui_verb = has_word(&lower, &[
        "make", "set", "change", "switch", "turn", "hide", "show", "toggle", "move", "put", "resize",
        "scale", "rename", "style", "theme", "customize", "reset", "restore", "open", "close", "go",
        "enable", "disable", "increase", "decrease", "boost", "reduce", "arrange", "apply",
    ]);
    let has_ui_noun = has_word(&lower, &[
        "ui", "interface", "theme", "preset", "color", "colour", "accent", "palette", "slate", "grey",
        "gray", "violet", "purple", "amber", "gold", "cyan", "blue", "rose", "pink", "crimson", "red",
        "emerald", "green", "mono", "monochrome", "stealth", "zen", "sakura", "solaris", "glacier",
        "sidebar", "panel", "nav", "tabs", "header", "title", "brand", "portrait", "character",
        "avatar", "horizon", "wave", "footer", "composer", "input", "font", "monospace", "jakarta",
        "text", "corner", "corners", "sharp", "pill", "rounded", "blur", "opacity", "glow", "border",
        "compact", "airy", "comfortable", "motion", "animation", "speed", "background", "backdrop",
        "aurora", "braille", "dots", "bloom", "phosphor", "peaks", "silk", "pulse", "rain", "matrix",
        "scanline", "nebula", "constellation", "mesh", "canvas", "queue", "cli", "thread", "studio",
        "customizer",
    ]);
    let is_direct_preset_or_hex = get_curated_preset(&lower).is_some() || lower.contains('#');
    if !is_direct_preset_or_hex && !(has_ui_verb && has_ui_noun) {
        return None;
    }

    let mut patch = CliUiPatch::default();

    // Reset
    if lower == "reset"
        || (has_word(&lower, &["reset", "restore", "default"])
            && has_word(&lower, &["ui", "theme", "layout", "style", "everything", "all", "settings", "interface"]))
    {
        return Some(CliUiPatch { reset_all: true, ..Default::default() });
    }

    // Open studio
    if has_word(&lower, &["open", "show", "launch"]) && has_word(&lower, &["studio", "customizer", "customize"]) {
        patch.open_customizer = true;
    }

    // Curated presets (keyword families mirror uiCustomization.ts)
    let preset_id = if lower == "cyber-violet" || has_word(&lower, &["ultraviolet"]) || lower.contains("cyber violet") || lower.contains("violet theme") {
        Some("cyber-violet")
    } else if lower == "solaris-amber" || has_word(&lower, &["solaris"]) || lower.contains("amber crt") || lower.contains("retro crt") || lower.contains("crt theme") {
        Some("solaris-amber")
    } else if lower == "glacier-cyan" || has_word(&lower, &["glacier", "arctic"]) || lower.contains("ice cyan") {
        Some("glacier-cyan")
    } else if lower == "sakura-rose" || has_word(&lower, &["sakura", "nocturne"]) || lower.contains("cherry blossom") {
        Some("sakura-rose")
    } else if lower == "zen-mono" || has_word(&lower, &["stealth", "monograph"]) || lower.contains("zen mode") || lower.contains("minimalist focus") {
        Some("zen-mono")
    } else if lower == "slate-terminal" || lower.contains("slate monochrome") || lower.contains("slate terminal") || lower.contains("slate theme") {
        Some("slate-terminal")
    } else if lower == "emerald-terminal" || lower.contains("emerald terminal") {
        Some("emerald-terminal")
    } else {
        None
    };
    if let Some(id) = preset_id {
        patch.preset_id = Some(id.to_string());
    }

    // Accent swatches (only when no curated preset matched)
    if patch.preset_id.is_none() {
        let accent = if has_word(&lower, &["slate", "grey", "gray", "graphite", "charcoal"]) {
            Some("slate")
        } else if has_word(&lower, &["violet", "purple", "lavender"]) {
            Some("violet")
        } else if has_word(&lower, &["cyan", "ice", "aqua", "sky", "blue"]) {
            Some("cyan")
        } else if has_word(&lower, &["amber", "gold", "yellow", "orange"]) {
            Some("amber")
        } else if has_word(&lower, &["rose", "pink", "magenta", "sakura"]) {
            Some("rose")
        } else if has_word(&lower, &["crimson", "scarlet", "red"]) {
            Some("crimson")
        } else if has_word(&lower, &["monochrome", "monograph", "silver", "grayscale", "greyscale"]) {
            Some("mono")
        } else if has_word(&lower, &["emerald", "mint", "green"]) {
            Some("emerald")
        } else {
            None
        };
        if let Some(a) = accent {
            patch.accent_preset = Some(a.to_string());
        }
    }

    // Backgrounds
    let background = if lower.contains("braille rain") || has_word(&lower, &["matrix"]) || lower.contains("glyph rain") {
        Some("braille-rain")
    } else if has_word(&lower, &["aurora"]) {
        Some("aurora-drift")
    } else if has_word(&lower, &["mesh"]) {
        Some("mesh-gradient")
    } else if has_word(&lower, &["scanline", "scanlines", "crt"]) {
        Some("scanline-shimmer")
    } else if has_word(&lower, &["nebula", "cosmos"]) {
        Some("nebula")
    } else if has_word(&lower, &["constellation", "stars", "orbital"]) {
        Some("constellation-drift")
    } else if lower.contains("horizon tide") || lower.contains("tide backdrop") {
        Some("horizon-tide")
    } else {
        None
    };
    if let Some(b) = background {
        patch.background_preset_id = Some(b.to_string());
    }

    // Sidebar position / visibility
    let sidebar_right = lower.contains("right sidebar") || (lower.contains("sidebar") && lower.contains("right"));
    let sidebar_left = lower.contains("left sidebar") || (lower.contains("sidebar") && lower.contains("left"));
    if has_word(&lower, &["hide", "remove", "close", "collapse"]) && lower.contains("sidebar") {
        patch.sidebar_visible = Some(false);
    } else if has_word(&lower, &["show", "open", "restore", "display"]) && lower.contains("sidebar") {
        patch.sidebar_visible = Some(true);
    }
    if sidebar_right {
        patch.sidebar_position = Some("right".to_string());
        patch.sidebar_visible = Some(true);
    } else if sidebar_left {
        patch.sidebar_position = Some("left".to_string());
        patch.sidebar_visible = Some(true);
    }

    // Corner radius
    if has_word(&lower, &["sharp", "square", "boxy", "brutalist", "angular"]) {
        patch.corner_radius = Some("sharp".to_string());
    } else if has_word(&lower, &["pill", "bubbly"]) || lower.contains("super round") || lower.contains("extra round") {
        patch.corner_radius = Some("pill".to_string());
    } else if lower.contains("rounded corners") || lower.contains("normal corners") {
        patch.corner_radius = Some("rounded".to_string());
    }

    // Density
    if has_word(&lower, &["compact", "dense", "tight"]) {
        patch.density = Some("compact".to_string());
    } else if has_word(&lower, &["airy", "spacious", "relaxed"]) {
        patch.density = Some("airy".to_string());
    }

    // Fonts
    if has_word(&lower, &["mono", "monospace", "jetbrains"]) || lower.contains("code font") || lower.contains("terminal font") {
        patch.font_preset = Some("mono".to_string());
    } else if has_word(&lower, &["archivo"]) || lower.contains("display font") || lower.contains("bold font") {
        patch.font_preset = Some("display".to_string());
    } else if lower.contains("system font") {
        patch.font_preset = Some("system".to_string());
    } else if has_word(&lower, &["jakarta"]) || lower.contains("default font") {
        patch.font_preset = Some("jakarta".to_string());
    }

    // Motion
    if lower.contains("calm motion") || lower.contains("reduce motion") || lower.contains("reduced motion") || lower.contains("freeze animations") || lower.contains("stop moving") {
        patch.reduced_motion = Some(true);
    } else if lower.contains("full motion") || lower.contains("enable animations") {
        patch.reduced_motion = Some(false);
    }

    if patch.touched() {
        Some(patch)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_slash_commands_and_prompts() {
        assert_eq!(SLASH_COMMANDS.len(), 25);
        assert!(matches!(parse_cli_input("/help"), CliAction::PrintHelp(_)));
        assert_eq!(
            parse_cli_input("/resume hermes-ops"),
            CliAction::ResumeSession("hermes-ops".into())
        );
        assert_eq!(
            parse_cli_input("/ui switch to cyan phosphor compact"),
            CliAction::ApplyUiCustomization("switch to cyan phosphor compact".into())
        );
        assert_eq!(
            parse_cli_input("check deployment status"),
            CliAction::SendPrompt("check deployment status".into())
        );
        assert_eq!(parse_cli_input("/profile coder"), CliAction::SetProfile(Some("coder".into())));
    }

    #[test]
    fn autocompletes_prefixes_and_aliases() {
        let matches = autocomplete_slash_commands("/ca");
        assert!(matches.iter().any(|c| c.name == "/canvas"));
        assert!(matches.iter().any(|c| c.name == "/stop"));
    }

    // ---- Mirrored cliEngine.test.ts ----

    fn test_approval() -> ApprovalCard {
        ApprovalCard {
            id: "approval-run_9-req_1".into(),
            channel_id: "general".into(),
            title: "Run shell command".into(),
            action_type: "send_message".into(),
            mcp_tool: Some("terminal".into()),
            run_id: Some("run_9".into()),
            request_id: Some("req_1".into()),
            execution_generation: Some(2),
            kind: Some("approval".into()),
            choices: vec!["once".into(), "always".into(), "deny".into()],
            command: Some("rm -rf /tmp/build-cache".into()),
            destination: "hermes-gateway".into(),
            summary: "Clear build cache directory".into(),
            draft: "rm -rf /tmp/build-cache".into(),
            status: ApprovalStatus::Pending,
            created_at: "10:00".into(),
        }
    }

    fn test_task() -> BackgroundTask {
        BackgroundTask {
            id: "task-turn-1".into(),
            channel_id: "general".into(),
            session_id: "hermes-general".into(),
            kind: None,
            background: None,
            title: "Inspect CI logs".into(),
            status: TaskLifecycleStatus::Running,
            raw_status: None,
            progress: 50.0,
            progress_label: None,
            last_step: Some("Running terminal".into()),
            steps: vec![],
            result: None,
            error_code: None,
            created_at_ms: None,
            updated_at_ms: None,
            agent_done_at_ms: None,
            confirmed_at_ms: None,
            updated_at: "10:00".into(),
        }
    }

    fn base_ctx() -> CliContext {
        CliContext {
            active_channel: "general".into(),
            available_channels: vec!["general".into(), "ops".into()],
            approvals: vec![test_approval()],
            tasks: vec![test_task()],
            brief: vec![],
            is_muted: false,
            skills: vec![CliSkill {
                name: "github-pr-workflow".into(),
                source: "category:git".into(),
                summary: Some("Inspect and manage GitHub PR checks".into()),
                enabled: true,
            }],
            toolsets: vec![CliToolset {
                name: "core".into(),
                label: "Core Tools".into(),
                description: "File and terminal execution".into(),
                enabled: true,
                configured: Some(true),
                tools: vec!["read_file".into(), "terminal".into()],
            }],
            cron_jobs: vec![CliCronJob {
                id: "job-daily-1".into(),
                name: "Morning check".into(),
                schedule: "0 8 * * *".into(),
                mode: "agent".into(),
                status: Some("active".into()),
                summary: "Summarize overnight runs".into(),
                channel_id: Some("ops".into()),
            }],
            gateway_connected: true,
            gateway_url: Some("http://127.0.0.1:8642".into()),
            active_model: Some("nous/hermes-4".into()),
            gateway_profile: Some("default".into()),
            profiles: vec![],
            draft_profile: None,
            route_prefix: Some(String::new()),
            health_status: Some("ok".into()),
            session_key_header: Some("X-Hermes-Session-Key".into()),
            capabilities: Some(CliCapabilities {
                session_key_header: Some("X-Hermes-Session-Key".into()),
                features: CliCapabilityFeatures {
                    chat_completions: true,
                    reasoning_streaming: true,
                    run_submission: true,
                    run_events_sse: true,
                    run_approval: true,
                    session_chat_stream: true,
                },
            }),
            transport_mode: Some(CliTransportMode::Auto),
            resolved_transport: Some("runs".into()),
            gateway_error: None,
        }
    }

    fn handled(res: CliExecutionResult) -> CliHandledResult {
        match res {
            CliExecutionResult::Handled(h) => h,
            CliExecutionResult::Chat(_) => panic!("expected handled result"),
        }
    }

    #[test]
    fn ts_routes_natural_language_to_chat() {
        let res = execute_cli_input("Summarize my morning brief", &base_ctx());
        assert_eq!(res, CliExecutionResult::Chat("Summarize my morning brief".into()));
    }

    #[test]
    fn ts_switch_resume_validate_synced_sessions() {
        let ok = handled(execute_cli_input("/switch ops", &base_ctx()));
        assert_eq!(ok.switch_channel.as_deref(), Some("ops"));

        let resume = handled(execute_cli_input("/resume hermes-ops", &base_ctx()));
        assert_eq!(resume.switch_channel.as_deref(), Some("ops"));

        let bad = handled(execute_cli_input("/switch invalid-room", &base_ctx()));
        assert_eq!(bad.mood, ExpressionMood::Alert);
        assert_eq!(bad.switch_channel, None);
    }

    #[test]
    fn ts_approve_once_always_deny_and_reject() {
        let once = handled(execute_cli_input("/approve approval-run_9-req_1", &base_ctx()));
        assert_eq!(once.approve_card_id.as_deref(), Some("approval-run_9-req_1"));
        assert_eq!(once.approve_choice, Some(ApprovalChoice::Once));
        assert_eq!(once.mood, ExpressionMood::Happy);

        let always = handled(execute_cli_input("/approve req_1 always", &base_ctx()));
        assert_eq!(always.approve_card_id.as_deref(), Some("approval-run_9-req_1"));
        assert_eq!(always.approve_choice, Some(ApprovalChoice::Always));

        let deny = handled(execute_cli_input("/reject approval-run_9-req_1", &base_ctx()));
        assert_eq!(deny.reject_card_id.as_deref(), Some("approval-run_9-req_1"));
        assert_eq!(deny.approve_choice, Some(ApprovalChoice::Deny));
    }

    #[test]
    fn ts_shader_modes_including_holo_norm_invalid() {
        let holo = handled(execute_cli_input("/shader holo", &base_ctx()));
        assert_eq!(holo.set_shader, Some(AsciiShaderMode::Holo));
        assert_eq!(holo.mood, ExpressionMood::Focused);

        let norm = handled(execute_cli_input("/shader norm", &base_ctx()));
        assert_eq!(norm.set_shader, Some(AsciiShaderMode::Norm));
        assert_eq!(norm.mood, ExpressionMood::Focused);

        let bad = handled(execute_cli_input("/shader invalid", &base_ctx()));
        assert_eq!(bad.set_shader, None);
        assert_eq!(bad.mood, ExpressionMood::Alert);
    }

    #[test]
    fn ts_style_halftone_dither_modes() {
        let braille = handled(execute_cli_input("/style braille", &base_ctx()));
        assert_eq!(braille.set_dither_style, Some("braille"));
        assert_eq!(braille.mood, ExpressionMood::Focused);

        let invalid = handled(execute_cli_input("/style nope", &base_ctx()));
        assert_eq!(invalid.set_dither_style, None);
        assert_eq!(invalid.mood, ExpressionMood::Alert);
    }

    #[test]
    fn ts_gateway_readouts_and_offline_states() {
        let skills = handled(execute_cli_input("/skills", &base_ctx()));
        assert!(skills.output.contains("github-pr-workflow"));
        assert!(skills.output.contains("GET /v1/skills"));

        let toolsets = handled(execute_cli_input("/toolsets", &base_ctx()));
        assert!(toolsets.output.contains("Core Tools"));
        assert!(toolsets.output.contains("read_file, terminal"));

        let mem = handled(execute_cli_input("/memory", &base_ctx()));
        assert!(mem.output.contains("X-Hermes-Session-Id"));
        assert!(mem.output.contains("X-Hermes-Session-Key"));

        let cron = handled(execute_cli_input("/cron", &base_ctx()));
        assert!(cron.output.contains("GET /api/jobs"));
        assert!(cron.output.contains("Summarize overnight runs"));

        let gw = handled(execute_cli_input("/gateway", &base_ctx()));
        assert!(gw.output.contains("GET /v1/capabilities"));
        assert!(gw.output.contains("GET /api/model/options"));
        assert!(gw.output.contains("nous/hermes-4"));

        let transport = handled(execute_cli_input("/transport session-stream", &base_ctx()));
        assert_eq!(transport.set_transport_mode, Some(CliTransportMode::SessionStream));

        let stop = handled(execute_cli_input("/stop", &base_ctx()));
        assert!(stop.cancel_active_run);

        let mut offline = base_ctx();
        offline.skills = vec![];
        offline.gateway_connected = false;
        offline.gateway_error = Some("ECONNREFUSED".into());
        let offline_skills = handled(execute_cli_input("/skills", &offline));
        assert_eq!(offline_skills.mood, ExpressionMood::Alert);
        assert!(offline_skills.output.to_ascii_lowercase().contains("gateway offline"));
    }

    #[test]
    fn ts_ui_presets_natural_reset_and_customize() {
        let studio = handled(execute_cli_input("/customize", &base_ctx()));
        assert_eq!(studio.ui_patch.as_ref().map(|p| p.open_customizer), Some(true));

        let preset = handled(execute_cli_input("/ui cyber-violet", &base_ctx()));
        assert_eq!(preset.ui_patch.as_ref().and_then(|p| p.preset_id.clone()).as_deref(), Some("cyber-violet"));

        let natural = handled(execute_cli_input("/ui amber mono sharp right sidebar", &base_ctx()));
        let patch = natural.ui_patch.expect("patch");
        assert_eq!(patch.accent_preset.as_deref(), Some("amber"));
        assert_eq!(patch.font_preset.as_deref(), Some("mono"));
        assert_eq!(patch.corner_radius.as_deref(), Some("sharp"));
        assert_eq!(patch.sidebar_position.as_deref(), Some("right"));

        let reset = handled(execute_cli_input("/ui reset", &base_ctx()));
        assert_eq!(reset.ui_patch.as_ref().map(|p| p.reset_all), Some(true));
    }

    #[test]
    fn ts_profile_listing_switching_and_validation() {
        // No profiles registered: report the active draft default.
        let empty = handled(execute_cli_input("/profile", &base_ctx()));
        assert!(empty.output.contains("active default=\"default\""));
        assert_eq!(empty.mood, ExpressionMood::Calm);

        let mut ctx = base_ctx();
        ctx.profiles = vec![
            CliProfileSummary {
                id: "default".into(),
                label: "Default".into(),
                mode: CliProfileMode::Standalone,
                base_url: "http://127.0.0.1:8642".into(),
                route_prefix: String::new(),
                connected: true,
            },
            CliProfileSummary {
                id: "coder".into(),
                label: "Coder".into(),
                mode: CliProfileMode::Multiplexed,
                base_url: "http://127.0.0.1:8642".into(),
                route_prefix: "/p/coder".into(),
                connected: false,
            },
        ];

        let listing = handled(execute_cli_input("/profiles", &ctx));
        assert!(listing.output.contains("HERMES AGENT // PARALLEL PROFILES"));
        assert!(listing.output.contains("[DRAFT] @default"));
        assert!(listing.output.contains("@coder"));
        assert!(listing.output.contains("multiplexed"));
        assert!(listing.output.contains("OFFLINE"));
        assert!(listing.output.contains("http://127.0.0.1:8642/p/coder"));
        assert!(listing.output.contains("Usage: /profile <slug>"));
        assert_eq!(listing.mood, ExpressionMood::Focused);

        let switch = handled(execute_cli_input("/profile @Coder", &ctx));
        assert_eq!(switch.switch_profile.as_deref(), Some("coder"));
        assert_eq!(switch.mood, ExpressionMood::Happy);
        assert!(switch.output.contains("launch on @coder"));

        let unknown = handled(execute_cli_input("/profile researcher", &ctx));
        assert_eq!(unknown.mood, ExpressionMood::Alert);
        assert_eq!(unknown.switch_profile, None);
        assert!(unknown.output.contains("Unknown profile \"researcher\""));
        assert!(unknown.output.contains("default, coder"));

        // With no registry, any slug is accepted as the new draft default.
        let free = handled(execute_cli_input("/profile researcher", &base_ctx()));
        assert_eq!(free.switch_profile.as_deref(), Some("researcher"));
    }
}