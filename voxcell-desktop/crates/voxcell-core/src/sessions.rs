//! Port of `src/utils/sidebarSessions.ts` (P1-06 audit) plus the workspace vs
//! gateway scoping helpers from the original draft.
//!
//! The TS-parity layer builds `SidebarSessionEntry` rows from canvas sessions +
//! gateway history: chats vs runs buckets, opaque-ID title cleanup, source
//! labels, multi-profile `@profile` badges, active-work indicators, search and
//! cross-bucket match counts, and the footer LANES strip.

use crate::canvas::{
    create_hermes_session_id, BrailleShapeKind, BrailleShapeTint, CanvasMood, CanvasSession,
    DotAutonomyMode, DotEyeStyle, DotSandboxEnv,
};
use crate::tasks::ChannelTaskSummary;
use crate::types::{ChannelMeta, SessionOrigin, SessionScopeTab, SessionStatus};
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Legacy drafted API (kept for voxcell-ui compatibility)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarSessionBucket {
    Workspace,
    External,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarSessionItem {
    pub channel: ChannelMeta,
    pub bucket: SidebarSessionBucket,
    pub is_active_work: bool,
    pub activity_label: Option<String>,
}

pub fn classify_sidebar_session(ch: &ChannelMeta) -> SidebarSessionBucket {
    match ch.origin {
        SessionOrigin::External => SidebarSessionBucket::External,
        SessionOrigin::Workspace => SidebarSessionBucket::Workspace,
    }
}

pub fn build_sidebar_session_list(
    channels: &[ChannelMeta],
    scope: SessionScopeTab,
    search_query: &str,
    task_summary_by_channel: &HashMap<String, ChannelTaskSummary>,
) -> Vec<SidebarSessionItem> {
    let q = search_query.trim().to_ascii_lowercase();
    let target_bucket = match scope {
        SessionScopeTab::Sessions => SidebarSessionBucket::Workspace,
        SessionScopeTab::Gateway => SidebarSessionBucket::External,
    };

    let mut items: Vec<SidebarSessionItem> = channels
        .iter()
        .filter(|ch| classify_sidebar_session(ch) == target_bucket)
        .filter(|ch| {
            if q.is_empty() {
                return true;
            }
            ch.name.to_ascii_lowercase().contains(&q)
                || ch.description.to_ascii_lowercase().contains(&q)
                || ch.session_id.to_ascii_lowercase().contains(&q)
        })
        .map(|ch| {
            let summary = task_summary_by_channel.get(&ch.id);
            let is_active_work = summary.is_some_and(|s| s.has_active_task || s.paused_count > 0);
            let activity_label = summary.and_then(|s| {
                if s.running_count > 0 {
                    Some(format!("{} running", s.running_count))
                } else if s.paused_count > 0 {
                    Some(format!("{} waiting", s.paused_count))
                } else if s.is_thinking {
                    Some("thinking".to_string())
                } else {
                    None
                }
            });
            SidebarSessionItem {
                channel: ch.clone(),
                bucket: target_bucket,
                is_active_work,
                activity_label,
            }
        })
        .collect();

    items.sort_by(|a, b| {
        b.is_active_work
            .cmp(&a.is_active_work)
            .then_with(|| {
                b.channel
                    .updated_at_ms
                    .unwrap_or(0)
                    .cmp(&a.channel.updated_at_ms.unwrap_or(0))
            })
            .then_with(|| a.channel.name.cmp(&b.channel.name))
    });
    items
}

pub fn count_sidebar_buckets(channels: &[ChannelMeta]) -> (usize, usize) {
    let mut workspace = 0;
    let mut external = 0;
    for ch in channels {
        match classify_sidebar_session(ch) {
            SidebarSessionBucket::Workspace => workspace += 1,
            SidebarSessionBucket::External => external += 1,
        }
    }
    (workspace, external)
}

// ---------------------------------------------------------------------------
// TS-parity layer (sidebarSessions.ts)
// ---------------------------------------------------------------------------

/// Rich gateway session record mirroring TS `HermesAgentSession` (the shared
/// `voxcell-core::types::HermesAgentSession` predates profile/origin/topic
/// fields; this local type carries the full sidebar surface).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GatewaySessionInfo {
    pub session_id: String,
    pub session_key: Option<String>,
    pub channel_id: String,
    pub name: String,
    pub persona: String,
    pub topic: String,
    pub profile: Option<String>,
    pub status: SessionStatus,
    pub turn_count: usize,
    pub last_message: Option<String>,
    pub updated_at_ms: u64,
    pub source: Option<String>,
    pub model: Option<String>,
    pub origin: Option<SessionOrigin>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionBucket {
    Chats,
    Runs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarFilterTab {
    Chats,
    Runs,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarOriginFilter {
    All,
    Workspace,
    External,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SidebarSessionEntry {
    pub session: CanvasSession,
    pub hermes_session: Option<GatewaySessionInfo>,
    pub origin: SessionOrigin,
    pub on_canvas: bool,
    pub bucket: SessionBucket,
    pub display_title: String,
    pub tooltip_title: String,
    pub source_label: String,
    pub relative_time: String,
    pub metadata_line: String,
    pub updated_at_ms: u64,
    pub turn_count: usize,
    pub has_active_work: bool,
    pub running_count: usize,
    pub active_task_title: Option<String>,
    pub is_opaque_id: bool,
    pub search_text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SidebarSessionFilterResult {
    pub visible: Vec<SidebarSessionEntry>,
    pub chats_count: usize,
    pub runs_count: usize,
    pub workspace_count: usize,
    pub external_count: usize,
    pub total_count: usize,
    pub cross_bucket_matches: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FooterLane {
    pub channel: ChannelMeta,
    pub short_label: String,
}

// ---- opaque identifier matchers (regex-equivalent, dependency-free) ----

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

fn is_hex_run(s: &str, min_len: usize) -> bool {
    s.chars().count() >= min_len && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// `^(?:#?hermes-)?(local|run)[-_]([0-9a-f]{6,})$` (sidebar variant) or
/// `[-_:]` separators (hermesProtocol variant). Returns the hex capture.
fn opaque_hash_capture(raw: &str, kind: &str, allow_colon: bool) -> Option<String> {
    let s = raw.strip_prefix('#').unwrap_or(raw);
    let body = strip_prefix_ci(s, "hermes-").unwrap_or(s);
    let seps: &[char] = if allow_colon { &['-', '_', ':'] } else { &['-', '_'] };
    for sep in seps {
        let prefix = format!("{kind}{sep}");
        if let Some(rest) = strip_prefix_ci(body, &prefix) {
            if is_hex_run(rest, 6) {
                return Some(rest.to_string());
            }
        }
    }
    None
}

fn opaque_cron_capture(raw: &str, allow_colon: bool) -> Option<String> {
    let s = raw.strip_prefix('#').unwrap_or(raw);
    let body = strip_prefix_ci(s, "hermes-").unwrap_or(s);
    let seps: &[char] = if allow_colon { &['-', '_', ':'] } else { &['-', '_'] };
    for sep in seps {
        let prefix = format!("cron{sep}");
        if let Some(rest) = strip_prefix_ci(body, &prefix) {
            if !rest.is_empty() && !rest.contains('\n') {
                return Some(rest.to_string());
            }
        }
    }
    None
}

fn is_uuid(raw: &str) -> bool {
    let s = raw.strip_prefix('#').unwrap_or(raw);
    let body = strip_prefix_ci(s, "hermes-").unwrap_or(s);
    let groups: Vec<&str> = body.split('-').collect();
    if groups.len() != 5 {
        return false;
    }
    let lens = [8, 4, 4, 4, 12];
    groups
        .iter()
        .zip(lens)
        .all(|(g, len)| g.chars().count() == len && g.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_raw_hex(raw: &str) -> bool {
    let s = raw.strip_prefix('#').unwrap_or(raw);
    let body = strip_prefix_ci(s, "hermes-").unwrap_or(s);
    is_hex_run(body, 20)
}

/// Mirrors sidebarSessions `isOpaqueSessionIdentifier`.
pub fn is_opaque_session_identifier(raw: Option<&str>) -> bool {
    let s = raw.unwrap_or("").trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.is_empty() {
        return false;
    }
    opaque_hash_capture(s, "local", false).is_some()
        || opaque_hash_capture(s, "run", false).is_some()
        || opaque_cron_capture(s, false).is_some()
        || is_uuid(s)
        || is_raw_hex(s)
}

/// hermesProtocol variant (allows `:` separators) used for origin classification.
fn is_opaque_protocol(raw: Option<&str>) -> bool {
    let s = raw.unwrap_or("").trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.is_empty() {
        return false;
    }
    opaque_hash_capture(s, "local", true).is_some()
        || opaque_hash_capture(s, "run", true).is_some()
        || opaque_cron_capture(s, true).is_some()
        || is_uuid(s)
        || is_raw_hex(s)
}

/// Mirrors hermesProtocol `classifyGatewaySessionOrigin`: workspace-created
/// sessions (voxcell-* keys, hermes-<slug> ids, web/voxcell/ui sources) vs
/// external hermes-agent history (cli, local:* API, cron, telegram, ...).
pub fn classify_gateway_session_origin(
    session_id: Option<&str>,
    session_key: Option<&str>,
    channel_id: Option<&str>,
    source: Option<&str>,
    created_in_workspace: bool,
) -> SessionOrigin {
    if created_in_workspace {
        return SessionOrigin::Workspace;
    }
    let key = session_key.unwrap_or("").trim().to_ascii_lowercase();
    if key.starts_with("voxcell-") {
        return SessionOrigin::Workspace;
    }
    let raw_source = source.unwrap_or("").trim().to_ascii_lowercase();
    if matches!(raw_source.as_str(), "web" | "voxcell" | "ui") {
        return SessionOrigin::Workspace;
    }
    if matches!(
        raw_source.as_str(),
        "cli" | "cron" | "scheduler" | "daemon" | "telegram" | "discord" | "slack" | "whatsapp" | "signal"
    ) {
        return SessionOrigin::External;
    }
    let sid = session_id.unwrap_or("").trim();
    let cid = channel_id.unwrap_or("").trim();
    if sid.to_ascii_lowercase().starts_with("hermes-")
        && !is_opaque_protocol(Some(sid))
        && !is_opaque_protocol(Some(cid))
    {
        return SessionOrigin::Workspace;
    }
    SessionOrigin::External
}

fn source_label_lookup(raw: &str) -> Option<&'static str> {
    Some(match raw {
        "cli" => "CLI",
        "telegram" => "Telegram",
        "discord" => "Discord",
        "slack" => "Slack",
        "whatsapp" => "WhatsApp",
        "signal" => "Signal",
        "cron" | "scheduler" | "daemon" => "Cron",
        "web" | "voxcell" | "ui" => "Web",
        "api" | "api_server" => "API",
        "runs" => "Run",
        _ => return None,
    })
}

pub fn format_source_label(source: Option<&str>, session_id: Option<&str>, channel_id: Option<&str>) -> String {
    let raw_source = source.unwrap_or("").trim().to_ascii_lowercase();
    let sid = session_id.unwrap_or("").trim();
    let cid = channel_id.unwrap_or("").trim().to_ascii_lowercase();

    if raw_source == "cron"
        || cid == "daemon"
        || opaque_cron_capture(sid, false).is_some()
        || opaque_cron_capture(&cid, false).is_some()
    {
        return "Cron".to_string();
    }
    if opaque_hash_capture(sid, "run", false).is_some() || opaque_hash_capture(&cid, "run", false).is_some() {
        return "Run".to_string();
    }
    if opaque_hash_capture(sid, "local", false).is_some() || opaque_hash_capture(&cid, "local", false).is_some() {
        return "Local API".to_string();
    }
    if !raw_source.is_empty() {
        if let Some(label) = source_label_lookup(&raw_source) {
            if (raw_source == "api" || raw_source == "api_server") && sid.starts_with("hermes-") {
                return "Web".to_string();
            }
            return label.to_string();
        }
        let mut chars = raw_source.chars();
        if let Some(first) = chars.next() {
            return first.to_uppercase().collect::<String>() + chars.as_str();
        }
    }
    if sid.starts_with("hermes-") {
        "Web".to_string()
    } else {
        "Session".to_string()
    }
}

fn clean_display_candidate(raw: &str) -> String {
    let trimmed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        return String::new();
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let scheme_len = if lower.starts_with("https://") { 8 } else { 7 };
        let rest = &trimmed[scheme_len..];
        let rest = strip_prefix_ci(rest, "www.").unwrap_or(rest);
        return rest.strip_suffix('/').unwrap_or(rest).to_string();
    }
    trimmed
}

fn first_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Strips a trailing `[-_]\d{6,}$` timestamp suffix (leftmost valid match).
fn strip_cron_timestamp_suffix(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    for (idx, c) in chars.iter().enumerate() {
        if *c == '-' || *c == '_' {
            let rest: String = chars[idx + 1..].iter().collect();
            if rest.chars().count() >= 6 && rest.chars().all(|d| d.is_ascii_digit()) {
                return chars[..idx].iter().collect();
            }
        }
    }
    raw.to_string()
}

fn format_opaque_fallback(session_id: &str, channel_id: &str, source_label: &str) -> String {
    let sid = session_id.trim();
    let cid = channel_id.trim();
    let src = {
        let s = source_label.trim();
        if s.is_empty() { "Session" } else { s }
    };

    if let Some(cap) = opaque_hash_capture(sid, "local", false).or_else(|| opaque_hash_capture(cid, "local", false)) {
        return format!("Local API · {}", first_chars(&cap, 6));
    }
    if let Some(cap) = opaque_hash_capture(sid, "run", false).or_else(|| opaque_hash_capture(cid, "run", false)) {
        return format!("Gateway run · {}", first_chars(&cap, 6));
    }
    if let Some(cap) = opaque_cron_capture(sid, false).or_else(|| opaque_cron_capture(cid, false)) {
        let clean_cron = first_chars(&strip_cron_timestamp_suffix(&cap), 14);
        return if clean_cron.is_empty() {
            "Scheduled cron".to_string()
        } else {
            format!("Cron · {clean_cron}")
        };
    }
    let short_hex = first_chars(strip_prefix_ci(sid, "hermes-").unwrap_or(sid), 8);
    let tail = if !short_hex.is_empty() {
        short_hex
    } else if !cid.is_empty() {
        cid.to_string()
    } else {
        "session".to_string()
    };
    format!("{src} · {tail}")
}

fn is_meaningful_label(raw: Option<&str>, normalized_sid: &str) -> Option<String> {
    let s = raw?.trim();
    if s.is_empty() {
        return None;
    }
    let no_hash = s.strip_prefix('#').unwrap_or(s);
    if is_opaque_session_identifier(Some(no_hash)) {
        return None;
    }
    let cleaned = clean_display_candidate(no_hash);
    if cleaned.is_empty() {
        return None;
    }
    if cleaned.to_ascii_lowercase() == normalized_sid && is_opaque_session_identifier(Some(&cleaned)) {
        return None;
    }
    Some(cleaned)
}

/// Mirrors `formatDisplaySessionTitle`: readable titles without hash noise.
pub fn format_display_session_title(
    topic: Option<&str>,
    name: Option<&str>,
    channel_id: Option<&str>,
    session_id: Option<&str>,
    last_message: Option<&str>,
    source_label: Option<&str>,
) -> (String, bool) {
    let safe_channel_id = channel_id.unwrap_or("").trim();
    let safe_session_id = session_id.unwrap_or("").trim();
    let safe_source_label = {
        let s = source_label.unwrap_or("Session").trim();
        if s.is_empty() { "Session" } else { s }
    };
    let normalized_channel = safe_channel_id.to_ascii_lowercase();
    let normalized_sid = safe_session_id.to_ascii_lowercase();

    let topic_label = is_meaningful_label(topic, &normalized_sid);
    if let Some(t) = &topic_label {
        if t.to_ascii_lowercase() != normalized_channel {
            return (t.clone(), false);
        }
    }

    let name_label = is_meaningful_label(name, &normalized_sid);
    if let Some(n) = &name_label {
        if n.to_ascii_lowercase() != normalized_channel {
            return (n.clone(), false);
        }
    }

    if let Some(preview) = is_meaningful_label(last_message, &normalized_sid) {
        let title = if preview.chars().count() > 64 {
            format!("{}…", first_chars(&preview, 61).trim_end())
        } else {
            preview
        };
        return (title, false);
    }

    if let Some(t) = topic_label {
        return (t, false);
    }
    if let Some(n) = name_label {
        if !is_opaque_session_identifier(Some(&n)) {
            return (n, false);
        }
    }

    (format_opaque_fallback(safe_session_id, safe_channel_id, safe_source_label), true)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionBucketParams<'a> {
    pub session_id: Option<&'a str>,
    pub channel_id: Option<&'a str>,
    pub source: Option<&'a str>,
    pub is_opaque_title: bool,
    pub turn_count: usize,
    pub has_active_work: bool,
}

/// Separates interactive chats from automated runs & ephemeral local API scratch.
pub fn classify_session_bucket(params: &SessionBucketParams<'_>) -> SessionBucket {
    let src = params.source.unwrap_or("").trim().to_ascii_lowercase();
    let sid = params.session_id.unwrap_or("").trim();
    let cid = params.channel_id.unwrap_or("").trim().to_ascii_lowercase();

    if matches!(src.as_str(), "cron" | "scheduler" | "daemon")
        || cid == "daemon"
        || opaque_cron_capture(sid, false).is_some()
        || opaque_cron_capture(&cid, false).is_some()
        || opaque_hash_capture(sid, "run", false).is_some()
        || opaque_hash_capture(&cid, "run", false).is_some()
    {
        return SessionBucket::Runs;
    }

    if (opaque_hash_capture(sid, "local", false).is_some()
        || opaque_hash_capture(&cid, "local", false).is_some()
        || is_uuid(sid)
        || is_raw_hex(sid))
        && params.is_opaque_title
        && !params.has_active_work
        && params.turn_count <= 1
    {
        return SessionBucket::Runs;
    }

    SessionBucket::Chats
}

pub fn format_compact_relative_time(epoch_ms: Option<u64>, now_ms: u64) -> String {
    let Some(epoch) = epoch_ms.filter(|v| *v != 0) else {
        return String::new();
    };
    let diff_ms = now_ms.saturating_sub(epoch);
    let mins = diff_ms / 60_000;
    if mins < 1 {
        return "now".to_string();
    }
    if mins < 60 {
        return format!("{mins}m");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = hours / 24;
    if days < 30 {
        return format!("{days}d");
    }
    let months = days / 30;
    if months < 12 {
        return format!("{months}mo");
    }
    format!("{}y", months / 12)
}

fn synthesize_session_from_gateway(hs: &GatewaySessionInfo) -> CanvasSession {
    let channel_id = if hs.channel_id.is_empty() {
        hs.session_id.clone()
    } else {
        hs.channel_id.clone()
    };
    let name_seed = if !hs.name.is_empty() {
        hs.name.clone()
    } else if !hs.topic.is_empty() {
        hs.topic.clone()
    } else {
        channel_id.clone()
    };
    CanvasSession {
        id: format!("gw-{channel_id}"),
        hermes_session_id: if hs.session_id.is_empty() {
            create_hermes_session_id(&channel_id)
        } else {
            hs.session_id.clone()
        },
        channel_id: channel_id.clone(),
        profile: hs.profile.clone().filter(|p| !p.is_empty()),
        agent_persona: if hs.persona.is_empty() {
            "Hermes Agent session synced from gateway.".to_string()
        } else {
            hs.persona.clone()
        },
        name: name_seed.strip_prefix('#').unwrap_or(&name_seed).to_string(),
        topic: if !hs.topic.is_empty() {
            hs.topic.clone()
        } else if !hs.name.is_empty() {
            hs.name.clone()
        } else {
            channel_id.clone()
        },
        shape: BrailleShapeKind::Clover,
        tint: BrailleShapeTint::Lime,
        x_pct: 50.0,
        y_pct: 46.0,
        mood: CanvasMood::Happy,
        eye_style: DotEyeStyle::Open,
        is_core: Some(false),
        origin: if hs.origin == Some(SessionOrigin::Workspace) {
            Some(crate::canvas::CanvasDotOrigin::Workspace)
        } else {
            None
        },
        autonomy_mode: DotAutonomyMode::Manual,
        schedule_label: "On-demand".to_string(),
        autonomy_paused: Some(false),
        sandbox_env: DotSandboxEnv::CloudSandbox,
        workspace_dir: format!("~/hermes/dots/{channel_id}"),
        skills: Vec::new(),
        connected_to: None,
    }
}

/// Builds sidebar rows from canvas dots + gateway history, pins active work
/// first, sorts by recency, and derives profile badges / metadata lines.
pub fn build_sidebar_session_entries(
    canvas_sessions: &[CanvasSession],
    hermes_sessions: &[GatewaySessionInfo],
    activity_by_channel: &HashMap<String, ChannelTaskSummary>,
    now_ms: u64,
) -> Vec<SidebarSessionEntry> {
    let mut hermes_by_key: HashMap<String, &GatewaySessionInfo> = HashMap::new();
    for hs in hermes_sessions {
        if !hs.channel_id.is_empty() {
            hermes_by_key.insert(hs.channel_id.clone(), hs);
        }
        if !hs.session_id.is_empty() {
            hermes_by_key.insert(hs.session_id.clone(), hs);
        }
    }

    let mut candidates: Vec<(CanvasSession, bool)> = Vec::new();
    let mut seen_channel_ids: HashSet<String> = HashSet::new();
    let mut seen_session_ids: HashSet<String> = HashSet::new();

    for s in canvas_sessions {
        if s.is_core != Some(false) && s.channel_id != "daemon" {
            continue;
        }
        seen_channel_ids.insert(s.channel_id.clone());
        if !s.hermes_session_id.is_empty() {
            seen_session_ids.insert(s.hermes_session_id.clone());
        }
        candidates.push((s.clone(), true));
    }

    for hs in hermes_sessions {
        if seen_channel_ids.contains(&hs.channel_id) || seen_session_ids.contains(&hs.session_id) {
            continue;
        }
        seen_channel_ids.insert(hs.channel_id.clone());
        seen_session_ids.insert(hs.session_id.clone());
        candidates.push((synthesize_session_from_gateway(hs), false));
    }

    let mut indexed: Vec<(usize, SidebarSessionEntry)> = Vec::new();
    for (original_index, (session, on_canvas)) in candidates.iter().enumerate() {
        let hs = hermes_by_key
            .get(&session.channel_id)
            .or_else(|| hermes_by_key.get(&session.hermes_session_id))
            .copied();
        let activity = activity_by_channel.get(&session.channel_id);
        let has_active_work = activity.is_some_and(|a| a.has_active_task)
            || hs.is_some_and(|h| {
                matches!(h.status, SessionStatus::Thinking | SessionStatus::Running | SessionStatus::Speaking)
            });
        let running_count = activity.map(|a| a.running_count).unwrap_or(0);

        let origin = if matches!(
            session.origin,
            Some(crate::canvas::CanvasDotOrigin::Workspace | crate::canvas::CanvasDotOrigin::Imported)
        ) {
            SessionOrigin::Workspace
        } else if let Some(h) = hs {
            h.origin.unwrap_or_else(|| {
                classify_gateway_session_origin(
                    Some(&h.session_id),
                    h.session_key.as_deref(),
                    Some(&session.channel_id),
                    h.source.as_deref(),
                    false,
                )
            })
        } else {
            classify_gateway_session_origin(
                Some(&session.hermes_session_id),
                None,
                Some(&session.channel_id),
                None,
                false,
            )
        };

        let source_label = format_source_label(
            hs.and_then(|h| h.source.as_deref()),
            Some(&session.hermes_session_id),
            Some(&session.channel_id),
        );
        let topic = hs
            .map(|h| h.topic.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| session.topic.clone());
        let name = hs
            .map(|h| h.name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| session.name.clone());
        let (display_title, is_opaque) = format_display_session_title(
            Some(&topic),
            Some(&name),
            Some(&session.channel_id),
            Some(&session.hermes_session_id),
            hs.and_then(|h| h.last_message.as_deref()),
            Some(&source_label),
        );

        let turn_count = hs.map(|h| h.turn_count).unwrap_or(0);
        let updated_at_ms = hs.map(|h| h.updated_at_ms).unwrap_or(0);
        let relative_time = format_compact_relative_time(Some(updated_at_ms), now_ms);
        let bucket = classify_session_bucket(&SessionBucketParams {
            session_id: Some(&session.hermes_session_id),
            channel_id: Some(&session.channel_id),
            source: hs.and_then(|h| h.source.as_deref()),
            is_opaque_title: is_opaque,
            turn_count,
            has_active_work,
        });

        let profile_id = hs
            .and_then(|h| h.profile.clone())
            .or_else(|| session.profile.clone())
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut meta_parts: Vec<String> = Vec::new();
        if !is_opaque {
            meta_parts.push(source_label.clone());
        }
        if !profile_id.is_empty() && profile_id != "default" {
            meta_parts.push(format!("@{profile_id}"));
        }
        if turn_count > 0 {
            meta_parts.push(format!("{turn_count} {}", if turn_count == 1 { "turn" } else { "turns" }));
        }
        let model = hs.and_then(|h| h.model.clone()).unwrap_or_default();
        if !model.is_empty() && model != "hermes-agent" {
            let short_model = model.split('/').last().filter(|s| !s.is_empty()).unwrap_or(&model);
            meta_parts.push(short_model.to_string());
        } else if is_opaque {
            meta_parts.push(format!("{}…", first_chars(&session.hermes_session_id, 14)));
        } else if !session.channel_id.is_empty()
            && !is_opaque_session_identifier(Some(&session.channel_id))
            && session.channel_id != "daemon"
            && !display_title
                .to_ascii_lowercase()
                .contains(&session.channel_id.to_ascii_lowercase().replace('-', " "))
        {
            meta_parts.push(format!("#{}", session.channel_id));
        }
        if meta_parts.is_empty() {
            meta_parts.push(source_label.clone());
        }

        let active_task_title = activity
            .and_then(|a| a.primary_title.clone())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        let active_lead_label = if !profile_id.is_empty() && profile_id != "default" {
            format!("{source_label} · @{profile_id}")
        } else {
            source_label.clone()
        };
        let metadata_line = match (&active_task_title, has_active_work) {
            (Some(title), true) => format!("{active_lead_label} · {title}"),
            _ => meta_parts.join(" · "),
        };

        let search_text = [
            display_title.clone(),
            session.topic.clone(),
            session.name.clone(),
            session.channel_id.clone(),
            session.hermes_session_id.clone(),
            profile_id.clone(),
            source_label.clone(),
            hs.and_then(|h| h.source.clone()).unwrap_or_default(),
            model.clone(),
            active_task_title.clone().unwrap_or_default(),
        ]
        .join(" ")
        .to_ascii_lowercase();

        let mut tooltip_parts = vec![display_title.clone()];
        if !profile_id.is_empty() && profile_id != "default" {
            tooltip_parts.push(format!("@{profile_id}"));
        }
        if !source_label.is_empty() {
            tooltip_parts.push(source_label.clone());
        }
        if !relative_time.is_empty() {
            tooltip_parts.push(relative_time.clone());
        }
        tooltip_parts.push(session.hermes_session_id.clone());

        indexed.push((
            original_index,
            SidebarSessionEntry {
                session: session.clone(),
                hermes_session: hs.cloned(),
                origin,
                on_canvas: *on_canvas,
                bucket,
                display_title,
                tooltip_title: tooltip_parts.join(" · "),
                source_label,
                relative_time,
                metadata_line,
                updated_at_ms,
                turn_count,
                has_active_work,
                running_count,
                active_task_title,
                is_opaque_id: is_opaque,
                search_text,
            },
        ));
    }

    indexed.sort_by(|a, b| {
        b.1.has_active_work
            .cmp(&a.1.has_active_work)
            .then_with(|| b.1.updated_at_ms.cmp(&a.1.updated_at_ms))
            .then_with(|| b.0.cmp(&a.0))
    });
    indexed.into_iter().map(|(_, entry)| entry).collect()
}

pub fn resolve_effective_sidebar_tab(requested_tab: SidebarFilterTab, entries: &[SidebarSessionEntry]) -> SidebarFilterTab {
    if entries.is_empty() {
        return SidebarFilterTab::All;
    }
    let chats_count = entries.iter().filter(|e| e.bucket == SessionBucket::Chats).count();
    let runs_count = entries.len() - chats_count;
    if requested_tab == SidebarFilterTab::Chats && chats_count == 0 && runs_count > 0 {
        return SidebarFilterTab::Runs;
    }
    if requested_tab == SidebarFilterTab::Runs && runs_count == 0 && chats_count > 0 {
        return SidebarFilterTab::Chats;
    }
    requested_tab
}

pub fn filter_sidebar_session_entries(
    entries: &[SidebarSessionEntry],
    tab: SidebarFilterTab,
    query: &str,
    origin_filter: SidebarOriginFilter,
) -> SidebarSessionFilterResult {
    let workspace_count = entries.iter().filter(|e| e.origin == SessionOrigin::Workspace).count();
    let external_count = entries.len() - workspace_count;
    let scoped: Vec<&SidebarSessionEntry> = match origin_filter {
        SidebarOriginFilter::All => entries.iter().collect(),
        SidebarOriginFilter::Workspace => entries.iter().filter(|e| e.origin == SessionOrigin::Workspace).collect(),
        SidebarOriginFilter::External => entries.iter().filter(|e| e.origin == SessionOrigin::External).collect(),
    };

    let chats_count = scoped.iter().filter(|e| e.bucket == SessionBucket::Chats).count();
    let runs_count = scoped.len() - chats_count;
    let scoped_owned: Vec<SidebarSessionEntry> = scoped.iter().map(|e| (*e).clone()).collect();
    let effective_tab = resolve_effective_sidebar_tab(tab, &scoped_owned);
    let q = query.trim().to_ascii_lowercase();

    let matches_query = |entry: &SidebarSessionEntry| q.is_empty() || entry.search_text.contains(&q);

    let visible: Vec<SidebarSessionEntry> = scoped
        .iter()
        .filter(|entry| {
            if effective_tab == SidebarFilterTab::Chats && entry.bucket != SessionBucket::Chats {
                return false;
            }
            if effective_tab == SidebarFilterTab::Runs && entry.bucket != SessionBucket::Runs {
                return false;
            }
            matches_query(entry)
        })
        .map(|e| (*e).clone())
        .collect();

    let cross_bucket_matches = if !q.is_empty() && effective_tab != SidebarFilterTab::All {
        scoped
            .iter()
            .filter(|entry| {
                let bucket_tab = if entry.bucket == SessionBucket::Chats {
                    SidebarFilterTab::Chats
                } else {
                    SidebarFilterTab::Runs
                };
                bucket_tab != effective_tab && matches_query(entry)
            })
            .count()
    } else {
        0
    };

    SidebarSessionFilterResult {
        visible,
        chats_count,
        runs_count,
        workspace_count,
        external_count,
        total_count: scoped.len(),
        cross_bucket_matches,
    }
}

pub fn compute_footer_lanes(channels: &[ChannelMeta], sidebar_entries: &[SidebarSessionEntry]) -> Vec<FooterLane> {
    let sidebar_channel_ids: HashSet<&str> = sidebar_entries.iter().map(|e| e.session.channel_id.as_str()).collect();
    channels
        .iter()
        .filter(|ch| {
            if sidebar_channel_ids.contains(ch.id.as_str()) {
                return false;
            }
            if ch.id == "daemon" {
                return false;
            }
            if is_opaque_session_identifier(Some(&ch.id)) || is_opaque_session_identifier(Some(&ch.session_id)) {
                return false;
            }
            true
        })
        .map(|ch| {
            let raw_candidate = if ch.id.chars().count() <= 18 {
                ch.id.clone()
            } else {
                first_chars(ch.name.strip_prefix('#').unwrap_or(&ch.name), 16)
            };
            let short_label = if raw_candidate.starts_with('#') {
                raw_candidate
            } else {
                format!("#{raw_candidate}")
            };
            FooterLane { channel: ch.clone(), short_label }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{normalize_canvas_sessions, RawCanvasSession};

    // ---- Legacy draft test (kept) ----

    fn mk(id: &str, name: &str, origin: SessionOrigin, updated: u64) -> ChannelMeta {
        ChannelMeta {
            id: id.into(),
            name: name.into(),
            category: "hermes".into(),
            description: String::new(),
            session_id: format!("s-{id}"),
            unread: 0,
            origin,
            has_local_history: origin == SessionOrigin::Workspace,
            model: None,
            updated_at_ms: Some(updated),
        }
    }

    #[test]
    fn separates_workspace_and_external_gateway_sessions() {
        let channels = vec![
            mk("1", "Home Chat", SessionOrigin::Workspace, 10),
            mk("2", "Cron Sync", SessionOrigin::External, 20),
            mk("3", "Rune Port", SessionOrigin::Workspace, 30),
        ];
        assert_eq!(count_sidebar_buckets(&channels), (2, 1));
        let ws = build_sidebar_session_list(&channels, SessionScopeTab::Sessions, "", &HashMap::new());
        assert_eq!(ws.len(), 2);
        assert_eq!(ws[0].channel.id, "3");

        let gw = build_sidebar_session_list(&channels, SessionScopeTab::Gateway, "cron", &HashMap::new());
        assert_eq!(gw.len(), 1);
        assert_eq!(gw[0].channel.id, "2");
    }

    // ---- Mirrored sidebarSessions.test.ts ----

    fn make_canvas_session(channel_id: &str, overrides: &[(&str, &str)]) -> CanvasSession {
        let mut raw = RawCanvasSession {
            id: Some(format!("session-{channel_id}")),
            channel_id: Some(channel_id.into()),
            hermes_session_id: Some(format!("hermes-{channel_id}")),
            name: Some(channel_id.into()),
            topic: Some(channel_id.into()),
            is_core: Some(false),
            ..Default::default()
        };
        for (k, v) in overrides {
            match *k {
                "id" => raw.id = Some((*v).into()),
                "channelId" => raw.channel_id = Some((*v).into()),
                "hermesSessionId" => raw.hermes_session_id = Some((*v).into()),
                "name" => raw.name = Some((*v).into()),
                "topic" => raw.topic = Some((*v).into()),
                "origin" => raw.origin = Some((*v).into()),
                _ => {}
            }
        }
        normalize_canvas_sessions(&[raw]).into_iter().next().unwrap()
    }

    fn make_hermes_session(channel_id: &str) -> GatewaySessionInfo {
        GatewaySessionInfo {
            session_id: format!("hermes-{channel_id}"),
            channel_id: channel_id.into(),
            name: channel_id.into(),
            topic: String::new(),
            status: SessionStatus::Idle,
            turn_count: 0,
            updated_at_ms: 1_700_000_000_000,
            ..Default::default()
        }
    }

    #[test]
    fn ts_opaque_ids_and_readable_titles() {
        assert!(is_opaque_session_identifier(Some("local-5e52c0948b0bfd7907bf92990a4140f94")));
        assert!(is_opaque_session_identifier(Some("run_6f2b7fcbed7c4eaeba5e67564d0a4958")));
        assert!(is_opaque_session_identifier(Some("cron_morning_17123456")));
        assert!(!is_opaque_session_identifier(Some("fill-me-in-on-what")));

        let hash = "local-5e52c0948b0bfd7907bf92990a4140f94";
        let (title, opaque) = format_display_session_title(Some(hash), Some(hash), Some(hash), Some(hash), None, Some("Local API"));
        assert_eq!(title, "Local API · 5e52c0");
        assert!(opaque);

        let run_hash = "run_6f2b7fcbed7c4eaeba5e67564d0a4958";
        let (title, opaque) = format_display_session_title(Some(""), Some(run_hash), Some(run_hash), Some(run_hash), None, Some("Run"));
        assert_eq!(title, "Gateway run · 6f2b7f");
        assert!(opaque);

        let (title, opaque) = format_display_session_title(
            Some("https://github.com/NousResearch/hermes-agent/issues/42"),
            Some("https-github-com"),
            Some("https-github-com"),
            Some("hermes-https-github-com"),
            None,
            Some("Web"),
        );
        assert_eq!(title, "github.com/NousResearch/hermes-agent/issues/42");
        assert!(!opaque);

        let hash2 = "local-07ee2d7b545bd3db563f9fcd83fb72482";
        let (title, opaque) = format_display_session_title(
            Some(hash2),
            Some(hash2),
            Some(hash2),
            Some(hash2),
            Some("Summarize overnight test failures across CI"),
            Some("Local API"),
        );
        assert_eq!(title, "Summarize overnight test failures across CI");
        assert!(!opaque);
    }

    #[test]
    fn ts_source_labels_and_relative_timestamps() {
        assert_eq!(format_source_label(Some("cli"), Some("cli_20260412"), Some("cli-20260412")), "CLI");
        assert_eq!(format_source_label(Some("telegram"), Some("tg-991"), Some("tg-991")), "Telegram");
        assert_eq!(format_source_label(Some("api_server"), Some("hermes-fill-me-in"), Some("fill-me-in")), "Web");
        assert_eq!(
            format_source_label(Some("api"), Some("local-5e52c0948b0bfd7907bf92990a4140f94"), Some("local-5e52c094")),
            "Local API"
        );
        assert_eq!(format_source_label(None, Some("run_6f2b7fcbed7c4eaeba5e67564d0a4958"), Some("run_6f2b")), "Run");
        assert_eq!(format_source_label(Some("cron"), Some("cron_watch_1"), Some("daemon")), "Cron");

        let now = 1_700_000_000_000u64;
        assert_eq!(format_compact_relative_time(Some(now - 20_000), now), "now");
        assert_eq!(format_compact_relative_time(Some(now - 14 * 60_000), now), "14m");
        assert_eq!(format_compact_relative_time(Some(now - 3 * 3_600_000), now), "3h");
        assert_eq!(format_compact_relative_time(Some(now - 4 * 86_400_000), now), "4d");
    }

    #[test]
    fn ts_bucket_classification_chats_vs_runs() {
        assert_eq!(
            classify_session_bucket(&SessionBucketParams {
                session_id: Some("local-5e52c0948b0bfd7907bf92990a4140f94"),
                channel_id: Some("local-5e52c0948b0bfd7907bf92990a4140f94"),
                source: Some("api"),
                is_opaque_title: true,
                turn_count: 0,
                has_active_work: false,
            }),
            SessionBucket::Runs
        );
        assert_eq!(
            classify_session_bucket(&SessionBucketParams {
                session_id: Some("run_6f2b7fcbed7c4eaeba5e67564d0a4958"),
                channel_id: Some("run_6f2b7fcbed7c4eaeba5e67564d0a4958"),
                source: Some("api"),
                is_opaque_title: true,
                turn_count: 1,
                has_active_work: false,
            }),
            SessionBucket::Runs
        );
        assert_eq!(
            classify_session_bucket(&SessionBucketParams {
                session_id: Some("hermes-fill-me-in"),
                channel_id: Some("fill-me-in"),
                source: Some("api_server"),
                is_opaque_title: false,
                turn_count: 4,
                has_active_work: false,
            }),
            SessionBucket::Chats
        );
    }

    #[test]
    fn ts_build_entries_pins_active_work_and_supports_search() {
        let now = 1_700_010_000_000u64;
        let local_hash = "local-5e52c0948b0bfd7907bf92990a4140f94";
        let canvas_sessions = vec![
            make_canvas_session(local_hash, &[("hermesSessionId", local_hash), ("name", local_hash), ("topic", local_hash)]),
            make_canvas_session("https-github-com", &[("topic", "https://github.com/NousResearch/hermes-agent")]),
            make_canvas_session("fill-me-in-we", &[("topic", "Fill me in on what happened")]),
            make_canvas_session("daemon", &[("topic", "Background cron watcher & ambient signal daemon")]),
        ];

        let mut hs_local = make_hermes_session(local_hash);
        hs_local.session_id = local_hash.into();
        hs_local.source = Some("api".into());
        hs_local.turn_count = 1;
        hs_local.updated_at_ms = now - 2 * 60_000;
        let mut hs_github = make_hermes_session("https-github-com");
        hs_github.source = Some("api_server".into());
        hs_github.turn_count = 7;
        hs_github.updated_at_ms = now - 30 * 60_000;
        let mut hs_fill = make_hermes_session("fill-me-in-we");
        hs_fill.source = Some("cli".into());
        hs_fill.turn_count = 3;
        hs_fill.updated_at_ms = now - 5 * 60_000;
        let hermes_sessions = vec![hs_local, hs_github, hs_fill];

        let mut activity = HashMap::new();
        activity.insert(
            "https-github-com".to_string(),
            ChannelTaskSummary {
                channel_id: "https-github-com".into(),
                running_count: 1,
                paused_count: 0,
                completed_count: 6,
                is_thinking: false,
                has_active_task: true,
                primary_title: Some("Checking repo disk cleanup".into()),
                started_at_ms: None,
            },
        );

        let entries = build_sidebar_session_entries(&canvas_sessions, &hermes_sessions, &activity, now);

        assert_eq!(entries[0].session.channel_id, "https-github-com");
        assert_eq!(entries[0].display_title, "github.com/NousResearch/hermes-agent");
        assert_eq!(entries[0].bucket, SessionBucket::Chats);
        assert!(entries[0].metadata_line.contains("Checking repo disk cleanup"));

        let chats_only = filter_sidebar_session_entries(&entries, SidebarFilterTab::Chats, "", SidebarOriginFilter::All);
        assert_eq!(chats_only.chats_count, 2);
        assert_eq!(chats_only.runs_count, 2);
        assert_eq!(
            chats_only.visible.iter().map(|e| e.session.channel_id.as_str()).collect::<Vec<_>>(),
            vec!["https-github-com", "fill-me-in-we"]
        );

        let search_in_chats = filter_sidebar_session_entries(&entries, SidebarFilterTab::Chats, "5e52c0", SidebarOriginFilter::All);
        assert_eq!(search_in_chats.visible.len(), 0);
        assert_eq!(search_in_chats.cross_bucket_matches, 1);

        let search_in_all = filter_sidebar_session_entries(&entries, SidebarFilterTab::All, "5e52c0", SidebarOriginFilter::All);
        assert_eq!(search_in_all.visible.len(), 1);
        assert_eq!(search_in_all.visible[0].display_title, "Local API · 5e52c0");

        let only_runs: Vec<SidebarSessionEntry> = entries.iter().filter(|e| e.bucket == SessionBucket::Runs).cloned().collect();
        assert_eq!(resolve_effective_sidebar_tab(SidebarFilterTab::Chats, &only_runs), SidebarFilterTab::Runs);
    }

    #[test]
    fn ts_footer_lanes_exclude_daemon_and_opaque_cron() {
        let canvas_sessions = vec![
            make_canvas_session("daemon", &[("topic", "Background cron watcher & ambient signal daemon")]),
            make_canvas_session("fill-me-in", &[("topic", "Fill me in on what happened")]),
        ];
        let entries = build_sidebar_session_entries(&canvas_sessions, &[], &HashMap::new(), 1_700_000_000_000);

        let channels = vec![
            ChannelMeta {
                id: "daemon".into(),
                name: "Background cron watcher & ambient signal daemon".into(),
                session_id: "cron_watch_1".into(),
                description: "Background cron watcher & ambient signal daemon".into(),
                ..mk("x", "x", SessionOrigin::Workspace, 0)
            },
            ChannelMeta {
                id: "local-5e52c0948b0bfd7907bf92990a4140f94".into(),
                name: "local-5e52c0948b0bfd7907bf92990a4140f94".into(),
                session_id: "local-5e52c0948b0bfd7907bf92990a4140f94".into(),
                description: String::new(),
                ..mk("x", "x", SessionOrigin::Workspace, 0)
            },
            ChannelMeta {
                id: "general".into(),
                name: "general".into(),
                session_id: "hermes-general".into(),
                description: "Primary general assistant lane".into(),
                ..mk("x", "x", SessionOrigin::Workspace, 0)
            },
        ];

        let footer = compute_footer_lanes(&channels, &entries);
        assert_eq!(footer.iter().map(|l| l.short_label.as_str()).collect::<Vec<_>>(), vec!["#general"]);
    }

    #[test]
    fn ts_missing_fields_do_not_panic() {
        let (title, opaque) = format_display_session_title(None, Some("ops-review"), None, None, None, None);
        assert_eq!(title, "ops-review");
        assert!(!opaque);

        let (title, opaque) = format_display_session_title(None, None, None, None, None, None);
        assert_eq!(title, "Session · session");
        assert!(opaque);

        assert_eq!(
            classify_session_bucket(&SessionBucketParams {
                session_id: None,
                channel_id: None,
                source: None,
                is_opaque_title: false,
                turn_count: 0,
                has_active_work: false,
            }),
            SessionBucket::Chats
        );
    }

    #[test]
    fn ts_workspace_vs_external_scoping_and_on_canvas() {
        let canvas_sessions = vec![
            make_canvas_session("design-review", &[("topic", "Review design tokens"), ("origin", "workspace")]),
            make_canvas_session("tg-pinned-1", &[
                ("hermesSessionId", "tg-pinned-1"),
                ("name", "Telegram Deploy"),
                ("topic", "Telegram Deploy"),
                ("origin", "imported"),
            ]),
        ];

        let mut hs_design = make_hermes_session("design-review");
        hs_design.session_key = Some("voxcell-design-review".into());
        hs_design.source = Some("web".into());
        hs_design.origin = Some(SessionOrigin::Workspace);
        hs_design.turn_count = 2;
        hs_design.updated_at_ms = 2_000_000;
        let mut hs_local = make_hermes_session("local-5e52c0948b0bfd7907bf92990a4140f94");
        hs_local.session_id = "local:5e52c0948b0bfd7907bf92990a4140f94".into();
        hs_local.source = Some("api".into());
        hs_local.origin = Some(SessionOrigin::External);
        hs_local.topic = "Explain local Forgejo CI".into();
        hs_local.turn_count = 4;
        hs_local.updated_at_ms = 3_000_000;
        let mut hs_cli = make_hermes_session("cli-20260412");
        hs_cli.session_id = "20260412_1100_abcd12".into();
        hs_cli.source = Some("cli".into());
        hs_cli.origin = Some(SessionOrigin::External);
        hs_cli.topic = "Rename nubcat-engine repo".into();
        hs_cli.turn_count = 6;
        hs_cli.updated_at_ms = 1_500_000;
        let hermes_sessions = vec![hs_design, hs_local, hs_cli];

        let entries = build_sidebar_session_entries(&canvas_sessions, &hermes_sessions, &HashMap::new(), 3_100_000);
        assert_eq!(entries.len(), 4);

        let workspace_scope = filter_sidebar_session_entries(&entries, SidebarFilterTab::All, "", SidebarOriginFilter::Workspace);
        assert_eq!(workspace_scope.workspace_count, 2);
        assert_eq!(workspace_scope.external_count, 2);
        assert_eq!(
            workspace_scope.visible.iter().map(|e| e.session.channel_id.as_str()).collect::<Vec<_>>(),
            vec!["design-review", "tg-pinned-1"]
        );
        assert!(workspace_scope.visible.iter().all(|e| e.on_canvas));

        let external_scope = filter_sidebar_session_entries(&entries, SidebarFilterTab::All, "", SidebarOriginFilter::External);
        assert_eq!(
            external_scope.visible.iter().map(|e| e.session.channel_id.as_str()).collect::<Vec<_>>(),
            vec!["local-5e52c0948b0bfd7907bf92990a4140f94", "cli-20260412"]
        );
        assert!(external_scope.visible.iter().all(|e| !e.on_canvas));
    }

    #[test]
    fn ts_profile_badges_and_active_indicator_metadata() {
        let canvas = vec![make_canvas_session("coder-lane", &[("topic", "Refactor the parser")])];
        let mut hs = make_hermes_session("coder-lane");
        hs.profile = Some("coder".into());
        hs.turn_count = 4;
        hs.model = Some("nous/hermes-4-70b".into());
        hs.updated_at_ms = 1_700_000_000_000;
        let mut activity = HashMap::new();
        activity.insert(
            "coder-lane".to_string(),
            ChannelTaskSummary {
                channel_id: "coder-lane".into(),
                running_count: 2,
                has_active_task: true,
                primary_title: Some("Parsing module split".into()),
                ..Default::default()
            },
        );
        let entries = build_sidebar_session_entries(&canvas, &[hs], &activity, 1_700_000_060_000);
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert!(e.has_active_work);
        assert_eq!(e.running_count, 2);
        assert_eq!(e.active_task_title.as_deref(), Some("Parsing module split"));
        assert!(e.metadata_line.contains("@coder"));
        assert!(e.metadata_line.contains("Parsing module split"));
        assert!(e.tooltip_title.contains("@coder"));
        assert!(e.search_text.contains("coder"));
        assert_eq!(e.relative_time, "1m");

        // Without active work the metadata line carries profile + turns + model.
        let idle = build_sidebar_session_entries(&canvas, &[], &HashMap::new(), 1_700_000_060_000);
        assert!(!idle[0].has_active_work);
    }
}