//! P3-04 Thread view — web parity with `VoiceWorkspacePane` (thread mode).
//!
//! - Virtualized transcript via `El::virtual_rows` (rows keyed by timeline block
//!   id; only the mounted window builds, so 500+ entries never rebuild the full
//!   tree). Grouping comes from `core::thread::group_thread_timeline` and is
//!   cached in `AppState::thread_timeline_cache` for the row builders.
//! - Collapsible `ThreadTraceTree` rows: collapsed by default (web parity),
//!   live traces auto-expanded, coalesced steps with a 6-step visible window
//!   and "+N earlier groups" reveal, per-step copy.
//! - Hidden-by-default right-corner Task drawer: `split` at stage width
//!   >= 900px on desktop-lg, `overlay` below, via
//!   `core::layout::resolve_thread_tasks_layout_mode`.
//! - `ThreadTurnMinimap` rail from `core::thread::extract_thread_minimap_turns`;
//!   ticks jump with `Tx::scroll_into_view_in`.
//! - Smart bottom-anchor scroll pin via `core::thread::compute_thread_scroll_pin`
//!   feeding `ScrollSpec::stick_bottom`; "Jump to latest" while away.
//! - Text selection (`selection_group` + `selectable`) and copy via `Tx::copy`.
//!
//! CRITICAL invariant (mirrors core::thread): activity runs interleave
//! CHRONOLOGICALLY with streamed replies; post-text activity renders as a NEW
//! trace block below the text — never hoisted/merged into the earlier tree.

use std::borrow::Cow;
use std::cell::Ref;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::state::{AppState, UiCommand};
use crate::theme::VoxcellTheme;
use rune::{Cx, El, GrowthSpec, Live, ScrollPos, ScrollSpec, TextStyle, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Divider, TextInput};
use voxcell_core::layout::{ThreadTasksDockMode, resolve_thread_tasks_layout_mode};
use voxcell_core::tasks::{
    format_elapsed_duration, get_task_status_display, partition_channel_tasks,
    resolve_task_display_title, resolve_task_progress, resolve_task_step_breakdown,
    summarize_thread_tasks,
};
use voxcell_core::thread::{
    MinimapAnchorRect, ScrollAnchorState, ScrollPinInput, ThreadTimelineBlock, TraceStepIcon,
    coalesce_consecutive_steps, compute_thread_scroll_pin, determine_active_minimap_turn,
    extract_thread_minimap_turns, group_thread_timeline,
};
use voxcell_core::{EntryRole, TerminalEntry};

/// Estimated row height for unvisited virtual rows (cards measure themselves
/// once mounted; the estimate only places the scroll window before measuring).
const EST_ROW_H: f32 = 88.0;
/// Web `DEFAULT_VISIBLE_LIMIT` for coalesced trace steps.
const TRACE_VISIBLE_LIMIT: usize = 6;
/// Right drawer width (web docks 312–328px; transcript keeps >= ~560px).
const TASK_DRAWER_W: f32 = 312.0;
/// Window width at/above which the web app considers itself `desktop-lg`
/// (`matchMedia('(min-width: 1024px)')` in VoiceWorkspacePane).
const DESKTOP_LG_MIN_W: f32 = 1024.0;

// ---------------------------------------------------------------------------
// Cached timeline (shared by the view and the virtual row builders)
// ---------------------------------------------------------------------------

/// Grouped-timeline cache kept in `AppState` (append-only field) so the
/// `virtual_rows` key/build closures — which only receive `&AppState` — can
/// reuse one grouping per commit instead of regrouping per row.
#[derive(Default)]
pub struct ThreadTimelineCache {
    pub fingerprint: u64,
    pub blocks: Vec<ThreadTimelineBlock>,
}

/// Cheap content fingerprint of the active channel's transcript: enough to
/// catch appends, streaming growth, status upgrades and channel switches
/// without cloning entries on every row build.
fn timeline_fingerprint(state: &AppState) -> u64 {
    let mut h = DefaultHasher::new();
    state.active_channel_id.hash(&mut h);
    state.active_is_thinking().hash(&mut h);
    let mut count = 0usize;
    for e in state
        .entries
        .iter()
        .filter(|e| e.channel_id == state.active_channel_id)
    {
        count += 1;
        e.id.hash(&mut h);
        if e.streaming {
            e.text.hash(&mut h);
        } else {
            e.text.len().hash(&mut h);
        }
        e.streaming.hash(&mut h);
        e.activity_status.hash(&mut h);
        e.error_code.hash(&mut h);
        e.terminal_outcome.hash(&mut h);
    }
    count.hash(&mut h);
    h.finish()
}

/// Grouped timeline blocks for the active channel, cached by fingerprint.
pub fn thread_timeline(state: &AppState) -> Ref<'_, Vec<ThreadTimelineBlock>> {
    let fp = timeline_fingerprint(state);
    {
        let mut cache = state.thread_timeline_cache.borrow_mut();
        if cache.fingerprint != fp {
            let entries: Vec<TerminalEntry> = state
                .entries
                .iter()
                .filter(|e| e.channel_id == state.active_channel_id)
                .cloned()
                .collect();
            cache.blocks = group_thread_timeline(&entries, state.active_is_thinking(), true);
            cache.fingerprint = fp;
        }
    }
    Ref::map(state.thread_timeline_cache.borrow(), |c| &c.blocks)
}

// ---------------------------------------------------------------------------
// Trace collapse / scroll pin / minimap state helpers
// ---------------------------------------------------------------------------

/// Web parity: traces start collapsed; live traces and explicitly expanded
/// ones are open. `collapsed_traces` (existing field) wins over everything.
fn trace_is_collapsed(state: &AppState, id: &str, is_live: bool) -> bool {
    if state.collapsed_traces.contains(id) {
        return true;
    }
    !(is_live || state.thread_expanded_traces.contains(id))
}

fn toggle_trace(state: &mut AppState, id: &str, is_live: bool) {
    if trace_is_collapsed(state, id, is_live) {
        state.collapsed_traces.remove(id);
        state.thread_expanded_traces.insert(id.to_string());
    } else {
        state.thread_expanded_traces.remove(id);
        state.collapsed_traces.insert(id.to_string());
    }
}

/// Fold a measured scroll position (packed `[offset_y, max_y, viewport_h, _]`
/// from `El::on_scroll_offset`) into the bottom-anchor pin state using the
/// TS-parity decision from `core::thread`.
fn sync_scroll_pin(state: &mut AppState, p: [f32; 4]) -> bool {
    let (top, max_y, viewport_h) = (p[0], p[1], p[2]);
    if viewport_h <= 0.0 {
        return false;
    }
    let content_h = max_y + viewport_h;
    let new_visit =
        state.thread_scroll_channel != state.active_channel_id || state.thread_scroll.viewport_h <= 0.0;
    let res = compute_thread_scroll_pin(&ScrollPinInput {
        prev_scroll_top: state.thread_prev_scroll_top,
        next_scroll_top: top,
        scroll_height: content_h,
        client_height: viewport_h,
        is_new_visit: new_visit,
        is_programmatic: false,
        prev_pinned: state.thread_scroll.pinned_to_bottom,
        has_user_scroll_intent: None,
        threshold_px: None,
    });
    let next = ScrollAnchorState {
        scroll_y: top,
        content_h,
        viewport_h,
        pinned_to_bottom: res.pinned,
    };
    // The view reads `pinned_to_bottom` (stick_bottom + jump-to-latest); a pure
    // scroll-position fold is bookkeeping. Report whether the pin decision
    // changed so `on_layout_tx` can `Tx::live_only` the remaining frames.
    let pin_changed = new_visit || res.pinned != state.thread_scroll.pinned_to_bottom;
    if new_visit || state.thread_prev_scroll_top != top || state.thread_scroll != next {
        state.thread_scroll_channel = state.active_channel_id.clone();
        state.thread_prev_scroll_top = top;
        state.thread_scroll = next;
    }
    pin_changed
}

