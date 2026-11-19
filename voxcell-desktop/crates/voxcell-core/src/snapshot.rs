//! Versioned preferences snapshot — port of `src/utils/settingsSnapshot.ts`
//! (spec P1-03, §5.1 / §6.2). Owned by the slate-snapshot workstream.
//!
//! Provides: `schemaVersion: 1` + `themeEpoch: "slate-v1"` snapshots, legacy
//! untouched-emerald auto-migration (customized palettes preserved),
//! `resolve_boot_hydration_source` (local vs vault precedence),
//! `summarize_preserved_categories`, backup metadata summaries, semver
//! comparison and the JSON import/export parser.
//!
//! Updater-only types from the TS module (`UpdateReceipt`, `UpdateStepResult`,
//! `AppUpdateStatus`) belong to `voxcell-store::updater` (P1-04) and are
//! intentionally not defined here.

use crate::types::{
    AccentPreset, BackgroundCustomSlot, BackgroundMediaType, BackgroundPresetId,
    BackgroundSettings, UiCustomizationConfig,
};
use crate::ui_config::{
    apply_sanitized_patch, sanitize_ui_patch, VALID_BACKGROUND_PRESET_IDS,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SNAPSHOT_SCHEMA_VERSION: u8 = 1;
pub const THEME_EPOCH_SLATE_V1: &str = "slate-v1";

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn kv<'a>(raw: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    let obj = raw.as_object()?;
    obj.get(camel).or_else(|| obj.get(snake))
}

fn trimmed_str(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn finite_i64(v: Option<&Value>) -> Option<i64> {
    let n = v?.as_f64()?;
    if n.is_finite() {
        Some(n as i64)
    } else {
        None
    }
}

fn round2(v: f64) -> f32 {
    ((v * 100.0).round() / 100.0) as f32
}

// ---------------------------------------------------------------------------
// Snapshot enums & preference structs
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotReason {
    #[default]
    Sync,
    Manual,
    PreUpdate,
    Restore,
    Import,
}

impl SnapshotReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Manual => "manual",
            Self::PreUpdate => "pre-update",
            Self::Restore => "restore",
            Self::Import => "import",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "sync" => Some(Self::Sync),
            "manual" => Some(Self::Manual),
            "pre-update" => Some(Self::PreUpdate),
            "restore" => Some(Self::Restore),
            "import" => Some(Self::Import),
            _ => None,
        }
    }
}

/// Workspace mood subset accepted by `settingsSnapshot.ts` (`VALID_MOODS`).
/// Kept separate from `types::ExpressionMood` (which models the character's
/// full live mood cycle and uses `Calm` rather than `Neutral`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMood {
    #[default]
    Happy,
    Focused,
    Thinking,
    Neutral,
    Alert,
}

impl SnapshotMood {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Happy => "happy",
            Self::Focused => "focused",
            Self::Thinking => "thinking",
            Self::Neutral => "neutral",
            Self::Alert => "alert",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "happy" => Some(Self::Happy),
            "focused" => Some(Self::Focused),
            "thinking" => Some(Self::Thinking),
            "neutral" => Some(Self::Neutral),
            "alert" => Some(Self::Alert),
            _ => None,
        }
    }
}

/// Transport preference incl. the snapshot-only `auto` mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotTransport {
    Auto,
    Runs,
    SessionStream,
    ChatCompletions,
}

