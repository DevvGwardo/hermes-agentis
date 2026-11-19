//! Port of `src/utils/canvasSessions.ts` + `src/utils/homeSession.ts` (P1-06).
//!
//! Spatial dot persistence (`duet.canvasSessions.v2`), Constellation/Orbit/Grid
//! formations, canvas pins, viewport clamp/zoom, slugging, and Home session
//! derivation. The legacy drafted pixel-space API (`CanvasDotData`,
//! `resolve_formation_positions`, ...) is kept for voxcell-ui/voxcell-halftone
//! compatibility; the TS-parity percentage-space model below is authoritative.

use crate::sessions::{
    classify_gateway_session_origin, classify_session_bucket, format_display_session_title,
    format_source_label, is_opaque_session_identifier, GatewaySessionInfo, SessionBucketParams,
};
use crate::types::{ChannelMeta, SessionOrigin, SessionStatus};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;

// ---------------------------------------------------------------------------
// Legacy drafted API (kept for cross-crate compatibility)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CanvasFormation {
    #[default]
    Constellation,
    Orbit,
    Grid,
}

impl CanvasFormation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Constellation => "Constellation",
            Self::Orbit => "Orbit",
            Self::Grid => "Grid",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CanvasShapeKind {
    #[default]
    HermesOrb,
    CatBot,
    AngelBot,
    CyberSkull,
    LotusCore,
    GhostShell,
    FoxSpirit,
    MechaHelm,
    PrismCore,
}