/// `on_visible_rows` bookkeeping: keep the minimap's active turn aligned with
/// the deepest turn anchor inside (or just above) the mounted window.
fn sync_minimap_active(state: &mut AppState, range: std::ops::Range<usize>) -> bool {
    // Compute the next anchor inside a scope so the timeline `Ref` (which
    // borrows `state`) drops before we mutate `state.thread_minimap_active`.
    let next: Option<Option<String>> = {
        let blocks = thread_timeline(state);
        let turns = extract_thread_minimap_turns(&blocks);
        if turns.is_empty() {
            Some(None)
        } else {
            let idx_of: HashMap<&str, usize> = blocks
                .iter()
                .enumerate()
                .map(|(i, b)| (b.id(), i))
                .collect();
            let mut visible_anchor: Option<String> = None;
            let mut preceding: Option<String> = None;
            for t in &turns {
                if let Some(&i) = idx_of.get(t.anchor_id.as_str()) {
                    if i < range.start {
                        preceding = Some(t.anchor_id.clone());
                    }
                    // First (topmost) visible anchor wins — the turn at the top
                    // of the viewport, matching the web probe position. Taking
                    // the deepest would overwrite an explicit minimap jump.
                    if range.contains(&i) && visible_anchor.is_none() {
                        visible_anchor = Some(t.anchor_id.clone());
                    }
                }
            }
            Some(visible_anchor.or(preceding))
        }
    };
    let Some(next) = next else { return false };
    if next != state.thread_minimap_active {
        state.thread_minimap_active = next;
    }
    // Bookkeeping only: the view derives the rendered minimap highlight from
    // layout (`determine_active_minimap_turn`), so a visible-range change does
    // not alter the committed view output. Returning false keeps a scroll from
    // re-running the view on every frame (rune `on_visible_rows` perf contract).
    false
}

// ---------------------------------------------------------------------------
// Row builders (run inside virtual_rows for the mounted window only)
// ---------------------------------------------------------------------------

fn build_timeline_row(state: &AppState, block: &ThreadTimelineBlock) -> El {
    let theme = VoxcellTheme::from_config(&state.ui_config);
    match block {
        ThreadTimelineBlock::Trace { id, items, is_live } => {
            build_trace_row(state, &theme, id, items, *is_live)
        }
        ThreadTimelineBlock::Entry { id, entry } => build_entry_row(state, &theme, id, entry),
    }
}

/// Collapsible ThreadTraceTree: header toggle + coalesced steps (last 6 shown
/// until "+N earlier groups" is pressed) + per-step copy.
fn build_trace_row(
    state: &AppState,
    theme: &VoxcellTheme,
    id: &str,
    items: &[voxcell_core::thread::ClassifiedTraceStep],
    is_live: bool,
) -> El {
    let k = &theme.kit;
    let collapsed = trace_is_collapsed(state, id, is_live);
    let coalesced = coalesce_consecutive_steps(items);
    let show_all = state.thread_trace_show_all.contains(id);
    let hidden_count = if !show_all && coalesced.len() > TRACE_VISIBLE_LIMIT {
        coalesced.len() - TRACE_VISIBLE_LIMIT
    } else {
        0
    };
    let visible: &[voxcell_core::thread::CoalescedTraceStep] = if hidden_count > 0 {
        &coalesced[coalesced.len() - TRACE_VISIBLE_LIMIT..]
    } else {
        &coalesced
    };

    let trace_id = id.to_string();
    let header = row()
        .key(format!("{id}-header"))
        .w_full()
        .gap(8.0)
        .items_center()
        .on_press(move |s: &mut AppState| toggle_trace(s, &trace_id, is_live))
        .child(
            el().w(7.0)
                .h(7.0)
                .radius(3.5)
                .fill(if is_live { theme.amber } else { theme.accent.alpha(0.7) }),
        )
        .child(
            text(if is_live { "Live activity" } else { "Activity" })
                .font(TextStyle::new(12.5, 600.0))
                .color(theme.text),
        )
        .child(
            text(format!("· {} steps", items.len()))
                .font(theme.mono_sm)
                .color(if is_live { theme.amber } else { theme.text_muted }),
        )
        .child(
            text(if collapsed { "▸" } else { "▾" })
                .font(theme.mono_sm)
                .color(theme.text_faint),
        );

    let mut card = col()
        .key(id.to_string())
        .w_full()
        .padding(10.0)
        .gap(6.0)
        .radius(8.0)
        // Web parity: toggling a trace hides/reveals its steps at once. Without
        // this the removed steps play an exit fade that outlives a headless
        // settle, and `rt.elements()` still lists the exiting step text.
        .instant_changes()
        .fill(theme.bg.alpha(0.68))
        .border(
            0.75,
            if is_live {
                theme.amber.alpha(0.35)
            } else {
                theme.border
            },
        )
        .child(header);

    if !collapsed {
        if hidden_count > 0 {
            let show_id = id.to_string();
            card = card.child(
                row().w_full().child(
                    Button::new(format!("+{hidden_count} earlier groups"))
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Mini)
                        .on_press(move |s: &mut AppState| {
                            s.thread_trace_show_all.insert(show_id.clone());
                        })
                        .build(k),
                ),
            );
        }
        for step in visible {
            let tag = match step.icon {
                TraceStepIcon::Reasoning => "THINK",
                TraceStepIcon::Tool => "TOOL ",
                TraceStepIcon::Spawn => "AGENT",
                TraceStepIcon::Commentary => "NOTE ",
                TraceStepIcon::Link => "LINK ",
                TraceStepIcon::Status => "STAT ",
            };
            let payload = if step.detail.is_empty() {
                step.label.clone()
            } else {
                format!("{}: {}", step.label, step.detail)
            };
            let mut step_row = row()
                .w_full()
                .gap(8.0)
                .items_center()
                .child(
                    text(tag)
                        .font(TextStyle::new(10.5, 700.0).mono())
                        .color(if step.is_reasoning {
                            theme.amber
                        } else {
                            theme.accent
                        }),
                )
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(
                            text(if step.repeat_count > 1 {
                                format!("{} ×{}", step.label, step.repeat_count)
                            } else {
                                step.label.clone()
                            })
                            .font(TextStyle::new(12.0, 600.0))
                            .color(theme.text)
                            .selectable(true),
                        )
                        .child(if step.detail.is_empty() {
                            el()
                        } else {
                            text(step.detail.clone())
                                .font(theme.mono_sm)
                                .color(theme.text_muted)
                                .selectable(true)
                        }),
                );
            if !step.timestamp.is_empty() {
                step_row = step_row.child(
                    // Uphill a11y: HH:MM timestamps were text_faint (~2.5:1);
                    // accent_soft is the brighter existing token (~10:1).
                    text(step.timestamp.clone())
                        .font(theme.mono_sm)
                        .color(theme.accent_soft),
                );
            }
            step_row = step_row.child(
                Button::new("Copy")
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Mini)
                    .on_press_tx(move |_s: &mut AppState, tx: &mut Tx| tx.copy(payload.clone()))
                    .build(k),
            );
            card = card.child(step_row);
        }
    }
    // Plain El, no `memo`: a virtual row's built El is flattened fresh on every
    // commit that mounts it, so collapse/expand/show-all always land. A memo
    // whose revision revisits earlier values (collapse returns to the boot
    // revision) is fragile here, and the card is small (header + <= 6 steps) —
    // rebuilding it with the mounted window is cheap.
    card
}