impl SnapshotTransport {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "auto" => Some(Self::Auto),
            "runs" => Some(Self::Runs),
            "session_stream" => Some(Self::SessionStream),
            "chat_completions" => Some(Self::ChatCompletions),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspacePreferences {
    pub show_thinking: bool,
    pub voice_muted: bool,
    pub mood: SnapshotMood,
    pub draft_profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport_mode: Option<SnapshotTransport>,
    pub onboarding_completed: bool,
}

impl Default for WorkspacePreferences {
    fn default() -> Self {
        Self {
            show_thinking: true,
            voice_muted: false,
            mood: SnapshotMood::Happy,
            draft_profile: "default".to_string(),
            transport_mode: None,
            onboarding_completed: false,
        }
    }
}

/// Port of `DEFAULT_WORKSPACE_PREFERENCES`.
pub static DEFAULT_WORKSPACE_PREFERENCES: LazyLock<WorkspacePreferences> =
    LazyLock::new(WorkspacePreferences::default);

fn normalize_workspace_preferences(raw: Option<&Value>) -> WorkspacePreferences {
    let Some(raw) = raw.filter(|v| v.is_object()) else {
        return WorkspacePreferences::default();
    };
    let d = WorkspacePreferences::default();
    let mood = trimmed_str(kv(raw, "mood", "mood"))
        .as_deref()
        .and_then(SnapshotMood::from_id)
        .unwrap_or(d.mood);
    let draft_profile = trimmed_str(kv(raw, "draftProfile", "draft_profile"))
        .map(|s| s.chars().take(64).collect())
        .unwrap_or(d.draft_profile);
    let transport_mode = kv(raw, "transportMode", "transport_mode")
        .and_then(Value::as_str)
        .and_then(SnapshotTransport::from_id);
    WorkspacePreferences {
        show_thinking: kv(raw, "showThinking", "show_thinking")
            .and_then(Value::as_bool)
            .unwrap_or(d.show_thinking),
        voice_muted: kv(raw, "voiceMuted", "voice_muted")
            .and_then(Value::as_bool)
            .unwrap_or(d.voice_muted),
        mood,
        draft_profile,
        transport_mode,
        onboarding_completed: kv(raw, "onboardingCompleted", "onboarding_completed")
            .and_then(Value::as_bool)
            .unwrap_or(d.onboarding_completed),
    }
}

// ---------------------------------------------------------------------------
// Canvas sessions & pins (snapshot-side normalization)
// ---------------------------------------------------------------------------

/// Snapshot view of `CanvasSession` (`src/utils/canvasSessions.ts`). Shape /
/// tint / mood stay string-typed here; the canonical shape catalog lives in
/// `brailleShapesEngine.ts` (halftone crate, P2-01). The 9-entry fallback meta
/// table below mirrors `BRAILLE_SHAPE_CATALOG` defaults for normalization.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasSession {
    pub id: String,
    pub channel_id: String,
    pub hermes_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub agent_persona: String,
    pub name: String,
    pub topic: String,
    pub shape: String,
    pub tint: String,
    pub x_pct: f32,
    pub y_pct: f32,
    pub mood: String,
    pub eye_style: String,
    pub is_core: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    pub autonomy_mode: String,
    pub schedule_label: String,
    pub autonomy_paused: bool,
    pub sandbox_env: String,
    pub workspace_dir: String,
    pub skills: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connected_to: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasPinKind {
    Brief,
    Repo,
    Note,
    Skill,
}

impl CanvasPinKind {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            "brief" => Some(Self::Brief),
            "repo" => Some(Self::Repo),
            "note" => Some(Self::Note),
            "skill" => Some(Self::Skill),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasPin {
    pub id: String,
    pub kind: CanvasPinKind,
    pub title: String,
    pub detail: String,
    pub meta: String,
    pub x_pct: f32,
    pub y_pct: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_channel_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CanvasPreferences {
    pub sessions: Vec<CanvasSession>,
    pub pins: Vec<CanvasPin>,
    pub dismissed_ids: Vec<String>,
}

const VALID_SHAPES: &[&str] = &[
    "clover", "star", "ghost", "heart", "cloud", "triangle", "pebble", "trapezoid", "blob",
];
const VALID_CANVAS_MOODS: &[&str] = &[
    "neutral", "listening", "thinking", "speaking", "happy", "focused", "alert",
];
const VALID_AUTONOMY: &[&str] = &["manual", "scheduled", "watch"];
const VALID_SANDBOX: &[&str] = &["cloud-sandbox", "local-repo", "browser-worker"];
const VALID_DOT_ORIGINS: &[&str] = &["workspace", "imported", "seed"];

/// Fallback meta per shape — mirrors `BRAILLE_SHAPE_CATALOG` (defaultTint /
/// defaultEyeStyle / tagline / defaultAgentPersona).
fn shape_meta(kind: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match kind {
        "star" => ("pink", "open", "Launch runner", "High-energy release and save_page builder running fast launch turns."),
        "ghost" => ("lavender", "open", "Silent daemon", "Stealthy background daemon monitoring cron watchers and ambient events."),
        "heart" => ("coral", "open", "Care & triage", "Attentive triage agent managing approvals, dispatches, and alerts."),
        "cloud" => ("orange", "open", "Memory synth", "Curious synthesis agent curating MEMORY.md and skill indexes."),
        "triangle" => ("cyan", "open", "Code reviewer", "Sharp analytical reviewer auditing diffs, security rules, and tests."),
        "pebble" => ("periwinkle", "sleepy", "Deep-work lane", "Quiet deep-research session focused on long-horizon synthesis."),
        "trapezoid" => ("amber", "sleepy", "Brief keeper", "Steady ops & morning-brief keeper guarding cron schedules."),
        "blob" => ("magenta", "open", "Sandbox bot", "Edgy experimental sandbox session with layered wolfcut styling."),
        // clover (catalog head) is the fallback for unknown shapes too.
        _ => ("lime", "open", "Core strategist", "Calm primary Hermes coordinator styled after the main cyber-bob operator."),
    }
}

struct AutonomyDefaults {
    autonomy_mode: &'static str,
    schedule_label: &'static str,
    sandbox_env: &'static str,
    skills: &'static [&'static str],
    connected_to: Option<&'static [&'static str]>,
}

/// Port of `defaultAutonomyForChannel` (`DEFAULT_SHAPE_PRESETS` autonomy
/// fields for the six seed channels + generic fallback).
fn default_autonomy_for_channel(channel_id: &str) -> AutonomyDefaults {
    match channel_id {
        "general" => AutonomyDefaults {
            autonomy_mode: "manual",
            schedule_label: "On-demand",
            sandbox_env: "cloud-sandbox",
            skills: &["browser", "files", "research"],
            connected_to: Some(&["ops", "research"]),
        },
        "ops" => AutonomyDefaults {
            autonomy_mode: "scheduled",
            schedule_label: "Every 15m",
            sandbox_env: "local-repo",
            skills: &["shell", "files", "mail"],
            connected_to: Some(&["launch", "daemon"]),
        },
        "research" => AutonomyDefaults {
            autonomy_mode: "scheduled",
            schedule_label: "Every 30m",
            sandbox_env: "browser-worker",
            skills: &["research", "browser"],
            connected_to: Some(&["general"]),
        },
        "launch" => AutonomyDefaults {
            autonomy_mode: "manual",
            schedule_label: "On-demand",
            sandbox_env: "cloud-sandbox",
            skills: &["mail", "calendar", "browser"],
            connected_to: Some(&["ops"]),
        },
        "daemon" => AutonomyDefaults {
            autonomy_mode: "watch",
            schedule_label: "Continuous watch",
            sandbox_env: "local-repo",
            skills: &["shell", "files"],
            connected_to: Some(&["ops"]),
        },
        "memory" => AutonomyDefaults {
            autonomy_mode: "watch",
            schedule_label: "Continuous watch",
            sandbox_env: "cloud-sandbox",
            skills: &["files", "research"],
            connected_to: None,
        },
        _ => AutonomyDefaults {
            autonomy_mode: "manual",
            schedule_label: "On-demand",
            sandbox_env: "cloud-sandbox",
            skills: &["browser", "files"],
            connected_to: None,
        },
    }
}

/// Port of `createHermesSessionId`.
fn create_hermes_session_id(channel_id: &str) -> String {
    let clean: String = channel_id
        .to_ascii_lowercase()
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let clean = clean.trim_matches('-');
    format!("hermes-{}", if clean.is_empty() { "session" } else { clean })
}

/// Port of `slugifySessionName`.
fn slugify_session_name(raw: &str, existing: &HashSet<String>) -> String {
    let mapped: String = raw
        .to_ascii_lowercase()
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let base: String = mapped.trim_matches('-').chars().take(24).collect();
    let base = base.trim_end_matches('-');
    let base = if base.is_empty() { "session" } else { base };
    let taken: HashSet<String> = existing.iter().map(|s| s.to_ascii_lowercase()).collect();
    if !taken.contains(base) {
        return base.to_string();
    }
    let mut n = 2u32;
    loop {
        let suffix = format!("-{n}");
        let keep = (24usize).saturating_sub(suffix.len()).max(1);
        let trimmed_base: String = base.chars().take(keep).collect();
        let trimmed_base = trimmed_base.trim_end_matches('-');
        let trimmed_base = if trimmed_base.is_empty() { "s" } else { trimmed_base };
        let candidate = format!("{trimmed_base}{suffix}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

fn clamp_canvas_position(x: f64, y: f64) -> (f32, f32) {
    (
        round2(x).clamp(10.0, 90.0),
        round2(y).clamp(18.0, 74.0),
    )
}

fn clamp_pin_position(x: f64, y: f64) -> (f32, f32) {
    (
        round2(x).clamp(10.0, 90.0),
        round2(y).clamp(14.0, 82.0),
    )
}

fn is_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// Port of `normalizeCanvasSessions`.
pub fn normalize_canvas_sessions(raw_list: &[Value]) -> Vec<CanvasSession> {
    let mut seen_channels: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (i, item) in raw_list.iter().enumerate() {
        if !item.is_object() {
            continue;
        }
        let name_raw = trimmed_str(item.get("name"));
        let channel_raw = trimmed_str(item.get("channelId").or_else(|| item.get("channel_id")));
        let base_name = name_raw
            .or(channel_raw.clone())
            .unwrap_or_else(|| format!("session-{}", i + 1));
        let base_name = base_name.strip_prefix('#').unwrap_or(&base_name).to_string();

        let channel_id = match channel_raw {
            Some(c) => c,
            None => slugify_session_name(&base_name, &seen_channels),
        };
        if seen_channels.contains(&channel_id) {
            continue;
        }
        seen_channels.insert(channel_id.clone());

        let shape = item
            .get("shape")
            .and_then(Value::as_str)
            .filter(|s| VALID_SHAPES.contains(s))
            .unwrap_or("clover");
        let (def_tint, def_eye, tagline, persona) = shape_meta(shape);
        let x = item.get("xPct").or_else(|| item.get("x_pct")).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(50.0);
        let y = item.get("yPct").or_else(|| item.get("y_pct")).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(46.0);
        let (x_pct, y_pct) = clamp_canvas_position(x, y);
        let mood = item
            .get("mood")
            .and_then(Value::as_str)
            .filter(|s| VALID_CANVAS_MOODS.contains(s))
            .unwrap_or("happy");

        let defaults = default_autonomy_for_channel(&channel_id);
        let autonomy_mode = item
            .get("autonomyMode").or_else(|| item.get("autonomy_mode"))
            .and_then(Value::as_str)
            .filter(|s| VALID_AUTONOMY.contains(s))
            .unwrap_or(defaults.autonomy_mode);
        let sandbox_env = item
            .get("sandboxEnv").or_else(|| item.get("sandbox_env"))
            .and_then(Value::as_str)
            .filter(|s| VALID_SANDBOX.contains(s))
            .unwrap_or(defaults.sandbox_env);
        let schedule_label = trimmed_str(item.get("scheduleLabel").or_else(|| item.get("schedule_label")))
            .unwrap_or_else(|| defaults.schedule_label.to_string());
        let workspace_dir = trimmed_str(item.get("workspaceDir").or_else(|| item.get("workspace_dir")))
            .unwrap_or_else(|| format!("~/hermes/dots/{channel_id}"));
        let skills = match item.get("skills").and_then(Value::as_array) {
            Some(arr) => arr
                .iter()
                .filter_map(|s| s.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string))
                .take(8)
                .collect(),
            None => defaults.skills.iter().map(|s| s.to_string()).collect(),
        };
        let connected_to = match item.get("connectedTo").or_else(|| item.get("connected_to")).and_then(Value::as_array) {
            Some(arr) => Some(
                arr.iter()
                    .filter_map(Value::as_str)
                    .filter(|c| *c != channel_id)
                    .map(str::to_string)
                    .collect(),
            ),
            None => defaults.connected_to.map(|cs| cs.iter().map(|s| s.to_string()).collect()),
        };
        let origin = item
            .get("origin")
            .and_then(Value::as_str)
            .filter(|s| VALID_DOT_ORIGINS.contains(s))
            .map(str::to_string);

        let hermes_raw = trimmed_str(item.get("hermesSessionId").or_else(|| item.get("hermes_session_id")));
        let hermes_session_id = match hermes_raw {
            Some(h) if !h.to_ascii_lowercase().starts_with("duet-") && h.to_ascii_lowercase() != "duet" => h,
            _ => create_hermes_session_id(&channel_id),
        };
        let profile = trimmed_str(item.get("profile")).map(|p| p.to_ascii_lowercase());
        let agent_persona = trimmed_str(item.get("agentPersona").or_else(|| item.get("agent_persona")))
            .unwrap_or_else(|| persona.to_string());
        let topic = trimmed_str(item.get("topic")).unwrap_or_else(|| tagline.to_string());
        let tint = item
            .get("tint")
            .and_then(Value::as_str)
            .unwrap_or(def_tint)
            .to_string();
        let eye_style = match item.get("eyeStyle").or_else(|| item.get("eye_style")).and_then(Value::as_str) {
            Some("sleepy") => "sleepy",
            Some("open") => "open",
            _ => def_eye,
        };
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("session-{channel_id}"));

        out.push(CanvasSession {
            id,
            channel_id,
            hermes_session_id,
            profile,
            agent_persona,
            name: base_name,
            topic,
            shape: shape.to_string(),
            tint,
            x_pct,
            y_pct,
            mood: mood.to_string(),
            eye_style: eye_style.to_string(),
            is_core: is_truthy(item.get("isCore").or_else(|| item.get("is_core"))),
            origin,
            autonomy_mode: autonomy_mode.to_string(),
            schedule_label,
            autonomy_paused: is_truthy(item.get("autonomyPaused").or_else(|| item.get("autonomy_paused"))),
            sandbox_env: sandbox_env.to_string(),
            workspace_dir,
            skills,
            connected_to,
        });
    }
    out
}

/// Port of `normalizeCanvasPins`.
pub fn normalize_canvas_pins(raw_list: &[Value]) -> Vec<CanvasPin> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (i, item) in raw_list.iter().enumerate() {
        if !item.is_object() {
            continue;
        }
        let id = trimmed_str(item.get("id")).unwrap_or_else(|| format!("pin-{}", i + 1));
        if seen.contains(&id) {
            continue;
        }
        seen.insert(id.clone());
        let kind = item
            .get("kind")
            .and_then(Value::as_str)
            .and_then(CanvasPinKind::from_id)
            .unwrap_or(CanvasPinKind::Note);
        let x = item.get("xPct").or_else(|| item.get("x_pct")).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(20.0);
        let y = item.get("yPct").or_else(|| item.get("y_pct")).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(20.0);
        let (x_pct, y_pct) = clamp_pin_position(x, y);
        let title = trimmed_str(item.get("title"))
            .map(|s| s.chars().take(48).collect())
            .unwrap_or_else(|| "Workspace Note".to_string());
        let detail = trimmed_str(item.get("detail"))
            .map(|s| s.chars().take(180).collect())
            .unwrap_or_else(|| "Pinned spatial context for connected Dots.".to_string());
        let meta = trimmed_str(item.get("meta"))
            .map(|s| s.chars().take(44).collect())
            .unwrap_or_else(|| "Canvas pin".to_string());
        let linked_channel_id = trimmed_str(item.get("linkedChannelId").or_else(|| item.get("linked_channel_id")));
        out.push(CanvasPin { id, kind, title, detail, meta, x_pct, y_pct, linked_channel_id });
    }
    out
}

// ---------------------------------------------------------------------------
// Background settings normalization (backgrounds/persistence.ts parity)
// ---------------------------------------------------------------------------

fn is_valid_custom_media_src(src: &str) -> bool {
    let trimmed = src.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 2_500_000 {
        return false;
    }
    trimmed.starts_with("https://")
        || trimmed.starts_with("http://")
        || trimmed.starts_with('/')
        || trimmed.starts_with("data:image/")
        || trimmed.starts_with("data:video/")
}

fn infer_media_type_from_src(src: &str) -> BackgroundMediaType {
    let lower = src.trim().to_ascii_lowercase();
    if lower.starts_with("data:video/") {
        return BackgroundMediaType::Video;
    }
    let path = lower.split('?').next().unwrap_or("");
    if [".mp4", ".webm", ".ogg", ".mov"].iter().any(|ext| path.ends_with(ext)) {
        return BackgroundMediaType::Video;
    }
    BackgroundMediaType::Image
}

fn sanitize_custom_slot(raw: &Value, now: i64) -> Option<BackgroundCustomSlot> {
    if !raw.is_object() {
        return None;
    }
    let id = trimmed_str(raw.get("id"))?;
    let src = raw.get("src").and_then(Value::as_str)?.trim().to_string();
    if !is_valid_custom_media_src(&src) {
        return None;
    }
    let media_type = match raw.get("mediaType").or_else(|| raw.get("media_type")).and_then(Value::as_str) {
        Some("video") => BackgroundMediaType::Video,
        Some("image") => BackgroundMediaType::Image,
        _ => infer_media_type_from_src(&src),
    };
    let label = trimmed_str(raw.get("label"))
        .map(|s| s.chars().take(36).collect())
        .unwrap_or_else(|| {
            if media_type == BackgroundMediaType::Video { "Custom Video".to_string() } else { "Custom Image".to_string() }
        });
    let created_at = raw
        .get("createdAt").or_else(|| raw.get("created_at"))
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .map(|v| v as i64)
        .unwrap_or(now);
    Some(BackgroundCustomSlot {
        id: id.chars().take(64).collect(),
        label,
        media_type,
        src,
        created_at,
    })
}

fn clamp_opacity(v: Option<&Value>) -> f32 {
    match v.and_then(Value::as_f64).filter(|n| n.is_finite()) {
        Some(n) => round2(n).clamp(0.15, 0.85),
        None => 0.64,
    }
}

fn clamp_blur_px(v: Option<&Value>) -> f32 {
    match v.and_then(Value::as_f64).filter(|n| n.is_finite()) {
        Some(n) => (n.round() as f32).clamp(0.0, 28.0),
        None => 12.0,
    }
}

/// Port of `parseSavedBackgroundSettings` (accepts the web JSON shape; also
/// understands the legacy scaffold `{preset, opacity, motionSpeed, intensity}`
/// shape and crosswalks the preset enum to its web id).
pub fn parse_saved_background_settings(raw: &Value) -> BackgroundSettings {
    let mut settings = BackgroundSettings::default();
    if !raw.is_object() {
        return settings;
    }
    let now = now_ms();
    let mut custom_slots: Vec<BackgroundCustomSlot> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    if let Some(arr) = raw.get("customSlots").or_else(|| raw.get("custom_slots")).and_then(Value::as_array) {
        for item in arr {
            if let Some(slot) = sanitize_custom_slot(item, now) {
                if !seen.contains(&slot.id) {
                    seen.insert(slot.id.clone());
                    custom_slots.push(slot);
                }
            }
            if custom_slots.len() >= 6 {
                break;
            }
        }
    }

    let candidate_active = raw
        .get("activeId").or_else(|| raw.get("active_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let mut is_valid_active = VALID_BACKGROUND_PRESET_IDS.contains(&candidate_active)
        || custom_slots.iter().any(|s| s.id == candidate_active);
    let mut active_id = candidate_active.to_string();

    // Legacy scaffold shape: derive the web id from the preset enum.
    if !is_valid_active {
        if let Some(preset_val) = raw.get("preset") {
            if let Ok(preset) = serde_json::from_value::<BackgroundPresetId>(preset_val.clone()) {
                active_id = preset.web_id().to_string();
                is_valid_active = true;
                settings.preset = preset;
                if let Some(ms) = raw.get("motionSpeed").or_else(|| raw.get("motion_speed")).and_then(Value::as_f64).filter(|v| v.is_finite()) {
                    settings.motion_speed = ms as f32;
                }
                if let Some(it) = raw.get("intensity").and_then(Value::as_f64).filter(|v| v.is_finite()) {
                    settings.intensity = it as f32;
                }
            }
        }
    }
    if !is_valid_active {
        active_id = "aurora-drift".to_string();
    }
    if let Some(preset) = BackgroundPresetId::from_web_id(&active_id) {
        settings.preset = preset;
    }
    settings.active_id = active_id;
    settings.opacity = clamp_opacity(raw.get("opacity"));
    settings.blur_px = clamp_blur_px(raw.get("blurPx").or_else(|| raw.get("blur_px")));
    settings.custom_slots = custom_slots;
    settings
}

// ---------------------------------------------------------------------------
// UserPreferencesSnapshot
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPreferencesSnapshot {
    pub schema_version: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_epoch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    pub captured_at: i64,
    pub app_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_origin: Option<String>,
    #[serde(default)]
    pub reason: SnapshotReason,
    pub ui_config: UiCustomizationConfig,
    pub background: BackgroundSettings,
    pub workspace: WorkspacePreferences,
    pub canvas: CanvasPreferences,
}

/// Port of `normalizePreferencesSnapshot(raw, fallbackVersion = '0.1.0')`.
pub fn normalize_preferences_snapshot(raw: &Value) -> Option<UserPreferencesSnapshot> {
    normalize_preferences_snapshot_with_version(raw, "0.1.0")
}

/// Spec §6.2 entry point name.
pub fn normalize_snapshot(raw: &Value) -> Option<UserPreferencesSnapshot> {
    normalize_preferences_snapshot(raw)
}

pub fn normalize_preferences_snapshot_with_version(
    raw: &Value,
    fallback_version: &str,
) -> Option<UserPreferencesSnapshot> {
    if !raw.is_object() {
        return None;
    }

    let raw_ui = kv(raw, "uiConfig", "ui_config").unwrap_or(&Value::Null);
    let sanitized_ui = sanitize_ui_patch(raw_ui);
    let ui_config = apply_sanitized_patch(&UiCustomizationConfig::default(), &sanitized_ui).next;

    let background = match kv(raw, "background", "background") {
        Some(bg) if !bg.is_null() => parse_saved_background_settings(bg),
        _ => BackgroundSettings::default(),
    };

    let workspace = normalize_workspace_preferences(kv(raw, "workspace", "workspace"));

    let empty_obj = serde_json::Map::new();
    let raw_canvas = kv(raw, "canvas", "canvas").and_then(Value::as_object).unwrap_or(&empty_obj);
    let sessions = raw_canvas
        .get("sessions")
        .and_then(Value::as_array)
        .map(|arr| normalize_canvas_sessions(arr))
        .unwrap_or_default();
    let pins = raw_canvas
        .get("pins")
        .and_then(Value::as_array)
        .map(|arr| normalize_canvas_pins(arr))
        .unwrap_or_default();
    let dismissed_ids = raw_canvas
        .get("dismissedIds").or_else(|| raw_canvas.get("dismissed_ids"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .take(200)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let captured_at = finite_i64(kv(raw, "capturedAt", "captured_at")).unwrap_or_else(now_ms);
    let app_version = trimmed_str(kv(raw, "appVersion", "app_version"))
        .map(|s| s.chars().take(48).collect())
        .unwrap_or_else(|| fallback_version.to_string());
    let source_origin = trimmed_str(kv(raw, "sourceOrigin", "source_origin"))
        .map(|s| s.chars().take(200).collect());
    let snapshot_id = trimmed_str(kv(raw, "snapshotId", "snapshot_id"))
        .map(|s| s.chars().take(96).collect());
    let reason = kv(raw, "reason", "reason")
        .and_then(Value::as_str)
        .and_then(SnapshotReason::from_id)
        .unwrap_or(SnapshotReason::Sync);
    let theme_epoch = kv(raw, "themeEpoch", "theme_epoch")
        .and_then(Value::as_str)
        .filter(|e| *e == THEME_EPOCH_SLATE_V1)
        .map(str::to_string);

    // Legacy untouched-emerald auto-migration (spec §6.2): only pristine
    // pre-epoch sync snapshots upgrade; customized palettes are preserved.
    let is_legacy_untouched_emerald = theme_epoch.is_none()
        && reason == SnapshotReason::Sync
        && ui_config.accent_preset == AccentPreset::Emerald
        && ui_config.accent_hex == "#34D399"
        && ui_config.surface_hex == "#0A1613"
        && ui_config.text_hex == "#ECF3F0"
        && ui_config.panel_alpha == 0.76
        && ui_config.border_opacity == 0.42
        && ui_config.glow_intensity == 1.0
        && ui_config.portrait_tint_hex.is_none();

    let ui_config = if is_legacy_untouched_emerald {
        let d = UiCustomizationConfig::default();
        let mut migrated = ui_config;
        migrated.accent_preset = d.accent_preset;
        migrated.accent_hex = d.accent_hex;
        migrated.accent_soft_hex = d.accent_soft_hex;
        migrated.accent_bright_hex = d.accent_bright_hex;
        migrated.surface_hex = d.surface_hex;
        migrated.text_hex = d.text_hex;
        migrated.panel_alpha = d.panel_alpha;
        migrated.border_opacity = d.border_opacity;
        migrated.glow_intensity = d.glow_intensity;
        migrated.glass_opacity = d.panel_alpha;
        migrated.preset_name = d.preset_name;
        migrated
    } else {
        ui_config
    };

    Some(UserPreferencesSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        theme_epoch,
        snapshot_id,
        captured_at,
        app_version,
        source_origin,
        reason,
        ui_config,
        background,
        workspace,
        canvas: CanvasPreferences { sessions, pins, dismissed_ids },
    })
}

// ---------------------------------------------------------------------------
// Customization detection & boot hydration
// ---------------------------------------------------------------------------

pub fn is_ui_config_default(config: &UiCustomizationConfig) -> bool {
    // Whole-struct equality: the web key set plus desktop-only legacy fields
    // (which always hold defaults for web-imported snapshots).
    config == &UiCustomizationConfig::default()
}

pub fn is_background_default(bg: &BackgroundSettings) -> bool {
    bg.active_id == "aurora-drift"
        && bg.opacity == 0.64
        && bg.blur_px == 12.0
        && bg.custom_slots.is_empty()
}

pub fn has_customized_preferences(snapshot: Option<&UserPreferencesSnapshot>) -> bool {
    let Some(snapshot) = snapshot else { return false };
    if !is_ui_config_default(&snapshot.ui_config) {
        return true;
    }
    if !is_background_default(&snapshot.background) {
        return true;
    }
    let d = WorkspacePreferences::default();
    let w = &snapshot.workspace;
    if w.show_thinking != d.show_thinking
        || w.voice_muted != d.voice_muted
        || w.mood != d.mood
        || w.draft_profile != d.draft_profile
        || w.onboarding_completed != d.onboarding_completed
        || w.transport_mode.map(|t| t != SnapshotTransport::Auto).unwrap_or(false)
    {
        return true;
    }
    if !snapshot.canvas.sessions.is_empty()
        || !snapshot.canvas.pins.is_empty()
        || !snapshot.canvas.dismissed_ids.is_empty()
    {
        return true;
    }
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootHydrationSource {
    Local,
    Vault,
}

impl BootHydrationSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Vault => "vault",
        }
    }
}

/// Port of `resolveBootHydrationSource` — decides whether boot hydrates from
/// the durable vault instead of local defaults.
pub fn resolve_boot_hydration_source(
    local_snapshot: &UserPreferencesSnapshot,
    vault_snapshot: Option<&UserPreferencesSnapshot>,
) -> BootHydrationSource {
    let Some(vault) = vault_snapshot else {
        return BootHydrationSource::Local;
    };
    let local_custom = has_customized_preferences(Some(local_snapshot));
    let vault_custom = has_customized_preferences(Some(vault));

    if !local_custom && vault_custom {
        return BootHydrationSource::Vault;
    }
    if (vault.reason == SnapshotReason::PreUpdate || vault.reason == SnapshotReason::Restore)
        && vault.captured_at >= local_snapshot.captured_at
    {
        return BootHydrationSource::Vault;
    }
    if local_snapshot.captured_at <= 0 && vault.captured_at > 0 {
        return BootHydrationSource::Vault;
    }
    BootHydrationSource::Local
}

// ---------------------------------------------------------------------------
// Preserved-category & backup summaries
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreservedCategoryId {
    Theme,
    Backdrop,
    Layout,
    Stage,
    Canvas,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PreservedCategorySummary {
    pub id: PreservedCategoryId,
    pub label: String,
    pub detail: String,
    pub customized: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceBackupSummary {
    pub id: String,
    pub created_at: i64,
    pub reason: String,
    pub app_version: String,
    pub accent_hex: String,
    pub accent_preset: String,
    pub background_id: String,
    pub custom_slot_count: usize,
    pub workspace_title: String,
}

fn density_str(d: crate::types::DensityMode) -> &'static str {
    match d {
        crate::types::DensityMode::Compact => "compact",
        crate::types::DensityMode::Comfortable => "comfortable",
        crate::types::DensityMode::Airy => "airy",
    }
}

fn sidebar_str(s: crate::types::SidebarPosition) -> &'static str {
    match s {
        crate::types::SidebarPosition::Left => "left",
        crate::types::SidebarPosition::Right => "right",
    }
}

fn dot_str(d: crate::types::DotProfile) -> &'static str {
    match d {
        crate::types::DotProfile::Crisp => "crisp",
        crate::types::DotProfile::Balanced => "balanced",
        crate::types::DotProfile::Phosphor => "phosphor",
    }
}

/// Port of `summarizePreservedCategories` — the five pre-update preservation
/// categories with the exact TS detail-string formats.
pub fn summarize_preserved_categories(snapshot: &UserPreferencesSnapshot) -> Vec<PreservedCategorySummary> {
    let ui = &snapshot.ui_config;
    let bg = &snapshot.background;
    let ws = &snapshot.workspace;
    let canvas = &snapshot.canvas;
    let d = UiCustomizationConfig::default();
    let wd = WorkspacePreferences::default();

    let theme_customized = ui.accent_preset != d.accent_preset
        || ui.accent_hex != d.accent_hex
        || ui.surface_hex != d.surface_hex
        || ui.panel_alpha != d.panel_alpha
        || ui.border_opacity != d.border_opacity
        || ui.glow_intensity != d.glow_intensity;

    let bg_customized = !is_background_default(bg);

    let layout_customized = ui.sidebar_visible != d.sidebar_visible
        || ui.sidebar_position != d.sidebar_position
        || ui.sidebar_width != d.sidebar_width
        || ui.corner_radius != d.corner_radius
        || ui.density != d.density
        || ui.font_preset != d.font_preset
        || ui.font_scale != d.font_scale
        || ui.show_top_nav != d.show_top_nav
        || ui.composer_style != d.composer_style
        || ui.workspace_title != d.workspace_title;

    let stage_customized = ui.portrait_visible != d.portrait_visible
        || ui.portrait_scale != d.portrait_scale
        || ui.portrait_tint_hex != d.portrait_tint_hex
        || ui.dot_profile != d.dot_profile
        || ui.horizon_visible != d.horizon_visible
        || ui.horizon_mode != d.horizon_mode
        || ui.reduced_motion != d.reduced_motion
        || ws.show_thinking != wd.show_thinking
        || ws.voice_muted != wd.voice_muted
        || ws.mood != wd.mood;

    let backdrop_detail = if !bg.custom_slots.is_empty() {
        format!("{} · {} custom", bg.active_id, bg.custom_slots.len())
    } else {
        format!("{} · {}%", bg.active_id, (bg.opacity as f64 * 100.0).round() as i64)
    };

    vec![
        PreservedCategorySummary {
            id: PreservedCategoryId::Theme,
            label: "Theme & Colors".to_string(),
            detail: format!(
                "{} · {} · {}% glass",
                ui.accent_preset.as_str(),
                ui.accent_hex,
                (ui.panel_alpha as f64 * 100.0).round() as i64
            ),
            customized: theme_customized,
        },
        PreservedCategorySummary {
            id: PreservedCategoryId::Backdrop,
            label: "Backdrop & Media".to_string(),
            detail: backdrop_detail,
            customized: bg_customized,
        },
        PreservedCategorySummary {
            id: PreservedCategoryId::Layout,
            label: "Layout & Typography".to_string(),
            detail: format!(
                "{} rail · {} · {}",
                sidebar_str(ui.sidebar_position),
                density_str(ui.density),
                ui.font_preset.as_str()
            ),
            customized: layout_customized,
        },
        PreservedCategorySummary {
            id: PreservedCategoryId::Stage,
            label: "Stage, Voice & Trace".to_string(),
            detail: format!(
                "{} braille · {} · {}",
                dot_str(ui.dot_profile),
                if ws.show_thinking { "trace on" } else { "clean" },
                if ws.voice_muted { "muted" } else { "audio on" }
            ),
            customized: stage_customized,
        },
        PreservedCategorySummary {
            id: PreservedCategoryId::Canvas,
            label: "Canvas & Tour State".to_string(),
            detail: format!(
                "{} dots · {} pins · {}",
                canvas.sessions.len(),
                canvas.pins.len(),
                if ws.onboarding_completed { "tour done" } else { "tour pending" }
            ),
            customized: !canvas.sessions.is_empty()
                || !canvas.pins.is_empty()
                || ws.onboarding_completed,
        },
    ]
}

/// Port of `summarizeBackupRecord`.
pub fn summarize_backup_record(id: &str, snapshot: &UserPreferencesSnapshot) -> PreferenceBackupSummary {
    PreferenceBackupSummary {
        id: id.to_string(),
        created_at: snapshot.captured_at,
        reason: snapshot.reason.as_str().to_string(),
        app_version: snapshot.app_version.clone(),
        accent_hex: snapshot.ui_config.accent_hex.clone(),
        accent_preset: snapshot.ui_config.accent_preset.as_str().to_string(),
        background_id: snapshot.background.active_id.clone(),
        custom_slot_count: snapshot.background.custom_slots.len(),
        workspace_title: snapshot.ui_config.workspace_title.clone(),
    }
}

// ---------------------------------------------------------------------------
// Semver comparison & JSON import/export
// ---------------------------------------------------------------------------

/// Port of `compareSemver`: -1 when current < latest, 0 equal, 1 greater.
/// Tolerates `v` prefixes and prerelease suffixes; garbage parts become 0.
pub fn compare_semver(current_raw: &str, latest_raw: &str) -> i8 {
    fn parse(v: &str) -> Vec<u64> {
        let trimmed = v.trim();
        let stripped = trimmed.strip_prefix('v').or_else(|| trimmed.strip_prefix('V')).unwrap_or(trimmed);
        let core = stripped.split('-').next().unwrap_or("");
        core.split('.')
            .take(3)
            .map(|part| {
                let p = part.trim();
                let (neg, digits) = match p.strip_prefix('-') {
                    Some(rest) => (true, rest),
                    None => (false, p.strip_prefix('+').unwrap_or(p)),
                };
                let digit_str: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
                if digit_str.is_empty() || neg {
                    0
                } else {
                    digit_str.parse::<u64>().unwrap_or(0)
                }
            })
            .collect()
    }
    let a = parse(current_raw);
    let b = parse(latest_raw);
    for i in 0..3 {
        let av = a.get(i).copied().unwrap_or(0);
        let bv = b.get(i).copied().unwrap_or(0);
        if av < bv {
            return -1;
        }
        if av > bv {
            return 1;
        }
    }
    0
}

/// Port of `parseImportedPreferencesJson` — normalize, then stamp a fresh
/// `capturedAt` and `reason: 'import'`. Malformed payloads return `None`.
pub fn parse_imported_preferences_json(raw_text: &str) -> Option<UserPreferencesSnapshot> {
    let parsed: Value = serde_json::from_str(raw_text).ok()?;
    let mut normalized = normalize_preferences_snapshot(&parsed)?;
    normalized.captured_at = now_ms();
    normalized.reason = SnapshotReason::Import;
    Some(normalized)
}

/// Export a snapshot as compact JSON (web `JSON.stringify` parity).
pub fn export_preferences_json(snapshot: &UserPreferencesSnapshot) -> Result<String, serde_json::Error> {
    serde_json::to_string(snapshot)
}

/// Export a snapshot as pretty-printed JSON for human-readable backup files.
pub fn export_preferences_json_pretty(snapshot: &UserPreferencesSnapshot) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_config::apply_ui_patch;
    use serde_json::json;

