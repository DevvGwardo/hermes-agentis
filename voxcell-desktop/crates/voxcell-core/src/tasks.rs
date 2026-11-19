//! Port of `src/utils/taskInsights.ts` (P1-06 audit).
//!
//! Thread-right task UX: the panel is hidden by default (`ThreadTaskPanelState`)
//! and surfaces clean status/step rollups (`summarize_thread_tasks`,
//! `summarize_thread_task_aggregate`) instead of noisy progress bars.
//! `partition_channel_tasks` keeps tasks inside their owning thread while
//! `summarize_channel_task_activity` feeds the sidebar active indicator.

use crate::types::{
    ActivityKind, BackgroundTask, ChannelMeta, EntryRole, HermesAgentSession, SessionStatus,
    TaskLifecycleStatus, TaskStepItem, TerminalEntry,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedStepState {
    Done,
    Active,
    Pending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTaskStep {
    pub text: String,
    pub state: ResolvedStepState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskStepBreakdown {
    pub steps: Vec<ResolvedTaskStep>,
    pub completed_count: usize,
    pub total_count: usize,
    pub active_step_text: String,
    pub ratio_label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskProgressInfo {
    pub pct: u8,
    pub ratio_label: String,
    pub is_indeterminate: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskTone {
    Amber,
    Emerald,
    Slate,
    Rose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskPulseState {
    Thinking,
    Live,
    Waiting,
    Done,
    Error,
    Idle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskStatusDisplay {
    pub label: &'static str,
    pub pulse_state: TaskPulseState,
    pub tone: TaskTone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskConfirmationState {
    Confirmed,
    NeedsConfirm,
    Open,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChannelTaskPartition {
    pub running: Vec<BackgroundTask>,
    pub paused: Vec<BackgroundTask>,
    pub completed: Vec<BackgroundTask>,
    pub all: Vec<BackgroundTask>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ChannelTaskSummary {
    pub channel_id: String,
    pub running_count: usize,
    pub paused_count: usize,
    pub completed_count: usize,
    pub is_thinking: bool,
    pub has_active_task: bool,
    pub primary_title: Option<String>,
    pub started_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadTasksSummary {
    pub total_count: usize,
    pub running_count: usize,
    pub paused_count: usize,
    pub needs_confirm_count: usize,
    pub confirmed_count: usize,
    pub header_badge: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadTaskAggregateOverview {
    pub total_tasks: usize,
    pub active_count: usize,
    pub paused_count: usize,
    pub completed_count: usize,
    pub failed_count: usize,
    pub aggregate_pct: u8,
}

/// Desktop-side visibility state for the Thread right-corner task drawer (P1-06).
/// Hidden by default; the upper-right thread control toggles it. Docking geometry
/// (`split` >= 900px vs `overlay`) is resolved by `voxcell-core::layout` (P1-05).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadTaskPanelState {
    pub visible: bool,
}

impl Default for ThreadTaskPanelState {
    fn default() -> Self {
        Self { visible: false }
    }
}

impl ThreadTaskPanelState {
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }
}

/// Formats an elapsed duration into a compact monospace-friendly string
/// ("42s", "4m 08s", "1h 07m"). Mirrors TS falsy semantics: a missing or zero
/// `createdAtMs` renders "0s".
pub fn format_elapsed_duration(created_at_ms: Option<u64>, now_ms: u64, show_seconds: bool) -> String {
    let Some(created) = created_at_ms.filter(|v| *v != 0) else {
        return "0s".to_string();
    };
    let total_seconds = now_ms.saturating_sub(created) / 1000;
    if total_seconds < 60 {
        return format!("{total_seconds}s");
    }
    let total_minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    if total_minutes < 60 {
        if show_seconds {
            format!("{total_minutes}m {seconds:02}s")
        } else {
            format!("{total_minutes}m")
        }
    } else {
        let hours = total_minutes / 60;
        let minutes = total_minutes % 60;
        format!("{hours}h {minutes:02}m")
    }
}

/// Formats epoch ms into HH:MM (24-hour clock).
///
/// Deviation note: the TS reference uses the host local timezone; voxcell-core is
/// dependency-free (no chrono), so this renders UTC. The TS suite only asserts the
/// `\d{2}:\d{2}` shape. Local-time rendering belongs to the UI layer.
pub fn format_start_clock(created_at_ms: Option<u64>) -> String {
    let Some(ms) = created_at_ms.filter(|v| *v != 0) else {
        return String::new();
    };
    let secs_of_day = (ms / 1000) % 86_400;
    let hours = secs_of_day / 3600;
    let minutes = (secs_of_day % 3600) / 60;
    format!("{hours:02}:{minutes:02}")
}

pub fn get_task_kind_label(task: &BackgroundTask) -> String {
    if task.background == Some(false) {
        return "In chat".to_string();
    }
    match task.kind.as_deref() {
        Some(kind) if !kind.is_empty() => kind.to_string(),
        _ => "Background".to_string(),
    }
}

pub fn get_task_confirmation_state(task: &BackgroundTask) -> TaskConfirmationState {
    if task.confirmed_at_ms.is_some() {
        return TaskConfirmationState::Confirmed;
    }
    let raw = task.raw_status.as_deref().unwrap_or("");
    if task.status == TaskLifecycleStatus::Completed || raw == "done" {
        if raw == "failed" || raw == "cancelled" {
            return TaskConfirmationState::Open;
        }
        return TaskConfirmationState::NeedsConfirm;
    }
    TaskConfirmationState::Open
}

pub fn get_task_confirmation_label(task: &BackgroundTask) -> &'static str {
    match get_task_confirmation_state(task) {
        TaskConfirmationState::Confirmed => "Confirmed by you",
        TaskConfirmationState::NeedsConfirm => "Looks done — confirm",
        TaskConfirmationState::Open => "Open",
    }
}

pub fn get_task_status_display(task: &BackgroundTask) -> TaskStatusDisplay {
    let raw = task.raw_status.as_deref().unwrap_or("");
    if raw == "failed" {
        return TaskStatusDisplay {
            label: "Failed",
            pulse_state: TaskPulseState::Error,
            tone: TaskTone::Rose,
        };
    }
    if raw == "cancelled" || raw == "interrupted" {
        return TaskStatusDisplay {
            label: "Cancelled",
            pulse_state: TaskPulseState::Idle,
            tone: TaskTone::Slate,
        };
    }
    if raw == "iteration_budget_exhausted"
        || task.error_code.as_deref() == Some("iteration_budget_exhausted")
    {
        return TaskStatusDisplay {
            label: "Budget limit",
            pulse_state: TaskPulseState::Waiting,
            tone: TaskTone::Amber,
        };
    }
    match task.status {
        TaskLifecycleStatus::Running => {
            if raw == "queued" {
                TaskStatusDisplay {
                    label: "Queued",
                    pulse_state: TaskPulseState::Thinking,
                    tone: TaskTone::Amber,
                }
            } else {
                TaskStatusDisplay {
                    label: "Working",
                    pulse_state: TaskPulseState::Thinking,
                    tone: TaskTone::Amber,
                }
            }
        }
        TaskLifecycleStatus::Paused => {
            if raw == "waiting" || raw == "waiting_for_approval" {
                TaskStatusDisplay {
                    label: "Waiting",
                    pulse_state: TaskPulseState::Waiting,
                    tone: TaskTone::Amber,
                }
            } else {
                TaskStatusDisplay {
                    label: "Paused",
                    pulse_state: TaskPulseState::Idle,
                    tone: TaskTone::Slate,
                }
            }
        }
        TaskLifecycleStatus::Completed => TaskStatusDisplay {
            label: "Done",
            pulse_state: TaskPulseState::Done,
            tone: TaskTone::Emerald,
        },
    }
}

/// Builds a structured step checklist and progress ratio from a task's steps,
/// progressLabel (e.g. "0/6"), or fallback lastStep.
pub fn resolve_task_step_breakdown(task: &BackgroundTask) -> TaskStepBreakdown {
    let raw = task.raw_status.as_deref().unwrap_or("");
    if !task.steps.is_empty() {
        if task.status == TaskLifecycleStatus::Completed && raw != "failed" && raw != "cancelled" {
            let steps: Vec<ResolvedTaskStep> = task
                .steps
                .iter()
                .map(|s| ResolvedTaskStep {
                    text: s.text.clone(),
                    state: ResolvedStepState::Done,
                })
                .collect();
            let total = steps.len();
            let active_step_text = task
                .last_step
                .clone()
                .filter(|s| !s.is_empty())
                .or_else(|| steps.last().map(|s| s.text.clone()))
                .unwrap_or_else(|| task.title.clone());
            return TaskStepBreakdown {
                steps,
                completed_count: total,
                total_count: total,
                active_step_text,
                ratio_label: format!("{total}/{total}"),
            };
        }

        let active_idx = task
            .steps
            .iter()
            .position(|s| s.active && !s.done)
            .or_else(|| task.steps.iter().position(|s| !s.done));

        let steps: Vec<ResolvedTaskStep> = task
            .steps
            .iter()
            .enumerate()
            .map(|(idx, s)| {
                let state = if s.done {
                    ResolvedStepState::Done
                } else if Some(idx) == active_idx && raw != "cancelled" && raw != "failed" {
                    ResolvedStepState::Active
                } else {
                    ResolvedStepState::Pending
                };
                ResolvedTaskStep {
                    text: s.text.clone(),
                    state,
                }
            })
            .collect();

        let completed_count = steps.iter().filter(|s| s.state == ResolvedStepState::Done).count();
        let total_count = steps.len();
        let active_step_text = active_idx
            .and_then(|i| steps.get(i))
            .map(|s| s.text.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| task.last_step.clone().filter(|s| !s.is_empty()))
            .unwrap_or_else(|| task.title.clone());

        return TaskStepBreakdown {
            steps,
            completed_count,
            total_count,
            active_step_text,
            ratio_label: format!("{completed_count}/{total_count}"),
        };
    }

    let parsed_frac = task.progress_label.as_deref().and_then(parse_fraction_label);
    let fallback_text = task
        .last_step
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| task.title.clone());
    let is_terminal_failure = raw == "failed" || raw == "cancelled";
    let fallback_state = if task.status == TaskLifecycleStatus::Completed && !is_terminal_failure {
        ResolvedStepState::Done
    } else if task.status == TaskLifecycleStatus::Running {
        ResolvedStepState::Active
    } else {
        ResolvedStepState::Pending
    };

    let (completed_count, total_count, ratio_label) = match parsed_frac {
        // TS keeps the raw matched digit strings in the ratio label and clamps only totalCount.
        Some((done, total, done_str, total_str)) => {
            (done, total.max(1), format!("{done_str}/{total_str}"))
        }
        None => (
            if task.status == TaskLifecycleStatus::Completed && !is_terminal_failure {
                1
            } else {
                0
            },
            1,
            format!("{}%", task.progress.round() as i64),
        ),
    };

    TaskStepBreakdown {
        steps: vec![ResolvedTaskStep {
            text: fallback_text.clone(),
            state: fallback_state,
        }],
        completed_count,
        total_count,
        active_step_text: fallback_text,
        ratio_label,
    }
}

/// Mirrors `/^(\d+)\s*\/\s*(\d+)$/` — digits only, optional spaces around the slash.
fn parse_fraction_label(label: &str) -> Option<(usize, usize, String, String)> {
    let (a, b) = label.split_once('/')?;
    let a = a.trim();
    let b = b.trim();
    if a.is_empty()
        || b.is_empty()
        || !a.bytes().all(|c| c.is_ascii_digit())
        || !b.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let done: usize = a.parse().ok()?;
    let total: usize = b.parse().ok()?;
    Some((done, total, a.to_string(), b.to_string()))
}

/// Computes normalized 0..100 completion percentage, display ratio, and whether
/// a running task has 0% known progress (indeterminate state).
pub fn resolve_task_progress(task: &BackgroundTask) -> TaskProgressInfo {
    let breakdown = resolve_task_step_breakdown(task);
    let raw = task.raw_status.as_deref().unwrap_or("");
    if task.status == TaskLifecycleStatus::Completed && raw != "failed" && raw != "cancelled" {
        return TaskProgressInfo {
            pct: 100,
            ratio_label: breakdown.ratio_label,
            is_indeterminate: false,
        };
    }

    let mut pct = if task.progress.is_finite() {
        task.progress.round().clamp(0.0, 100.0) as u8
    } else {
        0
    };
    if breakdown.total_count > 1 || !task.steps.is_empty() {
        let step_pct = ((breakdown.completed_count as f32 / breakdown.total_count.max(1) as f32)
            * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8;
        if step_pct > 0 || pct == 0 {
            pct = step_pct;
        }
    }

    TaskProgressInfo {
        pct,
        ratio_label: breakdown.ratio_label,
        is_indeterminate: task.status == TaskLifecycleStatus::Running && pct == 0,
    }
}

/// Strictly scopes tasks to the given thread/channel and orders them with active
/// running tasks first, then paused/waiting, then completed (unconfirmed
/// "needs-confirm" before confirmed). Tasks never leak across threads.
pub fn partition_channel_tasks(tasks: &[BackgroundTask], channel_id: &str) -> ChannelTaskPartition {
    let mut running = Vec::new();
    let mut paused = Vec::new();
    let mut completed = Vec::new();

    for t in tasks.iter().filter(|t| t.channel_id == channel_id) {
        match t.status {
            TaskLifecycleStatus::Running => running.push(t.clone()),
            TaskLifecycleStatus::Paused => paused.push(t.clone()),
            TaskLifecycleStatus::Completed => completed.push(t.clone()),
        }
    }

    let recency = |t: &BackgroundTask| t.updated_at_ms.or(t.created_at_ms).unwrap_or(0);
    running.sort_by(|a, b| recency(b).cmp(&recency(a)));
    paused.sort_by(|a, b| recency(b).cmp(&recency(a)));
    completed.sort_by(|a, b| {
        let a_needs = matches!(get_task_confirmation_state(a), TaskConfirmationState::NeedsConfirm);
        let b_needs = matches!(get_task_confirmation_state(b), TaskConfirmationState::NeedsConfirm);
        b_needs.cmp(&a_needs).then_with(|| recency(b).cmp(&recency(a)))
    });

    let mut all = Vec::with_capacity(running.len() + paused.len() + completed.len());
    all.extend(running.iter().cloned());
    all.extend(paused.iter().cloned());
    all.extend(completed.iter().cloned());

    ChannelTaskPartition {
        running,
        paused,
        completed,
        all,
    }
}

/// Concise task summary metrics and single-line badge text for the Thread right
/// sidebar header and status-bar toggle (clean rollups, no progress bars).
pub fn summarize_thread_tasks(
    partition: &ChannelTaskPartition,
    active_is_thinking: bool,
) -> ThreadTasksSummary {
    let has_running = !partition.running.is_empty();
    let synthetic = usize::from(active_is_thinking && !has_running);
    let total_count = partition.all.len() + synthetic;
    let running_count = partition.running.len() + synthetic;
    let paused_count = partition.paused.len();
    let mut needs_confirm_count = 0;
    let mut confirmed_count = 0;

    for t in &partition.completed {
        match get_task_confirmation_state(t) {
            TaskConfirmationState::NeedsConfirm => needs_confirm_count += 1,
            TaskConfirmationState::Confirmed => confirmed_count += 1,
            TaskConfirmationState::Open => {}
        }
    }

    let header_badge = if running_count > 0 {
        format!("{running_count} active")
    } else if paused_count > 0 {
        format!("{paused_count} paused")
    } else if needs_confirm_count > 0 {
        format!("{needs_confirm_count} to confirm")
    } else if total_count > 0 {
        format!("{total_count} done")
    } else {
        String::new()
    };

    ThreadTasksSummary {
        total_count,
        running_count,
        paused_count,
        needs_confirm_count,
        confirmed_count,
        header_badge,
    }
}

/// Overall completion rollup across all tasks scoped to a Thread so the panel
/// header can render a calm aggregate strip and filter tabs.
pub fn summarize_thread_task_aggregate(
    partition: &ChannelTaskPartition,
    active_is_thinking: bool,
) -> ThreadTaskAggregateOverview {
    let synthetic = usize::from(active_is_thinking && partition.running.is_empty());
    let active_count = partition.running.len() + synthetic;
    let paused_count = partition.paused.len();
    let completed_count = partition.completed.len();
    let failed_count = partition
        .completed
        .iter()
        .filter(|t| matches!(t.raw_status.as_deref(), Some("failed") | Some("cancelled")))
        .count();
    let total_tasks = partition.all.len() + synthetic;

    if total_tasks == 0 {
        return ThreadTaskAggregateOverview {
            total_tasks: 0,
            active_count: 0,
            paused_count: 0,
            completed_count: 0,
            failed_count: 0,
            aggregate_pct: 0,
        };
    }

    let pct_sum: usize = partition
        .all
        .iter()
        .map(|t| resolve_task_progress(t).pct as usize)
        .sum();
    // TS: Math.round(pctSum / totalTasks) — round, not floor.
    let aggregate_pct = (pct_sum as f32 / total_tasks as f32).round().clamp(0.0, 100.0) as u8;

    ThreadTaskAggregateOverview {
        total_tasks,
        active_count,
        paused_count,
        completed_count,
        failed_count,
        aggregate_pct,
    }
}

/// Extracts a concise, non-redundant task outcome note from `task.result`.
pub fn format_task_outcome_snippet(task: &BackgroundTask, max_chars: usize) -> Option<String> {
    let raw = task
        .result
        .as_deref()
        .map(collapse_whitespace)
        .unwrap_or_default();
    let raw = raw.trim().to_string();
    if raw.is_empty() {
        return None;
    }
    if task.last_step.as_deref().map(str::trim) == Some(raw.as_str()) || raw == task.title.trim() {
        return None;
    }
    Some(truncate_at_word_boundary(&raw, max_chars, 0.6))
}

/// Whether a task card defaults to expanded inside the Thread right sidebar
/// (only the lead active running task expands cleanly).
pub fn is_task_initially_expanded(
    task: &BackgroundTask,
    lead_running_task_id: Option<&str>,
    in_sidebar: bool,
) -> bool {
    if !in_sidebar {
        return false;
    }
    match lead_running_task_id {
        Some(id) if !id.is_empty() => task.id == id && task.status == TaskLifecycleStatus::Running,
        _ => false,
    }
}

/// Lightweight per-channel active-task metadata so the sidebar can highlight
/// which threads currently have active work without rendering task rows.
pub fn summarize_channel_task_activity(
    tasks: &[BackgroundTask],
    channels: &[ChannelMeta],
    thinking_by_channel: &HashMap<String, bool>,
) -> HashMap<String, ChannelTaskSummary> {
    let mut map: HashMap<String, ChannelTaskSummary> = HashMap::new();
    let ensure = |map: &mut HashMap<String, ChannelTaskSummary>, id: &str| {
        if !map.contains_key(id) {
            let thinking = thinking_by_channel.get(id).copied().unwrap_or(false);
            map.insert(
                id.to_string(),
                ChannelTaskSummary {
                    channel_id: id.to_string(),
                    running_count: 0,
                    paused_count: 0,
                    completed_count: 0,
                    is_thinking: thinking,
                    has_active_task: thinking,
                    primary_title: None,
                    started_at_ms: None,
                },
            );
        }
    };

    for ch in channels {
        ensure(&mut map, &ch.id);
    }
    for id in thinking_by_channel.keys() {
        ensure(&mut map, id);
    }

    for task in tasks {
        ensure(&mut map, &task.channel_id);
        let entry = map.get_mut(&task.channel_id).expect("inserted above");
        match task.status {
            TaskLifecycleStatus::Running => {
                entry.running_count += 1;
                entry.has_active_task = true;
                if entry.primary_title.is_none() {
                    entry.primary_title = Some(task.title.clone());
                }
                if let Some(created) = task.created_at_ms.filter(|v| *v != 0) {
                    entry.started_at_ms = Some(match entry.started_at_ms {
                        Some(prev) => prev.min(created),
                        None => created,
                    });
                }
            }
            TaskLifecycleStatus::Paused => entry.paused_count += 1,
            TaskLifecycleStatus::Completed => entry.completed_count += 1,
        }
    }

    map
}

/// Derives a compact, human-readable task title from a user prompt while keeping
/// repository/URL targets legible inside narrow cards.
pub fn derive_turn_task_title(prompt: &str, fallback_label: Option<&str>) -> String {
    let stripped = strip_url_schemes(prompt);
    let normalized = collapse_whitespace(&stripped).trim().to_string();
    if normalized.is_empty() {
        return fallback_label
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Active session turn")
            .to_string();
    }
    let chars: Vec<char> = normalized.chars().collect();
    if chars.len() <= 64 {
        return normalized;
    }
    let clipped: String = chars.iter().take(61).collect();
    if let Some(idx) = clipped.rfind(' ') {
        if clipped[..idx].chars().count() > 36 {
            return format!("{}…", clipped[..idx].trim_end());
        }
    }
    format!("{}…", clipped.trim_end())
}

/// Mirrors `.replace(/https?:\/\/(?:www\.)?/gi, '')` — case-insensitive, anywhere.
fn strip_url_schemes(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let lower: Vec<char> = raw.to_ascii_lowercase().chars().collect();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < chars.len() {
        let rest: String = lower[i..].iter().collect();
        let scheme_len = if rest.starts_with("https://") {
            Some(8)
        } else if rest.starts_with("http://") {
            Some(7)
        } else {
            None
        };
        if let Some(len) = scheme_len {
            i += len;
            let after: String = lower[i..].iter().collect();
            if after.starts_with("www.") {
                i += 4;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn collapse_whitespace(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Truncates at a word boundary when the last space sits past `min_ratio` of the
/// budget; otherwise hard-cuts. Appends the TS ellipsis character.
fn truncate_at_word_boundary(raw: &str, max_chars: usize, min_ratio: f32) -> String {
    let chars: Vec<char> = raw.chars().collect();
    if chars.len() <= max_chars {
        return raw.to_string();
    }
    let slice: String = chars.iter().take(max_chars).collect();
    let last_space = slice.rfind(' ').map(|b| slice[..b].chars().count());
    let cutoff = match last_space {
        Some(idx) if idx as f32 >= (max_chars as f32 * min_ratio).floor() => idx,
        _ => max_chars,
    };
    let head: String = chars.iter().take(cutoff).collect();
    format!("{}…", head.trim_end())
}

const VAGUE_PROMPT_TITLES: &[&str] = &[
    "go",
    "yes",
    "yep",
    "ok",
    "okay",
    "sure",
    "continue",
    "next",
    "proceed",
    "try again",
    "retry",
    "check",
    "status",
    "what do you suggest",
    "what do you suggest?",
    "what should we do",
    "what should we do?",
    "any thoughts",
    "any thoughts?",
];

fn is_vague_prompt(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    if VAGUE_PROMPT_TITLES.contains(&lower.as_str()) {
        return true;
    }
    let normalized = lower.trim_end_matches(['.', '!', '?']).trim();
    VAGUE_PROMPT_TITLES.contains(&normalized)
}

/// TS does not trim inside this helper — match exactly.
fn is_bootstrap_or_composing_step(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.starts_with("working in ") || lower.starts_with("routing ") || lower == "composing response"
}

/// Mirrors `/^running\s+/i`.
fn starts_with_running(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    match lower.strip_prefix("running") {
        Some(rest) => rest.chars().next().is_some_and(char::is_whitespace),
        None => false,
    }
}

/// Resolves a clear display title for task cards: vague conversational nudges
/// ("go", "what do you suggest") promote concise action context from steps.
pub fn resolve_task_display_title(task: &BackgroundTask) -> String {
    let raw = task.title.trim();
    if raw.is_empty() {
        return "Active task".to_string();
    }
    if !is_vague_prompt(raw) {
        return raw.to_string();
    }
    for s in &task.steps {
        let txt = s.text.trim();
        if !txt.is_empty()
            && !starts_with_running(txt)
            && !is_bootstrap_or_composing_step(txt)
            && txt.to_ascii_lowercase() != "completed response"
        {
            return derive_turn_task_title(txt, Some(raw));
        }
    }
    if let Some(last) = task.last_step.as_deref() {
        if !starts_with_running(last) && !is_bootstrap_or_composing_step(last) {
            return derive_turn_task_title(last, Some(raw));
        }
    }
    raw.to_string()
}

fn extract_tool_needle_from_text(text: &str) -> String {
    let trimmed = text.trim().to_ascii_lowercase();
    if let Some(rest) = trimmed.strip_prefix("running ") {
        return rest
            .split(|c: char| c.is_whitespace() || c == ':')
            .next()
            .unwrap_or("")
            .to_string();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if let Some(colon) = chars.iter().position(|c| *c == ':') {
        if colon > 0 && colon < 32 {
            return chars[..colon].iter().collect::<String>().trim().to_string();
        }
    }
    // ' — ' window (space, em-dash, space)
    if chars.len() >= 3 {
        for idx in 0..chars.len() - 2 {
            if chars[idx] == ' ' && chars[idx + 1] == '—' && chars[idx + 2] == ' ' {
                if idx > 0 && idx < 32 {
                    return chars[..idx].iter().collect::<String>().trim().to_string();
                }
                break;
            }
        }
    }
    trimmed.split_whitespace().next().unwrap_or("").to_string()
}

fn is_terminal_tool_done(status: &str) -> bool {
    matches!(
        status,
        "done" | "completed" | "complete" | "finished" | "finish" | "success" | "succeeded" | "passed" | "pass" | "ok"
    )
}

fn is_terminal_tool_error(status: &str) -> bool {
    matches!(status, "error" | "failed" | "cancelled" | "canceled" | "stopped")
}

/// Updates a task's step checklist and ratio string when a tool-progress event
/// arrives from the Hermes Gateway (`hermes.tool.progress`).
pub fn apply_tool_progress_to_steps(
    prev_steps: &[TaskStepItem],
    tool_name: &str,
    label: Option<&str>,
    status: &str,
    summary: Option<&str>,
) -> (Vec<TaskStepItem>, String) {
    // TS: (event.status || 'running').toLowerCase() — no trim.
    let norm = if status.is_empty() {
        "running".to_string()
    } else {
        status.to_ascii_lowercase()
    };
    let is_done = is_terminal_tool_done(&norm);
    let is_err = is_terminal_tool_error(&norm);
    // TS: (summary || label || toolName || 'tool').trim() — first non-empty raw string wins.
    let step_text = summary
        .filter(|s| !s.is_empty())
        .or_else(|| label.filter(|s| !s.is_empty()))
        .unwrap_or(if tool_name.is_empty() { "tool" } else { tool_name })
        .trim()
        .to_string();
    let tool_needle = tool_name.trim().to_ascii_lowercase();
    let is_generic_placeholder = starts_with_running(&step_text);

    let mut next: Vec<TaskStepItem> = prev_steps.to_vec();

    if !is_done && !is_err {
        let upgrade_idx = if !is_generic_placeholder && !tool_needle.is_empty() {
            next.iter().position(|s| {
                !s.done
                    && starts_with_running(s.text.trim())
                    && extract_tool_needle_from_text(&s.text) == tool_needle
            })
        } else {
            None
        };

        if let Some(idx) = upgrade_idx {
            for (i, item) in next.iter_mut().enumerate() {
                if i != idx && !item.done && (item.active || is_bootstrap_or_composing_step(&item.text)) {
                    item.done = true;
                    item.active = false;
                }
            }
            next[idx] = TaskStepItem {
                text: step_text,
                done: false,
                active: true,
            };
        } else {
            for item in &mut next {
                if !item.done && (item.active || is_bootstrap_or_composing_step(&item.text)) {
                    item.done = true;
                    item.active = false;
                }
            }
            next.push(TaskStepItem {
                text: step_text,
                done: false,
                active: true,
            });
        }
    } else {
        let match_idx = next.iter().rposition(|s| {
            !s.done
                && ((!tool_needle.is_empty()
                    && s.text.to_ascii_lowercase().contains(&tool_needle))
                    || s.active)
        });

        if let Some(idx) = match_idx {
            let existing = &next[idx];
            let prefer_new = !is_generic_placeholder
                && (step_text.chars().count() > existing.text.chars().count()
                    || starts_with_running(existing.text.trim()));
            next[idx] = TaskStepItem {
                text: if prefer_new {
                    step_text
                } else {
                    existing.text.clone()
                },
                done: !is_err,
                active: false,
            };
        } else {
            let upgrade_last = next.last().is_some_and(|last| {
                !is_generic_placeholder
                    && starts_with_running(last.text.trim())
                    && !tool_needle.is_empty()
                    && extract_tool_needle_from_text(&last.text) == tool_needle
            });
            if upgrade_last {
                if let Some(last) = next.last_mut() {
                    *last = TaskStepItem {
                        text: step_text,
                        done: !is_err,
                        active: false,
                    };
                }
            } else {
                next.push(TaskStepItem {
                    text: step_text,
                    done: !is_err,
                    active: false,
                });
            }
        }
    }

    let done_count = next.iter().filter(|s| s.done).count();
    let ratio = format!("{done_count}/{}", next.len().max(1));
    (next, ratio)
}

/// Transitions a task into response-streaming phase once text deltas start arriving.
pub fn apply_streaming_delta_to_steps(prev_steps: &[TaskStepItem]) -> (Vec<TaskStepItem>, String, bool) {
    if let Some(last) = prev_steps.last() {
        if !last.done && last.active && last.text == "Composing response" {
            let done_count = prev_steps.iter().filter(|s| s.done).count();
            return (
                prev_steps.to_vec(),
                format!("{done_count}/{}", prev_steps.len().max(1)),
                false,
            );
        }
    }
    let mut next: Vec<TaskStepItem> = prev_steps
        .iter()
        .map(|s| TaskStepItem {
            text: s.text.clone(),
            done: true,
            active: false,
        })
        .collect();
    next.push(TaskStepItem {
        text: "Composing response".to_string(),
        done: false,
        active: true,
    });
    let done_count = next.iter().filter(|s| s.done).count();
    let ratio = format!("{done_count}/{}", next.len());
    (next, ratio, true)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnFinalOutcome {
    Done,
    Failed,
    Cancelled,
    IterationBudgetExhausted,
}

/// Settles a turn task's steps cleanly when the run finishes, fails, or aborts.
pub fn finalize_turn_task_steps(
    prev_steps: &[TaskStepItem],
    outcome: TurnFinalOutcome,
    final_label: Option<&str>,
) -> (Vec<TaskStepItem>, String) {
    let final_label = final_label.filter(|s| !s.is_empty());
    if outcome == TurnFinalOutcome::Done || outcome == TurnFinalOutcome::IterationBudgetExhausted {
        let mut next: Vec<TaskStepItem> = if !prev_steps.is_empty() {
            prev_steps
                .iter()
                .map(|s| TaskStepItem {
                    text: match final_label {
                        Some(label) if s.text.starts_with("Working in ") => label.to_string(),
                        _ => s.text.clone(),
                    },
                    done: true,
                    active: false,
                })
                .collect()
        } else {
            vec![TaskStepItem {
                text: final_label
                    .unwrap_or(if outcome == TurnFinalOutcome::IterationBudgetExhausted {
                        "Iteration budget reached (partial answer)"
                    } else {
                        "Completed response"
                    })
                    .to_string(),
                done: true,
                active: false,
            }]
        };

        if outcome == TurnFinalOutcome::IterationBudgetExhausted {
            if let Some(label) = final_label {
                if !next.iter().any(|s| s.text == label) {
                    next.push(TaskStepItem {
                        text: label.to_string(),
                        done: true,
                        active: false,
                    });
                }
            }
        }

        let total = next.len();
        return (next, format!("{total}/{total}"));
    }

    let mut next: Vec<TaskStepItem> = prev_steps
        .iter()
        .map(|s| TaskStepItem {
            text: s.text.clone(),
            done: s.done,
            active: false,
        })
        .collect();
    if let Some(label) = final_label {
        if !next.iter().any(|s| s.text == label) {
            next.push(TaskStepItem {
                text: label.to_string(),
                done: false,
                active: false,
            });
        }
    }
    let done_count = next.iter().filter(|s| s.done).count();
    let ratio = format!("{done_count}/{}", next.len().max(1));
    (next, ratio)
}

/// Collapses redundant generic "Running <tool>" placeholder entries when a
/// concrete or completed step for the same tool appears immediately beside it.
pub fn dedupe_activity_entries(activities: &[TerminalEntry]) -> Vec<TerminalEntry> {
    let mut result: Vec<TerminalEntry> = Vec::with_capacity(activities.len());
    for entry in activities {
        let trimmed = entry.text.trim();
        let is_generic = starts_with_running(trimmed);
        let needle = extract_tool_needle_from_text(trimmed);

        if let Some(prev) = result.last() {
            if !needle.is_empty()
                && entry.activity_kind != Some(ActivityKind::Reasoning)
                && prev.activity_kind != Some(ActivityKind::Reasoning)
            {
                let prev_trimmed = prev.text.trim();
                let prev_generic = starts_with_running(prev_trimmed);
                let prev_needle = extract_tool_needle_from_text(prev_trimmed);

                // Case 1: "Running <tool>" placeholder upgraded in-place by "<tool>: <detail>".
                if prev_generic && prev_needle == needle && !is_generic {
                    *result.last_mut().expect("non-empty") = entry.clone();
                    continue;
                }
                // Case 2: unsettled "<tool>: <detail>" followed by generic "Running <tool>" dupe.
                if !prev_generic
                    && is_generic
                    && prev_needle == needle
                    && prev.activity_status.as_deref() != Some("done")
                {
                    continue;
                }
            }
        }
        result.push(entry.clone());
    }
    result
}

/// True if an activity entry represents an in-flight tool or step.
pub fn is_activity_entry_running(entry: &TerminalEntry) -> bool {
    if entry.role != EntryRole::Activity {
        return false;
    }
    if let Some(status) = entry.activity_status.as_deref() {
        let s = status.trim().to_ascii_lowercase();
        if !s.is_empty() {
            if is_terminal_tool_done(&s) || is_terminal_tool_error(&s) {
                return false;
            }
            if matches!(s.as_str(), "running" | "queued" | "thinking") {
                return true;
            }
        }
    }
    // Mirrors /^running\b/i — "running" alone or followed by a non-word char.
    let text = entry.text.trim().to_ascii_lowercase();
    match text.strip_prefix("running") {
        Some(rest) => match rest.chars().next() {
            None => true,
            Some(c) => !(c.is_ascii_alphanumeric() || c == '_'),
        },
        None => false,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrailingTurnContext {
    pub has_active_activity: bool,
    pub latest_prompt: String,
    pub latest_prompt_at_ms: Option<u64>,
    pub trailing_activities: Vec<TerminalEntry>,
    pub latest_activity_batch: Vec<TerminalEntry>,
}

/// Inspects a channel's entries to detect whether the latest turn still has
/// active/unsettled tool trace activity (even after an assistant preamble or a
/// mid-run client rehydration).
pub fn extract_trailing_turn_context(entries: &[TerminalEntry]) -> TrailingTurnContext {
    let mut latest_prompt = String::new();
    let mut latest_prompt_at_ms: Option<u64> = None;
    let mut last_settled_assistant_idx: Option<usize> = None;
    let mut last_user_idx: Option<usize> = None;

    for (i, e) in entries.iter().enumerate() {
        if matches!(e.role, EntryRole::User | EntryRole::Command) {
            last_user_idx = Some(i);
            if !e.text.trim().starts_with('/') {
                latest_prompt = e.text.trim().to_string();
                latest_prompt_at_ms = e.created_at_ms;
            }
        } else if e.role == EntryRole::Hermes && !e.streaming {
            last_settled_assistant_idx = Some(i);
        }
    }

    let trailing_start = match (last_settled_assistant_idx, last_user_idx) {
        (Some(a), Some(b)) => a.max(b) + 1,
        (Some(a), None) => a + 1,
        (None, Some(b)) => b + 1,
        (None, None) => 0,
    };
    let trailing_activities: Vec<TerminalEntry> = entries[trailing_start.min(entries.len())..]
        .iter()
        .filter(|e| e.role == EntryRole::Activity)
        .cloned()
        .collect();
    let has_active_activity = trailing_activities.iter().any(is_activity_entry_running);

    let mut batch_start = last_user_idx.map(|i| i + 1).unwrap_or(0);
    if batch_start >= entries.len() {
        batch_start = 0;
    }
    let latest_activity_batch: Vec<TerminalEntry> = entries[batch_start..]
        .iter()
        .filter(|e| e.role == EntryRole::Activity)
        .cloned()
        .collect();

    TrailingTurnContext {
        has_active_activity,
        latest_prompt,
        latest_prompt_at_ms,
        trailing_activities,
        latest_activity_batch,
    }
}

fn build_steps_from_activities(
    activities: &[TerminalEntry],
    session_id: &str,
    still_running: bool,
) -> Vec<TaskStepItem> {
    let deduped = dedupe_activity_entries(activities);
    if deduped.is_empty() {
        return vec![TaskStepItem {
            text: format!("Working in {session_id}"),
            done: !still_running,
            active: still_running,
        }];
    }
    let last_idx = deduped.len() - 1;
    deduped
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let status_missing = e.activity_status.as_deref().unwrap_or("").is_empty();
            let running =
                still_running && i == last_idx && (is_activity_entry_running(e) || status_missing);
            let trimmed = e.text.trim();
            TaskStepItem {
                text: if trimmed.is_empty() {
                    "Tool activity".to_string()
                } else {
                    trimmed.to_string()
                },
                done: !running,
                active: running,
            }
        })
        .collect()
}

/// Ensures every thread/session that is actively thinking, running tools, or
/// carrying unsettled activity traces surfaces a live task in that thread — and
/// that completed tool runs without a server task record stay visible as
/// thread-scoped completed tasks. Never duplicates an explicit running task.
pub fn merge_tasks_with_session_activity(
    tasks: &[BackgroundTask],
    entries: &[TerminalEntry],
    sessions: &[HermesAgentSession],
    thinking_by_channel: &HashMap<String, bool>,
    channels: &[ChannelMeta],
    confirmed_task_ids: &HashSet<String>,
    now_ms: u64,
) -> Vec<BackgroundTask> {
    let mut running_channels: HashSet<&str> = HashSet::new();
    let mut channels_with_tasks: HashSet<&str> = HashSet::new();
    for t in tasks {
        channels_with_tasks.insert(t.channel_id.as_str());
        if t.status == TaskLifecycleStatus::Running {
            running_channels.insert(t.channel_id.as_str());
        }
    }

    // Ordered candidate set mirroring TS insertion order (thinking keys, active
    // sessions, entry channels). Thinking keys are sorted for determinism.
    let mut candidate_channels: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    fn push_candidate(id: String, seen: &mut HashSet<String>, out: &mut Vec<String>) {
        if seen.insert(id.clone()) {
            out.push(id);
        }
    }
    let mut thinking_keys: Vec<&String> = thinking_by_channel
        .iter()
        .filter(|(_, v)| **v)
        .map(|(k, _)| k)
        .collect();
    thinking_keys.sort();
    for k in thinking_keys {
        push_candidate(k.clone(), &mut seen, &mut candidate_channels);
    }
    for s in sessions {
        if matches!(
            s.status,
            SessionStatus::Thinking | SessionStatus::Speaking | SessionStatus::Running
        ) {
            push_candidate(s.channel_id.clone(), &mut seen, &mut candidate_channels);
        }
    }
    for e in entries {
        push_candidate(e.channel_id.clone(), &mut seen, &mut candidate_channels);
    }

    let mut synthetic = Vec::new();
    for ch_id in candidate_channels {
        if running_channels.contains(ch_id.as_str()) {
            continue;
        }
        let ch_entries: Vec<TerminalEntry> = entries
            .iter()
            .filter(|e| e.channel_id == ch_id)
            .cloned()
            .collect();
        // TS Map.set semantics: last session/meta for a channel wins.
        let session = sessions.iter().rev().find(|s| s.channel_id == ch_id);
        let meta = channels.iter().rev().find(|c| c.id == ch_id);
        let ctx = extract_trailing_turn_context(&ch_entries);

        let session_active = session.is_some_and(|s| {
            matches!(
                s.status,
                SessionStatus::Thinking | SessionStatus::Speaking | SessionStatus::Running
            )
        });
        let is_busy = thinking_by_channel.get(&ch_id).copied().unwrap_or(false)
            || session_active
            || ctx.has_active_activity;

        let session_id = session
            .map(|s| s.session_id.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| meta.map(|m| m.session_id.clone()).filter(|s| !s.is_empty()))
            .unwrap_or_else(|| format!("hermes-{ch_id}"));
        let title_seed = if !ctx.latest_prompt.is_empty() {
            ctx.latest_prompt.clone()
        } else {
            session
                .map(|s| s.last_message.clone())
                .filter(|s| !s.is_empty())
                .or_else(|| meta.map(|m| m.description.clone()).filter(|s| !s.is_empty()))
                .unwrap_or_default()
        };
        let fallback_name = meta
            .map(|m| m.name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| ch_id.clone());
        let title = derive_turn_task_title(&title_seed, Some(&format!("#{fallback_name}")));

        if is_busy {
            let source_activities: &[TerminalEntry] = if !ctx.trailing_activities.is_empty() {
                &ctx.trailing_activities
            } else {
                &ctx.latest_activity_batch
            };
            let steps = build_steps_from_activities(source_activities, &session_id, true);
            let done_count = steps.iter().filter(|s| s.done).count();
            let pct = ((done_count as f32 / steps.len().max(1) as f32) * 100.0).round();
            let active_step = steps
                .iter()
                .rev()
                .find(|s| s.active)
                .or_else(|| steps.last())
                .map(|s| s.text.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("Working in {session_id}"));
            let created_at_ms = ctx
                .latest_prompt_at_ms
                .or_else(|| source_activities.first().and_then(|e| e.created_at_ms))
                .or_else(|| session.and_then(|s| s.updated_at_ms))
                .unwrap_or(now_ms);
            let updated_at_ms = source_activities
                .last()
                .and_then(|e| e.created_at_ms)
                .or_else(|| session.and_then(|s| s.updated_at_ms))
                .unwrap_or(created_at_ms);
            let clock = format_start_clock(Some(updated_at_ms));

            synthetic.push(BackgroundTask {
                id: format!("live-turn-{ch_id}"),
                channel_id: ch_id.clone(),
                session_id,
                kind: Some("In chat".to_string()),
                background: Some(false),
                title,
                status: TaskLifecycleStatus::Running,
                raw_status: Some("running".to_string()),
                progress: pct,
                progress_label: Some(format!("{done_count}/{}", steps.len())),
                last_step: Some(active_step),
                steps,
                result: None,
                error_code: None,
                created_at_ms: Some(created_at_ms),
                updated_at_ms: Some(updated_at_ms),
                agent_done_at_ms: None,
                confirmed_at_ms: None,
                updated_at: if clock.is_empty() { "live".to_string() } else { clock },
            });
            continue;
        }

        // Completed tool run on a thread with no server task record at all.
        if !channels_with_tasks.contains(ch_id.as_str()) && !ctx.latest_activity_batch.is_empty() {
            let steps = build_steps_from_activities(&ctx.latest_activity_batch, &session_id, false);
            let created_at_ms = ctx
                .latest_prompt_at_ms
                .or_else(|| ctx.latest_activity_batch.first().and_then(|e| e.created_at_ms))
                .unwrap_or(now_ms);
            let updated_at_ms = ctx
                .latest_activity_batch
                .last()
                .and_then(|e| e.created_at_ms)
                .unwrap_or(created_at_ms);
            let task_id = format!("trace-turn-{ch_id}");
            let confirmed_at_ms = if confirmed_task_ids.contains(&task_id) {
                Some(updated_at_ms)
            } else {
                None
            };
            let clock = format_start_clock(Some(updated_at_ms));
            let last_step = steps
                .last()
                .map(|s| s.text.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Completed tool run".to_string());
            let total = steps.len();

            synthetic.push(BackgroundTask {
                id: task_id,
                channel_id: ch_id.clone(),
                session_id,
                kind: Some("In chat".to_string()),
                background: Some(false),
                title,
                status: TaskLifecycleStatus::Completed,
                raw_status: Some("done".to_string()),
                progress: 100.0,
                progress_label: Some(format!("{total}/{total}")),
                last_step: Some(last_step),
                steps,
                result: None,
                error_code: None,
                created_at_ms: Some(created_at_ms),
                updated_at_ms: Some(updated_at_ms),
                agent_done_at_ms: Some(updated_at_ms),
                confirmed_at_ms,
                updated_at: if clock.is_empty() { "done".to_string() } else { clock },
            });
        }
    }

    let mut merged = tasks.to_vec();
    merged.extend(synthetic);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_task(status: TaskLifecycleStatus) -> BackgroundTask {
        BackgroundTask {
            id: "t1".into(),
            channel_id: "ch1".into(),
            session_id: "s1".into(),
            kind: Some("Background".into()),
            background: Some(true),
            title: "Build RUNE client".into(),
            status,
            raw_status: None,
            progress: 0.0,
            progress_label: None,
            last_step: None,
            steps: vec![
                TaskStepItem { text: "Parse schema".into(), done: true, active: false },
                TaskStepItem { text: "Compile".into(), done: false, active: true },
            ],
            result: None,
            error_code: None,
            created_at_ms: Some(1_000),
            updated_at_ms: Some(5_000),
            agent_done_at_ms: None,
            confirmed_at_ms: None,
            updated_at: "now".into(),
        }
    }

    // ---- TS taskInsights.test.ts fixtures (SAMPLE_TASKS) ----

    fn ts_sample_tasks() -> Vec<BackgroundTask> {
        vec![
            BackgroundTask {
                id: "task-101".into(),
                channel_id: "launch".into(),
                session_id: "duet-run-409".into(),
                kind: Some("Background".into()),
                background: Some(true),
                title: "Rebuild standing morning-brief.html (save_page)".into(),
                status: TaskLifecycleStatus::Running,
                raw_status: None,
                progress: 67.0,
                progress_label: Some("4/6".into()),
                last_step: Some("Streaming update_task delta across Playwright parity lane".into()),
                steps: vec![
                    TaskStepItem { text: "Collect changed thread summaries".into(), done: true, active: false },
                    TaskStepItem { text: "Render static morning-brief.html template".into(), done: true, active: false },
                    TaskStepItem { text: "Run desktop viewport diff".into(), done: true, active: false },
                    TaskStepItem { text: "Run mobile viewport parity check".into(), done: true, active: false },
                    TaskStepItem { text: "Streaming update_task delta across Playwright parity lane".into(), done: false, active: true },
                    TaskStepItem { text: "Publish final save_page artifact".into(), done: false, active: false },
                ],
                result: None,
                error_code: None,
                created_at_ms: Some(1_700_000_000_000),
                updated_at_ms: Some(1_700_000_240_000),
                agent_done_at_ms: None,
                confirmed_at_ms: None,
                updated_at: "14:21".into(),
            },
            BackgroundTask {
                id: "task-102".into(),
                channel_id: "research".into(),
                session_id: String::new(),
                kind: None,
                background: None,
                title: "Index ~/.hermes/skills/ & MCP connectors".into(),
                status: TaskLifecycleStatus::Completed,
                raw_status: Some("done".into()),
                progress: 100.0,
                progress_label: None,
                last_step: Some("Synced 4 skills".into()),
                steps: vec![
                    TaskStepItem { text: "Scan ~/.hermes/skills/".into(), done: true, active: false },
                    TaskStepItem { text: "Verify mcp_servers.duet manifest".into(), done: true, active: false },
                ],
                result: None,
                error_code: None,
                created_at_ms: Some(1_699_999_000_000),
                updated_at_ms: Some(1_699_999_900_000),
                agent_done_at_ms: Some(1_699_999_900_000),
                confirmed_at_ms: None,
                updated_at: "13:45".into(),
            },
            BackgroundTask {
                id: "task-103".into(),
                channel_id: "ops".into(),
                session_id: String::new(),
                kind: None,
                background: None,
                title: "Audit ~/Downloads watcher rule".into(),
                status: TaskLifecycleStatus::Paused,
                raw_status: Some("waiting".into()),
                progress: 40.0,
                progress_label: None,
                last_step: Some("Waiting on confirmation".into()),
                steps: vec![],
                result: None,
                error_code: None,
                created_at_ms: Some(1_699_999_500_000),
                updated_at_ms: Some(1_699_999_700_000),
                agent_done_at_ms: None,
                confirmed_at_ms: None,
                updated_at: "12:10".into(),
            },
        ]
    }

    fn mk_entry(id: &str, channel: &str, role: EntryRole, text: &str) -> TerminalEntry {
        TerminalEntry::new(id, channel, role, text, "19:19")
    }

    #[test]
    fn formats_durations() {
        let base = 1_700_000_000_000u64;
        assert_eq!(format_elapsed_duration(Some(base), base + 42_000, true), "42s");
        assert_eq!(format_elapsed_duration(Some(base), base + (14 * 60 + 8) * 1000, true), "14m 08s");
        assert_eq!(format_elapsed_duration(Some(base), base + 67 * 60 * 1000, true), "1h 07m");
        assert_eq!(format_elapsed_duration(Some(base), base - 5000, true), "0s");
        assert_eq!(format_elapsed_duration(None, base, true), "0s");
        // TS falsy semantics: createdAtMs === 0 renders 0s.
        assert_eq!(format_elapsed_duration(Some(0), base, true), "0s");
        assert_eq!(format_elapsed_duration(Some(base), base + 14 * 60 * 1000, false), "14m");
    }

    #[test]
    fn formats_start_clock() {
        assert_eq!(format_start_clock(None), "");
        let clock = format_start_clock(Some(1_700_000_000_000));
        assert_eq!(clock.len(), 5);
        assert!(clock.as_bytes()[2] == b':');
        assert!(clock.chars().all(|c| c.is_ascii_digit() || c == ':'));
        // 1_700_000_000_000 ms = 2023-11-14 22:13:20 UTC.
        assert_eq!(clock, "22:13");
    }

    #[test]
    fn upgrades_generic_running_placeholder_in_place() {
        let initial = vec![TaskStepItem {
            text: "Running terminal".into(),
            done: false,
            active: true,
        }];
        let (steps, ratio) = apply_tool_progress_to_steps(
            &initial,
            "terminal",
            Some("terminal: cargo test"),
            "running",
            None,
        );
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].text, "terminal: cargo test");
        assert_eq!(ratio, "0/1");

        let (done_steps, done_ratio) = apply_tool_progress_to_steps(
            &steps,
            "terminal",
            None,
            "done",
            Some("terminal: cargo test passed"),
        );
        assert_eq!(done_steps.len(), 1);
        assert!(done_steps[0].done);
        assert_eq!(done_ratio, "1/1");
    }

    #[test]
    fn resolves_progress_and_display_title() {
        let mut t = sample_task(TaskLifecycleStatus::Running);
        let p = resolve_task_progress(&t);
        assert_eq!(p.pct, 50);
        assert_eq!(p.ratio_label, "1/2");

        t.title = "go".into();
        t.steps[0].text = "Run gateway check".into();
        assert_eq!(resolve_task_display_title(&t), "Run gateway check");
    }

    // ---- Mirrored TS suite ----

    #[test]
    fn ts_resolve_task_step_breakdown_marks_completed_active_pending() {
        let tasks = ts_sample_tasks();
        let breakdown = resolve_task_step_breakdown(&tasks[0]);
        assert_eq!(breakdown.completed_count, 4);
        assert_eq!(breakdown.total_count, 6);
        assert_eq!(breakdown.ratio_label, "4/6");
        assert_eq!(breakdown.active_step_text, "Streaming update_task delta across Playwright parity lane");
        assert_eq!(breakdown.steps[3].state, ResolvedStepState::Done);
        assert_eq!(breakdown.steps[4].state, ResolvedStepState::Active);
        assert_eq!(breakdown.steps[5].state, ResolvedStepState::Pending);

        let done_breakdown = resolve_task_step_breakdown(&tasks[1]);
        assert_eq!(done_breakdown.completed_count, 2);
        assert_eq!(done_breakdown.total_count, 2);
        assert!(done_breakdown.steps.iter().all(|s| s.state == ResolvedStepState::Done));

        let mut fallback = sample_task(TaskLifecycleStatus::Running);
        fallback.id = "t-x".into();
        fallback.channel_id = "general".into();
        fallback.title = "Quick job".into();
        fallback.progress = 0.0;
        fallback.progress_label = Some("0/6".into());
        fallback.last_step = Some("Reproduce mute bug".into());
        fallback.steps = vec![];
        let fb = resolve_task_step_breakdown(&fallback);
        assert_eq!(fb.ratio_label, "0/6");
        assert_eq!(fb.total_count, 6);
        assert_eq!(fb.steps[0].state, ResolvedStepState::Active);
    }

    #[test]
    fn ts_partition_channel_tasks_isolates_threads_and_orders_unconfirmed_first() {
        let tasks = ts_sample_tasks();
        let launch = partition_channel_tasks(&tasks, "launch");
        assert_eq!(launch.running.len(), 1);
        assert_eq!(launch.paused.len(), 0);
        assert_eq!(launch.completed.len(), 0);
        assert_eq!(launch.all[0].id, "task-101");

        let general = partition_channel_tasks(&tasks, "general");
        assert_eq!(general.all.len(), 0);

        let ops = partition_channel_tasks(&tasks, "ops");
        assert_eq!(ops.paused.len(), 1);
        assert_eq!(ops.paused[0].id, "task-103");

        let mut confirmed_older = tasks[1].clone();
        confirmed_older.id = "task-confirmed".into();
        confirmed_older.confirmed_at_ms = Some(1_700_000_500_000);
        confirmed_older.updated_at_ms = Some(1_700_000_500_000);
        let research = partition_channel_tasks(&[confirmed_older, tasks[1].clone()], "research");
        assert_eq!(research.completed[0].id, "task-102");
        assert_eq!(research.completed[1].id, "task-confirmed");
    }

    #[test]
    fn ts_summarize_channel_task_activity_highlights_active_threads() {
        let tasks = ts_sample_tasks();
        let channels: Vec<ChannelMeta> = ["general", "launch", "ops", "research"]
            .iter()
            .map(|id| ChannelMeta {
                id: (*id).into(),
                name: (*id).into(),
                category: "hermes".into(),
                description: String::new(),
                session_id: format!("hermes-{id}"),
                unread: 0,
                origin: crate::types::SessionOrigin::Workspace,
                has_local_history: true,
                model: None,
                updated_at_ms: None,
            })
            .collect();
        let mut thinking = HashMap::new();
        thinking.insert("general".to_string(), true);
        let summary = summarize_channel_task_activity(&tasks, &channels, &thinking);

        assert_eq!(summary["launch"].running_count, 1);
        assert!(summary["launch"].has_active_task);
        assert_eq!(summary["launch"].primary_title.as_deref(), Some("Rebuild standing morning-brief.html (save_page)"));
        assert_eq!(summary["general"].running_count, 0);
        assert!(summary["general"].is_thinking);
        assert!(summary["general"].has_active_task);
        assert_eq!(summary["ops"].running_count, 0);
        assert_eq!(summary["ops"].paused_count, 1);
        assert!(!summary["ops"].has_active_task);
        assert_eq!(summary["research"].completed_count, 1);
        assert!(!summary["research"].has_active_task);
    }

    #[test]
    fn ts_status_display_and_confirmation_helpers() {
        let tasks = ts_sample_tasks();
        assert_eq!(get_task_status_display(&tasks[0]).label, "Working");
        let mut queued = tasks[0].clone();
        queued.raw_status = Some("queued".into());
        assert_eq!(get_task_status_display(&queued).label, "Queued");
        assert_eq!(get_task_status_display(&tasks[2]).label, "Waiting");
        let mut paused = tasks[2].clone();
        paused.raw_status = Some("paused".into());
        assert_eq!(get_task_status_display(&paused).label, "Paused");
        assert_eq!(get_task_status_display(&tasks[1]).label, "Done");
        let mut failed = tasks[1].clone();
        failed.raw_status = Some("failed".into());
        assert_eq!(get_task_status_display(&failed).label, "Failed");
        let mut cancelled = tasks[1].clone();
        cancelled.raw_status = Some("cancelled".into());
        assert_eq!(get_task_status_display(&cancelled).label, "Cancelled");

        assert_eq!(get_task_kind_label(&tasks[0]), "Background");
        let mut in_chat = tasks[0].clone();
        in_chat.background = Some(false);
        assert_eq!(get_task_kind_label(&in_chat), "In chat");

        assert_eq!(get_task_confirmation_state(&tasks[0]), TaskConfirmationState::Open);
        assert_eq!(get_task_confirmation_label(&tasks[0]), "Open");
        assert_eq!(get_task_confirmation_state(&tasks[1]), TaskConfirmationState::NeedsConfirm);
        assert_eq!(get_task_confirmation_label(&tasks[1]), "Looks done — confirm");
        let mut confirmed = tasks[1].clone();
        confirmed.confirmed_at_ms = Some(1_700_000_000_000);
        assert_eq!(get_task_confirmation_state(&confirmed), TaskConfirmationState::Confirmed);
        assert_eq!(get_task_confirmation_label(&confirmed), "Confirmed by you");
    }

    #[test]
    fn ts_resolve_task_progress_percentages_and_indeterminate() {
        let tasks = ts_sample_tasks();
        let p0 = resolve_task_progress(&tasks[0]);
        assert_eq!((p0.pct, p0.ratio_label.as_str(), p0.is_indeterminate), (67, "4/6", false));
        let p1 = resolve_task_progress(&tasks[1]);
        assert_eq!((p1.pct, p1.ratio_label.as_str(), p1.is_indeterminate), (100, "2/2", false));
        let p2 = resolve_task_progress(&tasks[2]);
        assert_eq!((p2.pct, p2.ratio_label.as_str(), p2.is_indeterminate), (40, "40%", false));

        let mut zero_running = sample_task(TaskLifecycleStatus::Running);
        zero_running.id = "t-zero".into();
        zero_running.title = "Starting task".into();
        zero_running.progress = 0.0;
        zero_running.progress_label = Some("0/4".into());
        zero_running.last_step = Some("Initializing runner".into());
        zero_running.steps = vec![];
        let pz = resolve_task_progress(&zero_running);
        assert_eq!((pz.pct, pz.ratio_label.as_str(), pz.is_indeterminate), (0, "0/4", true));

        let mut failed_partial = tasks[0].clone();
        failed_partial.status = TaskLifecycleStatus::Completed;
        failed_partial.raw_status = Some("failed".into());
        failed_partial.progress = 50.0;
        assert_eq!(resolve_task_progress(&failed_partial).pct, 67);
    }

    #[test]
    fn ts_turn_task_lifecycle_helpers() {
        assert_eq!(
            derive_turn_task_title("https://github.com/Dev/hermes-agentis check git log", None),
            "github.com/Dev/hermes-agentis check git log"
        );
        assert_eq!(derive_turn_task_title("   ", Some("#https-github-com")), "#https-github-com");

        let initial = vec![TaskStepItem { text: "Working in hermes-https-github-com".into(), done: false, active: true }];
        let (after_read, progress) = apply_tool_progress_to_steps(&initial, "read_file", None, "running", Some("Running read_file"));
        assert_eq!(progress, "1/2");
        assert!(after_read[0].done);
        assert_eq!(after_read[1].text, "Running read_file");
        assert!(after_read[1].active);

        let (after_term_running, _) = apply_tool_progress_to_steps(&after_read, "terminal", None, "running", Some("Running terminal"));
        assert_eq!(after_term_running.len(), 3);
        assert!(after_term_running[1].done);
        assert!(after_term_running[2].active);

        let (after_term, _) = apply_tool_progress_to_steps(&after_term_running, "terminal", None, "done", Some("terminal: git log --oneline -3 main"));
        assert_eq!(after_term.len(), 3);
        assert!(after_term[2].done);
        assert_eq!(after_term[2].text, "terminal: git log --oneline -3 main");

        let (streaming, _, changed) = apply_streaming_delta_to_steps(&after_term);
        assert!(changed);
        assert_eq!(streaming.last().unwrap().text, "Composing response");

        let (settled, settled_progress) = finalize_turn_task_steps(&streaming, TurnFinalOutcome::Done, Some("Completed response"));
        assert_eq!(settled_progress, format!("{}/{}", settled.len(), settled.len()));
        assert!(settled.iter().all(|s| s.done && !s.active));

        let (cancelled, _) = finalize_turn_task_steps(&after_read, TurnFinalOutcome::Cancelled, Some("Stopped by operator"));
        assert_eq!(cancelled.last().unwrap().text, "Stopped by operator");
    }

    #[test]
    fn ts_merge_tasks_with_session_activity_surfaces_dynamic_threads() {
        let tasks = ts_sample_tasks();
        let mut u1 = mk_entry("u-1", "https-github-com", EntryRole::User, "https://github.com/Dev/demo scrub repo and make public");
        u1.created_at_ms = Some(1_700_000_000_000);
        u1.timestamp = "19:17".into();
        let mut h1 = mk_entry("h-1", "https-github-com", EntryRole::Hermes, "On it — surveying the repo first.");
        h1.created_at_ms = Some(1_700_000_005_000);
        h1.timestamp = "19:17".into();
        let mut a1 = mk_entry("a-1", "https-github-com", EntryRole::Activity, "Running read_file");
        a1.activity_status = Some("running".into());
        a1.created_at_ms = Some(1_700_000_120_000);
        let mut a2 = mk_entry("a-2", "https-github-com", EntryRole::Activity, "terminal: git log --oneline -3 main");
        a2.activity_status = Some("done".into());
        a2.created_at_ms = Some(1_700_000_125_000);
        let mut a3 = mk_entry("a-3", "https-github-com", EntryRole::Activity, "Running terminal");
        a3.activity_status = Some("running".into());
        a3.created_at_ms = Some(1_700_000_130_000);
        let entries = vec![u1.clone(), h1.clone(), a1.clone(), a2.clone(), a3.clone()];

        let session = HermesAgentSession {
            channel_id: "https-github-com".into(),
            session_id: "hermes-https-github-com".into(),
            title: "https-github-com".into(),
            status: SessionStatus::Idle,
            model: String::new(),
            source: String::new(),
            last_message: String::new(),
            updated_at_ms: Some(1_700_000_130_000),
            message_count: 1,
        };
        let merged = merge_tasks_with_session_activity(&tasks, &entries, &[session], &HashMap::new(), &[], &HashSet::new(), 1_700_000_999_000);
        let dynamic = partition_channel_tasks(&merged, "https-github-com");
        assert_eq!(dynamic.running.len(), 1);
        assert_eq!(dynamic.running[0].id, "live-turn-https-github-com");
        assert_eq!(dynamic.running[0].steps.len(), 3);
        assert_eq!(dynamic.running[0].steps[2].text, "Running terminal");
        assert!(dynamic.running[0].steps[2].active);
        // createdAtMs from latest prompt, updatedAtMs from last activity (not now).
        assert_eq!(dynamic.running[0].created_at_ms, Some(1_700_000_000_000));
        assert_eq!(dynamic.running[0].updated_at_ms, Some(1_700_000_130_000));

        // Never duplicates when an explicit running task already exists.
        let launch_session = HermesAgentSession {
            channel_id: "launch".into(),
            session_id: "hermes-launch".into(),
            title: "launch".into(),
            status: SessionStatus::Running,
            model: String::new(),
            source: String::new(),
            last_message: String::new(),
            updated_at_ms: Some(1_700_000_240_000),
            message_count: 1,
        };
        let mut thinking = HashMap::new();
        thinking.insert("launch".to_string(), true);
        let no_duplicate = merge_tasks_with_session_activity(&tasks, &[], &[launch_session], &thinking, &[], &HashSet::new(), 0);
        assert_eq!(partition_channel_tasks(&no_duplicate, "launch").running.len(), 1);

        // Completed tool trace on a thread with no server task surfaces as completed.
        let mut a1_done = a1.clone();
        a1_done.text = "terminal: git status".into();
        a1_done.activity_status = Some("done".into());
        let mut h1_later = h1.clone();
        h1_later.created_at_ms = Some(1_700_000_140_000);
        let completed_entries = vec![u1.clone(), a1_done, h1_later];
        let mut confirmed = HashSet::new();
        confirmed.insert("trace-turn-https-github-com".to_string());
        let with_trace = merge_tasks_with_session_activity(&tasks, &completed_entries, &[], &HashMap::new(), &[], &confirmed, 0);
        let completed_partition = partition_channel_tasks(&with_trace, "https-github-com");
        assert_eq!(completed_partition.completed.len(), 1);
        assert_eq!(completed_partition.completed[0].id, "trace-turn-https-github-com");
        assert!(completed_partition.completed[0].confirmed_at_ms.is_some());
    }

    #[test]
    fn ts_summarize_thread_tasks_and_initial_expansion() {
        let tasks = ts_sample_tasks();
        let launch_partition = partition_channel_tasks(&tasks, "launch");
        let launch_summary = summarize_thread_tasks(&launch_partition, false);
        assert_eq!(launch_summary.total_count, 1);
        assert_eq!(launch_summary.running_count, 1);
        assert_eq!(launch_summary.header_badge, "1 active");

        assert!(is_task_initially_expanded(&tasks[0], Some("task-101"), true));
        assert!(!is_task_initially_expanded(&tasks[0], Some("task-101"), false));
        assert!(!is_task_initially_expanded(&tasks[1], Some("task-101"), true));

        let research_partition = partition_channel_tasks(&tasks, "research");
        let research_summary = summarize_thread_tasks(&research_partition, false);
        assert_eq!(research_summary.total_count, 1);
        assert_eq!(research_summary.needs_confirm_count, 1);
        assert_eq!(research_summary.header_badge, "1 to confirm");

        let empty_partition = partition_channel_tasks(&tasks, "general");
        assert_eq!(summarize_thread_tasks(&empty_partition, true).header_badge, "1 active");
        assert_eq!(summarize_thread_tasks(&empty_partition, false).total_count, 0);
        assert_eq!(summarize_thread_tasks(&empty_partition, false).header_badge, "");

        // P1-06: task panel hidden by default.
        let mut panel = ThreadTaskPanelState::default();
        assert!(!panel.visible);
        panel.toggle();
        assert!(panel.visible);
    }

    #[test]
    fn ts_dedupe_and_tool_progress_collapse_running_placeholders() {
        let mut d1 = mk_entry("d1", "general", EntryRole::Activity, "Running read_file");
        d1.activity_status = Some("running".into());
        let mut d2 = mk_entry("d2", "general", EntryRole::Activity, "read_file: ci-workflows.test.ts L40-119");
        d2.activity_status = Some("done".into());
        let mut d3 = mk_entry("d3", "general", EntryRole::Activity, "Running terminal");
        d3.activity_status = Some("running".into());
        let mut d4 = mk_entry("d4", "general", EntryRole::Activity, "terminal: gh pr checks 392");
        d4.activity_status = Some("running".into());
        let deduped = dedupe_activity_entries(&[d1, d2, d3, d4]);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].text, "read_file: ci-workflows.test.ts L40-119");
        assert_eq!(deduped[1].text, "terminal: gh pr checks 392");

        let (s1, _) = apply_tool_progress_to_steps(
            &[TaskStepItem { text: "Working in hermes-general".into(), done: false, active: true }],
            "terminal",
            None,
            "running",
            Some("Running terminal"),
        );
        let (s2, _) = apply_tool_progress_to_steps(&s1, "terminal", None, "running", Some("terminal: gh pr checks 392"));
        assert_eq!(s2.len(), 2);
        assert_eq!(s2[1].text, "terminal: gh pr checks 392");

        let (s3, progress3) = apply_tool_progress_to_steps(&s2, "terminal", None, "finished", Some("terminal: gh pr checks 392 (passed)"));
        assert_eq!(s3.len(), 2);
        assert!(s3[1].done);
        assert_eq!(progress3, "2/2");
    }

    #[test]
    fn ts_resolve_task_display_title_promotes_vague_nudges() {
        let mut vague = sample_task(TaskLifecycleStatus::Running);
        vague.id = "task-turn-1".into();
        vague.channel_id = "general".into();
        vague.title = "what do you suggest".into();
        vague.progress = 38.0;
        vague.last_step = Some("terminal: gh pr checks 392 --repo Maia-VM/hermes".into());
        vague.steps = vec![
            TaskStepItem { text: "Working in hermes-general".into(), done: true, active: false },
            TaskStepItem { text: "Running read_file".into(), done: true, active: false },
            TaskStepItem { text: "terminal: gh pr checks 392 --repo Maia-VM/hermes".into(), done: false, active: true },
        ];
        assert_eq!(resolve_task_display_title(&vague), "terminal: gh pr checks 392 --repo Maia-VM/hermes");

        let mut concrete = vague.clone();
        concrete.title = "Audit CI checks on Maia-VM/hermes PR 392".into();
        assert_eq!(resolve_task_display_title(&concrete), "Audit CI checks on Maia-VM/hermes PR 392");
    }

    #[test]
    fn ts_failed_breakdown_outcome_snippet_and_aggregate_rollup() {
        let tasks = ts_sample_tasks();
        let mut failed = tasks[0].clone();
        failed.id = "task-failed-1".into();
        failed.status = TaskLifecycleStatus::Completed;
        failed.raw_status = Some("failed".into());
        failed.result = Some("Gateway returned 502 Bad Gateway during publish step after retries.".into());
        failed.steps = vec![
            TaskStepItem { text: "Collect changed thread summaries".into(), done: true, active: false },
            TaskStepItem { text: "Publish final save_page artifact".into(), done: false, active: false },
        ];
        let fb = resolve_task_step_breakdown(&failed);
        assert_eq!(fb.completed_count, 1);
        assert_eq!(fb.total_count, 2);
        assert_eq!(fb.steps[0].state, ResolvedStepState::Done);
        assert_eq!(fb.steps[1].state, ResolvedStepState::Pending);

        assert_eq!(format_task_outcome_snippet(&failed, 45).as_deref(), Some("Gateway returned 502 Bad Gateway during…"));
        let mut redundant = failed.clone();
        redundant.result = failed.last_step.clone();
        assert_eq!(format_task_outcome_snippet(&redundant, 45), None);

        let launch_plus_failed = partition_channel_tasks(&[tasks[0].clone(), failed], "launch");
        let agg = summarize_thread_task_aggregate(&launch_plus_failed, false);
        assert_eq!(agg.total_tasks, 2);
        assert_eq!(agg.active_count, 1);
        assert_eq!(agg.completed_count, 1);
        assert_eq!(agg.failed_count, 1);
        // 67% (4/6) + 50% (1/2) -> Math.round(58.5) = 59.
        assert_eq!(agg.aggregate_pct, 59);
    }

    #[test]
    fn ts_extract_trailing_turn_context_and_running_detection() {
        let mut u = mk_entry("u", "ch", EntryRole::User, "audit the repo");
        u.created_at_ms = Some(100);
        let h = mk_entry("h", "ch", EntryRole::Hermes, "preamble");
        let mut a_run = mk_entry("a1", "ch", EntryRole::Activity, "Running terminal");
        a_run.activity_status = Some("running".into());
        let mut a_done = mk_entry("a2", "ch", EntryRole::Activity, "terminal: ls");
        a_done.activity_status = Some("done".into());

        let ctx = extract_trailing_turn_context(&[u.clone(), h, a_run.clone(), a_done.clone()]);
        assert!(ctx.has_active_activity);
        assert_eq!(ctx.latest_prompt, "audit the repo");
        assert_eq!(ctx.latest_prompt_at_ms, Some(100));
        assert_eq!(ctx.trailing_activities.len(), 2);
        assert_eq!(ctx.latest_activity_batch.len(), 2);

        // Slash commands never become the latest prompt.
        let mut slash = mk_entry("c", "ch", EntryRole::Command, "/gateway");
        slash.created_at_ms = Some(200);
        let ctx2 = extract_trailing_turn_context(&[u, slash]);
        assert_eq!(ctx2.latest_prompt, "audit the repo");

        assert!(is_activity_entry_running(&a_run));
        assert!(!is_activity_entry_running(&a_done));
        let mut bare = mk_entry("a3", "ch", EntryRole::Activity, "Running");
        bare.activity_status = None;
        assert!(is_activity_entry_running(&bare));
        let mut wordy = mk_entry("a4", "ch", EntryRole::Activity, "runningaway");
        wordy.activity_status = None;
        assert!(!is_activity_entry_running(&wordy));
    }

    #[test]
    fn ts_finalize_budget_and_streaming_idempotence() {
        // Budget outcome appends the final label when missing.
        let prev = vec![TaskStepItem { text: "Working in hermes-general".into(), done: false, active: true }];
        let (steps, progress) = finalize_turn_task_steps(&prev, TurnFinalOutcome::IterationBudgetExhausted, Some("Partial answer"));
        assert!(steps.iter().all(|s| s.done));
        assert!(steps.iter().any(|s| s.text == "Partial answer"));
        assert_eq!(progress, format!("{}/{}", steps.len(), steps.len()));

        let (empty_steps, _) = finalize_turn_task_steps(&[], TurnFinalOutcome::IterationBudgetExhausted, None);
        assert_eq!(empty_steps[0].text, "Iteration budget reached (partial answer)");
        let (done_steps, _) = finalize_turn_task_steps(&[], TurnFinalOutcome::Done, None);
        assert_eq!(done_steps[0].text, "Completed response");

        // Streaming delta is idempotent while Composing response is the open active step.
        let (s1, _, changed1) = apply_streaming_delta_to_steps(&prev);
        assert!(changed1);
        let (s2, ratio2, changed2) = apply_streaming_delta_to_steps(&s1);
        assert!(!changed2);
        assert_eq!(s2.len(), s1.len());
        assert_eq!(ratio2, format!("{}/{}", s1.iter().filter(|s| s.done).count(), s1.len()));
    }
}