/// Chat bubble row: role header + copy + selectable body text.
fn build_entry_row(_state: &AppState, theme: &VoxcellTheme, id: &str, entry: &TerminalEntry) -> El {
    let k = &theme.kit;
    let is_user = entry.role == EntryRole::User;
    let is_err = entry.error_code.is_some();
    let role_label = match entry.role {
        EntryRole::User => "YOU",
        EntryRole::Hermes => "HERMES",
        EntryRole::Command => "CMD",
        EntryRole::System => "SYS",
        EntryRole::Activity => "EVENT",
    };
    let border = if is_err {
        theme.rose.alpha(0.55)
    } else if is_user {
        theme.accent.alpha(0.35)
    } else {
        theme.border
    };
    let fill = if is_err {
        theme.rose.alpha(0.10)
    } else if is_user {
        theme.accent.alpha(0.09)
    } else {
        theme.card_fill
    };
    let payload = entry.text.clone();

    col()
        .key(format!("msg-{id}"))
        .w_full()
        .padding(12.0)
        .gap(6.0)
        .radius(10.0)
        .fill(fill)
        .border(0.75, border)
        .child(
            row()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    row()
                        .gap(8.0)
                        .items_center()
                        .child(
                            text(role_label)
                                .font(TextStyle::new(10.5, 700.0).tracking(0.6).mono())
                                .color(if is_err {
                                    theme.rose
                                } else if is_user {
                                    theme.accent
                                } else {
                                    theme.text_muted
                                }),
                        )
                        .child(
                            text(if entry.streaming {
                                "streaming...".to_string()
                            } else {
                                entry.timestamp.clone()
                            })
                            .font(theme.mono_sm)
                            // Uphill a11y: brighter existing token (accent_soft)
                            // instead of text_faint so HH:MM clears WCAG 3.0.
                            .color(theme.accent_soft),
                        ),
                )
                .child(
                    Button::new("Copy")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Mini)
                        .on_press_tx(move |_s: &mut AppState, tx: &mut Tx| tx.copy(payload.clone()))
                        .build(k),
                ),
        )
        .child(
            text(entry.text.clone())
                .font(TextStyle::new(13.5, 400.0).lh(20.0))
                .color(theme.text)
                .selectable(true),
        )
}

// ---------------------------------------------------------------------------
// Minimap rail + Task drawer
// ---------------------------------------------------------------------------

fn build_minimap_rail(
    theme: &VoxcellTheme,
    turns: &[voxcell_core::thread::ThreadMinimapTurn],
    active_anchor: &str,
) -> El {
    let mut ticks = col().key("thread-minimap-ticks").gap(4.0).items_center().padding(4.0);
    for t in turns {
        let is_active = t.anchor_id == active_anchor;
        let color = match t.status {
            voxcell_core::thread::MinimapTurnStatus::Live => theme.amber,
            voxcell_core::thread::MinimapTurnStatus::Error => theme.rose,
            voxcell_core::thread::MinimapTurnStatus::Budget => theme.amber.alpha(0.65),
            voxcell_core::thread::MinimapTurnStatus::Cancelled => theme.text_faint,
            voxcell_core::thread::MinimapTurnStatus::Done => {
                if is_active {
                    theme.accent
                } else {
                    theme.accent.alpha(0.45)
                }
            }
        };
        let anchor = t.anchor_id.clone();
        // Uphill a11y: every tick is a pressable jump target, so it must carry an
        // accessible name (rune El::label -> AccessKit name, the same API the
        // committed sidebar/settings controls use). The name is built from the
        // core::thread minimap data: turn number + status role (+ turn title).
        let status_word = match t.status {
            voxcell_core::thread::MinimapTurnStatus::Live => "live",
            voxcell_core::thread::MinimapTurnStatus::Error => "error",
            voxcell_core::thread::MinimapTurnStatus::Budget => "budget",
            voxcell_core::thread::MinimapTurnStatus::Cancelled => "cancelled",
            voxcell_core::thread::MinimapTurnStatus::Done => "done",
        };
        let mut tick_name = format!("Turn {} \u{b7} {}", t.turn_number, status_word);
        if !t.title.is_empty() {
            tick_name.push_str(" \u{2014} ");
            tick_name.push_str(&t.title);
        }
        ticks = ticks.child(
            el().key(format!("minimap-tick-{}", t.anchor_id))
                .label(tick_name)
                .w(if is_active { 18.0 } else { 12.0 })
                .h(6.0)
                .radius(3.0)
                .fill(color)
                .role("button")
                .on_press_tx(move |s: &mut AppState, tx: &mut Tx| {
                    s.thread_minimap_active = Some(anchor.clone());
                    tx.scroll_into_view_in("thread-scroll", anchor.clone());
                }),
        );
    }
    col()
        .key("thread-turn-minimap")
        .w(24.0)
        .h_full()
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .child(ticks)
}