    fn make_base_snapshot() -> UserPreferencesSnapshot {
        UserPreferencesSnapshot {
            schema_version: 1,
            theme_epoch: None,
            snapshot_id: None,
            captured_at: 1_700_000_000_000,
            app_version: "0.1.0".to_string(),
            source_origin: None,
            reason: SnapshotReason::Sync,
            ui_config: UiCustomizationConfig::default(),
            background: BackgroundSettings::default(),
            workspace: WorkspacePreferences::default(),
            canvas: CanvasPreferences::default(),
        }
    }

    fn themed(preset_id: &str) -> UiCustomizationConfig {
        apply_ui_patch(&UiCustomizationConfig::default(), &json!({ "presetId": preset_id })).next
    }

    // ---- Mirror: settingsSnapshot.test.ts #1 ----
    #[test]
    fn normalize_sanitizes_custom_themes_uploaded_backdrops_and_workspace_prefs() {
        let normalized = normalize_preferences_snapshot(&json!({
            "schemaVersion": 1,
            "capturedAt": 1_750_000_000_000i64,
            "appVersion": "0.1.0",
            "reason": "pre-update",
            "uiConfig": {
                "accentPreset": "violet",
                "accentHex": "#a855f7",
                "workspaceTitle": "My Hermes Rig",
                "sidebarPosition": "right",
                "panelAlpha": 0.88
            },
            "background": {
                "activeId": "custom-1",
                "opacity": 0.52,
                "blurPx": 16,
                "customSlots": [
                    {
                        "id": "custom-1",
                        "label": "Sunset Coast",
                        "mediaType": "image",
                        "src": "data:image/png;base64,iVBORw0KGgo=",
                        "createdAt": 1_750_000_000_000i64
                    },
                    {
                        "id": "bad-slot",
                        "label": "Bad",
                        "mediaType": "image",
                        "src": "javascript:alert(1)"
                    }
                ]
            },
            "workspace": {
                "showThinking": false,
                "voiceMuted": true,
                "mood": "focused",
                "draftProfile": "agentis",
                "onboardingCompleted": true
            },
            "canvas": {
                "sessions": [],
                "pins": [{ "id": "pin-1", "label": "Ops", "kind": "label", "xPct": 40, "yPct": 50 }],
                "dismissedIds": ["old-chan"]
            }
        }))
        .expect("normalized snapshot");

        assert_eq!(normalized.reason, SnapshotReason::PreUpdate);
        assert_eq!(normalized.ui_config.accent_preset, AccentPreset::Violet);
        assert_eq!(normalized.ui_config.accent_hex, "#A855F7");
        assert_eq!(normalized.ui_config.workspace_title, "My Hermes Rig");
        assert_eq!(normalized.ui_config.sidebar_position, crate::types::SidebarPosition::Right);
        assert_eq!(normalized.background.active_id, "custom-1");
        assert_eq!(normalized.background.custom_slots.len(), 1);
        assert_eq!(normalized.background.custom_slots[0].label, "Sunset Coast");
        assert!(!normalized.workspace.show_thinking);
        assert!(normalized.workspace.voice_muted);
        assert_eq!(normalized.workspace.mood, SnapshotMood::Focused);
        assert_eq!(normalized.workspace.draft_profile, "agentis");
        assert!(normalized.workspace.onboarding_completed);
        assert_eq!(normalized.canvas.pins.len(), 1);
        assert_eq!(normalized.canvas.dismissed_ids, vec!["old-chan".to_string()]);
    }