impl CanvasShapeKind {
    pub fn all() -> &'static [Self] {
        &[
            Self::HermesOrb,
            Self::CatBot,
            Self::AngelBot,
            Self::CyberSkull,
            Self::LotusCore,
            Self::GhostShell,
            Self::FoxSpirit,
            Self::MechaHelm,
            Self::PrismCore,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::HermesOrb => "Hermes",
            Self::CatBot => "Neko",
            Self::AngelBot => "Seraph",
            Self::CyberSkull => "Skull",
            Self::LotusCore => "Lotus",
            Self::GhostShell => "Phantom",
            Self::FoxSpirit => "Kitsune",
            Self::MechaHelm => "Mecha",
            Self::PrismCore => "Prism",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CanvasDotData {
    pub id: String,
    pub channel_id: String,
    pub label: String,
    pub shape: CanvasShapeKind,
    pub x: f32,
    pub y: f32,
    pub pinned: bool,
    pub color_hex: String,
}

pub fn slugify_session_id(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "session".to_string()
    } else {
        trimmed.chars().take(40).collect()
    }
}

pub fn create_home_session_channel(prompt: &str, counter: usize, now_ms: u64) -> ChannelMeta {
    let slug = slugify_session_id(prompt);
    let short_name: String = prompt.split_whitespace().take(4).collect::<Vec<_>>().join(" ");
    let name = if short_name.is_empty() {
        format!("Session {counter}")
    } else {
        short_name
    };
    let id = format!("local-{slug}-{counter}");
    ChannelMeta {
        id: id.clone(),
        name,
        category: "workspace".to_string(),
        description: prompt.trim().to_string(),
        session_id: id,
        unread: 0,
        origin: SessionOrigin::Workspace,
        has_local_history: true,
        model: None,
        updated_at_ms: Some(now_ms),
    }
}

pub fn resolve_formation_positions(
    count: usize,
    formation: CanvasFormation,
    width: f32,
    height: f32,
) -> Vec<(f32, f32)> {
    if count == 0 {
        return Vec::new();
    }
    let cx = width * 0.5;
    let cy = height * 0.5;
    match formation {
        CanvasFormation::Constellation => {
            let rx = (width * 0.34).max(120.0);
            let ry = (height * 0.30).max(95.0);
            (0..count)
                .map(|i| {
                    let t = i as f32 / count as f32;
                    let angle = t * TAU - std::f32::consts::FRAC_PI_2;
                    let ring_scale = if i % 2 == 0 { 1.0 } else { 0.62 };
                    (
                        cx + angle.cos() * rx * ring_scale,
                        cy + angle.sin() * ry * ring_scale,
                    )
                })
                .collect()
        }
        CanvasFormation::Orbit => {
            let base_r = (width.min(height) * 0.28).max(110.0);
            (0..count)
                .map(|i| {
                    if i == 0 && count > 2 {
                        (cx, cy)
                    } else {
                        let idx = if count > 2 { i - 1 } else { i };
                        let total = if count > 2 { count - 1 } else { count };
                        let angle = (idx as f32 / total.max(1) as f32) * TAU - std::f32::consts::FRAC_PI_2;
                        (cx + angle.cos() * base_r, cy + angle.sin() * base_r * 0.82)
                    }
                })
                .collect()
        }
        CanvasFormation::Grid => {
            let cols = (count as f32).sqrt().ceil().max(1.0) as usize;
            let rows = count.div_ceil(cols).max(1);
            let cell_w = 210.0_f32;
            let cell_h = 160.0_f32;
            let start_x = cx - ((cols as f32 - 1.0) * cell_w) * 0.5;
            let start_y = cy - ((rows as f32 - 1.0) * cell_h) * 0.5;
            (0..count)
                .map(|i| {
                    let r = i / cols;
                    let c = i % cols;
                    (start_x + c as f32 * cell_w, start_y + r as f32 * cell_h)
                })
                .collect()
        }
    }
}

pub fn apply_formation(dots: &mut [CanvasDotData], formation: CanvasFormation, width: f32, height: f32) {
    let coords = resolve_formation_positions(dots.len(), formation, width, height);
    for (dot, (x, y)) in dots.iter_mut().zip(coords) {
        dot.x = x;
        dot.y = y;
    }
}

// ---------------------------------------------------------------------------
// TS-parity canvas model (canvasSessions.ts)
// ---------------------------------------------------------------------------

pub const CANVAS_STORAGE_KEY: &str = "duet.canvasSessions.v2";
pub const CANVAS_PINS_STORAGE_KEY: &str = "duet.canvasPins.v1";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasBounds {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

pub const CANVAS_BOUNDS: CanvasBounds = CanvasBounds { min_x: 10.0, max_x: 90.0, min_y: 18.0, max_y: 74.0 };
pub const CANVAS_ZOOM_MIN: f32 = 0.65;
pub const CANVAS_ZOOM_MAX: f32 = 1.5;
pub const CANVAS_ZOOM_STEP: f32 = 0.15;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasViewport {
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
}

pub const DEFAULT_CANVAS_VIEWPORT: CanvasViewport = CanvasViewport { pan_x: 0.0, pan_y: 0.0, zoom: 1.0 };

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrailleShapeKind {
    Clover,
    Star,
    Ghost,
    Heart,
    Cloud,
    Triangle,
    Pebble,
    Trapezoid,
    Blob,
}

impl BrailleShapeKind {
    pub fn from_id(id: &str) -> Option<Self> {
        BRAILLE_SHAPE_CATALOG.iter().find(|m| m.kind.as_id() == id).map(|m| m.kind)
    }

    pub fn as_id(self) -> &'static str {
        match self {
            Self::Clover => "clover",
            Self::Star => "star",
            Self::Ghost => "ghost",
            Self::Heart => "heart",
            Self::Cloud => "cloud",
            Self::Triangle => "triangle",
            Self::Pebble => "pebble",
            Self::Trapezoid => "trapezoid",
            Self::Blob => "blob",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrailleShapeTint {
    Lime,
    Pink,
    Lavender,
    Coral,
    Orange,
    Cyan,
    Periwinkle,
    Amber,
    Magenta,
}

impl BrailleShapeTint {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "lime" => Some(Self::Lime),
            "pink" => Some(Self::Pink),
            "lavender" => Some(Self::Lavender),
            "coral" => Some(Self::Coral),
            "orange" => Some(Self::Orange),
            "cyan" => Some(Self::Cyan),
            "periwinkle" => Some(Self::Periwinkle),
            "amber" => Some(Self::Amber),
            "magenta" => Some(Self::Magenta),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CanvasMood {
    Neutral,
    Listening,
    Thinking,
    Speaking,
    Happy,
    Focused,
    Alert,
}

impl CanvasMood {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "neutral" => Some(Self::Neutral),
            "listening" => Some(Self::Listening),
            "thinking" => Some(Self::Thinking),
            "speaking" => Some(Self::Speaking),
            "happy" => Some(Self::Happy),
            "focused" => Some(Self::Focused),
            "alert" => Some(Self::Alert),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DotEyeStyle {
    Open,
    Sleepy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DotAutonomyMode {
    Manual,
    Scheduled,
    Watch,
}

impl DotAutonomyMode {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "manual" => Some(Self::Manual),
            "scheduled" => Some(Self::Scheduled),
            "watch" => Some(Self::Watch),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DotSandboxEnv {
    CloudSandbox,
    LocalRepo,
    BrowserWorker,
}

impl DotSandboxEnv {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "cloud-sandbox" => Some(Self::CloudSandbox),
            "local-repo" => Some(Self::LocalRepo),
            "browser-worker" => Some(Self::BrowserWorker),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CanvasDotOrigin {
    Workspace,
    Imported,
    Seed,
}

impl CanvasDotOrigin {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "workspace" => Some(Self::Workspace),
            "imported" => Some(Self::Imported),
            "seed" => Some(Self::Seed),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CanvasPinKind {
    Brief,
    Repo,
    Note,
    Skill,
}

impl CanvasPinKind {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "brief" => Some(Self::Brief),
            "repo" => Some(Self::Repo),
            "note" => Some(Self::Note),
            "skill" => Some(Self::Skill),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CanvasSessionFilter {
    Chats,
    All,
    Live,
    Autonomous,
}

/// Percentage-space canvas session (TS `CanvasSession`). Serialized with the
/// exact camelCase keys used by the `duet.canvasSessions.v2` localStorage vault.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasSession {
    pub id: String,
    pub channel_id: String,
    pub hermes_session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub agent_persona: String,
    pub name: String,
    pub topic: String,
    pub shape: BrailleShapeKind,
    pub tint: BrailleShapeTint,
    pub x_pct: f32,
    pub y_pct: f32,
    pub mood: CanvasMood,
    pub eye_style: DotEyeStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_core: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<CanvasDotOrigin>,
    pub autonomy_mode: DotAutonomyMode,
    pub schedule_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autonomy_paused: Option<bool>,
    pub sandbox_env: DotSandboxEnv,
    pub workspace_dir: String,
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connected_to: Option<Vec<String>>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_channel_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DotStarterTemplate {
    pub id: &'static str,
    pub label: &'static str,
    pub subtitle: &'static str,
    pub shape: BrailleShapeKind,
    pub tint: BrailleShapeTint,
    pub autonomy_mode: DotAutonomyMode,
    pub schedule_label: &'static str,
    pub sandbox_env: DotSandboxEnv,
    pub skills: &'static [&'static str],
    pub persona: &'static str,
}

pub const AVAILABLE_DOT_SKILLS: &[(&str, &str, &str)] = &[
    ("browser", "Browser", "Headless web inspection & screenshots"),
    ("shell", "Terminal", "Isolated sandbox shell execution"),
    ("files", "Workspace FS", "Read/patch project files"),
    ("mail", "Mail Gate", "Draft & dispatch gated email"),
    ("calendar", "Calendar", "Schedule triage & conflict checks"),
    ("research", "Web Research", "Multi-source citations & synthesis"),
];

pub const DOT_STARTER_TEMPLATES: &[DotStarterTemplate] = &[
    DotStarterTemplate {
        id: "custom",
        label: "Custom Companion",
        subtitle: "Interactive single-thread collaborator",
        shape: BrailleShapeKind::Ghost,
        tint: BrailleShapeTint::Pink,
        autonomy_mode: DotAutonomyMode::Manual,
        schedule_label: "On-demand",
        sandbox_env: DotSandboxEnv::CloudSandbox,
        skills: &["browser", "files"],
        persona: "Adaptive Hermes specialist balancing fast terminal execution with clear status briefs.",
    },
    DotStarterTemplate {
        id: "ops-watchdog",
        label: "Ops Watchdog",
        subtitle: "Continuous build & health monitor",
        shape: BrailleShapeKind::Trapezoid,
        tint: BrailleShapeTint::Amber,
        autonomy_mode: DotAutonomyMode::Watch,
        schedule_label: "Continuous watch",
        sandbox_env: DotSandboxEnv::LocalRepo,
        skills: &["shell", "files", "browser"],
        persona: "Proactive reliability monitor tracking CI pipelines, dev servers, and regression alerts.",
    },
    DotStarterTemplate {
        id: "research-scout",
        label: "Research Scout",
        subtitle: "Scheduled multi-source synthesis",
        shape: BrailleShapeKind::Pebble,
        tint: BrailleShapeTint::Periwinkle,
        autonomy_mode: DotAutonomyMode::Scheduled,
        schedule_label: "Every 30m",
        sandbox_env: DotSandboxEnv::BrowserWorker,
        skills: &["research", "browser", "files"],
        persona: "Methodical research scout gathering citations, diffing specs, and posting concise briefs.",
    },
    DotStarterTemplate {
        id: "release-dispatch",
        label: "Release Dispatch",
        subtitle: "Gated rollout & comms coordinator",
        shape: BrailleShapeKind::Star,
        tint: BrailleShapeTint::Lime,
        autonomy_mode: DotAutonomyMode::Scheduled,
        schedule_label: "Hourly check",
        sandbox_env: DotSandboxEnv::CloudSandbox,
        skills: &["mail", "calendar", "shell"],
        persona: "Release coordinator preparing changelog drafts, gating outbound mail, and checking windows.",
    },
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeMeta {
    pub kind: BrailleShapeKind,
    pub default_tint: BrailleShapeTint,
    pub default_eye_style: DotEyeStyle,
    pub tagline: &'static str,
    pub default_agent_persona: &'static str,
}

pub const BRAILLE_SHAPE_CATALOG: &[ShapeMeta] = &[
    ShapeMeta {
        kind: BrailleShapeKind::Clover,
        default_tint: BrailleShapeTint::Lime,
        default_eye_style: DotEyeStyle::Open,
        tagline: "Core strategist",
        default_agent_persona: "Calm primary Hermes coordinator styled after the main cyber-bob operator.",
    },
    ShapeMeta {
        kind: BrailleShapeKind::Star,
        default_tint: BrailleShapeTint::Pink,
        default_eye_style: DotEyeStyle::Open,
        tagline: "Launch runner",
        default_agent_persona: "High-energy release and save_page builder running fast launch turns.",
    },
    ShapeMeta {
        kind: BrailleShapeKind::Ghost,
        default_tint: BrailleShapeTint::Lavender,
        default_eye_style: DotEyeStyle::Open,
        tagline: "Silent daemon",
        default_agent_persona: "Stealthy background daemon monitoring cron watchers and ambient events.",
    },
    ShapeMeta {
        kind: BrailleShapeKind::Heart,
        default_tint: BrailleShapeTint::Coral,
        default_eye_style: DotEyeStyle::Open,
        tagline: "Care & triage",
        default_agent_persona: "Attentive triage agent managing approvals, dispatches, and alerts.",
    },
    ShapeMeta {
        kind: BrailleShapeKind::Cloud,
        default_tint: BrailleShapeTint::Orange,
        default_eye_style: DotEyeStyle::Open,
        tagline: "Memory synth",
        default_agent_persona: "Curious synthesis agent curating MEMORY.md and skill indexes.",
    },
    ShapeMeta {
        kind: BrailleShapeKind::Triangle,
        default_tint: BrailleShapeTint::Cyan,
        default_eye_style: DotEyeStyle::Open,
        tagline: "Code reviewer",
        default_agent_persona: "Sharp analytical reviewer auditing diffs, security rules, and tests.",
    },
    ShapeMeta {
        kind: BrailleShapeKind::Pebble,
        default_tint: BrailleShapeTint::Periwinkle,
        default_eye_style: DotEyeStyle::Sleepy,
        tagline: "Deep-work lane",
        default_agent_persona: "Quiet deep-research session focused on long-horizon synthesis.",
    },
    ShapeMeta {
        kind: BrailleShapeKind::Trapezoid,
        default_tint: BrailleShapeTint::Amber,
        default_eye_style: DotEyeStyle::Sleepy,
        tagline: "Brief keeper",
        default_agent_persona: "Steady ops & morning-brief keeper guarding cron schedules.",
    },
    ShapeMeta {
        kind: BrailleShapeKind::Blob,
        default_tint: BrailleShapeTint::Magenta,
        default_eye_style: DotEyeStyle::Open,
        tagline: "Sandbox bot",
        default_agent_persona: "Edgy experimental sandbox session with layered wolfcut styling.",
    },
];

pub fn get_shape_meta(kind: BrailleShapeKind) -> &'static ShapeMeta {
    BRAILLE_SHAPE_CATALOG
        .iter()
        .find(|m| m.kind == kind)
        .unwrap_or(&BRAILLE_SHAPE_CATALOG[0])
}

pub fn create_hermes_session_id(channel_id: &str) -> String {
    let clean = slug_base(channel_id);
    format!("hermes-{}", if clean.is_empty() { "session" } else { &clean })
}

/// lowercase + trim + collapse non-[a-z0-9-_] runs to '-' + trim dashes.
fn slug_base(raw: &str) -> String {
    let lowered = raw.trim().to_ascii_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut prev_dash = false;
    for c in lowered.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
            prev_dash = c == '-';
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn round2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

pub fn clamp_canvas_position(x_pct: f32, y_pct: f32) -> (f32, f32) {
    if !x_pct.is_finite() || !y_pct.is_finite() {
        return (f32::NAN, f32::NAN);
    }
    (
        round2(x_pct).clamp(CANVAS_BOUNDS.min_x, CANVAS_BOUNDS.max_x),
        round2(y_pct).clamp(CANVAS_BOUNDS.min_y, CANVAS_BOUNDS.max_y),
    )
}

pub fn clamp_pin_position(x_pct: f32, y_pct: f32) -> (f32, f32) {
    if !x_pct.is_finite() || !y_pct.is_finite() {
        return (f32::NAN, f32::NAN);
    }
    (round2(x_pct).clamp(10.0, 90.0), round2(y_pct).clamp(14.0, 82.0))
}

pub fn clamp_canvas_zoom(zoom: f32) -> f32 {
    if !zoom.is_finite() {
        return 1.0;
    }
    ((zoom.clamp(CANVAS_ZOOM_MIN, CANVAS_ZOOM_MAX) * 1000.0).round()) / 1000.0
}

pub fn clamp_canvas_pan(pan_x: f32, pan_y: f32, zoom: f32) -> (f32, f32) {
    let max_pan = 420.0 * 1.0_f32.max(if zoom.is_finite() { zoom } else { 1.0 });
    let clean = |v: f32| {
        let v = if v.is_finite() { v } else { 0.0 };
        ((v * 10.0).round() / 10.0).clamp(-max_pan, max_pan)
    };
    (clean(pan_x), clean(pan_y))
}

pub fn zoom_viewport_around_point(
    current: CanvasViewport,
    next_zoom_raw: f32,
    cursor_local_x: f32,
    cursor_local_y: f32,
    stage_width: f32,
    stage_height: f32,
) -> CanvasViewport {
    let next_zoom = clamp_canvas_zoom(next_zoom_raw);
    let prev_zoom = clamp_canvas_zoom(current.zoom);
    if (next_zoom - prev_zoom).abs() < 0.0001 || stage_width <= 0.0 || stage_height <= 0.0 {
        return CanvasViewport { pan_x: current.pan_x, pan_y: current.pan_y, zoom: next_zoom };
    }
    let cx = cursor_local_x - stage_width * 0.5;
    let cy = cursor_local_y - stage_height * 0.5;
    let ratio = next_zoom / prev_zoom;
    let raw_pan_x = cx - (cx - current.pan_x) * ratio;
    let raw_pan_y = cy - (cy - current.pan_y) * ratio;
    let (pan_x, pan_y) = clamp_canvas_pan(raw_pan_x, raw_pan_y, next_zoom);
    CanvasViewport { pan_x, pan_y, zoom: next_zoom }
}

pub fn format_sandbox_label(env: DotSandboxEnv) -> &'static str {
    match env {
        DotSandboxEnv::LocalRepo => "Local Repo",
        DotSandboxEnv::BrowserWorker => "Browser Worker",
        DotSandboxEnv::CloudSandbox => "Cloud Sandbox",
    }
}

pub fn format_autonomy_label(mode: DotAutonomyMode, schedule_label: &str, paused: bool) -> String {
    if mode == DotAutonomyMode::Manual {
        return "On-demand".to_string();
    }
    let base = if !schedule_label.is_empty() {
        schedule_label.to_string()
    } else if mode == DotAutonomyMode::Watch {
        "Continuous watch".to_string()
    } else {
        "Scheduled".to_string()
    };
    if paused {
        format!("{base} (paused)")
    } else {
        base
    }
}

pub fn is_dot_autonomous(session: &CanvasSession) -> bool {
    session.autonomy_mode != DotAutonomyMode::Manual && session.autonomy_paused != Some(true)
}

fn session_title_info(session: &CanvasSession, hermes: Option<&GatewaySessionInfo>) -> (String, bool, String) {
    let source_label = format_source_label(
        hermes.and_then(|h| h.source.as_deref()),
        Some(&session.hermes_session_id),
        Some(&session.channel_id),
    );
    let topic = hermes
        .map(|h| h.topic.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| session.topic.clone());
    let name = hermes
        .map(|h| h.name.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| session.name.clone());
    let (title, is_opaque) = format_display_session_title(
        Some(&topic),
        Some(&name),
        Some(&session.channel_id),
        Some(&session.hermes_session_id),
        hermes.and_then(|h| h.last_message.as_deref()),
        Some(&source_label),
    );
    (title, is_opaque, source_label)
}

pub fn is_canvas_background_run(session: &CanvasSession, hermes: Option<&GatewaySessionInfo>) -> bool {
    if session.is_core == Some(true) {
        return false;
    }
    if session.autonomy_mode != DotAutonomyMode::Manual {
        return false;
    }
    let (_, is_opaque, _) = session_title_info(session, hermes);
    let turn_count = hermes.map(|h| h.turn_count).unwrap_or(0);
    let has_active_work = hermes.is_some_and(|h| {
        matches!(h.status, SessionStatus::Thinking | SessionStatus::Running | SessionStatus::Speaking)
    });
    classify_session_bucket(&SessionBucketParams {
        session_id: Some(&session.hermes_session_id),
        channel_id: Some(&session.channel_id),
        source: hermes.and_then(|h| h.source.as_deref()),
        is_opaque_title: is_opaque,
        turn_count,
        has_active_work,
    }) == crate::sessions::SessionBucket::Runs
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanvasDotLabel {
    pub primary_label: String,
    pub secondary_label: String,
    pub prompt_target: String,
    pub is_opaque: bool,
}

pub fn format_canvas_dot_label(
    session: &CanvasSession,
    hermes: Option<&GatewaySessionInfo>,
    turn_count: usize,
) -> CanvasDotLabel {
    let (title, is_opaque, source_label) = session_title_info(session, hermes);

    let title_chars: Vec<char> = title.chars().collect();
    let compact_primary = if title_chars.len() > 24 {
        format!("{}…", title_chars.iter().take(22).collect::<String>().trim_end())
    } else {
        title.clone()
    };
    let is_slug_like = !compact_primary.is_empty()
        && compact_primary
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    let channel_chars: Vec<char> = session.channel_id.chars().collect();
    let short_prompt_base = if is_opaque {
        title
            .split('·')
            .last()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| channel_chars.iter().take(8).collect())
    } else if is_slug_like && channel_chars.len() <= 16 {
        session.channel_id.clone()
    } else if compact_primary.contains('/') {
        compact_primary
            .split('/')
            .next()
            .unwrap_or("")
            .chars()
            .take(14)
            .collect()
    } else {
        compact_primary.chars().take(14).collect::<String>().trim_end().to_string()
    };

    let mut parts: Vec<String> = Vec::new();
    if !is_opaque {
        parts.push(source_label);
    }
    let bound_profile = hermes
        .and_then(|h| h.profile.clone())
        .or_else(|| session.profile.clone())
        .unwrap_or_default()
        .trim()
        .to_string();
    if !bound_profile.is_empty() && bound_profile != "default" {
        parts.push(format!("@{bound_profile}"));
    }
    let effective_turns = turn_count.max(hermes.map(|h| h.turn_count).unwrap_or(0));
    if effective_turns > 0 {
        parts.push(format!("{effective_turns}t"));
    }
    if let Some(model) = hermes.and_then(|h| h.model.clone()).filter(|m| !m.is_empty() && m != "hermes-agent") {
        parts.push(model.split('/').last().filter(|s| !s.is_empty()).unwrap_or(&model).to_string());
    }
    if parts.is_empty() {
        parts.push(format!("{effective_turns}t"));
    }

    CanvasDotLabel {
        primary_label: if !is_opaque && is_slug_like {
            format!("#{compact_primary}")
        } else {
            compact_primary
        },
        secondary_label: parts.join(" · "),
        prompt_target: short_prompt_base,
        is_opaque,
    }
}

pub fn filter_canvas_sessions(
    sessions: &[CanvasSession],
    filter: CanvasSessionFilter,
    thinking_by_channel: &HashMap<String, bool>,
    hermes_status_by_channel: Option<&HashMap<String, String>>,
    hermes_sessions_by_channel: Option<&HashMap<String, GatewaySessionInfo>>,
) -> Vec<CanvasSession> {
    let is_live = |s: &CanvasSession| {
        thinking_by_channel.get(&s.channel_id).copied().unwrap_or(false)
            || hermes_status_by_channel
                .and_then(|m| m.get(&s.channel_id))
                .is_some_and(|st| st == "thinking" || st == "running")
    };
    match filter {
        CanvasSessionFilter::All => sessions.to_vec(),
        CanvasSessionFilter::Chats => sessions
            .iter()
            .filter(|s| {
                is_live(s)
                    || !is_canvas_background_run(s, hermes_sessions_by_channel.and_then(|m| m.get(&s.channel_id)))
            })
            .cloned()
            .collect(),
        CanvasSessionFilter::Live => sessions.iter().filter(|s| is_live(s)).cloned().collect(),
        CanvasSessionFilter::Autonomous => sessions
            .iter()
            .filter(|s| s.autonomy_mode != DotAutonomyMode::Manual)
            .cloned()
            .collect(),
    }
}

pub fn slugify_session_name(raw: &str, existing_ids: &[String]) -> String {
    let lowered = raw.trim().to_ascii_lowercase();
    let mut replaced = String::with_capacity(lowered.len());
    let mut prev_dash = false;
    for c in lowered.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            replaced.push(c);
            prev_dash = c == '-';
        } else if !prev_dash {
            replaced.push('-');
            prev_dash = true;
        }
    }
    let trimmed = replaced.trim_matches('-');
    let clipped: String = trimmed.chars().take(24).collect();
    let base = clipped.trim_end_matches('-');
    let base = if base.is_empty() { "session".to_string() } else { base.to_string() };

    let taken: HashSet<String> = existing_ids.iter().map(|id| id.to_ascii_lowercase()).collect();
    if !taken.contains(&base) {
        return base;
    }
    let mut n = 2u32;
    loop {
        let suffix = format!("-{n}");
        let budget = 24usize.saturating_sub(suffix.len()).max(1);
        let trimmed_base: String = base.chars().take(budget).collect();
        let trimmed_base = trimmed_base.trim_end_matches('-');
        let trimmed_base = if trimmed_base.is_empty() { "s" } else { trimmed_base };
        let candidate = format!("{trimmed_base}{suffix}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

pub fn find_open_canvas_spot(existing: &[(f32, f32)]) -> (f32, f32) {
    if existing.is_empty() {
        return (50.0, 46.0);
    }
    let mut candidates: Vec<(f32, f32)> = Vec::new();
    let mut y = 20.0_f32;
    while y <= 70.0 {
        let mut x = 14.0_f32;
        while x <= 86.0 {
            candidates.push(clamp_canvas_position(x, y));
            x += 6.0;
        }
        y += 5.0;
    }

    let mut best = candidates[0];
    let mut best_score = -1.0_f64;
    for cand in &candidates {
        let mut min_box_clearance = f64::INFINITY;
        let mut min_euclid = f64::INFINITY;
        for item in existing {
            let rx = (cand.0 - item.0).abs() as f64 / 25.0;
            let ry = (cand.1 - item.1).abs() as f64 / 31.0;
            let box_clearance = rx.max(ry);
            let euclid = (rx * rx + ry * ry).sqrt();
            if box_clearance < min_box_clearance {
                min_box_clearance = box_clearance;
            }
            if euclid < min_euclid {
                min_euclid = euclid;
            }
        }
        let center_bias = 1.0
            - ((((cand.0 - 50.0) / 100.0) as f64).powi(2) + (((cand.1 - 48.0) / 100.0) as f64).powi(2)).sqrt() * 0.04;
        let score = 1.05_f64.min(min_box_clearance) * 2.0 + min_euclid * 0.35 + center_bias * 0.02;
        if score > best_score {
            best_score = score;
            best = *cand;
        }
    }
    best
}

pub fn find_open_pin_spot(sessions: &[(f32, f32)], pins: &[(f32, f32)]) -> (f32, f32) {
    let occupied: Vec<(f32, f32)> = sessions.iter().chain(pins.iter()).copied().collect();
    if occupied.is_empty() {
        return (18.0, 22.0);
    }
    let candidates = [
        (14.0, 22.0),
        (86.0, 22.0),
        (14.0, 72.0),
        (86.0, 72.0),
        (50.0, 18.0),
        (34.0, 76.0),
        (66.0, 76.0),
    ];
    let mut best = candidates[0];
    let mut best_dist = -1.0_f64;
    for c in &candidates {
        let mut nearest = f64::INFINITY;
        for o in &occupied {
            let dx = (c.0 - o.0) as f64 / 20.0;
            let dy = (c.1 - o.1) as f64 / 24.0;
            let d = (dx * dx + dy * dy).sqrt();
            if d < nearest {
                nearest = d;
            }
        }
        if nearest > best_dist {
            best_dist = nearest;
            best = *c;
        }
    }
    clamp_pin_position(best.0, best.1)
}

fn with_position(session: &CanvasSession, x: f32, y: f32) -> CanvasSession {
    let mut next = session.clone();
    let (x_pct, y_pct) = clamp_canvas_position(x, y);
    next.x_pct = x_pct;
    next.y_pct = y_pct;
    next
}

pub fn arrange_sessions_formation(sessions: &[CanvasSession], formation: CanvasFormation) -> Vec<CanvasSession> {
    let count = sessions.len();
    if count == 0 {
        return Vec::new();
    }

    if formation == CanvasFormation::Orbit {
        if count == 1 {
            return vec![with_position(&sessions[0], 50.0, 44.0)];
        }
        let has_hub = count >= 5;
        return sessions
            .iter()
            .enumerate()
            .map(|(idx, session)| {
                if has_hub && idx == 0 {
                    return with_position(session, 50.0, 44.0);
                }
                let ring_count = if has_hub { count - 1 } else { count };
                let ring_idx = if has_hub { idx - 1 } else { idx };
                let angle = -std::f32::consts::FRAC_PI_2 + (ring_idx as f32 / ring_count as f32) * TAU;
                let rx = if ring_count <= 3 { 25.0 } else { 29.0 };
                let ry = if ring_count <= 3 { 20.0 } else { 26.0 };
                with_position(session, 50.0 + angle.cos() * rx, 46.0 + angle.sin() * ry)
            })
            .collect();
    }

    if formation == CanvasFormation::Grid {
        let cols = if count <= 2 {
            count
        } else if count <= 4 {
            2
        } else if count <= 9 {
            3
        } else {
            4
        };
        let rows = count.div_ceil(cols);
        return sessions
            .iter()
            .enumerate()
            .map(|(idx, session)| {
                let r = idx / cols;
                let c = idx % cols;
                let cols_in_row = if r == rows - 1 { count - r * cols } else { cols };
                let t_x = if cols_in_row == 1 { 0.5 } else { c as f32 / (cols_in_row - 1) as f32 };
                let t_y = if rows == 1 { 0.5 } else { r as f32 / (rows - 1) as f32 };
                let x_margin = if cols_in_row == 2 { 26.0 } else if cols >= 4 { 13.0 } else { 18.0 };
                let x_span = if cols_in_row == 2 { 48.0 } else if cols >= 4 { 74.0 } else { 64.0 };
                let y_min = if rows == 1 { 45.0 } else if rows >= 4 { 19.0 } else { 24.0 };
                let y_span = if rows == 1 { 0.0 } else if rows >= 4 { 54.0 } else { 44.0 };
                with_position(session, x_margin + t_x * x_span, y_min + t_y * y_span)
            })
            .collect();
    }

    auto_arrange_sessions(sessions)
}

pub fn auto_arrange_sessions(sessions: &[CanvasSession]) -> Vec<CanvasSession> {
    let count = sessions.len();
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![with_position(&sessions[0], 50.0, 44.0)];
    }
    if count == 2 {
        return vec![
            with_position(&sessions[0], 33.0, 46.0),
            with_position(&sessions[1], 67.0, 46.0),
        ];
    }
    if count == 3 {
        let coords = [(22.0, 48.0), (50.0, 38.0), (78.0, 48.0)];
        return sessions
            .iter()
            .enumerate()
            .map(|(idx, s)| with_position(s, coords[idx].0, coords[idx].1))
            .collect();
    }
    if count <= 6 {
        let top_count = count.div_ceil(2);
        let bottom_count = count - top_count;
        return sessions
            .iter()
            .enumerate()
            .map(|(idx, session)| {
                let is_top = idx < top_count;
                let row_idx = if is_top { idx } else { idx - top_count };
                let row_len = if is_top { top_count } else { bottom_count };
                let t = if row_len == 1 { 0.5 } else { row_idx as f32 / (row_len - 1) as f32 };
                let x_margin = if is_top { 18.0 } else { 26.0 };
                let x_span = if is_top { 64.0 } else { 48.0 };
                let y_pct = if is_top { 28.0 } else { 66.0 };
                with_position(session, x_margin + t * x_span, y_pct)
            })
            .collect();
    }

    // Multi-row staggered constellation for 7+ sessions.
    let cols = if count <= 9 { 3 } else { 4 };
    let rows = count.div_ceil(cols);
    sessions
        .iter()
        .enumerate()
        .map(|(idx, session)| {
            let r = idx / cols;
            let c = idx % cols;
            let cols_in_row = if r == rows - 1 { count - r * cols } else { cols };
            let t_x = if cols_in_row == 1 { 0.5 } else { c as f32 / (cols_in_row - 1) as f32 };
            let t_y = if rows == 1 { 0.5 } else { r as f32 / (rows - 1) as f32 };
            let row_stagger = if r % 2 == 1 && cols_in_row < cols { 4.0 } else { 0.0 };
            let x_margin = if cols_in_row == 2 { 26.0 } else { 14.0 };
            let x_span = if cols_in_row == 2 { 48.0 } else { 72.0 };
            let y_min = if rows >= 4 { 19.0 } else { 22.0 };
            let y_span = if rows >= 4 { 54.0 } else { 48.0 };
            with_position(session, x_margin + row_stagger + t_x * x_span, y_min + t_y * y_span)
        })
        .collect()
}

#[derive(Clone, Debug)]
struct ShapePreset {
    shape: BrailleShapeKind,
    tint: BrailleShapeTint,
    x_pct: f32,
    y_pct: f32,
    eye_style: DotEyeStyle,
    mood: CanvasMood,
    autonomy_mode: DotAutonomyMode,
    schedule_label: &'static str,
    sandbox_env: DotSandboxEnv,
    skills: &'static [&'static str],
    connected_to: Option<&'static [&'static str]>,
}

fn default_shape_preset(channel_id: &str) -> Option<ShapePreset> {
    Some(match channel_id {
        "general" => ShapePreset {
            shape: BrailleShapeKind::Clover,
            tint: BrailleShapeTint::Lime,
            x_pct: 20.0,
            y_pct: 30.0,
            eye_style: DotEyeStyle::Open,
            mood: CanvasMood::Happy,
            autonomy_mode: DotAutonomyMode::Manual,
            schedule_label: "On-demand",
            sandbox_env: DotSandboxEnv::CloudSandbox,
            skills: &["browser", "files", "research"],
            connected_to: Some(&["ops", "research"]),
        },
        "ops" => ShapePreset {
            shape: BrailleShapeKind::Trapezoid,
            tint: BrailleShapeTint::Amber,
            x_pct: 50.0,
            y_pct: 26.0,
            eye_style: DotEyeStyle::Sleepy,
            mood: CanvasMood::Focused,
            autonomy_mode: DotAutonomyMode::Scheduled,
            schedule_label: "Every 15m",
            sandbox_env: DotSandboxEnv::LocalRepo,
            skills: &["shell", "files", "mail"],
            connected_to: Some(&["launch", "daemon"]),
        },
        "research" => ShapePreset {
            shape: BrailleShapeKind::Pebble,
            tint: BrailleShapeTint::Periwinkle,
            x_pct: 80.0,
            y_pct: 30.0,
            eye_style: DotEyeStyle::Sleepy,
            mood: CanvasMood::Thinking,
            autonomy_mode: DotAutonomyMode::Scheduled,
            schedule_label: "Every 30m",
            sandbox_env: DotSandboxEnv::BrowserWorker,
            skills: &["research", "browser"],
            connected_to: Some(&["general"]),
        },
        "launch" => ShapePreset {
            shape: BrailleShapeKind::Star,
            tint: BrailleShapeTint::Pink,
            x_pct: 30.0,
            y_pct: 68.0,
            eye_style: DotEyeStyle::Open,
            mood: CanvasMood::Happy,
            autonomy_mode: DotAutonomyMode::Manual,
            schedule_label: "On-demand",
            sandbox_env: DotSandboxEnv::CloudSandbox,
            skills: &["mail", "calendar", "browser"],
            connected_to: Some(&["ops"]),
        },
        "daemon" => ShapePreset {
            shape: BrailleShapeKind::Ghost,
            tint: BrailleShapeTint::Lavender,
            x_pct: 72.0,
            y_pct: 68.0,
            eye_style: DotEyeStyle::Open,
            mood: CanvasMood::Neutral,
            autonomy_mode: DotAutonomyMode::Watch,
            schedule_label: "Continuous watch",
            sandbox_env: DotSandboxEnv::LocalRepo,
            skills: &["shell", "files"],
            connected_to: Some(&["ops"]),
        },
        "memory" => ShapePreset {
            shape: BrailleShapeKind::Cloud,
            tint: BrailleShapeTint::Orange,
            x_pct: 50.0,
            y_pct: 68.0,
            eye_style: DotEyeStyle::Open,
            mood: CanvasMood::Happy,
            autonomy_mode: DotAutonomyMode::Watch,
            schedule_label: "Continuous watch",
            sandbox_env: DotSandboxEnv::CloudSandbox,
            skills: &["files", "research"],
            connected_to: None,
        },
        _ => return None,
    })
}

struct AutonomyDefaults {
    autonomy_mode: DotAutonomyMode,
    schedule_label: String,
    sandbox_env: DotSandboxEnv,
    skills: Vec<String>,
    connected_to: Option<Vec<String>>,
}

fn default_autonomy_for_channel(channel_id: &str) -> AutonomyDefaults {
    if let Some(preset) = default_shape_preset(channel_id) {
        return AutonomyDefaults {
            autonomy_mode: preset.autonomy_mode,
            schedule_label: preset.schedule_label.to_string(),
            sandbox_env: preset.sandbox_env,
            skills: preset.skills.iter().map(|s| s.to_string()).collect(),
            connected_to: preset.connected_to.map(|c| c.iter().map(|s| s.to_string()).collect()),
        };
    }
    AutonomyDefaults {
        autonomy_mode: DotAutonomyMode::Manual,
        schedule_label: "On-demand".to_string(),
        sandbox_env: DotSandboxEnv::CloudSandbox,
        skills: vec!["browser".to_string(), "files".to_string()],
        connected_to: None,
    }
}

const LEGACY_SEED_CHANNEL_IDS: &[&str] = &["general", "ops", "research", "launch", "daemon", "memory"];

pub fn prune_and_migrate_canvas_sessions(
    sessions: &[CanvasSession],
    gateway_sessions: &[GatewaySessionInfo],
    migrate_legacy_seeds: bool,
) -> (Vec<CanvasSession>, bool) {
    let mut gw_by_key: HashMap<String, &GatewaySessionInfo> = HashMap::new();
    for hs in gateway_sessions {
        if !hs.channel_id.is_empty() {
            gw_by_key.insert(hs.channel_id.clone(), hs);
        }
        if !hs.session_id.is_empty() {
            gw_by_key.insert(hs.session_id.clone(), hs);
        }
    }

    let mut changed = false;
    let mut kept: Vec<CanvasSession> = Vec::new();
    for s in sessions {
        let hs = gw_by_key
            .get(&s.channel_id)
            .or_else(|| gw_by_key.get(&s.hermes_session_id))
            .copied();
        let resolved_gw_origin: Option<SessionOrigin> = hs.map(|h| {
            h.origin.unwrap_or_else(|| {
                classify_gateway_session_origin(
                    Some(&h.session_id),
                    h.session_key.as_deref(),
                    Some(&h.channel_id),
                    h.source.as_deref(),
                    false,
                )
            })
        });

        // 1. Explicitly pinned external Dots stay until the user removes them.
        if s.origin == Some(CanvasDotOrigin::Imported) {
            kept.push(s.clone());
            continue;
        }

        // 2. Strip opaque gateway run/local/cron IDs unless workspace-confirmed.
        if (is_opaque_session_identifier(Some(&s.channel_id))
            || is_opaque_session_identifier(Some(&s.hermes_session_id)))
            && resolved_gw_origin != Some(SessionOrigin::Workspace)
            && s.origin != Some(CanvasDotOrigin::Workspace)
        {
            changed = true;
            continue;
        }

        // 3. Strip legacy hardcoded seed dots unless active in workspace.
        let looks_like_legacy_seed = s.is_core == Some(true)
            || s.origin == Some(CanvasDotOrigin::Seed)
            || (LEGACY_SEED_CHANNEL_IDS.contains(&s.channel_id.as_str())
                && s.id == format!("session-{}", s.channel_id));
        if migrate_legacy_seeds
            && looks_like_legacy_seed
            && s.origin != Some(CanvasDotOrigin::Workspace)
            && resolved_gw_origin != Some(SessionOrigin::Workspace)
        {
            changed = true;
            continue;
        }

        // 4. Gateway-confirmed external history never floods the stage.
        if resolved_gw_origin == Some(SessionOrigin::External) {
            changed = true;
            continue;
        }

        // 5. Backfill workspace origin on genuine workspace dots.
        if s.origin != Some(CanvasDotOrigin::Workspace)
            && (resolved_gw_origin == Some(SessionOrigin::Workspace)
                || (s.hermes_session_id.starts_with("hermes-")
                    && !is_opaque_session_identifier(Some(&s.hermes_session_id))))
        {
            changed = true;
            let mut next = s.clone();
            next.origin = Some(CanvasDotOrigin::Workspace);
            next.is_core = Some(false);
            kept.push(next);
            continue;
        }

        kept.push(s.clone());
    }

    (kept, changed)
}

/// Partial persisted session record (TS `Partial<CanvasSession>`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RawCanvasSession {
    pub id: Option<String>,
    pub channel_id: Option<String>,
    pub hermes_session_id: Option<String>,
    pub profile: Option<String>,
    pub agent_persona: Option<String>,
    pub name: Option<String>,
    pub topic: Option<String>,
    pub shape: Option<String>,
    pub tint: Option<String>,
    pub x_pct: Option<f32>,
    pub y_pct: Option<f32>,
    pub mood: Option<String>,
    pub eye_style: Option<String>,
    pub is_core: Option<bool>,
    pub origin: Option<String>,
    pub autonomy_mode: Option<String>,
    pub schedule_label: Option<String>,
    pub autonomy_paused: Option<bool>,
    pub sandbox_env: Option<String>,
    pub workspace_dir: Option<String>,
    pub skills: Option<Vec<String>>,
    pub connected_to: Option<Vec<String>>,
}

fn non_empty_trimmed(value: Option<&String>) -> Option<String> {
    value.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn normalize_canvas_sessions(raw_list: &[RawCanvasSession]) -> Vec<CanvasSession> {
    let mut seen_channels: HashSet<String> = HashSet::new();
    let mut normalized: Vec<CanvasSession> = Vec::new();

    for (i, item) in raw_list.iter().enumerate() {
        let base_name = item
            .name
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(item.channel_id.as_deref().filter(|s| !s.is_empty()))
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("session-{}", i + 1));
        let base_name = base_name.strip_prefix('#').unwrap_or(&base_name).to_string();

        let channel_id = non_empty_trimmed(item.channel_id.as_ref())
            .unwrap_or_else(|| slugify_session_name(&base_name, &seen_channels.iter().cloned().collect::<Vec<_>>()));
        if seen_channels.contains(&channel_id) {
            continue;
        }
        seen_channels.insert(channel_id.clone());

        let shape = item
            .shape
            .as_deref()
            .and_then(BrailleShapeKind::from_id)
            .unwrap_or(BrailleShapeKind::Clover);
        let meta = get_shape_meta(shape);
        let x = item.x_pct.filter(|v| v.is_finite()).unwrap_or(50.0);
        let y = item.y_pct.filter(|v| v.is_finite()).unwrap_or(46.0);
        let (x_pct, y_pct) = clamp_canvas_position(x, y);
        let mood = item
            .mood
            .as_deref()
            .and_then(CanvasMood::from_id)
            .unwrap_or(CanvasMood::Happy);

        let defaults = default_autonomy_for_channel(&channel_id);
        let autonomy_mode = item
            .autonomy_mode
            .as_deref()
            .and_then(DotAutonomyMode::from_id)
            .unwrap_or(defaults.autonomy_mode);
        let sandbox_env = item
            .sandbox_env
            .as_deref()
            .and_then(DotSandboxEnv::from_id)
            .unwrap_or(defaults.sandbox_env);
        let schedule_label = non_empty_trimmed(item.schedule_label.as_ref())
            .unwrap_or(defaults.schedule_label);
        let workspace_dir = non_empty_trimmed(item.workspace_dir.as_ref())
            .unwrap_or_else(|| format!("~/hermes/dots/{channel_id}"));
        let skills = match &item.skills {
            Some(list) => list
                .iter()
                .filter(|s| !s.trim().is_empty())
                .take(8)
                .cloned()
                .collect(),
            None => defaults.skills,
        };
        let connected_to = match &item.connected_to {
            Some(list) => Some(list.iter().filter(|c| *c != &channel_id).cloned().collect()),
            None => defaults.connected_to,
        };
        let origin = item.origin.as_deref().and_then(CanvasDotOrigin::from_id);

        let hermes_session_id = non_empty_trimmed(item.hermes_session_id.as_ref())
            .filter(|sid| {
                let lower = sid.to_ascii_lowercase();
                lower != "duet" && !lower.starts_with("duet-")
            })
            .unwrap_or_else(|| create_hermes_session_id(&channel_id));

        normalized.push(CanvasSession {
            id: item
                .id
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("session-{channel_id}")),
            channel_id: channel_id.clone(),
            hermes_session_id,
            profile: non_empty_trimmed(item.profile.as_ref()).map(|p| p.to_ascii_lowercase()),
            agent_persona: non_empty_trimmed(item.agent_persona.as_ref())
                .unwrap_or_else(|| meta.default_agent_persona.to_string()),
            name: base_name,
            topic: non_empty_trimmed(item.topic.as_ref()).unwrap_or_else(|| meta.tagline.to_string()),
            shape,
            tint: item
                .tint
                .as_deref()
                .and_then(BrailleShapeTint::from_id)
                .unwrap_or(meta.default_tint),
            x_pct,
            y_pct,
            mood,
            eye_style: match item.eye_style.as_deref() {
                Some("sleepy") => DotEyeStyle::Sleepy,
                Some("open") => DotEyeStyle::Open,
                _ => meta.default_eye_style,
            },
            is_core: Some(item.is_core.unwrap_or(false)),
            origin,
            autonomy_mode,
            schedule_label,
            autonomy_paused: Some(item.autonomy_paused.unwrap_or(false)),
            sandbox_env,
            workspace_dir,
            skills,
            connected_to,
        });
    }

    normalized
}

pub fn create_seed_canvas_sessions(channels: &[ChannelMeta]) -> Vec<CanvasSession> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut seeded: Vec<CanvasSession> = Vec::new();

    for (idx, ch) in channels.iter().enumerate() {
        let preset = default_shape_preset(&ch.id);
        let fallback_meta = &BRAILLE_SHAPE_CATALOG[idx % BRAILLE_SHAPE_CATALOG.len()];
        let shape = preset.as_ref().map(|p| p.shape).unwrap_or(fallback_meta.kind);
        let shape_meta = get_shape_meta(shape);
        let (x_pct, y_pct) = match &preset {
            Some(p) => clamp_canvas_position(p.x_pct, p.y_pct),
            None => find_open_canvas_spot(&seeded.iter().map(|s| (s.x_pct, s.y_pct)).collect::<Vec<_>>()),
        };
        let defaults = default_autonomy_for_channel(&ch.id);
        seen.insert(ch.id.clone());
        seeded.push(CanvasSession {
            id: format!("session-{}", ch.id),
            channel_id: ch.id.clone(),
            hermes_session_id: create_hermes_session_id(&ch.id),
            profile: None,
            // ChannelMeta carries no persona field; TS Channel.persona falls back
            // to the shape default here (documented parity assumption).
            agent_persona: shape_meta.default_agent_persona.to_string(),
            name: ch.name.strip_prefix('#').unwrap_or(&ch.name).to_string(),
            topic: if ch.description.is_empty() {
                format!("Hermes session #{}", ch.name)
            } else {
                ch.description.clone()
            },
            shape,
            tint: preset.as_ref().map(|p| p.tint).unwrap_or(shape_meta.default_tint),
            x_pct,
            y_pct,
            mood: preset.as_ref().map(|p| p.mood).unwrap_or(CanvasMood::Happy),
            eye_style: preset.as_ref().map(|p| p.eye_style).unwrap_or(shape_meta.default_eye_style),
            is_core: Some(true),
            origin: Some(CanvasDotOrigin::Seed),
            autonomy_mode: defaults.autonomy_mode,
            schedule_label: defaults.schedule_label,
            autonomy_paused: Some(false),
            sandbox_env: defaults.sandbox_env,
            workspace_dir: format!("~/hermes/dots/{}", ch.id),
            skills: defaults.skills,
            connected_to: defaults.connected_to,
        });
    }

    if !seen.contains("daemon") && !seeded.is_empty() && seeded.len() < 5 {
        let p = default_shape_preset("daemon").expect("daemon preset");
        let daemon_meta = get_shape_meta(p.shape);
        seeded.push(CanvasSession {
            id: "session-daemon".to_string(),
            channel_id: "daemon".to_string(),
            hermes_session_id: create_hermes_session_id("daemon"),
            profile: None,
            agent_persona: daemon_meta.default_agent_persona.to_string(),
            name: "daemon".to_string(),
            topic: "Background cron watcher & ambient signal daemon".to_string(),
            shape: p.shape,
            tint: p.tint,
            x_pct: p.x_pct,
            y_pct: p.y_pct,
            mood: p.mood,
            eye_style: p.eye_style,
            is_core: Some(false),
            origin: Some(CanvasDotOrigin::Seed),
            autonomy_mode: p.autonomy_mode,
            schedule_label: p.schedule_label.to_string(),
            autonomy_paused: Some(false),
            sandbox_env: p.sandbox_env,
            workspace_dir: "~/hermes/dots/daemon".to_string(),
            skills: p.skills.iter().map(|s| s.to_string()).collect(),
            connected_to: p.connected_to.map(|c| c.iter().map(|s| s.to_string()).collect()),
        });
    }

    seeded
}

pub fn create_seed_canvas_pins() -> Vec<CanvasPin> {
    vec![
        CanvasPin {
            id: "pin-workspace-repo".to_string(),
            kind: CanvasPinKind::Repo,
            title: "Workspace Brief".to_string(),
            detail: "Isolated hermes-agent sandboxes share portable skills & gated outbound actions.".to_string(),
            meta: "~/hermes/workspace · main".to_string(),
            x_pct: 12.0,
            y_pct: 16.0,
            linked_channel_id: Some("general".to_string()),
        },
        CanvasPin {
            id: "pin-ops-runbook".to_string(),
            kind: CanvasPinKind::Brief,
            title: "Release Checklist".to_string(),
            detail: "1. Verify tests  2. Review draft mail gate  3. Confirm rollout window".to_string(),
            meta: "Pinned artifact · #ops".to_string(),
            x_pct: 88.0,
            y_pct: 16.0,
            linked_channel_id: Some("ops".to_string()),
        },
    ]
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RawCanvasPin {
    pub id: Option<String>,
    pub kind: Option<String>,
    pub title: Option<String>,
    pub detail: Option<String>,
    pub meta: Option<String>,
    pub x_pct: Option<f32>,
    pub y_pct: Option<f32>,
    pub linked_channel_id: Option<String>,
}

pub fn normalize_canvas_pins(raw_list: &[RawCanvasPin]) -> Vec<CanvasPin> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<CanvasPin> = Vec::new();
    for (i, item) in raw_list.iter().enumerate() {
        let id = non_empty_trimmed(item.id.as_ref()).unwrap_or_else(|| format!("pin-{}", i + 1));
        if seen.contains(&id) {
            continue;
        }
        seen.insert(id.clone());
        let kind = item
            .kind
            .as_deref()
            .and_then(CanvasPinKind::from_id)
            .unwrap_or(CanvasPinKind::Note);
        let x = item.x_pct.filter(|v| v.is_finite()).unwrap_or(20.0);
        let y = item.y_pct.filter(|v| v.is_finite()).unwrap_or(20.0);
        let (x_pct, y_pct) = clamp_pin_position(x, y);
        out.push(CanvasPin {
            id,
            kind,
            title: non_empty_trimmed(item.title.as_ref())
                .map(|t| t.chars().take(48).collect())
                .unwrap_or_else(|| "Workspace Note".to_string()),
            detail: non_empty_trimmed(item.detail.as_ref())
                .map(|d| d.chars().take(180).collect())
                .unwrap_or_else(|| "Pinned spatial context for connected Dots.".to_string()),
            meta: non_empty_trimmed(item.meta.as_ref())
                .map(|m| m.chars().take(44).collect())
                .unwrap_or_else(|| "Canvas pin".to_string()),
            x_pct,
            y_pct,
            linked_channel_id: non_empty_trimmed(item.linked_channel_id.as_ref()),
        });
    }
    out
}

/// Boot hydration mirroring App.tsx: parse the `duet.canvasSessions.v2` payload,
/// normalize, then prune/migrate legacy seeds with no gateway sessions yet.
pub fn load_canvas_sessions_from_json(raw: &str) -> Vec<CanvasSession> {
    let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(raw) else {
        return Vec::new();
    };
    let raws: Vec<RawCanvasSession> = values
        .iter()
        .filter(|v| v.is_object())
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect();
    if raws.is_empty() {
        return Vec::new();
    }
    let normalized = normalize_canvas_sessions(&raws);
    prune_and_migrate_canvas_sessions(&normalized, &[], true).0
}

pub fn load_canvas_pins_from_json(raw: &str) -> Vec<CanvasPin> {
    let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(raw) else {
        return Vec::new();
    };
    let raws: Vec<RawCanvasPin> = values
        .iter()
        .filter(|v| v.is_object())
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect();
    normalize_canvas_pins(&raws)
}

pub fn save_canvas_sessions_json(sessions: &[CanvasSession]) -> String {
    serde_json::to_string(sessions).unwrap_or_else(|_| "[]".to_string())
}

pub fn save_canvas_pins_json(pins: &[CanvasPin]) -> String {
    serde_json::to_string(pins).unwrap_or_else(|_| "[]".to_string())
}

// ---------------------------------------------------------------------------
// homeSession.ts
// ---------------------------------------------------------------------------

const FILLER_WORDS: &[&str] = &[
    "a", "an", "and", "are", "can", "could", "do", "for", "help", "hermes", "hey", "hi", "how", "i",
    "in", "is", "let", "lets", "me", "my", "new", "of", "on", "please", "session", "start", "that",
    "the", "this", "to", "want", "with", "would", "you",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DerivedHomeSession {
    pub slug: String,
    pub hermes_session_id: String,
    pub topic: String,
    pub persona: String,
}

/// Derives a clean, unique channel slug, hermes-agent session ID, topic, and
/// standing persona instructions from the user's opening prompt on Home.
pub fn derive_session_from_prompt(prompt: &str, existing_channel_ids: &[String]) -> DerivedHomeSession {
    let cleaned_topic = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    let topic_chars: Vec<char> = cleaned_topic.chars().collect();
    let topic = if topic_chars.len() > 120 {
        format!("{}...", topic_chars.iter().take(117).collect::<String>().trim_end())
    } else if cleaned_topic.is_empty() {
        "New Hermes Agent session".to_string()
    } else {
        cleaned_topic.clone()
    };

    let sanitized: String = cleaned_topic
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c.is_whitespace() || c == '-' { c } else { ' ' })
        .collect();
    let words: Vec<&str> = sanitized.split_whitespace().collect();
    let meaningful: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| !FILLER_WORDS.contains(w) && w.chars().count() > 1)
        .collect();
    let chosen: Vec<&str> = if !meaningful.is_empty() { meaningful } else { words }
        .into_iter()
        .take(3)
        .collect();
    let raw_base = if chosen.is_empty() { "session".to_string() } else { chosen.join("-") };
    let candidate_base = if raw_base == "general" { "general-session".to_string() } else { raw_base };
    let mut taken: Vec<String> = existing_channel_ids.to_vec();
    taken.push("general".to_string());
    let slug = slugify_session_name(&candidate_base, &taken);

    DerivedHomeSession {
        hermes_session_id: create_hermes_session_id(&slug),
        slug,
        persona: format!("Focused Hermes Agent session started from Home for: {topic}"),
        topic,
    }
}

/// Picks a distinct shape/tint/eye style for a newly started Home session.
pub fn pick_next_shape_and_tint(existing: &[(BrailleShapeKind, BrailleShapeTint)]) -> (BrailleShapeKind, BrailleShapeTint, DotEyeStyle) {
    let mut counts: HashMap<BrailleShapeKind, usize> = HashMap::new();
    for (shape, _) in existing {
        *counts.entry(*shape).or_insert(0) += 1;
    }
    let mut best = &BRAILLE_SHAPE_CATALOG[existing.len() % BRAILLE_SHAPE_CATALOG.len()];
    let mut best_count = usize::MAX;
    for meta in BRAILLE_SHAPE_CATALOG {
        let count = counts.get(&meta.kind).copied().unwrap_or(0);
        if count < best_count {
            best_count = count;
            best = meta;
        }
    }
    (best.kind, best.default_tint, best.default_eye_style)
}

pub fn should_start_new_home_session(home_session_draft: bool, panel_view: &str, raw_text: &str) -> bool {
    let trimmed = raw_text.trim();
    !trimmed.is_empty() && home_session_draft && panel_view == "home" && !trimmed.starts_with('/')
}

pub fn should_auto_enter_thread_on_submit(panel_view: &str, raw_text: &str, target_channel_id: Option<&str>) -> bool {
    if target_channel_id.is_some_and(|t| !t.is_empty()) {
        return false;
    }
    if panel_view != "home" {
        return false;
    }
    let trimmed = raw_text.trim();
    !trimmed.is_empty() && !trimmed.starts_with('/')
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeStarterPrompt {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub prompt: &'static str,
    pub kind: &'static str,
}

pub const CURATED_HOME_STARTERS: &[HomeStarterPrompt] = &[
    HomeStarterPrompt {
        id: "workspace-audit",
        title: "Inspect this workspace",
        description: "Review open tasks, recent changes, and next priorities",
        prompt: "Inspect the current workspace state, recent activity, and what needs attention next.",
        kind: "prompt",
    },
    HomeStarterPrompt {
        id: "skills-tools",
        title: "Explore skills & tools",
        description: "Check active Hermes capabilities available in this session",
        prompt: "Show what skills, toolsets, and connected capabilities are available right now.",
        kind: "prompt",
    },
    HomeStarterPrompt {
        id: "plan-task",
        title: "Plan a multi-step task",
        description: "Break down a goal into live trackable steps and approvals",
        prompt: "Help me break down a multi-step engineering task with clear checkpoints.",
        kind: "prompt",
    },
    HomeStarterPrompt {
        id: "gateway-doctor",
        title: "Run gateway diagnostics",
        description: "Verify endpoint health, active model, and transport mode",
        prompt: "/gateway",
        kind: "command",
    },
];

fn truncate_at_word_boundary_home(text: &str, max_len: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_len {
        return text.to_string();
    }
    let slice: String = chars.iter().take(max_len - 1).collect();
    let last_space = slice.rfind(' ').map(|b| slice[..b].chars().count()).unwrap_or(0);
    let last_dash = slice.rfind('-').map(|b| slice[..b].chars().count()).unwrap_or(0);
    let has_sep = slice.contains(' ') || slice.contains('-');
    let last_sep = last_space.max(last_dash);
    let clean: String = if has_sep && last_sep >= (max_len as f32 * 0.45).floor() as usize {
        chars.iter().take(last_sep).collect()
    } else {
        slice
    };
    format!("{}…", clean.trim_end_matches([' ', '-']))
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComposerPlaceholderParams<'a> {
    pub is_home_new_draft: bool,
    pub active_channel: &'a str,
    pub prompt_target: Option<&'a str>,
    pub is_thinking: bool,
    pub is_speaking: bool,
    pub is_listening: bool,
    pub compact: bool,
}

/// Formats a concise, word-safe composer placeholder so long session slugs never
/// clip awkwardly mid-character inside narrow inputs.
pub fn resolve_composer_placeholder(params: &ComposerPlaceholderParams<'_>) -> String {
    if params.is_home_new_draft || params.active_channel.is_empty() {
        if params.is_listening {
            return if params.compact {
                "Listening…".to_string()
            } else {
                "Listening for new session prompt…".to_string()
            };
        }
        return if params.compact {
            "Ask Hermes or type /…".to_string()
        } else {
            "Ask Hermes anything or type / for commands…".to_string()
        };
    }
    let raw_target = params
        .prompt_target
        .filter(|t| !t.is_empty())
        .unwrap_or(params.active_channel)
        .trim();
    let formatted_target = if raw_target.starts_with('#')
        || raw_target.starts_with("chan-")
        || raw_target.starts_with("hermes-")
    {
        raw_target.to_string()
    } else {
        format!("#{raw_target}")
    };
    let compact_target = truncate_at_word_boundary_home(&formatted_target, if params.compact { 14 } else { 22 });

    if params.is_thinking || params.is_speaking {
        return if params.compact {
            format!("Steer {compact_target}…")
        } else {
            format!("Steer {compact_target} or send next prompt…")
        };
    }
    if params.is_listening {
        return format!("Listening in {compact_target}…");
    }
    if params.compact {
        format!("Message {compact_target} or /")
    } else {
        format!("Message {compact_target} or type / for commands…")
    }
}

/// Normalizes a Hermes channel/lane id into the canonical local slug.
pub fn normalize_channel_id(raw_id: Option<&str>, aliases: Option<&HashMap<String, String>>) -> String {
    let input = raw_id.unwrap_or("").trim();
    if !input.is_empty() {
        if let Some(map) = aliases {
            if let Some(exact) = map.get(input).or_else(|| map.get(&input.to_ascii_lowercase())) {
                if !exact.is_empty() {
                    return exact.clone();
                }
            }
        }
    }
    let stripped_hash = input.strip_prefix('#').unwrap_or(input);
    let stripped_prefix = ["chan-", "hermes-", "duet-"]
        .iter()
        .find_map(|p| {
            if stripped_hash.len() >= p.len() && stripped_hash[..p.len()].eq_ignore_ascii_case(p) {
                Some(&stripped_hash[p.len()..])
            } else {
                None
            }
        })
        .unwrap_or(stripped_hash);
    let stripped = stripped_prefix.trim().to_ascii_lowercase();
    if !stripped.is_empty() {
        if let Some(map) = aliases {
            if let Some(via) = map.get(&stripped) {
                if !via.is_empty() {
                    return via.clone();
                }
            }
        }
    }
    if stripped.is_empty() {
        "general".to_string()
    } else {
        stripped
    }
}

/// Alias kept for TS naming parity (`normalizeDuetChannelId`).
pub fn normalize_duet_channel_id(raw_id: Option<&str>, aliases: Option<&HashMap<String, String>>) -> String {
    normalize_channel_id(raw_id, aliases)
}

const TERMINAL_ACTIVITY_STATUSES: &[&str] = &[
    "done", "completed", "complete", "error", "failed", "cancelled", "canceled", "stopped",
];

/// Detects whether an incoming SSE/snapshot item marks a turn as settled so
/// channel thinking state never remains stuck at "Working · 0s".
pub fn is_terminal_settled_item(kind: Option<&str>, streaming: bool, activity_status: Option<&str>) -> bool {
    if kind == Some("assistant") && !streaming {
        return true;
    }
    if kind == Some("activity") {
        if let Some(status) = activity_status {
            return TERMINAL_ACTIVITY_STATUSES.contains(&status.to_ascii_lowercase().as_str());
        }
    }
    false
}

/// Guarantees a fresh unique slug that does not collide with existing slugs.
pub fn build_unique_slug_for_fresh_channel(desired_slug: &str, taken_slugs: &[String]) -> String {
    slugify_session_name(desired_slug, taken_slugs)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Legacy draft tests (kept) ----

    #[test]
    fn slugifies_and_creates_home_session() {
        assert_eq!(slugify_session_id("Fix RUNE!! Audio 2026"), "fix-rune-audio-2026");
        let ch = create_home_session_channel("Inspect hermes gateway runs", 1, 9000);
        assert_eq!(ch.origin, SessionOrigin::Workspace);
        assert!(ch.id.starts_with("local-inspect-hermes"));
    }

    #[test]
    fn computes_distinct_formation_positions() {
        let grid = resolve_formation_positions(4, CanvasFormation::Grid, 900.0, 600.0);
        assert_eq!(grid.len(), 4);
        assert!(grid[1].0 > grid[0].0);
        assert!(grid[2].1 > grid[0].1);

        let orbit = resolve_formation_positions(4, CanvasFormation::Orbit, 900.0, 600.0);
        assert_eq!(orbit[0], (450.0, 300.0));
    }

    // ---- Mirrored canvasSessions.test.ts ----

    fn test_channels() -> Vec<ChannelMeta> {
        [
            ("general", "Primary session lane"),
            ("ops", "Build & gateway ops"),
            ("research", "Deep technical research"),
            ("launch", "Release tracking"),
        ]
        .iter()
        .map(|(id, desc)| ChannelMeta {
            id: (*id).into(),
            name: (*id).into(),
            category: "hermes".into(),
            description: (*desc).into(),
            session_id: format!("hermes-{id}"),
            unread: 0,
            origin: SessionOrigin::Workspace,
            has_local_history: true,
            model: None,
            updated_at_ms: None,
        })
        .collect()
    }

    fn raw_session(pairs: &[(&str, &str)]) -> RawCanvasSession {
        let mut raw = RawCanvasSession::default();
        for (k, v) in pairs {
            match *k {
                "id" => raw.id = Some((*v).into()),
                "channelId" => raw.channel_id = Some((*v).into()),
                "hermesSessionId" => raw.hermes_session_id = Some((*v).into()),
                "name" => raw.name = Some((*v).into()),
                "topic" => raw.topic = Some((*v).into()),
                "shape" => raw.shape = Some((*v).into()),
                "tint" => raw.tint = Some((*v).into()),
                "autonomyMode" => raw.autonomy_mode = Some((*v).into()),
                "scheduleLabel" => raw.schedule_label = Some((*v).into()),
                "origin" => raw.origin = Some((*v).into()),
                _ => {}
            }
        }
        raw
    }

    #[test]
    fn ts_clamp_canvas_position_enforces_bounds() {
        let low = clamp_canvas_position(-20.0, 0.0);
        assert_eq!(low.0, CANVAS_BOUNDS.min_x);
        assert_eq!(low.1, CANVAS_BOUNDS.min_y);
        let high = clamp_canvas_position(120.0, 99.0);
        assert_eq!(high.0, CANVAS_BOUNDS.max_x);
        assert_eq!(high.1, CANVAS_BOUNDS.max_y);
    }

    #[test]
    fn ts_slugify_session_name_normalizes_and_deduplicates() {
        assert_eq!(slugify_session_name("Design Critique!", &["general".into(), "ops".into()]), "design-critique");
        assert_eq!(slugify_session_name("Ops", &["general".into(), "ops".into()]), "ops-2");
        assert_eq!(slugify_session_name("Ops", &["general".into(), "ops".into(), "ops-2".into()]), "ops-3");
        assert_eq!(slugify_session_name("   ", &[]), "session");
    }

    #[test]
    fn ts_find_open_canvas_spot_maximizes_distance() {
        let first = find_open_canvas_spot(&[]);
        assert_eq!(first, (50.0, 46.0));

        let next = find_open_canvas_spot(&[first]);
        let dist = ((next.0 - first.0).powi(2) + (next.1 - first.1).powi(2)).sqrt();
        assert!(dist >= 15.0, "expected well-separated spot, got dist={dist}");

        let seeded = create_seed_canvas_sessions(&test_channels());
        let extra = find_open_canvas_spot(&seeded.iter().map(|s| (s.x_pct, s.y_pct)).collect::<Vec<_>>());
        for item in &seeded {
            let clearance = (((extra.0 - item.x_pct) / 24.0).powi(2) + ((extra.1 - item.y_pct) / 26.0).powi(2)).sqrt();
            assert!(clearance >= 0.9, "expected clearance from #{} got {clearance}", item.channel_id);
        }
    }

    #[test]
    fn ts_seed_sessions_unique_and_formations_within_bounds() {
        let seeded = create_seed_canvas_sessions(&test_channels());
        assert!(seeded.len() >= 4);
        let ids: HashSet<&str> = seeded.iter().map(|s| s.hermes_session_id.as_str()).collect();
        assert_eq!(ids.len(), seeded.len());
        for s in &seeded {
            assert!(s.hermes_session_id.starts_with("hermes-"));
            assert!(s.agent_persona.chars().count() > 10);
            assert!(!s.skills.is_empty());
            assert!(s.workspace_dir.starts_with("~/"));
        }

        for formation in [CanvasFormation::Constellation, CanvasFormation::Orbit, CanvasFormation::Grid] {
            let arranged = arrange_sessions_formation(&seeded, formation);
            assert_eq!(arranged.len(), seeded.len());
            for s in &arranged {
                assert!(s.x_pct >= CANVAS_BOUNDS.min_x && s.x_pct <= CANVAS_BOUNDS.max_x);
                assert!(s.y_pct >= CANVAS_BOUNDS.min_y && s.y_pct <= CANVAS_BOUNDS.max_y);
            }
        }

        let legacy = auto_arrange_sessions(&seeded);
        assert_eq!(legacy.len(), seeded.len());
    }

    #[test]
    fn ts_normalize_backfills_legacy_entries_and_migrates_duet_ids() {
        let mut raw = raw_session(&[
            ("id", "legacy-1"),
            ("channelId", "kitsune-lab"),
            ("name", "kitsune-lab"),
            ("shape", "heart"),
            ("tint", "coral"),
        ]);
        raw.x_pct = Some(2.0);
        raw.y_pct = Some(95.0);
        let normalized = normalize_canvas_sessions(&[raw]);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].hermes_session_id, create_hermes_session_id("kitsune-lab"));
        assert!(!normalized[0].agent_persona.is_empty());
        assert_eq!(normalized[0].x_pct, CANVAS_BOUNDS.min_x);
        assert_eq!(normalized[0].y_pct, CANVAS_BOUNDS.max_y);
        assert_eq!(normalized[0].autonomy_mode, DotAutonomyMode::Manual);
        assert_eq!(normalized[0].sandbox_env, DotSandboxEnv::CloudSandbox);
        assert!(!normalized[0].skills.is_empty());

        let migrated = normalize_canvas_sessions(&[raw_session(&[
            ("id", "legacy-duet-1"),
            ("channelId", "what-being-done-2"),
            ("hermesSessionId", "duet-channel-what-being-done-2"),
            ("name", "what-being-done-2"),
            ("autonomyMode", "watch"),
            ("scheduleLabel", "Continuous watch"),
        ])]);
        assert_eq!(migrated[0].hermes_session_id, "hermes-what-being-done-2");
        assert_eq!(migrated[0].autonomy_mode, DotAutonomyMode::Watch);
        assert!(is_dot_autonomous(&migrated[0]));
    }

    #[test]
    fn ts_viewport_zoom_pan_clamps_and_cursor_anchor() {
        assert_eq!(clamp_canvas_zoom(0.1), CANVAS_ZOOM_MIN);
        assert_eq!(clamp_canvas_zoom(4.2), CANVAS_ZOOM_MAX);
        assert_eq!(clamp_canvas_zoom(f32::NAN), 1.0);

        let pan = clamp_canvas_pan(2000.0, -2000.0, 1.0);
        assert_eq!(pan, (420.0, -420.0));

        let center = zoom_viewport_around_point(DEFAULT_CANVAS_VIEWPORT, 1.3, 500.0, 300.0, 1000.0, 600.0);
        assert_eq!(center.zoom, 1.3);
        assert_eq!(center.pan_x, 0.0);
        assert_eq!(center.pan_y, 0.0);

        let corner = zoom_viewport_around_point(DEFAULT_CANVAS_VIEWPORT, 1.2, 100.0, 100.0, 1000.0, 600.0);
        assert!(corner.pan_x > 0.0 && corner.pan_y > 0.0);
    }

    #[test]
    fn ts_filters_formatters_templates_and_pins() {
        let seeded = create_seed_canvas_sessions(&test_channels());
        let auto_only = filter_canvas_sessions(&seeded, CanvasSessionFilter::Autonomous, &HashMap::new(), None, None);
        assert!(auto_only.iter().all(|s| s.autonomy_mode != DotAutonomyMode::Manual));
        assert!(!auto_only.is_empty());

        let mut thinking = HashMap::new();
        thinking.insert(seeded[0].channel_id.clone(), true);
        let live_only = filter_canvas_sessions(&seeded, CanvasSessionFilter::Live, &thinking, None, None);
        assert_eq!(live_only.len(), 1);
        assert_eq!(live_only[0].channel_id, seeded[0].channel_id);

        assert_eq!(format_autonomy_label(DotAutonomyMode::Manual, "On-demand", false), "On-demand");
        assert_eq!(format_autonomy_label(DotAutonomyMode::Scheduled, "Every 15m", true), "Every 15m (paused)");
        assert_eq!(format_sandbox_label(DotSandboxEnv::LocalRepo), "Local Repo");
        assert!(DOT_STARTER_TEMPLATES.len() >= 4);

        let pins = create_seed_canvas_pins();
        assert!(pins.len() >= 2);
        let mut raw_pins: Vec<RawCanvasPin> = pins
            .iter()
            .map(|p| RawCanvasPin {
                id: Some(p.id.clone()),
                kind: Some(match p.kind {
                    CanvasPinKind::Brief => "brief".into(),
                    CanvasPinKind::Repo => "repo".into(),
                    CanvasPinKind::Note => "note".into(),
                    CanvasPinKind::Skill => "skill".into(),
                }),
                title: Some(p.title.clone()),
                detail: Some(p.detail.clone()),
                meta: Some(p.meta.clone()),
                x_pct: Some(p.x_pct),
                y_pct: Some(p.y_pct),
                linked_channel_id: p.linked_channel_id.clone(),
            })
            .collect();
        raw_pins.push(RawCanvasPin {
            id: Some("custom-note".into()),
            kind: Some("note".into()),
            title: Some("Audit".into()),
            x_pct: Some(2.0),
            y_pct: Some(99.0),
            ..Default::default()
        });
        let norm_pins = normalize_canvas_pins(&raw_pins);
        assert_eq!(norm_pins.len(), pins.len() + 1);
        let last = norm_pins.last().unwrap();
        assert_eq!(last.x_pct, 10.0);
        assert_eq!(last.y_pct, 82.0);

        let pin_spot = find_open_pin_spot(
            &seeded.iter().map(|s| (s.x_pct, s.y_pct)).collect::<Vec<_>>(),
            &pins.iter().map(|p| (p.x_pct, p.y_pct)).collect::<Vec<_>>(),
        );
        assert!(pin_spot.0 >= 10.0 && pin_spot.0 <= 90.0);
    }

    #[test]
    fn ts_chats_filter_excludes_opaque_runs_and_labels_bound_hashes() {
        let interactive = normalize_canvas_sessions(&[raw_session(&[
            ("id", "session-repo"),
            ("channelId", "https-github-com-nousresearch-hermes-agent"),
            ("hermesSessionId", "hermes-https-github-com-nousresearch-hermes-agent"),
            ("name", "https://github.com/NousResearch/hermes-agent"),
            ("topic", "https://github.com/NousResearch/hermes-agent"),
        ])])[0]
            .clone();
        let opaque_run = normalize_canvas_sessions(&[raw_session(&[
            ("id", "session-local-5e52c0948b0bfd7907bf92990a4140f94"),
            ("channelId", "local-5e52c0948b0bfd7907bf92990a4140f94"),
            ("hermesSessionId", "local:5e52c0948b0bfd7907bf92990a4140f94"),
            ("name", "local-5e52c0948b0bfd7907bf92990a4140f94"),
            ("topic", "local-5e52c0948b0bfd7907bf92990a4140f94"),
        ])])[0]
            .clone();

        assert!(!is_canvas_background_run(&interactive, None));
        assert!(is_canvas_background_run(&opaque_run, None));

        let chats_only = filter_canvas_sessions(&[interactive.clone(), opaque_run.clone()], CanvasSessionFilter::Chats, &HashMap::new(), None, None);
        assert_eq!(chats_only.len(), 1);
        assert_eq!(chats_only[0].id, interactive.id);

        let hash_label = format_canvas_dot_label(&opaque_run, None, 0);
        assert!(hash_label.primary_label.chars().count() <= 24);
        assert!(!hash_label.primary_label.contains("92990a4140f94"));

        // 14-item multi-row layout avoids horizontal collisions on 4 columns.
        let many: Vec<CanvasSession> = (0..14)
            .map(|i| {
                let mut s = interactive.clone();
                s.id = format!("s-{i}");
                s.channel_id = format!("c-{i}");
                s
            })
            .collect();
        let arranged = auto_arrange_sessions(&many);
        for i in 0..arranged.len() {
            for j in (i + 1)..arranged.len() {
                let dist = ((arranged[i].x_pct - arranged[j].x_pct).powi(2)
                    + (arranged[i].y_pct - arranged[j].y_pct).powi(2))
                .sqrt();
                assert!(dist >= 12.0, "nodes {i}/{j} too close: {dist}");
            }
        }
    }

    #[test]
    fn ts_prune_and_migrate_strips_seeds_and_external_history() {
        let legacy_seed = create_seed_canvas_sessions(&test_channels());
        let opaque_local = normalize_canvas_sessions(&[raw_session(&[
            ("id", "session-local-abc123def456"),
            ("channelId", "local-abc123def456"),
            ("hermesSessionId", "local:abc123def456"),
            ("name", "local-abc123def456"),
            ("topic", "Explain Forgejo CI"),
        ])])[0]
            .clone();
        let external_cached = normalize_canvas_sessions(&[raw_session(&[
            ("id", "session-cli-run"),
            ("channelId", "cli-run"),
            ("hermesSessionId", "hermes-cli-run"),
            ("name", "cli-run"),
            ("topic", "Rename repo"),
        ])])[0]
            .clone();
        let pinned_external = normalize_canvas_sessions(&[raw_session(&[
            ("id", "session-pinned-tg"),
            ("channelId", "tg-deploy"),
            ("hermesSessionId", "tg-deploy"),
            ("name", "Telegram Deploy"),
            ("topic", "Telegram Deploy"),
            ("origin", "imported"),
        ])])[0]
            .clone();
        let user_workspace = normalize_canvas_sessions(&[raw_session(&[
            ("id", "session-my-workspace"),
            ("channelId", "my-workspace"),
            ("hermesSessionId", "hermes-my-workspace"),
            ("name", "my-workspace"),
            ("topic", "Build voice onboarding"),
        ])])[0]
            .clone();

        let gateway = vec![
            GatewaySessionInfo {
                session_id: "hermes-cli-run".into(),
                channel_id: "cli-run".into(),
                name: "cli-run".into(),
                topic: "Rename repo".into(),
                source: Some("cli".into()),
                origin: Some(SessionOrigin::External),
                turn_count: 3,
                updated_at_ms: 1_700_000_000_000,
                ..Default::default()
            },
            GatewaySessionInfo {
                session_id: "hermes-my-workspace".into(),
                session_key: Some("voxcell-my-workspace".into()),
                channel_id: "my-workspace".into(),
                name: "my-workspace".into(),
                topic: "Build voice onboarding".into(),
                source: Some("web".into()),
                origin: Some(SessionOrigin::Workspace),
                turn_count: 2,
                updated_at_ms: 1_700_000_010_000,
                ..Default::default()
            },
        ];

        let mut all = legacy_seed;
        all.extend([opaque_local, external_cached, pinned_external, user_workspace.clone()]);
        let (pruned, changed) = prune_and_migrate_canvas_sessions(&all, &gateway, true);
        assert!(changed);
        assert_eq!(pruned.iter().map(|s| s.channel_id.as_str()).collect::<Vec<_>>(), vec!["tg-deploy", "my-workspace"]);
        assert_eq!(pruned[0].origin, Some(CanvasDotOrigin::Imported));
        assert_eq!(pruned[1].origin, Some(CanvasDotOrigin::Workspace));

        let url_session = CanvasSession {
            channel_id: "repo-check".into(),
            hermes_session_id: "hermes-repo-check".into(),
            name: "repo-check".into(),
            topic: "https://github.com/NousResearch/hermes-agent".into(),
            ..user_workspace.clone()
        };
        let url_label = format_canvas_dot_label(&url_session, None, 2);
        assert!(!url_label.primary_label.starts_with('#'));
        assert_eq!(url_label.primary_label, "github.com/NousResearc…");

        let slug_label = format_canvas_dot_label(&user_workspace, Some(&gateway[1]), 2);
        assert_eq!(slug_label.primary_label, "Build voice onboarding");
        assert!(!slug_label.prompt_target.starts_with('#'));
    }

    #[test]
    fn ts_persistence_roundtrip_uses_v2_keys() {
        assert_eq!(CANVAS_STORAGE_KEY, "duet.canvasSessions.v2");
        assert_eq!(CANVAS_PINS_STORAGE_KEY, "duet.canvasPins.v1");

        let seeded = create_seed_canvas_sessions(&test_channels());
        let json = save_canvas_sessions_json(&seeded);
        assert!(json.contains("\"xPct\""));
        assert!(json.contains("\"hermesSessionId\""));
        // Boot hydration prunes legacy seeds when no gateway sessions exist.
        let reloaded = load_canvas_sessions_from_json(&json);
        assert!(reloaded.is_empty());

        let workspace_dot = normalize_canvas_sessions(&[raw_session(&[
            ("channelId", "my-project"),
            ("hermesSessionId", "hermes-my-project"),
            ("name", "my-project"),
            ("origin", "workspace"),
        ])]);
        let json = save_canvas_sessions_json(&workspace_dot);
        let reloaded = load_canvas_sessions_from_json(&json);
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].channel_id, "my-project");
        assert_eq!(reloaded[0].origin, Some(CanvasDotOrigin::Workspace));

        let pins = create_seed_canvas_pins();
        let pin_json = save_canvas_pins_json(&pins);
        let reloaded_pins = load_canvas_pins_from_json(&pin_json);
        assert_eq!(reloaded_pins.len(), pins.len());
        assert_eq!(load_canvas_sessions_from_json("not json"), Vec::<CanvasSession>::new());
    }

    // ---- Mirrored homeSession.test.ts ----

    #[test]
    fn ts_derive_session_from_prompt_meaningful_unique_slugs() {
        let s1 = derive_session_from_prompt("Can you audit my CI runners and open PRs?", &["general".into(), "ops".into()]);
        assert_eq!(s1.slug, "audit-ci-runners");
        assert_eq!(s1.hermes_session_id, "hermes-audit-ci-runners");
        assert!(s1.persona.to_ascii_lowercase().contains("audit my ci runners"));

        let s2 = derive_session_from_prompt("Can you audit my CI runners?", &["general".into(), "ops".into(), "audit-ci-runners".into()]);
        assert_eq!(s2.slug, "audit-ci-runners-2");
        assert_eq!(s2.hermes_session_id, "hermes-audit-ci-runners-2");

        let s3 = derive_session_from_prompt("help me", &[]);
        assert_eq!(s3.slug, "help-me");
    }

    #[test]
    fn ts_should_start_new_home_session_gating() {
        assert!(should_start_new_home_session(true, "home", "Review backlog"));
        assert!(!should_start_new_home_session(true, "home", "/help"));
        assert!(!should_start_new_home_session(true, "voice", "Review backlog"));
        assert!(!should_start_new_home_session(false, "home", "Follow up question"));
        assert!(!should_start_new_home_session(true, "home", "   "));
    }

    #[test]
    fn ts_should_auto_enter_thread_on_submit_gating() {
        assert!(should_auto_enter_thread_on_submit("home", "Check open PRs", None));
        assert!(!should_auto_enter_thread_on_submit("home", "/skills", None));
        assert!(!should_auto_enter_thread_on_submit("home", "   ", None));
        assert!(!should_auto_enter_thread_on_submit("voice", "Follow up", None));
        assert!(!should_auto_enter_thread_on_submit("home", "Background session prompt", Some("ops")));
    }

    #[test]
    fn ts_composer_placeholder_word_safe() {
        let draft = resolve_composer_placeholder(&ComposerPlaceholderParams {
            is_home_new_draft: true,
            active_channel: "",
            prompt_target: None,
            is_thinking: false,
            is_speaking: false,
            is_listening: false,
            compact: false,
        });
        assert_eq!(draft, "Ask Hermes anything or type / for commands…");

        let long = resolve_composer_placeholder(&ComposerPlaceholderParams {
            is_home_new_draft: false,
            active_channel: "check-build-status-and-pipelines",
            prompt_target: Some("check-build-status-and-pipelines"),
            is_thinking: true,
            is_speaking: false,
            is_listening: false,
            compact: false,
        });
        assert_eq!(long, "Steer #check-build-status… or send next prompt…");

        let mobile = resolve_composer_placeholder(&ComposerPlaceholderParams {
            is_home_new_draft: false,
            active_channel: "check-build-status-and-pipelines",
            prompt_target: Some("check-build-status-and-pipelines"),
            is_thinking: false,
            is_speaking: false,
            is_listening: false,
            compact: true,
        });
        assert_eq!(mobile, "Message #check-build… or /");
        assert!(mobile.chars().count() <= 32);
    }

    #[test]
    fn ts_curated_home_starters_well_formed() {
        assert!(CURATED_HOME_STARTERS.len() >= 3);
        for starter in CURATED_HOME_STARTERS {
            assert!(!starter.id.is_empty());
            assert!(!starter.title.is_empty());
            assert!(!starter.prompt.trim().is_empty());
        }
    }

    #[test]
    fn ts_normalize_duet_channel_id_aliases_and_prefixes() {
        let mut aliases = HashMap::new();
        aliases.insert("chan-k9x2p".to_string(), "audit-ci-runners".to_string());
        aliases.insert("k9x2p".to_string(), "audit-ci-runners".to_string());
        aliases.insert("audit-ci-runners".to_string(), "audit-ci-runners".to_string());
        assert_eq!(normalize_duet_channel_id(Some("chan-k9x2p"), Some(&aliases)), "audit-ci-runners");
        assert_eq!(normalize_duet_channel_id(Some("chan-ops"), None), "ops");
        assert_eq!(normalize_duet_channel_id(Some("#https-github-com"), None), "https-github-com");
        assert_eq!(normalize_duet_channel_id(Some("hermes-https-github-com"), None), "https-github-com");
        assert_eq!(normalize_duet_channel_id(None, None), "general");
    }

    #[test]
    fn ts_is_terminal_settled_item_detection() {
        assert!(!is_terminal_settled_item(Some("assistant"), true, None));
        assert!(is_terminal_settled_item(Some("assistant"), false, None));
        assert!(!is_terminal_settled_item(Some("activity"), false, Some("running")));
        assert!(is_terminal_settled_item(Some("activity"), false, Some("error")));
        assert!(is_terminal_settled_item(Some("activity"), false, Some("done")));
        assert!(!is_terminal_settled_item(Some("user"), false, None));
    }

    #[test]
    fn ts_pick_next_shape_avoids_overuse() {
        let picked = pick_next_shape_and_tint(&[
            (BrailleShapeKind::Clover, BrailleShapeTint::Lime),
            (BrailleShapeKind::Trapezoid, BrailleShapeTint::Amber),
            (BrailleShapeKind::Pebble, BrailleShapeTint::Periwinkle),
            (BrailleShapeKind::Star, BrailleShapeTint::Pink),
            (BrailleShapeKind::Ghost, BrailleShapeTint::Lavender),
        ]);
        assert_ne!(picked.0, BrailleShapeKind::Clover);
    }

    #[test]
    fn ts_build_unique_slug_within_24_chars() {
        assert_eq!(
            build_unique_slug_for_fresh_channel("morning-brief", &["general".into(), "morning-brief".into()]),
            "morning-brief-2"
        );
        let max_base = "ultra-long-session-title";
        let collided = build_unique_slug_for_fresh_channel(max_base, &[max_base.to_string()]);
        assert!(collided.chars().count() <= 24);
        assert!(collided.ends_with("-2"));
    }

    #[test]
    fn ts_derive_session_never_yields_reserved_general() {
        let s = derive_session_from_prompt("general", &[]);
        assert_ne!(s.slug, "general");
        assert!(s.slug.chars().count() <= 24);
    }
}