fn build_tasks_drawer(state: &AppState, theme: &VoxcellTheme, split: bool) -> El {
    let k = &theme.kit;
    let ch_id = state.active_channel_id.clone();
    let partition = partition_channel_tasks(&state.tasks, &ch_id);
    let summary = summarize_thread_tasks(&partition, state.active_is_thinking());

    let mut drawer = col()
        .key("thread-tasks-drawer")
        .w(TASK_DRAWER_W)
        .padding(12.0)
        .gap(8.0)
        .radius(10.0)
        .fill(theme.card_fill)
        .border(1.0, theme.border_strong);
    if split {
        drawer = drawer.h_full().min_h(0.0);
    } else {
        drawer = drawer.h(480.0);
    }

    drawer = drawer.child(
        row()
            .w_full()
            .justify_between()
            .items_center()
            .child(
                text(format!("Thread Tasks · {}", summary.total_count))
                    .font(TextStyle::new(13.0, 700.0))
                    .color(theme.text),
            )
            .child(
                Button::new("Close")
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Mini)
                    .on_press(|s: &mut AppState| s.thread_tasks_open = false)
                    .build(k),
            ),
    );

    if !summary.header_badge.is_empty() {
        drawer = drawer.child(
            text(summary.header_badge.clone())
                .font(theme.mono_sm)
                .color(if state.active_is_thinking() {
                    theme.amber
                } else {
                    theme.text_muted
                }),
        );
    }

    if partition.all.is_empty() {
        drawer = drawer.child(
            text("No tasks in this thread yet.")
                .font(theme.mono_sm)
                .color(theme.text_faint),
        );
    }

    let mut list = col().w_full().gap(8.0);
    for task in partition.all.iter().take(12) {
        let disp = get_task_status_display(task);
        let prog = resolve_task_progress(task);
        let breakdown = resolve_task_step_breakdown(task);
        let elapsed = format_elapsed_duration(task.created_at_ms, state.now_ms, true);
        let status_color = if task.error_code.is_some() {
            theme.rose
        } else {
            match task.status {
                voxcell_core::TaskLifecycleStatus::Running => theme.amber,
                _ => theme.accent,
            }
        };
        let mut card = col()
            .key(format!("thread-task-{}", task.id))
            .w_full()
            .padding(10.0)
            .gap(4.0)
            .radius(8.0)
            .fill(theme.bg.alpha(0.62))
            .border(0.75, theme.border)
            .child(
                row()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        text(format!("[{}]", disp.label))
                            .font(TextStyle::new(10.5, 700.0).mono())
                            .color(status_color),
                    )
                    .child(
                        text(format!("{} · {}", prog.ratio_label, elapsed))
                            .font(theme.mono_sm)
                            .color(theme.text_muted),
                    ),
            )
            .child(
                text(resolve_task_display_title(task))
                    .font(TextStyle::new(12.5, 600.0))
                    .color(theme.text)
                    .selectable(true),
            );
        for step in breakdown.steps.iter().take(4) {
            let (glyph, colr) = match step.state {
                voxcell_core::tasks::ResolvedStepState::Done => ("[x]", theme.accent),
                voxcell_core::tasks::ResolvedStepState::Active => ("[>]", theme.amber),
                voxcell_core::tasks::ResolvedStepState::Pending => ("[ ]", theme.text_faint),
            };
            card = card.child(
                text(format!("  {glyph} {}", step.text))
                    .font(theme.mono_sm)
                    .color(colr),
            );
        }
        list = list.child(card);
    }
    if split {
        drawer = drawer.child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .scroll(ScrollSpec { stick_bottom: false })
                .child(list),
        );
    } else {
        drawer = drawer.child(list);
    }
    drawer
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn render_thread(state: &AppState, theme: &VoxcellTheme, cx: &mut Cx<AppState>) -> El {
    let k = &theme.kit;
    let ch = state.active_channel().cloned();
    let ch_id = state.active_channel_id.clone();
    let is_thinking = state.active_is_thinking();

    let partition = partition_channel_tasks(&state.tasks, &ch_id);
    let summary = summarize_thread_tasks(&partition, is_thinking);

    // 1. Thread Header + Active Task Banner + Tasks drawer toggle
    let mut header = col()
        .key("thread-header")
        .w_full()
        .gap(8.0)
        .padding(12.0)
        .radius(10.0)
        .fill(theme.card_fill)
        .border(1.0, theme.border)
        .child(
            row()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    col()
                        .gap(2.0)
                        .child(
                            text(ch.as_ref().map(|c| c.name.as_str()).unwrap_or("Thread"))
                                .font(theme.heading)
                                .color(theme.text),
                        )
                        .child(
                            text(format!(
                                "session={} · model={}",
                                state.active_session_id(),
                                state.gateway.model
                            ))
                            .font(theme.mono_sm)
                            .color(theme.text_muted),
                        ),
                )
                .child(
                    row()
                        .gap(6.0)
                        .items_center()
                        .child(if !summary.header_badge.is_empty() {
                            el().px(8.0)
                                .py(3.0)
                                .radius(4.0)
                                .fill(if is_thinking {
                                    theme.amber.alpha(0.15)
                                } else {
                                    theme.accent.alpha(0.14)
                                })
                                .border(
                                    0.5,
                                    if is_thinking {
                                        theme.amber.alpha(0.45)
                                    } else {
                                        theme.accent.alpha(0.4)
                                    },
                                )
                                .child(
                                    text(summary.header_badge.clone())
                                        .font(theme.mono_sm)
                                        .color(if is_thinking { theme.amber } else { theme.accent }),
                                )
                        } else {
                            el()
                        })
                        .child(if is_thinking {
                            Button::new("Stop Run")
                                .variant(ButtonVariant::Neutral)
                                .control_size(ControlSize::Small)
                                .on_press(move |s: &mut AppState| {
                                    let active_id = s.active_channel_id.clone();
                                    if let Some(run_id) = s.active_run_ids.get(&active_id).cloned() {
                                        s.pending_commands.push(UiCommand::StopRun {
                                            run_id,
                                            channel_id: active_id.clone(),
                                        });
                                    }
                                    s.thinking_by_channel.insert(active_id, false);
                                })
                                .build(k)
                        } else {
                            Button::new("Reset")
                                .variant(ButtonVariant::Ghost)
                                .control_size(ControlSize::Small)
                                .on_press(|s: &mut AppState| {
                                    let id = s.active_channel_id.clone();
                                    let sid = s.active_session_id();
                                    s.entries.retain(|e| e.channel_id != id);
                                    s.pending_commands.push(UiCommand::ResetSession {
                                        channel_id: id,
                                        session_id: sid,
                                    });
                                })
                                .build(k)
                        })
                        .child(
                            el().key("thread-tasks-toggle").child(
                                Button::new(format!("Tasks · {}", partition.all.len()))
                                    .variant(if state.thread_tasks_open {
                                        ButtonVariant::Submit
                                    } else {
                                        ButtonVariant::Neutral
                                    })
                                    .control_size(ControlSize::Small)
                                    .on_press(|s: &mut AppState| {
                                        s.thread_tasks_open = !s.thread_tasks_open;
                                    })
                                    .build(k),
                            ),
                        ),
                ),
        );

    if let Some(lead) = partition.running.first().or_else(|| partition.all.first()) {
        let disp = get_task_status_display(lead);
        let prog = resolve_task_progress(lead);
        let breakdown = resolve_task_step_breakdown(lead);
        let elapsed = format_elapsed_duration(lead.created_at_ms, state.now_ms, true);

        let mut banner = col()
            .key("thread-task-banner")
            .w_full()
            .gap(6.0)
            .padding(10.0)
            .radius(8.0)
            .fill(theme.bg.alpha(0.62))
            .border(0.75, theme.amber.alpha(0.28))
            .child(
                row()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        text(format!(
                            "[{}] {}",
                            disp.label,
                            resolve_task_display_title(lead)
                        ))
                        .font(TextStyle::new(12.5, 600.0))
                        .color(theme.text),
                    )
                    .child(
                        text(format!("{} · {}% · {}", prog.ratio_label, prog.pct, elapsed))
                            .font(theme.mono_sm)
                            .color(theme.amber),
                    ),
            );

        for step in breakdown.steps.iter().take(4) {
            let (glyph, colr) = match step.state {
                voxcell_core::tasks::ResolvedStepState::Done => ("[x]", theme.accent),
                voxcell_core::tasks::ResolvedStepState::Active => ("[>]", theme.amber),
                voxcell_core::tasks::ResolvedStepState::Pending => ("[ ]", theme.text_faint),
            };
            banner = banner.child(
                text(format!("  {glyph} {}", step.text))
                    .font(theme.mono_sm)
                    .color(colr),
            );
        }
        header = header.child(banner);
    }

    // 2. Virtualized transcript (grouped timeline blocks as virtual rows)
    let blocks = thread_timeline(state);
    let count = blocks.len();
    // `virtual_rows_version` must change whenever a row's BUILT content changes,
    // not only when the timeline regroupes: rune's memo reuse compares this
    // version (runtime/commit/memo_reuse.rs `same_el_cold`), and a stale version
    // keeps the previously built row. Trace collapse/expand/show-all flip a row's
    // content without touching the timeline fingerprint, so fold them in here.
    let version = {
        let mut h = DefaultHasher::new();
        state.thread_timeline_cache.borrow().fingerprint.hash(&mut h);
        for set in [
            &state.collapsed_traces,
            &state.thread_expanded_traces,
            &state.thread_trace_show_all,
        ] {
            let mut ids: Vec<&String> = set.iter().collect();
            ids.sort();
            ids.hash(&mut h);
        }
        h.finish()
    };
    let last_row_key = blocks.last().map(|b| b.id().to_string());

    let turns = extract_thread_minimap_turns(&blocks);
    let feed_top = cx.layout("thread-feed").map(|r| r[1]).unwrap_or(0.0);
    let anchors: Vec<MinimapAnchorRect> = turns
        .iter()
        .filter_map(|t| {
            cx.layout(&t.anchor_id).map(|r| MinimapAnchorRect {
                id: t.anchor_id.clone(),
                top: r[1] - feed_top,
                height: r[3],
            })
        })
        .collect();
    let active_anchor = determine_active_minimap_turn(
        &anchors,
        state.thread_scroll.scroll_y,
        state.thread_scroll.viewport_h,
        state.thread_scroll.max_scroll(),
    )
    .or_else(|| state.thread_minimap_active.clone())
    .or_else(|| turns.last().map(|t| t.anchor_id.clone()))
    .unwrap_or_default();

    // Scroll position feed: on_scroll_offset publishes into a Live (it cannot
    // touch state); the feed's on_layout folds it into the pin state.
    let scroll_live: Live<[f32; 4]> = Live::new([0.0; 4]);
    let live_write = scroll_live.clone();
    let live_read = scroll_live.clone();

    let feed = col()
        .key("thread-feed")
        .w_full()
        .gap(10.0)
        .padding(4.0)
        .growth(GrowthSpec::default())
        .selection_group(true)
        .virtual_rows(
            count,
            EST_ROW_H,
            |s: &AppState, i| Cow::Owned(thread_timeline(s)[i].id().to_string()),
            |s: &AppState, i| {
                let blocks = thread_timeline(s);
                build_timeline_row(s, &blocks[i])
            },
        )
        .virtual_rows_version(version)
        .on_visible_rows(|s: &mut AppState, range| sync_minimap_active(s, range))
        .on_layout_tx(move |s: &mut AppState, _rect, tx: &mut Tx| {
            // A pure scroll-position fold is bookkeeping; only a changed pin
            // decision (stick_bottom / jump-to-latest) needs a view re-commit.
            // Without `live_only` every fold re-runs the view mid-scroll, which
            // fights `stick_bottom`'s growth-follow (rune's on_layout perf
            // contract: bookkeeping handlers must say `Tx::live_only`).
            if !sync_scroll_pin(s, live_read.get()) {
                tx.live_only();
            }
        });

    let mut viewport = col()
        .key("thread-scroll")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .scroll(ScrollSpec {
            stick_bottom: state.thread_scroll.pinned_to_bottom,
        })
        .on_scroll_offset(move |pos: ScrollPos| {
            live_write.set([pos.offset[1], pos.max[1], pos.viewport[1], 0.0]);
        });

    if count == 0 {
        viewport = viewport.child(
            col()
                .key("thread-empty")
                .w_full()
                .padding(24.0)
                .radius(10.0)
                .fill(theme.card_fill)
                .border(0.75, theme.border)
                .items_center()
                .gap(6.0)
                .child(
                    text("Thread ready")
                        .font(TextStyle::new(14.0, 600.0))
                        .color(theme.text),
                )
                .child(
                    text("Send a prompt below to stream reasoning traces, tool executions, and voice replies.")
                        .font(theme.mono_sm)
                        .color(theme.text_muted),
                ),
        );
    }
    let viewport = viewport.child(feed);

    let mut transcript_row = row()
        .key("thread-transcript-row")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .gap(6.0);
    if !turns.is_empty() {
        transcript_row = transcript_row.child(build_minimap_rail(theme, &turns, &active_anchor));
    }
    transcript_row = transcript_row.child(viewport);

    // 3. Thread Composer
    let composer = row()
        .key("thread-composer")
        .w_full()
        .padding(8.0)
        .gap(8.0)
        .radius(10.0)
        .fill(theme.card_fill)
        .border(1.0, theme.border_strong)
        .items_center()
        .child(
            el().grow(1.0).min_w(0.0).child(
                TextInput::new(&state.composer_edit)
                    .placeholder(if is_thinking {
                        "Type to steer running turn or queue prompt..."
                    } else {
                        "Reply in thread..."
                    })
                    .label("Thread input")
                    .plain()
                    .width(None)
                    .on_edit(|s: &mut AppState| &mut s.composer_edit)
                    .on_submit(|s: &mut AppState, _tx| s.submit_composer_prompt())
                    .build(k),
            ),
        )
        .child(
            Button::new(if is_thinking { "Steer" } else { "Send" })
                .variant(ButtonVariant::Submit)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| {
                    let ch = s.active_channel_id.clone();
                    if let Some(run_id) = s.active_run_ids.get(&ch).cloned() {
                        let txt = s.composer_edit.text.trim().to_string();
                        if !txt.is_empty() {
                            s.composer_edit.clear();
                            s.pending_commands.push(UiCommand::SteerRun {
                                run_id,
                                text: txt,
                            });
                        }
                    } else {
                        s.submit_composer_prompt();
                    }
                })
                .build(k),
        );

    let main_col = col()
        .key("thread-main")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .h_full()
        .gap(10.0)
        .child(transcript_row)
        .child(Divider::horizontal().build(k))
        .child(composer);

    // 4. Task drawer docking: split >= 900px stage on desktop-lg, else overlay.
    let root_w = cx.layout("voxcell-root").map(|r| r[2]).unwrap_or(0.0);
    let stage_w = cx.layout_size("main-pane").map(|s| s[0]);
    let dock = resolve_thread_tasks_layout_mode(
        stage_w.map(|w| w as f64),
        root_w >= DESKTOP_LG_MIN_W,
    );

    let mut body = row()
        .key("thread-body")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .gap(10.0)
        .child(main_col);
    if state.thread_tasks_open && dock == ThreadTasksDockMode::Split {
        body = body.child(build_tasks_drawer(state, theme, true));
    }

    let mut root = col()
        .key("thread-view")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .h_full()
        .padding(14.0)
        .gap(10.0)
        .child(header)
        .child(body);

    // Overlay drawer floats in the right corner above the transcript.
    if state.thread_tasks_open && dock == ThreadTasksDockMode::Overlay {
        if let Some(pane_w) = stage_w {
            let x = (pane_w - TASK_DRAWER_W - 14.0).max(8.0);
            root = root.child(
                el().key("thread-tasks-overlay")
                    .absolute(x, 64.0)
                    .w(TASK_DRAWER_W)
                    .child(build_tasks_drawer(state, theme, false)),
            );
        }
    }

    // "Jump to latest" pill while the pin is away from the bottom.
    let away = !state.thread_scroll.pinned_to_bottom && state.thread_scroll.max_scroll() > 96.0;
    if away {
        if let (Some(view_rect), Some(scroll_rect), Some(last_key)) = (
            cx.layout("thread-view"),
            cx.layout("thread-scroll"),
            last_row_key,
        ) {
            let x = scroll_rect[0] - view_rect[0] + scroll_rect[2] - 150.0;
            let y = scroll_rect[1] - view_rect[1] + scroll_rect[3] - 40.0;
            root = root.child(
                el().key("thread-jump-latest")
                    .absolute(x.max(8.0), y.max(8.0))
                    .child(
                        Button::new("Jump to latest ↓")
                            .variant(ButtonVariant::Submit)
                            .control_size(ControlSize::Small)
                            .on_press_tx(move |s: &mut AppState, tx: &mut Tx| {
                                s.thread_scroll.pinned_to_bottom = true;
                                tx.scroll_into_view_in("thread-scroll", last_key.clone());
                            })
                            .build(k),
                    ),
            );
        }
    }

    root
}