    // ---- Mirror: settingsSnapshot.test.ts #2 ----
    #[test]
    fn has_customized_preferences_distinguishes_pristine_defaults_from_user_settings() {
        let pristine = make_base_snapshot();
        assert!(!has_customized_preferences(Some(&pristine)));

        let mut themed_snap = make_base_snapshot();
        themed_snap.ui_config = themed("solaris-amber");
        assert!(has_customized_preferences(Some(&themed_snap)));

        let mut custom_bg = make_base_snapshot();
        custom_bg.background = BackgroundSettings {
            active_id: "custom-sunset".to_string(),
            custom_slots: vec![BackgroundCustomSlot {
                id: "custom-sunset".to_string(),
                label: "Sunset".to_string(),
                media_type: BackgroundMediaType::Image,
                src: "https://example.com/bg.jpg".to_string(),
                created_at: 1000,
            }],
            ..BackgroundSettings::default()
        };
        assert!(has_customized_preferences(Some(&custom_bg)));

        let mut completed_tour = make_base_snapshot();
        completed_tour.workspace.onboarding_completed = true;
        assert!(has_customized_preferences(Some(&completed_tour)));

        assert!(!has_customized_preferences(None));
    }

    // ---- Mirror: settingsSnapshot.test.ts #3 ----
    #[test]
    fn resolve_boot_hydration_source_restores_vault_without_clobbering_local_edits() {
        let mut fresh_local = make_base_snapshot();
        fresh_local.captured_at = 0;

        let mut customized_vault = make_base_snapshot();
        customized_vault.captured_at = 1_750_000_000_000;
        customized_vault.ui_config = themed("cyber-violet");
        customized_vault.workspace.onboarding_completed = true;

        // Fresh origin / cleared storage → vault wins.
        assert_eq!(
            resolve_boot_hydration_source(&fresh_local, Some(&customized_vault)),
            BootHydrationSource::Vault
        );

        // Pre-update / restore snapshot wins when equal or newer than local.
        let mut older_local = make_base_snapshot();
        older_local.captured_at = 1_740_000_000_000;
        older_local.ui_config = apply_ui_patch(
            &UiCustomizationConfig::default(),
            &json!({ "accentPreset": "cyan" }),
        ).next;
        let mut pre_update_vault = make_base_snapshot();
        pre_update_vault.captured_at = 1_750_000_000_000;
        pre_update_vault.reason = SnapshotReason::PreUpdate;
        pre_update_vault.ui_config = themed("cyber-violet");
        assert_eq!(
            resolve_boot_hydration_source(&older_local, Some(&pre_update_vault)),
            BootHydrationSource::Vault
        );

        // Live newer local edits stay authoritative over older regular sync.
        let mut newer_local = make_base_snapshot();
        newer_local.captured_at = 1_760_000_000_000;
        newer_local.ui_config = themed("solaris-amber");
        assert_eq!(
            resolve_boot_hydration_source(&newer_local, Some(&customized_vault)),
            BootHydrationSource::Local
        );

        // No vault → local.
        assert_eq!(
            resolve_boot_hydration_source(&newer_local, None),
            BootHydrationSource::Local
        );
    }

