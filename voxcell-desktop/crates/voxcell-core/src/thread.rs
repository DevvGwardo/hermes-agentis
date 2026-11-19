//! Port of `src/utils/threadTrace.ts`, `threadScroll.ts`, `threadMinimap.ts`
//! (P1-06 parity audit).
//!
//! CRITICAL thread invariant (never break): activity runs interleave
//! CHRONOLOGICALLY with streamed replies. Activity that arrives after streamed
//! text renders as a NEW trace block below that text — late tool steps are never
//! hoisted/merged back into the earlier activity tree above the reply. Turn
//! boundaries are user/command/system/diagnostic entries. The live "thinking"
//! state attaches to the trailing trace block of the last turn only, and no
//! dangling "Activity · Running" placeholder is emitted below streamed text once
//! the turn has reply text and no running tools.

use crate::tasks::dedupe_activity_entries;
use crate::types::{ActivityKind, EntryRole, TerminalEntry, TerminalOutcome};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceStepIcon {
    Reasoning,
    Commentary,
    Spawn,
    Tool,
    Link,
    Status,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifiedTraceStep {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub timestamp: String,
    pub icon: TraceStepIcon,
    pub is_reasoning: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThreadTimelineBlock {
    Entry {
        id: String,
        entry: TerminalEntry,
    },
    Trace {
        id: String,
        items: Vec<ClassifiedTraceStep>,
        is_live: bool,
    },
}

impl ThreadTimelineBlock {
    pub fn id(&self) -> &str {
        match self {
            Self::Entry { id, .. } | Self::Trace { id, .. } => id,
        }
    }

    pub fn is_trace(&self) -> bool {
        matches!(self, Self::Trace { .. })
    }
}

/// Mirrors `/^<word>\s*[:—-]\s*/i` — strips the activity prefix when present.
fn strip_labeled_prefix(raw: &str, word: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let rest = lower.strip_prefix(word)?;
    // Work on the original string to preserve detail casing; locate the same
    // byte offset (word is ASCII so lowercasing does not shift offsets).
    let original_rest = &raw[rest.len()..];
    let mut chars = original_rest.char_indices().peekable();
    // \s* before the separator
    while let Some((_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    match chars.next() {
        Some((_, ':')) | Some((_, '—')) | Some((_, '-')) => {}
        _ => return None,
    }
    // \s* after the separator
    let mut end = original_rest.len();
    for (idx, c) in chars {
        if !c.is_whitespace() {
            end = idx;
            break;
        }
    }
    Some(original_rest[end..].to_string())
}

pub fn classify_trace_step(entry: &TerminalEntry) -> ClassifiedTraceStep {
    let raw = entry.text.trim();
    let lower = raw.to_ascii_lowercase();

    if entry.activity_kind == Some(ActivityKind::Reasoning) {
        let stripped = strip_labeled_prefix(raw, "reasoning").unwrap_or_else(|| raw.to_string());
        let detail = entry
            .reasoning_detail
            .as_deref()
            .unwrap_or(stripped.as_str())
            .trim()
            .to_string();
        return ClassifiedTraceStep {
            id: entry.id.clone(),
            label: "Reasoning".to_string(),
            detail,
            timestamp: entry.timestamp.clone(),
            icon: TraceStepIcon::Reasoning,
            is_reasoning: true,
        };
    }

    if entry.activity_kind == Some(ActivityKind::Commentary) {
        let detail = strip_labeled_prefix(raw, "commentary")
            .unwrap_or_else(|| raw.to_string())
            .trim()
            .to_string();
        return ClassifiedTraceStep {
            id: entry.id.clone(),
            label: "Commentary".to_string(),
            detail,
            timestamp: entry.timestamp.clone(),
            icon: TraceStepIcon::Commentary,
            is_reasoning: false,
        };
    }

    let icon = match entry.activity_kind {
        Some(ActivityKind::Subagent) => TraceStepIcon::Spawn,
        Some(ActivityKind::Tool) => TraceStepIcon::Tool,
        _ => {
            if lower.starts_with("reasoning:") || lower.starts_with("reasoning —") {
                TraceStepIcon::Reasoning
            } else if lower.contains("spawn") || lower.contains("agent") || lower.contains("session") {
                TraceStepIcon::Spawn
            } else if lower.contains("link")
                || lower.contains("http")
                || lower.contains("tab")
                || lower.contains("mcp")
                || lower.contains("gateway")
            {
                TraceStepIcon::Link
            } else if lower.contains("run")
                || lower.contains("exec")
                || lower.contains("tool")
                || lower.contains("read")
                || lower.contains("write")
                || lower.contains("search")
                || lower.contains("brief")
            {
                TraceStepIcon::Tool
            } else {
                TraceStepIcon::Status
            }
        }
    };

    // Split concise label vs trailing detail around colon or em-dash when present.
    let chars: Vec<char> = raw.chars().collect();
    let colon_idx = chars.iter().position(|c| *c == ':');
    let dash_idx = (0..chars.len().saturating_sub(2))
        .find(|&i| chars[i] == ' ' && chars[i + 1] == '—' && chars[i + 2] == ' ');

    let mut label = raw.to_string();
    let mut detail = String::new();
    if let Some(idx) = colon_idx {
        if idx > 2 && idx < 28 {
            label = chars[..idx].iter().collect::<String>().trim().to_string();
            detail = chars[idx + 1..].iter().collect::<String>().trim().to_string();
        }
    } else if let Some(idx) = dash_idx {
        if idx > 2 && idx < 28 {
            label = chars[..idx].iter().collect::<String>().trim().to_string();
            detail = chars[idx + 3..].iter().collect::<String>().trim().to_string();
        }
    } else if let Some((head, status)) = split_status_suffix(raw) {
        // /^(.+?)\s+(completed|complete|done|finished|running|started|failed|error|cancelled|interrupted)$/i
        label = head;
        detail = status;
    }

    ClassifiedTraceStep {
        id: entry.id.clone(),
        label: if label.is_empty() { "Activity".to_string() } else { label },
        detail,
        timestamp: entry.timestamp.clone(),
        icon,
        is_reasoning: icon == TraceStepIcon::Reasoning,
    }
}

const STATUS_SUFFIXES: &[&str] = &[
    "completed",
    "complete",
    "done",
    "finished",
    "running",
    "started",
    "failed",
    "error",
    "cancelled",
    "interrupted",
];

/// Non-greedy prefix + final status token: split at the last whitespace run and
/// check the trailing token against the status vocabulary.
fn split_status_suffix(raw: &str) -> Option<(String, String)> {
    let trimmed = raw.trim();
    let split_at = trimmed
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, _)| i)?;
    let head = trimmed[..split_at].trim();
    let tail = trimmed[split_at..].trim().to_ascii_lowercase();
    if head.is_empty() || !STATUS_SUFFIXES.contains(&tail.as_str()) {
        return None;
    }
    Some((head.to_string(), tail))
}

pub fn is_terminal_diagnostic_activity(entry: &TerminalEntry) -> bool {
    if entry.role != EntryRole::Activity {
        return false;
    }
    if matches!(
        entry.activity_kind,
        Some(
            ActivityKind::Tool
                | ActivityKind::Reasoning
                | ActivityKind::Subagent
                | ActivityKind::Commentary
        )
    ) {
        return false;
    }
    entry.error_code.is_some()
        || matches!(
            entry.terminal_outcome,
            Some(
                TerminalOutcome::Failed
                    | TerminalOutcome::Cancelled
                    | TerminalOutcome::AdmissionConflict
            )
        )
        || entry.activity_status.as_deref() == Some("error")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoalescedTraceStep {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub timestamp: String,
    pub icon: TraceStepIcon,
    pub is_reasoning: bool,
    pub repeat_count: usize,
}

/// Collapses consecutive identical completed tool/link/status steps (such as
/// repeated "terminal completed" entries) into one row with a repeat count and
/// latest timestamp while leaving reasoning/commentary steps unmerged.
pub fn coalesce_consecutive_steps(steps: &[ClassifiedTraceStep]) -> Vec<CoalescedTraceStep> {
    let mut result: Vec<CoalescedTraceStep> = Vec::new();
    for step in steps {
        let can_merge = result.last().is_some_and(|prev| {
            !step.is_reasoning
                && !prev.is_reasoning
                && step.icon != TraceStepIcon::Commentary
                && prev.icon != TraceStepIcon::Commentary
                && prev.icon == step.icon
                && prev.label.to_ascii_lowercase() == step.label.to_ascii_lowercase()
                && prev.detail.to_ascii_lowercase() == step.detail.to_ascii_lowercase()
        });
        if can_merge {
            let prev = result.last_mut().expect("checked above");
            prev.repeat_count += 1;
            if !step.timestamp.is_empty() {
                prev.timestamp = step.timestamp.clone();
            }
        } else {
            result.push(CoalescedTraceStep {
                id: step.id.clone(),
                label: step.label.clone(),
                detail: step.detail.clone(),
                timestamp: step.timestamp.clone(),
                icon: step.icon,
                is_reasoning: step.is_reasoning,
                repeat_count: 1,
            });
        }
    }
    result
}

fn is_turn_boundary_entry(entry: &TerminalEntry) -> bool {
    matches!(
        entry.role,
        EntryRole::User | EntryRole::Command | EntryRole::System
    ) || is_terminal_diagnostic_activity(entry)
}

enum FlowItem {
    Trace(Vec<TerminalEntry>),
    Reply(TerminalEntry),
}

struct TurnSegment {
    boundary: Option<TerminalEntry>,
    flow: Vec<FlowItem>,
}

impl TurnSegment {
    fn has_content(&self) -> bool {
        self.boundary.is_some() || !self.flow.is_empty()
    }
}

/// Groups consecutive `activity` entries into collapsible vertical trace trees
/// and optionally attaches the live thinking state to the trailing trace tree.
///
/// Within each conversational turn (delimited by user/command/system/diagnostic
/// boundaries), activity runs and assistant replies keep their chronological
/// interleaving: activity before a reply renders above it; activity after
/// streamed text renders as a NEW trace block below that text. Live thinking
/// attaches to the trailing trace block of the last turn only.
pub fn group_thread_timeline(
    entries: &[TerminalEntry],
    is_thinking: bool,
    include_live_placeholder: bool,
) -> Vec<ThreadTimelineBlock> {
    let mut blocks: Vec<ThreadTimelineBlock> = Vec::new();

    // 1. Partition entries into turn segments, preserving chronological flow.
    let mut segments: Vec<TurnSegment> = Vec::new();
    let mut current = TurnSegment { boundary: None, flow: Vec::new() };
    for entry in entries {
        if is_turn_boundary_entry(entry) {
            if current.has_content() {
                segments.push(current);
            }
            current = TurnSegment { boundary: Some(entry.clone()), flow: Vec::new() };
        } else if entry.role == EntryRole::Activity {
            match current.flow.last_mut() {
                Some(FlowItem::Trace(items)) => items.push(entry.clone()),
                _ => current.flow.push(FlowItem::Trace(vec![entry.clone()])),
            }
        } else {
            current.flow.push(FlowItem::Reply(entry.clone()));
        }
    }
    if current.has_content() {
        segments.push(current);
    }

    if segments.is_empty() {
        if is_thinking && include_live_placeholder {
            blocks.push(ThreadTimelineBlock::Trace {
                id: "trace-live".to_string(),
                items: Vec::new(),
                is_live: true,
            });
        }
        return blocks;
    }

    // 2. Emit each segment: boundary entry, then chronological flow items.
    let last_segment_idx = segments.len() - 1;
    for (idx, seg) in segments.iter().enumerate() {
        let is_last_segment = idx == last_segment_idx;
        if let Some(boundary) = &seg.boundary {
            blocks.push(ThreadTimelineBlock::Entry {
                id: boundary.id.clone(),
                entry: boundary.clone(),
            });
        }

        let seg_activities: Vec<&TerminalEntry> = seg
            .flow
            .iter()
            .flat_map(|item| match item {
                FlowItem::Trace(items) => items.iter().collect::<Vec<_>>(),
                FlowItem::Reply(_) => Vec::new(),
            })
            .collect();
        let has_non_empty_reply = seg.flow.iter().any(|item| match item {
            FlowItem::Reply(entry) => !entry.text.trim().is_empty(),
            FlowItem::Trace(_) => false,
        });
        let has_running_tool = seg_activities
            .iter()
            .any(|a| a.activity_status.as_deref() == Some("running"));
        let live_for_segment =
            is_last_segment && is_thinking && (!has_non_empty_reply || has_running_tool);

        let last_trace_idx = seg
            .flow
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, FlowItem::Trace(_)))
            .map(|(i, _)| i)
            .last();

        for (i, item) in seg.flow.iter().enumerate() {
            match item {
                FlowItem::Reply(entry) => {
                    blocks.push(ThreadTimelineBlock::Entry {
                        id: entry.id.clone(),
                        entry: entry.clone(),
                    });
                }
                FlowItem::Trace(items) => {
                    let deduped = dedupe_activity_entries(items);
                    if deduped.is_empty() {
                        continue;
                    }
                    blocks.push(ThreadTimelineBlock::Trace {
                        id: format!("trace-{}", deduped[0].id),
                        items: deduped.iter().map(classify_trace_step).collect(),
                        is_live: live_for_segment && last_trace_idx == Some(i),
                    });
                }
            }
        }

        let ends_with_trace = !seg.flow.is_empty() && last_trace_idx == Some(seg.flow.len() - 1);
        if live_for_segment && include_live_placeholder && !ends_with_trace {
            blocks.push(ThreadTimelineBlock::Trace {
                id: "trace-live".to_string(),
                items: Vec::new(),
                is_live: true,
            });
        }
    }

    blocks
}

// ---------------------------------------------------------------------------
// threadScroll.ts — bottom-anchor scroll pin
// ---------------------------------------------------------------------------

/// Mirrors `isNearBottom` (default threshold 96px).
pub fn is_near_bottom(scroll_top: f32, scroll_height: f32, client_height: f32, threshold_px: Option<f32>) -> bool {
    let threshold = threshold_px.unwrap_or(96.0);
    let distance = scroll_height - scroll_top - client_height;
    distance <= threshold
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollPinInput {
    pub prev_scroll_top: f32,
    pub next_scroll_top: f32,
    pub scroll_height: f32,
    pub client_height: f32,
    pub is_new_visit: bool,
    pub is_programmatic: bool,
    pub prev_pinned: bool,
    pub has_user_scroll_intent: Option<bool>,
    pub threshold_px: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollPinResult {
    pub pinned: bool,
    pub away: bool,
}

/// Computes whether a chat thread container should remain pinned to the bottom.
/// Growing content height never unpins when `is_programmatic` is active, the user
/// has not scrolled upward, or transient clamp jitter occurs without user intent.
pub fn compute_thread_scroll_pin(input: &ScrollPinInput) -> ScrollPinResult {
    if input.is_new_visit {
        return ScrollPinResult { pinned: true, away: false };
    }

    let threshold = input.threshold_px.unwrap_or(96.0);
    let distance_from_bottom =
        (input.scroll_height - input.next_scroll_top - input.client_height).max(0.0);
    let near_bottom = distance_from_bottom <= threshold;

    if near_bottom {
        return ScrollPinResult { pinned: true, away: false };
    }

    if input.is_programmatic {
        return ScrollPinResult { pinned: input.prev_pinned, away: false };
    }

    let far_threshold = 120.0_f32.max(input.client_height * 0.4);
    let scrolled_up = input.next_scroll_top < input.prev_scroll_top - 2.0;
    if !scrolled_up && input.prev_pinned {
        // Content grew beneath the viewport before the resize/rAF snap ran.
        return ScrollPinResult { pinned: true, away: false };
    }

    if input.prev_pinned
        && input.has_user_scroll_intent == Some(false)
        && distance_from_bottom <= far_threshold
    {
        // Browser clamped scrollTop during transient line-wrap shrink.
        return ScrollPinResult { pinned: true, away: false };
    }

    ScrollPinResult {
        pinned: false,
        away: distance_from_bottom > far_threshold,
    }
}

/// Convenience bottom-anchor state for the desktop virtualized transcript.
/// Kept from the original draft; the TS-parity pin decision lives in
/// `compute_thread_scroll_pin`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollAnchorState {
    pub scroll_y: f32,
    pub content_h: f32,
    pub viewport_h: f32,
    pub pinned_to_bottom: bool,
}

impl ScrollAnchorState {
    pub fn max_scroll(&self) -> f32 {
        (self.content_h - self.viewport_h).max(0.0)
    }

    pub fn is_near_bottom(&self, threshold: f32) -> bool {
        self.max_scroll() - self.scroll_y <= threshold.max(1.0)
    }

    pub fn on_content_grow(&mut self) -> f32 {
        if self.pinned_to_bottom {
            self.scroll_y = self.max_scroll();
        }
        self.scroll_y
    }

    pub fn on_user_scroll(&mut self, next_y: f32) {
        self.scroll_y = next_y.clamp(0.0, self.max_scroll());
        self.pinned_to_bottom = self.is_near_bottom(28.0);
    }
}

// ---------------------------------------------------------------------------
// threadMinimap.ts — turn minimap rail
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MinimapTurnStatus {
    Done,
    Live,
    Error,
    Cancelled,
    Budget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadMinimapTurn {
    pub id: String,
    pub anchor_id: String,
    pub turn_number: usize,
    pub title: String,
    pub snippet: String,
    pub timestamp: String,
    pub step_count: usize,
    pub has_reasoning: bool,
    pub status: MinimapTurnStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MinimapAnchorRect {
    pub id: String,
    pub top: f32,
    pub height: f32,
}

/// Strips light markdown syntax and collapses whitespace into a crisp, readable
/// single-paragraph preview suitable for floating turn cards.
pub fn clean_markdown_snippet(raw: &str, max_chars: usize) -> String {
    // 1. Fenced code blocks: keep inner content, drop the fence lines.
    let mut stage = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open) = rest.find("```") {
        stage.push_str(&rest[..open]);
        let after_open = &rest[open + 3..];
        if let Some(close) = after_open.find("```") {
            let block_inner = &after_open[..close];
            // Strip opening fence line (```lang\n?) and trim.
            let inner = match block_inner.find('\n') {
                Some(nl) => &block_inner[nl + 1..],
                None => "",
            };
            stage.push_str(inner.trim());
            rest = &after_open[close + 3..];
        } else {
            // No closing fence — leave the remainder untouched.
            stage.push_str(&rest[open..]);
            rest = "";
            break;
        }
    }
    stage.push_str(rest);

    // 2. Inline code: `x` -> x
    let mut stage2 = String::with_capacity(stage.len());
    let mut chars = stage.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '`' {
            let mut inner = String::new();
            let mut closed = false;
            while let Some(&n) = chars.peek() {
                chars.next();
                if n == '`' {
                    closed = true;
                    break;
                }
                inner.push(n);
            }
            if closed && !inner.is_empty() {
                stage2.push_str(&inner);
            } else {
                stage2.push('`');
                stage2.push_str(&inner);
            }
        } else {
            stage2.push(c);
        }
    }

    // 3. Headings: strip ^#{1,6}\s+ per line.
    let stage3: String = stage2
        .split('\n')
        .map(|line| {
            let hashes = line.chars().take_while(|c| *c == '#').count();
            if (1..=6).contains(&hashes) {
                let after = &line[hashes..];
                if after.chars().next().is_some_and(char::is_whitespace) {
                    return after.trim_start().to_string();
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // 4+5. Bold **x** then italic *x* (inner must be non-empty, no '*').
    let stage4 = strip_star_wrappers(&stage3, "**");
    let stage5 = strip_star_wrappers(&stage4, "*");

    // 6. Bullets: ^[-*+]\s+ -> '• ' per line.
    let stage6: String = stage5
        .split('\n')
        .map(|line| {
            let mut cs = line.chars();
            if let Some(first) = cs.next() {
                if matches!(first, '-' | '*' | '+') {
                    let rest: String = cs.collect();
                    if rest.chars().next().is_some_and(char::is_whitespace) {
                        return format!("• {}", rest.trim_start());
                    }
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // 7. Collapse whitespace.
    let cleaned = stage6.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = cleaned.chars().collect();
    if chars.len() <= max_chars {
        return cleaned;
    }
    let slice: String = chars.iter().take(max_chars).collect();
    let last_space = slice.rfind(' ').map(|b| slice[..b].chars().count());
    let cutoff = match last_space {
        Some(idx) if idx >= (max_chars as f32 * 0.6).floor() as usize => idx,
        _ => max_chars,
    };
    let head: String = chars.iter().take(cutoff).collect();
    format!("{}…", head.trim_end())
}

fn strip_star_wrappers(raw: &str, marker: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open) = rest.find(marker) {
        out.push_str(&rest[..open]);
        let after = &rest[open + marker.len()..];
        if let Some(close) = after.find(marker) {
            let inner = &after[..close];
            if !inner.is_empty() && !inner.contains('*') {
                out.push_str(inner);
                rest = &after[close + marker.len()..];
                continue;
            }
        }
        out.push_str(marker);
        rest = after;
    }
    out.push_str(rest);
    out
}

fn resolve_status_from_entry(entry: &TerminalEntry, prev_status: MinimapTurnStatus) -> MinimapTurnStatus {
    if entry.terminal_outcome == Some(TerminalOutcome::IterationBudgetExhausted) {
        return MinimapTurnStatus::Budget;
    }
    if matches!(
        entry.terminal_outcome,
        Some(TerminalOutcome::Cancelled | TerminalOutcome::Interrupted)
    ) {
        return MinimapTurnStatus::Cancelled;
    }
    if entry.error_code.as_deref() == Some("gateway_offline")
        || matches!(
            entry.terminal_outcome,
            Some(TerminalOutcome::Failed | TerminalOutcome::AdmissionConflict)
        )
    {
        return MinimapTurnStatus::Error;
    }
    if entry.streaming {
        return MinimapTurnStatus::Live;
    }
    match prev_status {
        MinimapTurnStatus::Error
        | MinimapTurnStatus::Cancelled
        | MinimapTurnStatus::Budget
        | MinimapTurnStatus::Live => prev_status,
        MinimapTurnStatus::Done => MinimapTurnStatus::Done,
    }
}

struct TurnDraft {
    id: String,
    anchor_id: String,
    turn_number: usize,
    title: String,
    reply_parts: Vec<String>,
    timestamp: String,
    step_count: usize,
    has_reasoning: bool,
    status: MinimapTurnStatus,
}

fn finalize_turn(draft: TurnDraft) -> ThreadMinimapTurn {
    let joined_reply = clean_markdown_snippet(&draft.reply_parts.join(" "), 190);
    let snippet = if !joined_reply.is_empty() {
        joined_reply
    } else if draft.status == MinimapTurnStatus::Live {
        if draft.step_count > 0 {
            format!(
                "Running · {} {}",
                draft.step_count,
                if draft.step_count == 1 { "step" } else { "steps" }
            )
        } else {
            "Thinking…".to_string()
        }
    } else if draft.status == MinimapTurnStatus::Cancelled {
        "Run cancelled before final output.".to_string()
    } else if draft.status == MinimapTurnStatus::Error {
        "Turn ended with an error.".to_string()
    } else if draft.step_count > 0 {
        format!(
            "{} activity {}",
            draft.step_count,
            if draft.step_count == 1 { "step" } else { "steps" }
        )
    } else {
        "Awaiting response".to_string()
    };

    ThreadMinimapTurn {
        id: draft.id,
        anchor_id: draft.anchor_id,
        turn_number: draft.turn_number,
        title: draft.title,
        snippet,
        timestamp: draft.timestamp,
        step_count: draft.step_count,
        has_reasoning: draft.has_reasoning,
        status: draft.status,
    }
}

fn accumulate_trace_block(
    items: &[ClassifiedTraceStep],
    is_live: bool,
    step_count: &mut usize,
    has_reasoning: &mut bool,
    status: &mut MinimapTurnStatus,
) {
    *step_count += items.len();
    if items.iter().any(|s| s.is_reasoning || s.icon == TraceStepIcon::Reasoning) {
        *has_reasoning = true;
    }
    if is_live {
        *status = MinimapTurnStatus::Live;
    }
}

/// Extracts navigable conversation turns from grouped timeline blocks so the
/// left-edge minimap rail mirrors every prompt/response turn 1:1.
pub fn extract_thread_minimap_turns(blocks: &[ThreadTimelineBlock]) -> Vec<ThreadMinimapTurn> {
    if blocks.is_empty() {
        return Vec::new();
    }

    let has_user_prompt = blocks.iter().any(|b| match b {
        ThreadTimelineBlock::Entry { entry, .. } => {
            matches!(entry.role, EntryRole::User | EntryRole::Command)
        }
        _ => false,
    });

    // When a thread has no user prompts yet, skip bare initial greeting cards
    // unless real activity, streaming output, diagnostics, or multi-message
    // history is present.
    if !has_user_prompt {
        let has_substantive_stream = blocks.iter().any(|b| match b {
            ThreadTimelineBlock::Trace { .. } => true,
            ThreadTimelineBlock::Entry { entry, .. } => {
                !entry.id.starts_with("init-") || entry.streaming
            }
        });
        if !has_substantive_stream {
            return Vec::new();
        }

        let first_anchor = blocks
            .iter()
            .find_map(|b| match b {
                ThreadTimelineBlock::Entry { entry, .. } => Some(entry.id.clone()),
                _ => None,
            })
            .unwrap_or_else(|| blocks[0].id().to_string());

        let mut step_count = 0usize;
        let mut has_reasoning = false;
        let mut status = MinimapTurnStatus::Done;
        let mut timestamp = String::new();
        let mut reply_parts: Vec<String> = Vec::new();

        for block in blocks {
            match block {
                ThreadTimelineBlock::Trace { items, is_live, .. } => {
                    accumulate_trace_block(items, *is_live, &mut step_count, &mut has_reasoning, &mut status);
                    if timestamp.is_empty() && !items.is_empty() {
                        timestamp = items.last().map(|s| s.timestamp.clone()).unwrap_or_default();
                    }
                }
                ThreadTimelineBlock::Entry { entry, .. } => {
                    if !entry.text.trim().is_empty() {
                        reply_parts.push(entry.text.clone());
                    }
                    if !entry.timestamp.is_empty() {
                        timestamp = entry.timestamp.clone();
                    }
                    status = resolve_status_from_entry(entry, status);
                }
            }
        }

        return vec![finalize_turn(TurnDraft {
            id: format!("turn-{first_anchor}"),
            anchor_id: first_anchor,
            turn_number: 1,
            title: "Active session".to_string(),
            reply_parts,
            timestamp,
            step_count,
            has_reasoning,
            status,
        })];
    }

    let mut turns: Vec<ThreadMinimapTurn> = Vec::new();
    let mut current: Option<TurnDraft> = None;

    for block in blocks {
        if let ThreadTimelineBlock::Entry { entry, .. } = block {
            if matches!(entry.role, EntryRole::User | EntryRole::Command) {
                if let Some(draft) = current.take() {
                    turns.push(finalize_turn(draft));
                }
                let next_num = turns.len() + 1;
                let cleaned_title = clean_markdown_snippet(&entry.text, 84);
                current = Some(TurnDraft {
                    id: format!("turn-{}", entry.id),
                    anchor_id: entry.id.clone(),
                    turn_number: next_num,
                    title: if cleaned_title.is_empty() {
                        format!("Turn {next_num}")
                    } else {
                        cleaned_title
                    },
                    reply_parts: Vec::new(),
                    timestamp: entry.timestamp.clone(),
                    step_count: 0,
                    has_reasoning: false,
                    status: MinimapTurnStatus::Done,
                });
                continue;
            }
        }

        let Some(draft) = current.as_mut() else { continue };
        match block {
            ThreadTimelineBlock::Trace { items, is_live, .. } => {
                accumulate_trace_block(
                    items,
                    *is_live,
                    &mut draft.step_count,
                    &mut draft.has_reasoning,
                    &mut draft.status,
                );
            }
            ThreadTimelineBlock::Entry { entry, .. } => {
                if !entry.text.trim().is_empty() {
                    draft.reply_parts.push(entry.text.clone());
                }
                if !entry.timestamp.is_empty() {
                    draft.timestamp = entry.timestamp.clone();
                }
                draft.status = resolve_status_from_entry(entry, draft.status);
            }
        }
    }

    if let Some(draft) = current {
        turns.push(finalize_turn(draft));
    }

    turns
}

/// Determines which turn tick should appear active given measured anchor offsets
/// and current scroll position inside the Thread scroll container.
pub fn determine_active_minimap_turn(
    anchors: &[MinimapAnchorRect],
    scroll_top: f32,
    viewport_height: f32,
    max_scroll_top: f32,
) -> Option<String> {
    if anchors.is_empty() {
        return None;
    }
    if anchors.len() == 1 {
        return Some(anchors[0].id.clone());
    }

    if max_scroll_top > 12.0 && scroll_top >= max_scroll_top - 24.0 {
        return Some(anchors[anchors.len() - 1].id.clone());
    }
    if scroll_top <= 8.0 {
        return Some(anchors[0].id.clone());
    }

    let probe_y = scroll_top + 140.0_f32.min(48.0_f32.max(viewport_height * 0.28));
    let mut matched = anchors[0].id.clone();
    for anchor in anchors {
        if anchor.top <= probe_y {
            matched = anchor.id.clone();
        } else {
            break;
        }
    }
    Some(matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: &str, role: EntryRole, text: &str, timestamp: &str) -> TerminalEntry {
        TerminalEntry::new(id, "general", role, text, timestamp)
    }

    // ---- Original draft tests (kept) ----

    #[test]
    fn groups_consecutive_activities_and_keeps_terminal_errors_visible() {
        let mut a1 = TerminalEntry::new("1", "ch", EntryRole::Activity, "terminal: ls", "12:00");
        a1.activity_kind = Some(ActivityKind::Tool);
        let mut a2 = TerminalEntry::new("2", "ch", EntryRole::Activity, "Reasoning: plan step", "12:00");
        a2.activity_kind = Some(ActivityKind::Reasoning);
        let mut err = TerminalEntry::new("3", "ch", EntryRole::Activity, "gateway_offline", "12:01");
        err.error_code = Some("gateway_offline".into());
        let reply = TerminalEntry::new("4", "ch", EntryRole::Hermes, "Done", "12:02");

        let blocks = group_thread_timeline(&[a1, a2, err, reply], false, true);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[0], ThreadTimelineBlock::Trace { items, .. } if items.len() == 2));
        assert!(matches!(&blocks[1], ThreadTimelineBlock::Entry { id, .. } if id == "3"));
        assert!(matches!(&blocks[2], ThreadTimelineBlock::Entry { id, .. } if id == "4"));
    }

    #[test]
    fn maintains_bottom_scroll_pin_until_user_scrolls_up() {
        let mut s = ScrollAnchorState {
            scroll_y: 400.0,
            content_h: 800.0,
            viewport_h: 400.0,
            pinned_to_bottom: true,
        };
        s.content_h = 960.0;
        assert_eq!(s.on_content_grow(), 560.0);

        s.on_user_scroll(200.0);
        assert!(!s.pinned_to_bottom);
        s.content_h = 1100.0;
        assert_eq!(s.on_content_grow(), 200.0);
    }

    // ---- Mirrored threadTrace.test.ts ----

    #[test]
    fn ts_classify_trace_step_categorizes_spawn_tool_link_status() {
        let spawn = classify_trace_step(&make_entry("1", EntryRole::Activity, "Agent spawned: researcher", "10:47"));
        assert_eq!(spawn.icon, TraceStepIcon::Spawn);
        assert_eq!(spawn.label, "Agent spawned");
        assert_eq!(spawn.detail, "researcher");

        let link = classify_trace_step(&make_entry("2", EntryRole::Activity, "MCP link verified", "10:47"));
        assert_eq!(link.icon, TraceStepIcon::Link);

        let tool = classify_trace_step(&make_entry("3", EntryRole::Activity, "Tool run: brief summary", "10:47"));
        assert_eq!(tool.icon, TraceStepIcon::Tool);
        assert_eq!(tool.label, "Tool run");
        assert_eq!(tool.detail, "brief summary");

        let tool_done = classify_trace_step(&make_entry("4", EntryRole::Activity, "tool_describe completed", "10:47"));
        assert_eq!(tool_done.icon, TraceStepIcon::Tool);
        assert_eq!(tool_done.label, "tool_describe");
        assert_eq!(tool_done.detail, "completed");
    }

    #[test]
    fn ts_group_collapses_adjacent_activities_into_single_trace() {
        let entries = vec![
            make_entry("u1", EntryRole::User, "Check repo status", "10:47"),
            make_entry("a1", EntryRole::Activity, "Agent spawned: inspector", "10:47"),
            make_entry("a2", EntryRole::Activity, "Tool run: git status", "10:47"),
            make_entry("a3", EntryRole::Activity, "Link x2: tabs", "10:47"),
            make_entry("h1", EntryRole::Hermes, "Everything is clean.", "10:47"),
        ];
        let blocks = group_thread_timeline(&entries, false, true);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[0], ThreadTimelineBlock::Entry { .. }));
        match &blocks[1] {
            ThreadTimelineBlock::Trace { items, is_live, .. } => {
                assert_eq!(items.len(), 3);
                assert!(!is_live);
            }
            _ => panic!("expected trace"),
        }
        assert!(matches!(&blocks[2], ThreadTimelineBlock::Entry { .. }));
    }

    #[test]
    fn ts_live_thinking_attaches_to_trailing_trace_or_creates_one() {
        let without = group_thread_timeline(&[make_entry("u1", EntryRole::User, "Run tests", "10:47")], true, true);
        assert_eq!(without.len(), 2);
        match &without[1] {
            ThreadTimelineBlock::Trace { items, is_live, .. } => {
                assert!(is_live);
                assert_eq!(items.len(), 0);
            }
            _ => panic!("expected live trace"),
        }

        let with = group_thread_timeline(
            &[
                make_entry("u1", EntryRole::User, "Run tests", "10:47"),
                make_entry("a1", EntryRole::Activity, "Tool run: npm test", "10:47"),
            ],
            true,
            true,
        );
        assert_eq!(with.len(), 2);
        match &with[1] {
            ThreadTimelineBlock::Trace { items, is_live, .. } => {
                assert!(is_live);
                assert_eq!(items.len(), 1);
            }
            _ => panic!("expected live trace"),
        }
    }

    #[test]
    fn ts_dedupes_tool_pairs_and_respects_include_live_placeholder_false() {
        let hidden_live = group_thread_timeline(&[make_entry("u1", EntryRole::User, "Audit repo", "10:47")], true, false);
        assert_eq!(hidden_live.len(), 1);
        assert!(matches!(&hidden_live[0], ThreadTimelineBlock::Entry { .. }));

        let mut a1 = make_entry("a1", EntryRole::Activity, "Running read_file", "10:47");
        a1.activity_status = Some("running".into());
        let mut a2 = make_entry("a2", EntryRole::Activity, "read_file: ci-workflows.test.ts", "10:47");
        a2.activity_status = Some("done".into());
        let mut a3 = make_entry("a3", EntryRole::Activity, "Running terminal", "10:47");
        a3.activity_status = Some("running".into());
        let deduped = group_thread_timeline(
            &[make_entry("u1", EntryRole::User, "Inspect CI", "10:47"), a1, a2, a3],
            false,
            true,
        );
        assert_eq!(deduped.len(), 2);
        match &deduped[1] {
            ThreadTimelineBlock::Trace { items, .. } => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].label, "read_file");
            }
            _ => panic!("expected trace"),
        }
    }

    #[test]
    fn ts_preserves_multiline_reasoning_and_commentary() {
        let mut reasoning = make_entry("r1", EntryRole::Activity, "Reasoning: Step 1\nStep 2", "10:47");
        reasoning.activity_kind = Some(ActivityKind::Reasoning);
        reasoning.reasoning_detail = Some("Step 1\nStep 2".into());
        let step = classify_trace_step(&reasoning);
        assert_eq!(step.icon, TraceStepIcon::Reasoning);
        assert!(step.is_reasoning);
        assert_eq!(step.label, "Reasoning");
        assert_eq!(step.detail, "Step 1\nStep 2");

        let mut commentary = make_entry("c1", EntryRole::Activity, "Checking git diff first", "10:47");
        commentary.activity_kind = Some(ActivityKind::Commentary);
        let step = classify_trace_step(&commentary);
        assert_eq!(step.icon, TraceStepIcon::Commentary);
        assert_eq!(step.label, "Commentary");
        assert_eq!(step.detail, "Checking git diff first");
    }

    #[test]
    fn ts_terminal_diagnostic_failures_render_as_standalone_entries() {
        let mut err = make_entry("e1", EntryRole::Activity, "Hermes Gateway offline at http://127.0.0.1:8642", "10:47");
        err.activity_kind = Some(ActivityKind::Status);
        err.activity_status = Some("error".into());
        err.error_code = Some("gateway_offline".into());
        err.terminal_outcome = Some(TerminalOutcome::Failed);
        let blocks = group_thread_timeline(
            &[make_entry("u1", EntryRole::User, "Check build status", "10:47"), err],
            false,
            false,
        );
        assert_eq!(blocks.len(), 2);
        match &blocks[1] {
            ThreadTimelineBlock::Entry { entry, .. } => {
                assert_eq!(entry.error_code.as_deref(), Some("gateway_offline"));
            }
            _ => panic!("expected entry"),
        }
    }

    #[test]
    fn ts_keeps_activity_runs_chronological_around_replies() {
        let mut a1 = make_entry("a1", EntryRole::Activity, "terminal completed", "08:23");
        a1.activity_kind = Some(ActivityKind::Tool);
        let mut a2 = make_entry("a2", EntryRole::Activity, "terminal completed", "08:24");
        a2.activity_kind = Some(ActivityKind::Tool);
        let chronological = group_thread_timeline(
            &[
                make_entry("u1", EntryRole::User, "what should we work on", "08:21"),
                make_entry("h1", EntryRole::Hermes, "Scanning ledgers...", "08:22"),
                a1,
                a2,
            ],
            false,
            true,
        );
        assert_eq!(chronological.len(), 3);
        assert!(matches!(&chronological[0], ThreadTimelineBlock::Entry { .. }));
        match &chronological[1] {
            ThreadTimelineBlock::Entry { entry, .. } => assert_eq!(entry.id, "h1"),
            _ => panic!("expected reply entry"),
        }
        match &chronological[2] {
            ThreadTimelineBlock::Trace { items, is_live, .. } => {
                assert_eq!(items.len(), 2);
                assert!(!is_live);
            }
            _ => panic!("expected trace below reply"),
        }

        // No dangling live placeholder below streamed text without running tools.
        let active_streaming = group_thread_timeline(
            &[
                make_entry("u2", EntryRole::User, "simplify this list", "08:25"),
                make_entry("h2", EntryRole::Hermes, "Here is the simplified list:", "08:25"),
            ],
            true,
            true,
        );
        assert_eq!(active_streaming.len(), 2);
        assert!(active_streaming.iter().all(|b| matches!(b, ThreadTimelineBlock::Entry { .. })));
    }

    #[test]
    fn ts_post_reply_activity_renders_as_new_live_trace_below_text() {
        let mut a1 = make_entry("a1", EntryRole::Activity, "search_files completed", "09:01");
        a1.activity_kind = Some(ActivityKind::Tool);
        let mut a2 = make_entry("a2", EntryRole::Activity, "Running patch", "09:03");
        a2.activity_kind = Some(ActivityKind::Tool);
        a2.activity_status = Some("running".into());
        let mut a3 = make_entry("a3", EntryRole::Activity, "patch completed", "09:04");
        a3.activity_kind = Some(ActivityKind::Tool);

        let blocks = group_thread_timeline(
            &[
                make_entry("u1", EntryRole::User, "fix the failing test", "09:00"),
                a1,
                make_entry("h1", EntryRole::Hermes, "Found it in threadTrace.ts. Patching now.", "09:02"),
                a2,
                a3,
            ],
            true,
            true,
        );

        // user entry, pre-text trace, streamed reply, NEW live trace below the reply
        assert_eq!(blocks.len(), 4);
        assert!(matches!(&blocks[0], ThreadTimelineBlock::Entry { .. }));
        match &blocks[1] {
            ThreadTimelineBlock::Trace { items, is_live, .. } => {
                assert_eq!(items.len(), 1);
                assert!(!is_live);
            }
            _ => panic!("expected pre-text trace"),
        }
        match &blocks[2] {
            ThreadTimelineBlock::Entry { entry, .. } => assert_eq!(entry.id, "h1"),
            _ => panic!("expected reply"),
        }
        match &blocks[3] {
            ThreadTimelineBlock::Trace { items, is_live, id, .. } => {
                // "Running patch" upgraded in place by dedupe when "patch completed" lands.
                assert_eq!(items.len(), 1);
                assert!(is_live);
                assert_eq!(id, "trace-a3");
            }
            _ => panic!("expected new live trace below reply"),
        }
    }

    #[test]
    fn ts_coalesce_merges_identical_steps_and_preserves_reasoning() {
        let mut mk = |id: &str, text: &str, ts: &str| {
            let mut e = make_entry(id, EntryRole::Activity, text, ts);
            e.activity_kind = Some(ActivityKind::Tool);
            e
        };
        let mut reasoning = make_entry("4", EntryRole::Activity, "Reasoning: Checking logs", "08:25");
        reasoning.activity_kind = Some(ActivityKind::Reasoning);
        reasoning.reasoning_detail = Some("Checking logs".into());
        let steps: Vec<ClassifiedTraceStep> = [
            mk("1", "terminal completed", "08:23"),
            mk("2", "terminal completed", "08:24"),
            mk("3", "terminal completed", "08:25"),
            reasoning,
            mk("5", "terminal completed", "08:26"),
        ]
        .iter()
        .map(classify_trace_step)
        .collect();

        let coalesced = coalesce_consecutive_steps(&steps);
        assert_eq!(coalesced.len(), 3);
        assert_eq!(coalesced[0].repeat_count, 3);
        assert_eq!(coalesced[0].timestamp, "08:25");
        assert_eq!(coalesced[1].icon, TraceStepIcon::Reasoning);
        assert_eq!(coalesced[1].repeat_count, 1);
        assert_eq!(coalesced[2].repeat_count, 1);
    }

    // ---- Mirrored threadScroll.test.ts ----

    #[test]
    fn ts_is_near_bottom_detects_bottom_viewport() {
        assert!(is_near_bottom(920.0, 1400.0, 400.0, Some(96.0)));
        assert!(!is_near_bottom(500.0, 1400.0, 400.0, Some(96.0)));
    }

    fn pin_input(prev: f32, next: f32, height: f32, client: f32, new_visit: bool, programmatic: bool, prev_pinned: bool) -> ScrollPinInput {
        ScrollPinInput {
            prev_scroll_top: prev,
            next_scroll_top: next,
            scroll_height: height,
            client_height: client,
            is_new_visit: new_visit,
            is_programmatic: programmatic,
            prev_pinned,
            has_user_scroll_intent: None,
            threshold_px: None,
        }
    }

    #[test]
    fn ts_scroll_pin_new_visit_always_pins() {
        let result = compute_thread_scroll_pin(&pin_input(120.0, 120.0, 2400.0, 500.0, true, false, false));
        assert_eq!(result, ScrollPinResult { pinned: true, away: false });
    }

    #[test]
    fn ts_scroll_pin_stays_pinned_when_content_grows_without_upward_scroll() {
        let result = compute_thread_scroll_pin(&pin_input(900.0, 900.0, 1850.0, 500.0, false, false, true));
        assert_eq!(result, ScrollPinResult { pinned: true, away: false });
    }

    #[test]
    fn ts_scroll_pin_unpins_only_on_intentional_upward_scroll() {
        let result = compute_thread_scroll_pin(&pin_input(900.0, 520.0, 1850.0, 500.0, false, false, true));
        assert_eq!(result, ScrollPinResult { pinned: false, away: true });
    }

    #[test]
    fn ts_scroll_pin_repins_near_bottom() {
        let result = compute_thread_scroll_pin(&pin_input(520.0, 1320.0, 1850.0, 500.0, false, false, false));
        assert_eq!(result, ScrollPinResult { pinned: true, away: false });
    }

    #[test]
    fn ts_scroll_pin_ignores_clamp_jitter_without_intent_but_unpins_on_far_jump() {
        let mut bounce = pin_input(1700.0, 1580.0, 2210.0, 520.0, false, false, true);
        bounce.has_user_scroll_intent = Some(false);
        assert_eq!(compute_thread_scroll_pin(&bounce), ScrollPinResult { pinned: true, away: false });

        let mut top_jump = pin_input(1700.0, 40.0, 2210.0, 520.0, false, false, true);
        top_jump.has_user_scroll_intent = Some(false);
        assert_eq!(compute_thread_scroll_pin(&top_jump), ScrollPinResult { pinned: false, away: true });
    }

    // ---- Mirrored threadMinimap.test.ts ----

    fn mm_entry(id: &str, role: EntryRole, text: &str, timestamp: &str) -> TerminalEntry {
        TerminalEntry::new(id, "general", role, text, timestamp)
    }

    #[test]
    fn ts_clean_markdown_snippet_strips_clutter_and_truncates_at_word_boundary() {
        let raw = "## Summary\nYes — **both** are safe. Use `git status` before pushing.\n- Step one\n- Step two";
        assert_eq!(
            clean_markdown_snippet(raw, 120),
            "Summary Yes — both are safe. Use git status before pushing. • Step one • Step two"
        );

        let long = "Alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november";
        let truncated = clean_markdown_snippet(long, 42);
        assert!(truncated.ends_with('…'));
        assert!(truncated.chars().count() <= 43);
    }

    #[test]
    fn ts_minimap_skips_bare_init_and_pairs_turns_with_traces_and_replies() {
        let bare_init = group_thread_timeline(
            &[mm_entry("init-1", EntryRole::Hermes, "Ready in #general.", "14:20")],
            false,
            true,
        );
        assert!(extract_thread_minimap_turns(&bare_init).is_empty());

        let mut a1 = mm_entry("a-1", EntryRole::Activity, "Reasoning step", "14:21");
        a1.activity_kind = Some(ActivityKind::Reasoning);
        a1.activity_status = Some("done".into());
        let mut a2 = mm_entry("a-2", EntryRole::Activity, "read_file: sitemap.xml", "14:21");
        a2.activity_status = Some("done".into());
        let mut err2 = mm_entry("err-2", EntryRole::Activity, "Gateway unreachable at 127.0.0.1:8642", "14:23");
        err2.activity_status = Some("error".into());
        err2.error_code = Some("gateway_offline".into());
        err2.terminal_outcome = Some(TerminalOutcome::Failed);

        let entries = vec![
            mm_entry("init-1", EntryRole::Hermes, "Welcome to Hermes.", "14:20"),
            mm_entry("u-1", EntryRole::User, "is this safe?", "14:21"),
            a1,
            a2,
            mm_entry("h-1", EntryRole::Hermes, "Yes — both are safe. Here's the simple version: #2 (sitemap date fix) is 100% safe.", "14:21"),
            mm_entry("u-2", EntryRole::User, "check gateway health", "14:23"),
            err2,
            mm_entry("u-3", EntryRole::User, "run live audit", "14:25"),
        ];
        let blocks = group_thread_timeline(&entries, true, true);
        let turns = extract_thread_minimap_turns(&blocks);
        assert_eq!(turns.len(), 3);

        assert_eq!(turns[0].anchor_id, "u-1");
        assert_eq!(turns[0].turn_number, 1);
        assert_eq!(turns[0].title, "is this safe?");
        assert!(turns[0].snippet.starts_with("Yes — both are safe"));
        assert_eq!(turns[0].step_count, 2);
        assert!(turns[0].has_reasoning);
        assert_eq!(turns[0].status, MinimapTurnStatus::Done);

        assert_eq!(turns[1].anchor_id, "u-2");
        assert_eq!(turns[1].status, MinimapTurnStatus::Error);
        assert!(turns[1].snippet.contains("Gateway unreachable"));

        assert_eq!(turns[2].anchor_id, "u-3");
        assert_eq!(turns[2].status, MinimapTurnStatus::Live);
        assert_eq!(turns[2].snippet, "Thinking…");
    }

    #[test]
    fn ts_minimap_supports_sessions_opening_with_agent_output() {
        let mut a_lead = mm_entry("a-lead", EntryRole::Activity, "Running terminal", "19:19");
        a_lead.activity_status = Some("running".into());
        let entries = vec![
            mm_entry("h-lead", EntryRole::Hermes, "Survey done. Scrubbing repository provenance files now.", "19:17"),
            a_lead,
        ];
        let blocks = group_thread_timeline(&entries, true, true);
        let turns = extract_thread_minimap_turns(&blocks);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].anchor_id, "h-lead");
        assert_eq!(turns[0].status, MinimapTurnStatus::Live);
        assert_eq!(turns[0].step_count, 1);
    }

    #[test]
    fn ts_determine_active_minimap_turn_tracks_scroll() {
        let anchors = vec![
            MinimapAnchorRect { id: "u-1".into(), top: 0.0, height: 180.0 },
            MinimapAnchorRect { id: "u-2".into(), top: 220.0, height: 240.0 },
            MinimapAnchorRect { id: "u-3".into(), top: 500.0, height: 300.0 },
        ];
        assert_eq!(determine_active_minimap_turn(&[], 0.0, 500.0, 300.0), None);
        assert_eq!(determine_active_minimap_turn(&anchors, 0.0, 500.0, 300.0).as_deref(), Some("u-1"));
        assert_eq!(determine_active_minimap_turn(&anchors, 140.0, 500.0, 400.0).as_deref(), Some("u-2"));
        assert_eq!(determine_active_minimap_turn(&anchors, 390.0, 500.0, 400.0).as_deref(), Some("u-3"));
    }
}