// ---------------------------------------------------------------------------
// Headless tests (Runtime pattern from voxcell-app/src/main.rs)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};
    use voxcell_core::ActivityKind;

    fn render(s: &AppState, cx: &mut Cx<AppState>) -> El {
        crate::views::render_main_window(s, cx)
    }

    fn thread_state() -> AppState {
        let mut s = AppState::new();
        s.apply_scene_preset("thread");
        s
    }

    fn boot(s: AppState, size: (f32, f32)) -> Runtime<AppState> {
        let mut rt = Runtime::new(s, render, size, Env::new(true, 1.0));
        rt.set_wheel_glide(false);
        rt.frame(0.0);
        settle(&mut rt);
        rt
    }

    fn settle(rt: &mut Runtime<AppState>) {
        for _ in 0..10 {
            rt.frame(1.0 / 60.0);
        }
    }

    fn click(rt: &mut Runtime<AppState>, x: f32, y: f32) {
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.016);
        settle(rt);
    }

    fn click_key(rt: &mut Runtime<AppState>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing element {key}"));
        click(rt, x, y);
    }

    fn click_text(rt: &mut Runtime<AppState>, needle: &str) {
        let e = rt
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref().is_some_and(|t| t.contains(needle)))
            .unwrap_or_else(|| panic!("missing text {needle:?}"));
        click(
            rt,
            e.target[0] + e.target[2] * 0.5,
            e.target[1] + e.target[3] * 0.5,
        );
    }

    fn has_text(rt: &Runtime<AppState>, needle: &str) -> bool {
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref().is_some_and(|t| t.contains(needle)))
    }

    /// Append `pairs` user/hermes turns to the active channel (2 blocks each),
    /// numbered from `start` so repeated calls never duplicate entry ids.
    fn push_many_entries(s: &mut AppState, start: usize, pairs: usize) {
        let ch = s.active_channel_id.clone();
        for i in start..start + pairs {
            s.entries.push(TerminalEntry::new(
                format!("u{i:03}"),
                &ch,
                EntryRole::User,
                format!("prompt number {i:03}"),
                "10:00",
            ));
            s.entries.push(TerminalEntry::new(
                format!("h{i:03}"),
                &ch,
                EntryRole::Hermes,
                format!("reply number {i:03} — the quick brown fox jumps over the lazy dog"),
                "10:01",
            ));
        }
    }

    /// Keys of the mounted virtual rows (rows surface under their key_fn key).
    fn built_row_keys(rt: &Runtime<AppState>) -> Vec<String> {
        let ids: std::collections::HashSet<String> =
            thread_timeline(&rt.state).iter().map(|b| b.id().to_string()).collect();
        rt.elements()
            .iter()
            .filter_map(|e| e.key.clone())
            .filter(|k| ids.contains(k))
            .collect()
    }

    fn viewport_rect(rt: &Runtime<AppState>) -> [f32; 4] {
        rt.elements()
            .iter()
            .find(|e| e.key.as_deref() == Some("thread-scroll"))
            .map(|e| e.target)
            .expect("thread-scroll viewport")
    }

    fn wheel_chunked(rt: &mut Runtime<AppState>, dy: f32) {
        let r = viewport_rect(rt);
        let (x, y) = (r[0] + r[2] * 0.5, r[1] + r[3] * 0.5);
        for _ in 0..6 {
            rt.wheel(x, y, dy / 6.0);
            rt.frame(0.016);
        }
        settle(rt);
    }

    /// Wheel direction that decreases scroll_y (toward the top), discovered
    /// empirically so the test does not assume rune's wheel sign convention.
    fn dir_to_top(rt: &mut Runtime<AppState>) -> f32 {
        let y0 = rt.state.thread_scroll.scroll_y;
        wheel_chunked(rt, -2000.0);
        if rt.state.thread_scroll.scroll_y < y0 { -1.0 } else { 1.0 }
    }

    fn scroll_to_top(rt: &mut Runtime<AppState>) {
        let dir = dir_to_top(rt);
        for _ in 0..8 {
            if rt.state.thread_scroll.scroll_y <= 64.0 {
                return;
            }
            wheel_chunked(rt, dir * 40000.0);
        }
        panic!(
            "could not reach top: scroll_y={}",
            rt.state.thread_scroll.scroll_y
        );
    }

    fn scroll_to_bottom(rt: &mut Runtime<AppState>) {
        let dir = dir_to_top(rt);
        for _ in 0..8 {
            if rt.state.thread_scroll.pinned_to_bottom {
                return;
            }
            wheel_chunked(rt, -dir * 40000.0);
        }
        panic!(
            "could not reach bottom: scroll_y={} max={}",
            rt.state.thread_scroll.scroll_y,
            rt.state.thread_scroll.max_scroll()
        );
    }

    // ---- 500+ row virtualization ----

    #[test]
    fn thread_virtualizes_500_rows_without_full_tree_rebuild() {
        let mut s = thread_state();
        push_many_entries(&mut s, 0, 260); // 520 blocks + 4 preset blocks
        let total = thread_timeline(&s).len();
        assert!(total >= 500, "fixture must exceed 500 rows, got {total}");
        let last_key = thread_timeline(&s).last().unwrap().id().to_string();
        let first_key = thread_timeline(&s).first().unwrap().id().to_string();

        let mut rt = boot(s, (1280.0, 800.0));
        let built = built_row_keys(&rt);
        assert!(built.len() >= 4, "viewport rows must build, got {}", built.len());
        assert!(built.len() < 200, "500+ rows must not all build, got {}", built.len());
        assert!(built.contains(&last_key), "stick_bottom opens at the tail");

        // Scroll far up: the window moves, the tail unbuilds, count stays bounded.
        scroll_to_top(&mut rt);
        let built2 = built_row_keys(&rt);
        assert!(built2.contains(&first_key), "top rows must build after scroll");
        assert!(!built2.contains(&last_key), "tail must unbuild once far away");
        assert!(built2.len() < 200, "window stays bounded, got {}", built2.len());

        // Streaming growth at the tail: still no full-tree rebuild.
        scroll_to_bottom(&mut rt);
        // Headless injection: a direct `rt.state` mutation + `frame` does NOT
        // re-run the view (rune re-runs it only for state changed in a handler
        // or via `update`), so the tail row would keep its stale pre-stream
        // text. Route the delta through `Runtime::update` to force the commit.
        // (rune-spike-lessons.md, "Headless Runtime state injection".)
        rt.update(|s, _tx| {
            if let Some(last) = s.entries.last_mut() {
                last.streaming = true;
                last.text.push_str(" STREAM-TAIL-MARKER");
            }
        });
        rt.frame(0.016);
        settle(&mut rt);
        let built3 = built_row_keys(&rt);
        assert!(built3.len() < 200, "streaming must not rebuild all rows, got {}", built3.len());
        assert!(has_text(&rt, "STREAM-TAIL-MARKER"), "streamed delta must render");
    }

    // ---- Trace collapse ----

    #[test]
    fn thread_trace_collapsed_by_default_and_toggles() {
        let mut rt = boot(thread_state(), (1280.0, 800.0));
        // Preset "thread": e1 user, e2 reasoning + e3 tool -> trace-e2, e4 reply.
        assert!(rt.find("trace-e2").is_some(), "trace row must exist");
        assert!(has_text(&rt, "2 steps"), "collapsed header shows step count");
        assert!(!has_text(&rt, "Measuring RGBA"), "steps hidden while collapsed");

        click_text(&mut rt, "2 steps");
        assert!(has_text(&rt, "Measuring RGBA"), "expand reveals reasoning detail");
        assert!(has_text(&rt, "cargo test"), "expand reveals tool step");
        assert!(rt.state.thread_expanded_traces.contains("trace-e2"));

        click_text(&mut rt, "2 steps");
        assert!(!has_text(&rt, "Measuring RGBA"), "collapse hides steps again");
        assert!(rt.state.collapsed_traces.contains("trace-e2"));

        // Live traces auto-expand (web parity): running tool after the reply.
        // Inject via `update` (not bare `rt.state` mutation) so the view
        // re-commits and mounts the new live trace row.
        let ch = rt.state.active_channel_id.clone();
        rt.update(move |s, _tx| {
            let mut tool = TerminalEntry::new("a-live", &ch, EntryRole::Activity, "terminal: cargo build", "15:42");
            tool.activity_kind = Some(ActivityKind::Tool);
            tool.activity_status = Some("running".into());
            s.entries.push(tool);
            s.thinking_by_channel.insert(ch, true);
        });
        rt.frame(0.016);
        settle(&mut rt);
        assert!(rt.find("trace-a-live").is_some(), "post-text activity forms a new trace");
        assert!(has_text(&rt, "Live activity"), "live trace header");
        assert!(has_text(&rt, "cargo build"), "live trace steps visible without a click");
    }

    // ---- Task drawer: hidden by default, split/overlay at the 900px boundary ----

    #[test]
    fn thread_tasks_drawer_hidden_by_default_and_splits_at_900px() {
        let mut s = thread_state();
        s.ui_config.sidebar_width = 380.0; // stage = 1280 - 380 = 900 exactly
        let mut rt = boot(s, (1280.0, 800.0));
        assert!(!rt.state.thread_tasks_open);
        assert!(rt.find("thread-tasks-drawer").is_none(), "drawer hidden by default");

        let w_closed = viewport_rect(&rt)[2];
        click_key(&mut rt, "thread-tasks-toggle");
        assert!(rt.state.thread_tasks_open);
        assert!(rt.find("thread-tasks-drawer").is_some(), "drawer opens");
        let w_open = viewport_rect(&rt)[2];
        assert!(
            w_open < w_closed - 250.0,
            "split dock must shrink the transcript: {w_closed} -> {w_open}"
        );
        let drawer = rt
            .elements()
            .iter()
            .find(|e| e.key.as_deref() == Some("thread-tasks-drawer"))
            .unwrap()
            .target;
        let vp = viewport_rect(&rt);
        assert!(
            drawer[0] >= vp[0] + vp[2] - 1.0,
            "split drawer sits right of the transcript"
        );

        click_text(&mut rt, "Close");
        assert!(!rt.state.thread_tasks_open);
        assert!(rt.find("thread-tasks-drawer").is_none(), "drawer closes");
    }

    #[test]
    fn thread_tasks_drawer_overlays_below_900px() {
        // 1279 - 380 = 899px stage: one pixel below the split boundary.
        let mut s = thread_state();
        s.ui_config.sidebar_width = 380.0;
        let mut rt = boot(s, (1279.0, 800.0));
        let w_closed = viewport_rect(&rt)[2];
        click_key(&mut rt, "thread-tasks-toggle");
        assert!(rt.find("thread-tasks-drawer").is_some(), "overlay drawer opens");
        let w_open = viewport_rect(&rt)[2];
        assert!(
            (w_open - w_closed).abs() < 2.0,
            "overlay must not resize the transcript: {w_closed} -> {w_open}"
        );
        assert!(rt.find("thread-tasks-overlay").is_some(), "overlay wrapper present");

        // Narrow window (not desktop-lg): overlay regardless of stage width.
        let mut s2 = thread_state();
        s2.ui_config.sidebar_collapsed = true;
        let mut rt2 = boot(s2, (800.0, 800.0));
        click_key(&mut rt2, "thread-tasks-toggle");
        assert!(rt2.find("thread-tasks-overlay").is_some(), "narrow window overlays");
    }

    // ---- Smart bottom-anchor scroll pin ----

    #[test]
    fn thread_scroll_pin_holds_until_user_scrolls_up_then_repins() {
        let mut s = thread_state();
        push_many_entries(&mut s, 0, 260);
        let mut rt = boot(s, (1280.0, 800.0));
        assert!(rt.state.thread_scroll.pinned_to_bottom, "new visit pins to bottom");
        assert!(rt.state.thread_scroll.scroll_y > 100.0, "tall content scrolled down");

        // User scrolls far up: pin releases.
        scroll_to_top(&mut rt);
        assert!(!rt.state.thread_scroll.pinned_to_bottom, "upward scroll unpins");
        let y_hold = rt.state.thread_scroll.scroll_y;

        // Content grows while unpinned: viewport must NOT be dragged down.
        // `update` (not bare mutation) so the growth actually commits.
        rt.update(|s, _tx| push_many_entries(s, 260, 2));
        rt.frame(0.016);
        settle(&mut rt);
        assert!(
            (rt.state.thread_scroll.scroll_y - y_hold).abs() < 2.0,
            "growth must not move an unpinned viewport: {y_hold} -> {}",
            rt.state.thread_scroll.scroll_y
        );
        assert!(!rt.state.thread_scroll.pinned_to_bottom);
        assert!(rt.find("thread-jump-latest").is_some(), "away state offers jump-to-latest");

        // Back at the bottom: re-pins and follows growth again.
        scroll_to_bottom(&mut rt);
        assert!(rt.state.thread_scroll.pinned_to_bottom, "returning to bottom re-pins");
        let y_before = rt.state.thread_scroll.scroll_y;
        // Same headless-injection rule: growth must commit through `update` so
        // the pinned viewport re-runs and `stick_bottom` re-follows the tail.
        rt.update(|s, _tx| push_many_entries(s, 262, 2));
        rt.frame(0.016);
        settle(&mut rt);
        assert!(
            rt.state.thread_scroll.scroll_y > y_before + 10.0,
            "pinned viewport follows content growth"
        );
    }

    // ---- Chronological post-text activity (CRITICAL invariant) ----

    #[test]
    fn thread_post_text_activity_renders_as_new_trace_below_reply() {
        let mut s = AppState::new();
        s.create_new_workspace_session(Some("Chrono"));
        let ch = s.active_channel_id.clone();
        let mut a1 = TerminalEntry::new("a1", &ch, EntryRole::Activity, "search_files completed", "09:01");
        a1.activity_kind = Some(ActivityKind::Tool);
        let mut a2 = TerminalEntry::new("a2", &ch, EntryRole::Activity, "Running patch", "09:03");
        a2.activity_kind = Some(ActivityKind::Tool);
        a2.activity_status = Some("running".into());
        let mut a3 = TerminalEntry::new("a3", &ch, EntryRole::Activity, "patch completed", "09:04");
        a3.activity_kind = Some(ActivityKind::Tool);
        s.entries.push(TerminalEntry::new("u1", &ch, EntryRole::User, "fix the failing test", "09:00"));
        s.entries.push(a1);
        s.entries.push(TerminalEntry::new("h1", &ch, EntryRole::Hermes, "Found it in threadTrace.ts. Patching now.", "09:02"));
        s.entries.push(a2);
        s.entries.push(a3);
        s.thinking_by_channel.insert(ch, true);
        s.active_view = voxcell_core::ActiveView::Thread;

        let rt = boot(s, (1280.0, 800.0));
        let blocks = thread_timeline(&rt.state);
        let ids: Vec<&str> = blocks.iter().map(|b| b.id()).collect();
        assert_eq!(ids, vec!["u1", "trace-a1", "h1", "trace-a3"], "no hoisting/merging");
        drop(blocks);

        // Rendered order must match: user < pre-text trace < reply < NEW live trace.
        let y = |rt: &Runtime<AppState>, key: &str| {
            rt.find(key).unwrap_or_else(|| panic!("row {key} must be mounted"))
        };
        let (_, y_u1) = y(&rt, "u1");
        let (_, y_t1) = y(&rt, "trace-a1");
        let (_, y_h1) = y(&rt, "h1");
        let (_, y_t2) = y(&rt, "trace-a3");
        assert!(y_u1 < y_t1, "pre-text activity above reply");
        assert!(y_t1 < y_h1, "reply below pre-text trace");
        assert!(y_h1 < y_t2, "post-text activity is a NEW block below the reply");
        assert!(has_text(&rt, "Live activity"), "trailing trace is live");
    }

    // ---- Selection + copy ----

    #[test]
    fn thread_drag_selection_and_copy_button() {
        let mut s = AppState::new();
        s.create_new_workspace_session(Some("Select"));
        let ch = s.active_channel_id.clone();
        let reply = "line one of the reply\nline two of the reply\nline three of the reply";
        s.entries.push(TerminalEntry::new("u-sel", &ch, EntryRole::User, "select alpha beta gamma delta", "11:00"));
        s.entries.push(TerminalEntry::new("h-sel", &ch, EntryRole::Hermes, reply, "11:01"));
        s.active_view = voxcell_core::ActiveView::Thread;

        let mut rt = boot(s, (1280.0, 800.0));
        let clip = rune::clipboard::memory();
        rt.use_clipboard(&clip);

        // Per-message Copy button writes the message text via Tx::copy.
        let copy_btn = rt
            .elements()
            .into_iter()
            .filter(|e| e.text.as_deref() == Some("Copy"))
            .max_by(|a, b| a.target[1].partial_cmp(&b.target[1]).unwrap())
            .expect("copy button on the reply row");
        click(
            &mut rt,
            copy_btn.target[0] + copy_btn.target[2] * 0.5,
            copy_btn.target[1] + copy_btn.target[3] * 0.5,
        );
        assert_eq!(
            clip.text().map(|t| t.to_string()).as_deref(),
            Some(reply),
            "Tx::copy must write the reply text"
        );

        // Headless drag-selection gesture (rune remount.rs pattern): anchor near
        // the first text's top-left, frames between moves, then selected_text.
        let first = rt
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref().is_some_and(|t| t.contains("select alpha")))
            .expect("user message text")
            .target;
        let second = rt
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref().is_some_and(|t| t.contains("line one")))
            .expect("reply text")
            .target;
        let (sx, sy) = (first[0] + 1.0, first[1] + 3.0);
        let (ex, ey) = (second[0] + 60.0, second[1] + 3.0);
        rt.pointer_down(sx, sy);
        rt.pointer_move((sx + ex) * 0.5, (sy + ey) * 0.5);
        rt.frame(1.0 / 120.0);
        rt.pointer_move(ex, ey);
        rt.frame(1.0 / 120.0);
        rt.pointer_up(ex, ey);
        rt.frame(1.0 / 120.0);
        let sel = rt.selected_text().expect("drag must select text");
        assert!(sel.contains("select alpha"), "selection starts in the user row: {sel:?}");
        assert!(sel.contains("line one"), "selection spans into the reply row: {sel:?}");
    }

    // ---- Minimap jump ----

    #[test]
    fn thread_minimap_ticks_jump_to_turn() {
        let mut s = thread_state();
        push_many_entries(&mut s, 0, 40);
        let mut rt = boot(s, (1280.0, 800.0));
        assert!(rt.find("thread-turn-minimap").is_some(), "minimap rail renders");
        assert!(rt.find("minimap-tick-u000").is_some(), "turn ticks keyed by anchor");

        // Jump to the first turn from the bottom-pinned tail.
        click_key(&mut rt, "minimap-tick-u000");
        settle(&mut rt);
        assert_eq!(rt.state.thread_minimap_active.as_deref(), Some("u000"));
        assert!(
            rt.state.thread_scroll.scroll_y < 600.0,
            "jump must scroll up to the early turn, got {}",
            rt.state.thread_scroll.scroll_y
        );
        assert!(built_row_keys(&rt).contains(&"u000".to_string()));
    }

    // ---- Uphill a11y: minimap tick names + timestamp contrast ----

    /// (role, name, key) triples from rune's AccessKit JSON — the same walk
    /// uphill's `audit_a11y` uses, so these tests assert on the real a11y tree.
    fn a11y_nodes(rt: &Runtime<AppState>) -> Vec<(String, String, String)> {
        fn walk(v: &serde_json::Value, out: &mut Vec<(String, String, String)>) {
            match v {
                serde_json::Value::Object(map) => {
                    if let Some(role) = map.get("role").and_then(|r| r.as_str()) {
                        out.push((
                            role.to_string(),
                            map.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                            map.get("key").and_then(|k| k.as_str()).unwrap_or("").to_string(),
                        ));
                    }
                    for (k, child) in map {
                        if k != "role" && k != "name" && k != "key" {
                            walk(child, out);
                        }
                    }
                }
                serde_json::Value::Array(items) => {
                    for child in items {
                        walk(child, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(&rt.accessibility_json(), &mut out);
        out
    }

    /// HH:MM transcript/step timestamp, the shape uphill's contrast audit keys on.
    fn is_hhmm(t: &str) -> bool {
        let b = t.trim().as_bytes();
        b.len() == 5 && b[2] == b':' && b[..2].iter().chain(b[3..].iter()).all(|c| c.is_ascii_digit())
    }

    fn composite_over(fg: [f32; 4], bg: [f32; 4]) -> [f32; 4] {
        let a = fg[3];
        [
            fg[0] * a + bg[0] * (1.0 - a),
            fg[1] * a + bg[1] * (1.0 - a),
            fg[2] * a + bg[2] * (1.0 - a),
            1.0,
        ]
    }

    fn srgb_luminance(r: f32, g: f32, b: f32) -> f64 {
        fn chan(c: f32) -> f64 {
            let c = c as f64;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        }
        0.2126 * chan(r) + 0.7152 * chan(g) + 0.0722 * chan(b)
    }

    fn has_ancestor_key(rt: &Runtime<AppState>, idx: usize, key: &str) -> bool {
        let elements = rt.elements();
        let mut cur = elements[idx].parent;
        while let Some(i) = cur {
            if elements[i].key.as_deref() == Some(key) {
                return true;
            }
            cur = elements[i].parent;
        }
        false
    }

    /// WCAG ratio of text node `idx` against its ancestor fills folded over the
    /// window clear color (uphill `audit_contrast`'s background model).
    fn contrast_ratio(rt: &Runtime<AppState>, idx: usize) -> f64 {
        let elements = rt.elements();
        let e = &elements[idx];
        let tc = e.text_color.expect("text node carries a color");
        let clear = rt.clear_color();
        let mut chain = Vec::new();
        let mut cur = e.parent;
        while let Some(i) = cur {
            chain.push(i);
            cur = elements[i].parent;
        }
        let mut bg = [clear[0], clear[1], clear[2], 1.0];
        for i in chain.iter().rev() {
            let f = elements[*i].fill;
            if f.a <= 0.001 {
                continue;
            }
            bg = composite_over([f.r, f.g, f.b, f.a * elements[*i].opacity], bg);
        }
        let fg = composite_over([tc.r, tc.g, tc.b, tc.a * e.opacity], bg);
        let l1 = srgb_luminance(fg[0], fg[1], fg[2]);
        let l2 = srgb_luminance(bg[0], bg[1], bg[2]);
        let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Every visible HH:MM timestamp (optionally restricted to the subtree under
    /// `ancestor_key`) must use the brighter existing `accent_soft` token and
    /// clear WCAG 3.0. Returns how many timestamps were checked.
    fn assert_timestamp_contrast(rt: &Runtime<AppState>, ancestor_key: Option<&str>) -> usize {
        let theme = VoxcellTheme::from_config(&rt.state.ui_config);
        let elements = rt.elements();
        let mut checked = 0;
        for (i, e) in elements.iter().enumerate() {
            let Some(t) = e.text.as_deref() else { continue };
            if !is_hhmm(t) || e.exiting || e.rect[2] <= 0.5 || e.visible[2] <= 0.5 {
                continue;
            }
            if let Some(key) = ancestor_key {
                if !has_ancestor_key(rt, i, key) {
                    continue;
                }
            }
            checked += 1;
            assert_eq!(
                e.text_color,
                Some(theme.accent_soft),
                "timestamp {t:?} must use the brighter existing accent_soft token"
            );
            assert_ne!(
                e.text_color,
                Some(theme.text_faint),
                "timestamp {t:?} must not regress to text_faint (~2.5:1)"
            );
            let ratio = contrast_ratio(rt, i);
            assert!(ratio >= 3.0, "timestamp {t:?} contrast {ratio:.2} must be >= 3.0");
        }
        checked
    }

    #[test]
    fn thread_minimap_ticks_expose_turn_names_in_accesskit() {
        let mut s = thread_state();
        push_many_entries(&mut s, 0, 12);
        let rt = boot(s, (1280.0, 800.0));
        let blocks = thread_timeline(&rt.state);
        let turns = extract_thread_minimap_turns(&blocks);
        assert!(!turns.is_empty(), "minimap data must exist for the injected turns");

        let nodes = a11y_nodes(&rt);
        let tick_nodes: Vec<&(String, String, String)> =
            nodes.iter().filter(|(_, _, k)| k.starts_with("minimap-tick-")).collect();
        assert_eq!(tick_nodes.len(), turns.len(), "one AccessKit node per minimap tick");

        for t in &turns {
            let key = format!("minimap-tick-{}", t.anchor_id);
            let node = nodes
                .iter()
                .find(|(_, _, k)| k == &key)
                .unwrap_or_else(|| panic!("tick {key} missing from the AccessKit tree"));
            assert_eq!(node.0, "button", "tick {key} keeps its button role");
            let status = match t.status {
                voxcell_core::thread::MinimapTurnStatus::Live => "live",
                voxcell_core::thread::MinimapTurnStatus::Error => "error",
                voxcell_core::thread::MinimapTurnStatus::Budget => "budget",
                voxcell_core::thread::MinimapTurnStatus::Cancelled => "cancelled",
                voxcell_core::thread::MinimapTurnStatus::Done => "done",
            };
            let mut expected = format!("Turn {} \u{b7} {}", t.turn_number, status);
            if !t.title.is_empty() {
                expected.push_str(&format!(" \u{2014} {}", t.title));
            }
            assert_eq!(node.1, expected, "tick {key} AccessKit name from core minimap data");
            assert!(!node.1.trim().is_empty(), "tick {key} must not be a screen-reader dead end");
            let el = rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key.as_str()))
                .unwrap_or_else(|| panic!("tick element {key} must be mounted"));
            assert_eq!(el.label.as_deref(), Some(expected.as_str()), "El::label feeds the a11y name");
        }
    }

    #[test]
    fn thread_message_timestamps_use_accent_soft_and_clear_wcag_3() {
        let mut s = thread_state();
        push_many_entries(&mut s, 0, 6);
        let rt = boot(s, (1280.0, 800.0));
        let checked = assert_timestamp_contrast(&rt, None);
        assert!(checked >= 2, "expected visible HH:MM message timestamps, checked {checked}");
    }

    #[test]
    fn thread_trace_step_timestamps_use_accent_soft_and_clear_wcag_3() {
        let mut rt = boot(thread_state(), (1280.0, 800.0));
        // Preset "thread" ships trace-e2 (reasoning + tool steps); expand it with
        // the same gesture the collapse test uses so step timestamps mount.
        assert!(rt.find("trace-e2").is_some(), "trace row must exist");
        click_text(&mut rt, "2 steps");
        assert!(rt.state.thread_expanded_traces.contains("trace-e2"), "trace expanded");
        let checked = assert_timestamp_contrast(&rt, Some("trace-e2"));
        assert!(checked >= 1, "expanded trace must show step timestamps, checked {checked}");
    }
}