    // ---- Mirror: settingsSnapshot.test.ts #4 ----
    #[test]
    fn compare_semver_orders_semantic_version_strings_accurately() {
        assert_eq!(compare_semver("0.1.0", "0.2.0"), -1);
        assert_eq!(compare_semver("v0.1.0", "0.1.1"), -1);
        assert_eq!(compare_semver("1.2.3", "v1.2.3"), 0);
        assert_eq!(compare_semver("1.10.0", "1.2.9"), 1);
    }

    // ---- Mirror: settingsSnapshot.test.ts #5 ----
    #[test]
    fn summarize_preserved_categories_and_backup_record_surface_customizations() {
        let mut snapshot = make_base_snapshot();
        snapshot.reason = SnapshotReason::PreUpdate;
        snapshot.ui_config = apply_ui_patch(
            &UiCustomizationConfig::default(),
            &json!({ "presetId": "cyber-violet", "sidebarPosition": "right" }),
        ).next;
        snapshot.background = BackgroundSettings {
            active_id: "custom-photo".to_string(),
            opacity: 0.5,
            blur_px: 8.0,
            custom_slots: vec![BackgroundCustomSlot {
                id: "custom-photo".to_string(),
                label: "Beach".to_string(),
                media_type: BackgroundMediaType::Image,
                src: "https://example.com/beach.jpg".to_string(),
                created_at: 1000,
            }],
            ..BackgroundSettings::default()
        };
        snapshot.workspace.onboarding_completed = true;

        let categories = summarize_preserved_categories(&snapshot);
        assert_eq!(categories.len(), 5);
        let find = |id: PreservedCategoryId| categories.iter().find(|c| c.id == id).unwrap();
        assert!(find(PreservedCategoryId::Theme).customized);
        assert!(find(PreservedCategoryId::Backdrop).customized);
        assert!(find(PreservedCategoryId::Layout).customized);
        assert_eq!(find(PreservedCategoryId::Theme).detail, "violet · #A855F7 · 74% glass");
        assert_eq!(find(PreservedCategoryId::Backdrop).detail, "custom-photo · 1 custom");
        assert_eq!(find(PreservedCategoryId::Layout).detail, "right rail · comfortable · display");
        assert_eq!(find(PreservedCategoryId::Stage).detail, "phosphor braille · trace on · audio on");
        assert_eq!(find(PreservedCategoryId::Canvas).detail, "0 dots · 0 pins · tour done");

        let summary = summarize_backup_record("backup-1", &snapshot);
        assert_eq!(summary.accent_preset, "violet");
        assert_eq!(summary.custom_slot_count, 1);
        assert_eq!(summary.background_id, "custom-photo");
        assert_eq!(summary.reason, "pre-update");
        assert_eq!(summary.id, "backup-1");
    }

    // ---- Mirror: settingsSnapshot.test.ts #6 ----
    #[test]
    fn parse_imported_preferences_json_accepts_valid_backups_and_rejects_malformed() {
        assert!(parse_imported_preferences_json("not-json").is_none());
        assert!(parse_imported_preferences_json("42").is_none());

        let mut base = make_base_snapshot();
        base.ui_config = apply_ui_patch(
            &UiCustomizationConfig::default(),
            &json!({ "accentHex": "#38BDF8", "cornerRadius": "sharp" }),
        ).next;
        let raw = export_preferences_json(&base).unwrap();
        let parsed = parse_imported_preferences_json(&raw).expect("imported snapshot");
        assert_eq!(parsed.reason, SnapshotReason::Import);
        assert_eq!(parsed.ui_config.accent_hex, "#38BDF8");
        assert_eq!(parsed.ui_config.corner_radius, crate::types::CornerRadius::Sharp);
        assert!(parsed.captured_at > 0);
    }

    // ---- Mirror: settingsSnapshot.test.ts #7 ----
    #[test]
    fn normalize_upgrades_legacy_untouched_emerald_but_preserves_explicit_emerald() {
        let legacy_untouched = normalize_preferences_snapshot(&json!({
            "schemaVersion": 1,
            "capturedAt": 1_750_000_000_000i64,
            "appVersion": "0.1.0",
            "reason": "sync",
            "uiConfig": {
                "accentPreset": "emerald",
                "accentHex": "#34D399",
                "accentSoftHex": "#6EE7B7",
                "accentBrightHex": "#4ADE80",
                "surfaceHex": "#0A1613",
                "textHex": "#ECF3F0",
                "panelAlpha": 0.76,
                "borderOpacity": 0.42,
                "glowIntensity": 1,
                "sidebarPosition": "right"
            },
            "workspace": { "onboardingCompleted": true }
        }))
        .expect("legacy snapshot");
        assert_eq!(legacy_untouched.ui_config.accent_preset, AccentPreset::Slate);
        assert_eq!(legacy_untouched.ui_config.accent_hex, "#94A3B8");
        assert_eq!(legacy_untouched.ui_config.surface_hex, "#11141B");
        assert_eq!(legacy_untouched.ui_config.text_hex, "#F1F5F9");
        assert_eq!(legacy_untouched.ui_config.sidebar_position, crate::types::SidebarPosition::Right);
        assert!(legacy_untouched.workspace.onboarding_completed);
        assert!(legacy_untouched.theme_epoch.is_none());

        let explicit_emerald = normalize_preferences_snapshot(&json!({
            "schemaVersion": 1,
            "themeEpoch": "slate-v1",
            "capturedAt": 1_760_000_000_000i64,
            "appVersion": "0.1.0",
            "reason": "sync",
            "uiConfig": themed("emerald-terminal")
        }))
        .expect("explicit emerald snapshot");
        assert_eq!(explicit_emerald.ui_config.accent_preset, AccentPreset::Emerald);
        assert_eq!(explicit_emerald.ui_config.accent_hex, "#34D399");
        assert_eq!(explicit_emerald.theme_epoch.as_deref(), Some("slate-v1"));
    }

    // ---- Additional migration / hydration coverage ----
    #[test]
    fn migration_preserves_customized_emerald_palettes() {
        // Same emerald accent but a user-changed surface → NOT untouched.
        let customized = normalize_preferences_snapshot(&json!({
            "reason": "sync",
            "uiConfig": {
                "accentPreset": "emerald",
                "accentHex": "#34D399",
                "surfaceHex": "#10201A",
                "textHex": "#ECF3F0",
                "panelAlpha": 0.76,
                "borderOpacity": 0.42,
                "glowIntensity": 1
            }
        }))
        .unwrap();
        assert_eq!(customized.ui_config.accent_preset, AccentPreset::Emerald);
        assert_eq!(customized.ui_config.accent_hex, "#34D399");
        assert_eq!(customized.ui_config.surface_hex, "#10201A");

        // A portrait tint also marks the palette as customized.
        let tinted = normalize_preferences_snapshot(&json!({
            "reason": "sync",
            "uiConfig": {
                "accentPreset": "emerald",
                "accentHex": "#34D399",
                "surfaceHex": "#0A1613",
                "textHex": "#ECF3F0",
                "panelAlpha": 0.76,
                "borderOpacity": 0.42,
                "glowIntensity": 1,
                "portraitTintHex": "#6EE7B7"
            }
        }))
        .unwrap();
        assert_eq!(tinted.ui_config.accent_hex, "#34D399");
    }

    #[test]
    fn migration_requires_sync_reason_and_missing_epoch() {
        let untouched = json!({
            "accentPreset": "emerald",
            "accentHex": "#34D399",
            "surfaceHex": "#0A1613",
            "textHex": "#ECF3F0",
            "panelAlpha": 0.76,
            "borderOpacity": 0.42,
            "glowIntensity": 1
        });
        // Manual backups are explicit user actions → never auto-migrated.
        let manual = normalize_preferences_snapshot(&json!({
            "reason": "manual", "uiConfig": untouched
        })).unwrap();
        assert_eq!(manual.ui_config.accent_hex, "#34D399");
        // Already epoch-stamped → preserved even if untouched emerald.
        let stamped = normalize_preferences_snapshot(&json!({
            "themeEpoch": "slate-v1", "reason": "sync", "uiConfig": untouched
        })).unwrap();
        assert_eq!(stamped.ui_config.accent_hex, "#34D399");
    }

    #[test]
    fn normalize_rejects_non_objects_and_defaults_missing_fields() {
        assert!(normalize_preferences_snapshot(&Value::Null).is_none());
        assert!(normalize_preferences_snapshot(&json!(42)).is_none());
        assert!(normalize_preferences_snapshot(&json!("snap")).is_none());

        let minimal = normalize_preferences_snapshot(&json!({})).unwrap();
        assert_eq!(minimal.schema_version, 1);
        assert_eq!(minimal.reason, SnapshotReason::Sync);
        assert_eq!(minimal.app_version, "0.1.0");
        assert!(minimal.captured_at > 0);
        assert_eq!(minimal.ui_config, UiCustomizationConfig::default());
        assert_eq!(minimal.workspace, WorkspacePreferences::default());
        assert!(minimal.canvas.sessions.is_empty());
        assert!(is_background_default(&minimal.background));

        let versioned = normalize_preferences_snapshot_with_version(&json!({}), "9.9.9").unwrap();
        assert_eq!(versioned.app_version, "9.9.9");
    }

    #[test]
    fn export_import_roundtrip_preserves_customizations() {
        let mut snap = make_base_snapshot();
        snap.theme_epoch = Some("slate-v1".to_string());
        snap.snapshot_id = Some("snap-42".to_string());
        snap.source_origin = Some("https://localhost:3000".to_string());
        snap.reason = SnapshotReason::Manual;
        snap.ui_config = themed("sakura-rose");
        snap.workspace.draft_profile = "agentis".to_string();
        snap.workspace.transport_mode = Some(SnapshotTransport::Runs);
        snap.canvas.dismissed_ids = vec!["chan-x".to_string()];

        let exported = export_preferences_json(&snap).unwrap();
        assert!(exported.contains("\"themeEpoch\":\"slate-v1\""));
        assert!(exported.contains("\"schemaVersion\":1"));
        let reimported = parse_imported_preferences_json(&exported).unwrap();
        assert_eq!(reimported.ui_config, snap.ui_config);
        assert_eq!(reimported.workspace, snap.workspace);
        assert_eq!(reimported.canvas.dismissed_ids, snap.canvas.dismissed_ids);
        assert_eq!(reimported.theme_epoch, snap.theme_epoch);
        assert_eq!(reimported.snapshot_id, snap.snapshot_id);
        assert_eq!(reimported.reason, SnapshotReason::Import);
        assert!(reimported.captured_at >= snap.captured_at);
    }

    #[test]
    fn background_slots_are_sanitized_deduped_and_capped() {
        let mut slots = Vec::new();
        for i in 0..8 {
            slots.push(json!({
                "id": format!("custom-{i}"),
                "src": format!("https://example.com/{i}.jpg")
            }));
        }
        slots.push(json!({ "id": "custom-0", "src": "https://example.com/dup.jpg" }));
        slots.push(json!({ "id": "evil", "src": "javascript:alert(1)" }));
        slots.push(json!({ "id": "no-src" }));
        let bg = parse_saved_background_settings(&json!({
            "activeId": "custom-2",
            "opacity": 5,
            "blurPx": -3,
            "customSlots": slots
        }));
        assert_eq!(bg.custom_slots.len(), 6);
        assert_eq!(bg.active_id, "custom-2");
        assert_eq!(bg.opacity, 0.85);
        assert_eq!(bg.blur_px, 0.0);
        assert_eq!(bg.custom_slots[0].label, "Custom Image");
        assert_eq!(bg.custom_slots[0].media_type, BackgroundMediaType::Image);

        // Video inference + invalid active id falls back to aurora-drift.
        let bg2 = parse_saved_background_settings(&json!({
            "activeId": "not-a-preset",
            "customSlots": [{ "id": "clip", "src": "https://example.com/loop.mp4?v=2" }]
        }));
        assert_eq!(bg2.active_id, "aurora-drift");
        assert_eq!(bg2.custom_slots[0].media_type, BackgroundMediaType::Video);
        assert_eq!(bg2.custom_slots[0].label, "Custom Video");

        // Active id pointing at an uploaded slot is valid.
        let bg3 = parse_saved_background_settings(&json!({
            "activeId": "clip",
            "customSlots": [{ "id": "clip", "src": "data:image/png;base64,AAA" }]
        }));
        assert_eq!(bg3.active_id, "clip");

        // Legacy scaffold shape crosswalks the preset enum.
        let legacy = parse_saved_background_settings(&json!({
            "preset": "phosphor_grain", "opacity": 0.5, "motionSpeed": 1.4, "intensity": 0.9
        }));
        assert_eq!(legacy.active_id, "braille-rain");
        assert_eq!(legacy.preset, BackgroundPresetId::PhosphorGrain);
        assert_eq!(legacy.motion_speed, 1.4);
    }

    #[test]
    fn canvas_pins_normalize_ids_kinds_positions_and_defaults() {
        let pins = normalize_canvas_pins(&[
            json!({ "id": "pin-1", "kind": "label", "xPct": 40, "yPct": 50 }),
            json!({ "id": "pin-1", "kind": "brief" }),
            json!({ "kind": "repo", "xPct": 500, "yPct": -20, "title": "  Repo pin  ", "linkedChannelId": " ops " }),
            json!(null),
        ]);
        assert_eq!(pins.len(), 2);
        assert_eq!(pins[0].kind, CanvasPinKind::Note);
        assert_eq!(pins[0].title, "Workspace Note");
        assert_eq!(pins[0].x_pct, 40.0);
        assert_eq!(pins[1].id, "pin-3");
        assert_eq!(pins[1].kind, CanvasPinKind::Repo);
        assert_eq!(pins[1].x_pct, 90.0);
        assert_eq!(pins[1].y_pct, 14.0);
        assert_eq!(pins[1].title, "Repo pin");
        assert_eq!(pins[1].linked_channel_id.as_deref(), Some("ops"));
    }

    #[test]
    fn canvas_sessions_normalize_channels_slugs_and_autonomy_defaults() {
        let sessions = normalize_canvas_sessions(&[
            json!({ "name": "#Ops Room", "xPct": 500, "yPct": 2 }),
            json!({ "channelId": "ops-room", "name": "Duplicate" }),
            json!({ "channelId": "general", "hermesSessionId": "duet-legacy", "mood": "bogus" }),
            json!(7),
        ]);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].name, "Ops Room");
        assert_eq!(sessions[0].channel_id, "ops-room");
        assert_eq!(sessions[0].id, "session-ops-room");
        assert_eq!(sessions[0].x_pct, 90.0);
        assert_eq!(sessions[0].y_pct, 18.0);
        assert_eq!(sessions[0].shape, "clover");
        assert_eq!(sessions[0].autonomy_mode, "manual");
        assert_eq!(sessions[1].channel_id, "general");
        assert_eq!(sessions[1].hermes_session_id, "hermes-general");
        assert_eq!(sessions[1].mood, "happy");
        assert_eq!(sessions[1].skills, vec!["browser", "files", "research"]);
        assert_eq!(
            sessions[1].connected_to,
            Some(vec!["ops".to_string(), "research".to_string()])
        );
        assert_eq!(sessions[1].workspace_dir, "~/hermes/dots/general");
    }

    #[test]
    fn dismissed_ids_filter_and_cap_at_200() {
        let mut ids: Vec<Value> = (0..250).map(|i| json!(format!("chan-{i}"))).collect();
        ids.push(json!("   "));
        ids.push(json!(42));
        let snap = normalize_preferences_snapshot(&json!({
            "canvas": { "dismissedIds": ids }
        }))
        .unwrap();
        assert_eq!(snap.canvas.dismissed_ids.len(), 200);
        assert_eq!(snap.canvas.dismissed_ids[0], "chan-0");
    }

    #[test]
    fn compare_semver_handles_prefixes_prerelease_and_garbage() {
        assert_eq!(compare_semver("1.2.3-beta.1", "1.2.3"), 0);
        assert_eq!(compare_semver("v2", "1.9.9"), 1);
        assert_eq!(compare_semver("garbage", "0.0.0"), 0);
        assert_eq!(compare_semver("1.2", "1.2.0"), 0);
        assert_eq!(compare_semver("1.2.3.4", "1.2.3"), 0);
        assert_eq!(compare_semver("0.0.1", "0.0.2"), -1);
    }

    #[test]
    fn hydration_prefers_vault_when_local_timestamp_is_zero() {
        let mut local = make_base_snapshot();
        local.captured_at = 0;
        let mut vault = make_base_snapshot();
        vault.captured_at = 10;
        // Both pristine: reason sync, but local timestamp <= 0 → vault.
        assert_eq!(
            resolve_boot_hydration_source(&local, Some(&vault)),
            BootHydrationSource::Vault
        );
        // Restore reason with equal timestamps also wins.
        let mut local2 = make_base_snapshot();
        local2.captured_at = 500;
        local2.ui_config = themed("zen-mono");
        let mut vault2 = make_base_snapshot();
        vault2.captured_at = 500;
        vault2.reason = SnapshotReason::Restore;
        assert_eq!(
            resolve_boot_hydration_source(&local2, Some(&vault2)),
            BootHydrationSource::Vault
        );
    }

    #[test]
    fn workspace_transport_and_mood_normalization() {
        let ws = normalize_workspace_preferences(Some(&json!({
            "mood": "speaking",
            "draftProfile": "  agentis  ",
            "transportMode": "session_stream",
            "showThinking": "yes"
        })));
        // 'speaking' is not in the snapshot mood subset → default happy.
        assert_eq!(ws.mood, SnapshotMood::Happy);
        assert_eq!(ws.draft_profile, "agentis");
        assert_eq!(ws.transport_mode, Some(SnapshotTransport::SessionStream));
        assert!(ws.show_thinking); // non-boolean falls back to default true

        let mut snap = make_base_snapshot();
        snap.workspace.transport_mode = Some(SnapshotTransport::Auto);
        assert!(!has_customized_preferences(Some(&snap)));
        snap.workspace.transport_mode = Some(SnapshotTransport::Runs);
        assert!(has_customized_preferences(Some(&snap)));
    }

    #[test]
    fn pristine_snapshot_summary_details_match_default_formats() {
        let snap = make_base_snapshot();
        let cats = summarize_preserved_categories(&snap);
        assert!(cats.iter().all(|c| !c.customized));
        assert_eq!(cats[0].detail, "slate · #94A3B8 · 80% glass");
        assert_eq!(cats[1].detail, "aurora-drift · 64%");
        assert_eq!(cats[2].detail, "left rail · comfortable · jakarta");
        assert_eq!(cats[3].detail, "balanced braille · trace on · audio on");
        assert_eq!(cats[4].detail, "0 dots · 0 pins · tour pending");
        let summary = summarize_backup_record("manual-1", &snap);
        assert_eq!(summary.reason, "sync");
        assert_eq!(summary.workspace_title, "Hermes");
        assert_eq!(summary.accent_preset, "slate");
    }
}
