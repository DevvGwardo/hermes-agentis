//! P3-02 SidebarWorkspace parity upgrade.
//!
//! Web reference: `src/components/SidebarWorkspace.tsx` +
//! `src/utils/sidebarCharacterState.ts` / `sidebarSessions.ts` (core ports:
//! `voxcell_core::sidebar_char`, `voxcell_core::sessions`).
//!
//! - 72x66 cursor-tracking reactive portrait (card-level gaze via
//!   `compute_sidebar_pointer_look_target`, halftone field in `sidebar_pixels`).
//! - Simplified session list from the TS-parity entry builder: readable titles,
//!   source labels, parallel-profile `@profile` badges, active-work indicators.
//! - "Needs attention" approval strip resolving through `UiCommand::ResolveApproval`.
//! - Sessions / Gateway scope filter + search.
//! - `sidebar_position` left/right swap + collapse toggle (shell placement lives
//!   in `views::render_main_window`; collapse hides the sidebar, the top-bar
//!   "Show Sidebar" button restores it).
//! - slate-v1 tokens only via `VoxcellTheme`; emerald/amber/rose are semantic
//!   live/working/error telemetry.

use crate::state::{AppState, UiCommand};
use crate::theme::VoxcellTheme;
use rune::{Cx, El, SurfaceFit, TextStyle, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Divider, TextInput};
use voxcell_core::sessions::{
    GatewaySessionInfo, SidebarFilterTab, SidebarOriginFilter, build_sidebar_session_entries,
    filter_sidebar_session_entries,
};
use voxcell_core::sidebar_char::{
    SidebarCharacterInput, SidebarWorkTone, compute_sidebar_pointer_look_target,
    derive_sidebar_character_state, lead_running_task,
};
use voxcell_core::tasks::{
    format_elapsed_duration, resolve_task_display_title, resolve_task_progress,
    summarize_channel_task_activity,
};
use voxcell_core::{
    ActiveView, ChannelMeta, EntryRole, SessionOrigin, SessionScopeTab, SessionStatus,
    SidebarPosition, TaskLifecycleStatus,
};

/// Parallel-profile badge lookup: channel_id first, then session_id.
/// Absent / empty / "default" profiles render no badge (web parity).
fn lookup_session_profile(state: &AppState, channel_id: &str, session_id: &str) -> Option<String> {
    state
        .session_profiles
        .get(channel_id)
        .or_else(|| state.session_profiles.get(session_id))
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty() && p != "default")
}

/// Feeds the TS-parity `build_sidebar_session_entries` layer: local channels
/// first (they win dedupe and keep their explicit workspace/external origin),
/// then gateway history records not yet adopted as channels.
fn sidebar_session_infos(state: &AppState) -> Vec<GatewaySessionInfo> {
    let mut infos = Vec::new();
    for ch in &state.channels {
        let thinking = state
            .thinking_by_channel
            .get(&ch.id)
            .copied()
            .unwrap_or(false);
        let running = state
            .tasks
            .iter()
            .any(|t| t.channel_id == ch.id && t.status == TaskLifecycleStatus::Running);
        let status = if thinking {
            SessionStatus::Thinking
        } else if running {
            SessionStatus::Running
        } else {
            SessionStatus::Idle
        };
        let turn_count = state
            .entries
            .iter()
            .filter(|e| e.channel_id == ch.id && e.role == EntryRole::User)
            .count();
        let last_message = state
            .entries
            .iter()
            .rev()
            .find(|e| e.channel_id == ch.id)
            .map(|e| e.text.clone())
            .filter(|t| !t.trim().is_empty());
        infos.push(GatewaySessionInfo {
            session_id: ch.session_id.clone(),
            session_key: None,
            channel_id: ch.id.clone(),
            name: ch.name.clone(),
            persona: String::new(),
            topic: ch.name.clone(),
            profile: lookup_session_profile(state, &ch.id, &ch.session_id),
            status,
            turn_count,
            last_message,
            updated_at_ms: ch.updated_at_ms.unwrap_or(0),
            source: match ch.origin {
                SessionOrigin::Workspace => Some("web".to_string()),
                SessionOrigin::External => None,
            },
            model: ch.model.clone(),
            origin: Some(ch.origin),
        });
    }
    for gs in &state.gateway_sessions {
        infos.push(GatewaySessionInfo {
            session_id: gs.session_id.clone(),
            session_key: None,
            channel_id: if gs.channel_id.is_empty() {
                gs.session_id.clone()
            } else {
                gs.channel_id.clone()
            },
            name: gs.title.clone(),
            persona: String::new(),
            topic: gs.title.clone(),
            profile: lookup_session_profile(state, &gs.channel_id, &gs.session_id),
            status: gs.status,
            turn_count: gs.message_count,
            last_message: Some(gs.last_message.clone()).filter(|t| !t.trim().is_empty()),
            updated_at_ms: gs.updated_at_ms.unwrap_or(0),
            source: Some(gs.source.clone()).filter(|s| !s.trim().is_empty()),
            model: Some(gs.model.clone()).filter(|m| !m.trim().is_empty()),
            origin: None,
        });
    }
    infos
}

/// Select a session channel; Home jumps to Thread once the channel has history.
fn select_sidebar_channel(s: &mut AppState, channel_id: &str) {
    s.active_channel_id = channel_id.to_string();
    if s.active_view == ActiveView::Home
        && s.entries.iter().any(|e| e.channel_id == channel_id)
    {
        s.active_view = ActiveView::Thread;
    }
}

pub fn render_sidebar(state: &AppState, theme: &VoxcellTheme, _cx: &mut Cx<AppState>) -> El {
    let k = &theme.kit;
    let width = state.ui_config.sidebar_width.clamp(240.0, 380.0);

    let running_count = state
        .tasks
        .iter()
        .filter(|t| t.status == TaskLifecycleStatus::Running)
        .count();
    let pending_approvals = state.pending_approvals();
    let lead_task = lead_running_task(&state.tasks);

    let char_pres = derive_sidebar_character_state(&SidebarCharacterInput {
        connected: state.gateway.online,
        is_speaking: state.audio.is_speaking
            || state.stage_mood == voxcell_core::ExpressionMood::Speaking,
        is_listening: state.audio.is_listening || state.voice.hands_free_vad,
        active_channel_thinking: state.active_is_thinking(),
        any_thinking: state.thinking_by_channel.values().any(|&v| v),
        running_task_count: running_count,
        pending_approval_count: pending_approvals.len(),
        voice_amplitude: state.audio.output_level.max(state.audio.input_level),
        active_task_progress: lead_task.map(|t| t.progress),
        active_task_title: lead_task.map(resolve_task_display_title),
        look_x: state.pointer_sidebar.map(|p| p.0).unwrap_or(0.0),
        look_y: state.pointer_sidebar.map(|p| p.1).unwrap_or(0.0),
    });

    let tone_color = match char_pres.tone {
        SidebarWorkTone::Emerald => theme.accent,
        SidebarWorkTone::Amber => theme.amber,
        SidebarWorkTone::Rose => theme.rose,
        SidebarWorkTone::Slate => theme.text_muted,
    };
    let is_voice = char_pres.is_speaking || char_pres.is_listening;

    // 1. Header: workspace title + position swap / collapse / New Session.
    let header = row()
        .key("sidebar-header")
        .w_full()
        .items_center()
        .justify_between()
        .child(
            row()
                .gap(8.0)
                .items_center()
                .min_w(0.0)
                .child(
                    el().w(8.0)
                        .h(8.0)
                        .radius(4.0)
                        .fill(if state.gateway.online {
                            theme.accent
                        } else {
                            theme.rose
                        }),
                )
                .child(
                    text(state.ui_config.workspace_title.clone())
                        .font(TextStyle::new(11.5, 700.0).tracking(0.8).mono())
                        .color(theme.text),
                ),
        )
        .child(
            row()
                .gap(4.0)
                .items_center()
                .child(
                    Button::new("Swap")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Mini)
                        .on_press(|s: &mut AppState| {
                            s.ui_config.sidebar_position = match s.ui_config.sidebar_position {
                                SidebarPosition::Left => SidebarPosition::Right,
                                SidebarPosition::Right => SidebarPosition::Left,
                            };
                            s.pending_commands.push(UiCommand::PersistUiConfig);
                        })
                        .build(k),
                )
                .child(
                    Button::new("Hide")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Mini)
                        .on_press(|s: &mut AppState| {
                            s.ui_config.sidebar_collapsed = true;
                            s.pending_commands.push(UiCommand::PersistUiConfig);
                        })
                        .build(k),
                )
                .child(
                    Button::new("+ New")
                        .variant(ButtonVariant::Neutral)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| s.create_new_workspace_session(None))
                        .build(k),
                ),
        );

    // 2. Reactive companion card. Gaze is tracked over the whole card (web
    //    parity: computeSidebarPointerLookTarget with a 46px portrait offset),
    //    reset to center when the pointer leaves.
    let card_w = (width - 24.0).max(160.0);
    let card_h = 86.0
        + if lead_task.is_some() {
            68.0
        } else if !pending_approvals.is_empty() {
            34.0
        } else {
            0.0
        };

    let avatar_surface = el()
        .key("sidebar-avatar-hit")
        // Uphill a11y: the pressable portrait must carry an accessible name
        // (rune El::label -> AccessKit name, same API as settings.rs controls).
        .label("Cycle companion mood")
        .w(72.0)
        .h(66.0)
        .radius(8.0)
        .fill(theme.bg.alpha(0.92))
        .border(1.0, tone_color.alpha(0.35))
        .on_press(|s: &mut AppState| {
            s.toggle_stage_mood();
        })
        .child(
            el().w(72.0)
                .h(66.0)
                .pixels_fit(&state.sidebar_pixels, SurfaceFit::Fit),
        );

    let gateway_line = if state.gateway.online {
        format!("{} · {}", state.gateway.model, state.gateway.active_profile)
    } else {
        "Gateway offline · Open Ops".to_string()
    };

    // Voice / working telemetry line (web parity).
    let telemetry: Option<El> = if is_voice {
        let mut label = if char_pres.is_listening {
            "mic active".to_string()
        } else {
            "voice live".to_string()
        };
        if running_count > 0 {
            label.push_str(&format!(" · {running_count} active"));
        }
        let mut bars = row().gap(1.0).items_center().h(10.0);
        for band in state.audio.bands.iter().take(6) {
            bars = bars.child(
                el().w(2.0)
                    .h(10.0 * band.clamp(0.24, 1.0))
                    .radius(1.0)
                    .fill(theme.accent),
            );
        }
        Some(
            row().w_full()
                .gap(6.0)
                .items_center()
                .justify_between()
                .child(
                    text(label)
                        .font(TextStyle::new(10.0, 500.0).lh(12.0).mono())
                        .color(theme.accent),
                )
                .child(bars),
        )
    } else if let Some(task) = lead_task {
        let mut label = match task.created_at_ms {
            Some(created) => format!(
                "{} elapsed",
                format_elapsed_duration(Some(created), state.now_ms, true)
            ),
            None => "active execution".to_string(),
        };
        if running_count > 1 {
            label.push_str(&format!(" · {running_count} active"));
        }
        Some(
            text(label)
                .font(TextStyle::new(10.0, 500.0).lh(12.0).mono())
                .color(theme.amber),
        )
    } else {
        None
    };

    let mut info_col = col()
        .key("sidebar-companion-info")
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .on_press(|s: &mut AppState| s.active_view = ActiveView::Queue)
        .child(
            row()
                .w_full()
                .items_center()
                .justify_between()
                .child(
                    text("Hermes Agent")
                        .font(TextStyle::new(13.0, 600.0).lh(17.0))
                        .color(theme.text),
                )
                .child(
                    row()
                        .gap(4.0)
                        .items_center()
                        .px(6.0)
                        .py(2.0)
                        .radius(4.0)
                        .fill(tone_color.alpha(0.16))
                        .border(0.5, tone_color.alpha(0.45))
                        .child(el().w(5.0).h(5.0).radius(2.5).fill(tone_color))
                        .child(
                            // Uphill contrast: tone_color text on the tinted chip
                            // measured ~2.7 (<3.0). The label now uses the bright
                            // slate token accent.soft (#CBD5E1); the dot + chip
                            // tint keep the semantic work tone.
                            text(char_pres.status_label.clone())
                                .font(TextStyle::new(10.0, 600.0).lh(12.0).mono())
                                .color(theme.accent_soft),
                        ),
                ),
        )
        .child(
            text(gateway_line)
                .font(TextStyle::new(11.0, 400.0).lh(14.0))
                .color(theme.text_muted),
        );
    if let Some(t) = telemetry {
        info_col = info_col.child(t);
    }

    let mut companion_card = col()
        .key("sidebar-companion-card")
        .w_full()
        .gap(10.0)
        .padding(10.0)
        .radius(10.0)
        .fill(theme.card_fill)
        .border(1.0, tone_color.alpha(0.28))
        .on_pointer_move_local(move |s: &mut AppState, ev, _tx| {
            s.pointer_sidebar = Some(compute_sidebar_pointer_look_target(
                0.0, 0.0, card_w, card_h, ev.x, ev.y, 46.0,
            ));
        })
        .on_hover_change(|s: &mut AppState, inside: bool, _tx| {
            if !inside {
                s.pointer_sidebar = None;
            }
        })
        .child(
            row()
                .w_full()
                .gap(10.0)
                .items_center()
                .child(avatar_surface)
                .child(info_col),
        );

    // Active-task strip (web: sidebar-active-task-strip) — click opens the lane.
    if let Some(task) = lead_task {
        let progress = resolve_task_progress(task);
        let elapsed = format_elapsed_duration(task.created_at_ms, state.now_ms, true);
        let title = resolve_task_display_title(task);
        let task_channel = task.channel_id.clone();
        companion_card = companion_card.child(
            col()
                .key("sidebar-task-tray")
                .w_full()
                .gap(5.0)
                .padding(8.0)
                .radius(6.0)
                .fill(theme.bg.alpha(0.68))
                .border(0.5, theme.amber.alpha(0.32))
                .on_press(move |s: &mut AppState| {
                    select_sidebar_channel(s, &task_channel);
                })
                .child(
                    row()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .child(
                            text(title)
                                .font(TextStyle::new(11.5, 600.0).lh(15.0))
                                .color(theme.text),
                        )
                        .child(
                            text(format!("{} · {}", progress.ratio_label, elapsed))
                                .font(theme.mono_sm)
                                .color(theme.amber),
                        ),
                )
                .child(
                    text(
                        task.last_step
                            .clone()
                            .unwrap_or_else(|| "Executing turn...".into()),
                    )
                    .font(theme.mono_sm)
                    .color(theme.amber.alpha(0.85)),
                ),
        );
    } else if let Some(req) = pending_approvals.first() {
        // Approval strip (web: sidebar-approval-strip) — review in Queue.
        companion_card = companion_card.child(
            row()
                .key("sidebar-approval-strip")
                .w_full()
                .justify_between()
                .items_center()
                .gap(6.0)
                .padding(8.0)
                .radius(6.0)
                .fill(theme.amber.alpha(0.06))
                .border(0.5, theme.amber.alpha(0.30))
                .on_press(|s: &mut AppState| s.active_view = ActiveView::Queue)
                .child(
                    text(format!("Review in Queue · #{}", req.channel_id))
                        .font(TextStyle::new(10.0, 500.0).lh(12.0).mono())
                        .color(theme.amber.alpha(0.9)),
                )
                .child(
                    text("Open →")
                        .font(TextStyle::new(10.0, 600.0).lh(12.0).mono())
                        .color(theme.amber),
                ),
        );
    }

    // 3. Needs-attention section, pinned above sessions so approval gates are
    //    never buried. First request resolves inline (Approve/Always/Deny ->
    //    UiCommand::ResolveApproval); the rest deep-link into the Queue.
    let attention_card = if !pending_approvals.is_empty() && state.ui_config.sidebar_queue_visible {
        let req = &pending_approvals[0];
        let req_id_once = req.id.clone();
        let req_id_always = req.id.clone();
        let req_id_deny = req.id.clone();
        let mut section = col()
            .key("sidebar-attention-card")
            .w_full()
            .gap(8.0)
            .padding(10.0)
            .radius(8.0)
            .fill(theme.amber.alpha(0.11))
            .border(1.0, theme.amber.alpha(0.42))
            .child(
                row()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        text("NEEDS ATTENTION")
                            .font(TextStyle::new(10.0, 700.0).tracking(0.7).mono())
                            .color(theme.amber),
                    )
                    .child(
                        text(format!("{} pending", pending_approvals.len()))
                            .font(theme.mono_sm)
                            .color(theme.amber),
                    ),
            )
            .child(
                text(req.title.clone())
                    .font(TextStyle::new(12.5, 600.0).lh(16.0))
                    .color(theme.text),
            )
            .child(
                text(if req.command.is_empty() {
                    req.summary.clone()
                } else {
                    req.command.clone()
                })
                .font(theme.mono_sm)
                .color(theme.text_muted),
            )
            .child(
                row()
                    .gap(6.0)
                    .w_full()
                    .child(
                        Button::new("Approve")
                            .variant(ButtonVariant::Submit)
                            .control_size(ControlSize::Small)
                            .on_press(move |s: &mut AppState| {
                                s.resolve_approval(&req_id_once, "once");
                            })
                            .build(k),
                    )
                    .child(
                        Button::new("Always")
                            .variant(ButtonVariant::Neutral)
                            .control_size(ControlSize::Small)
                            .on_press(move |s: &mut AppState| {
                                s.resolve_approval(&req_id_always, "always");
                            })
                            .build(k),
                    )
                    .child(
                        Button::new("Deny")
                            .variant(ButtonVariant::Ghost)
                            .control_size(ControlSize::Small)
                            .on_press(move |s: &mut AppState| {
                                s.resolve_approval(&req_id_deny, "deny");
                            })
                            .build(k),
                    ),
            );
        for extra in pending_approvals.iter().skip(1).take(2) {
            section = section.child(
                row()
                    .key(format!("sidebar-attention-row-{}", extra.id))
                    .w_full()
                    .gap(6.0)
                    .items_center()
                    .padding(6.0)
                    .radius(6.0)
                    .fill(theme.bg.alpha(0.35))
                    .on_press(|s: &mut AppState| s.active_view = ActiveView::Queue)
                    .child(el().w(5.0).h(5.0).radius(2.5).fill(theme.amber))
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(1.0)
                            .child(
                                text(extra.title.clone())
                                    .font(TextStyle::new(11.5, 500.0).lh(14.0))
                                    .color(theme.text),
                            )
                            .child(
                                text(format!("Approval · #{}", extra.channel_id))
                                    .font(TextStyle::new(10.0, 400.0).lh(12.0).mono())
                                    .color(theme.amber.alpha(0.75)),
                            ),
                    ),
            );
        }
        if pending_approvals.len() > 3 {
            section = section.child(
                row()
                    .key("sidebar-attention-more")
                    .w_full()
                    .padding(4.0)
                    .on_press(|s: &mut AppState| s.active_view = ActiveView::Queue)
                    .child(
                        text(format!("+{} more in Queue", pending_approvals.len() - 3))
                            .font(theme.mono_sm)
                            .color(theme.amber.alpha(0.8)),
                    ),
            );
        }
        Some(section)
    } else {
        None
    };

    // 4. Sessions / Gateway scope filter + search (web clears the query when
    //    switching scope).
    let activity = summarize_channel_task_activity(
        &state.tasks,
        &state.channels,
        &state.thinking_by_channel,
    );
    let infos = sidebar_session_infos(state);
    let entries = build_sidebar_session_entries(&[], &infos, &activity, state.now_ms);
    let origin_filter = match state.session_scope {
        SessionScopeTab::Sessions => SidebarOriginFilter::Workspace,
        SessionScopeTab::Gateway => SidebarOriginFilter::External,
    };
    let filtered = filter_sidebar_session_entries(
        &entries,
        SidebarFilterTab::All,
        &state.search_edit.text,
        origin_filter,
    );

    let scope_tabs = row()
        .key("sidebar-scope-tabs")
        .w_full()
        .gap(6.0)
        .child(
            Button::new(format!("Sessions ({})", filtered.workspace_count))
                .variant(if state.session_scope == SessionScopeTab::Sessions {
                    ButtonVariant::Neutral
                } else {
                    ButtonVariant::Ghost
                })
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| {
                    s.session_scope = SessionScopeTab::Sessions;
                    s.search_edit.clear();
                })
                .build(k),
        )
        .child(
            Button::new(format!("Gateway ({})", filtered.external_count))
                .variant(if state.session_scope == SessionScopeTab::Gateway {
                    ButtonVariant::Neutral
                } else {
                    ButtonVariant::Ghost
                })
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| {
                    s.session_scope = SessionScopeTab::Gateway;
                    s.search_edit.clear();
                })
                .build(k),
        );

    let search_bar = TextInput::new(&state.search_edit)
        .placeholder(if state.session_scope == SessionScopeTab::Gateway {
            "Search gateway history..."
        } else {
            "Filter sessions..."
        })
        .label("Filter sessions")
        .width(None)
        .on_edit(|s: &mut AppState| &mut s.search_edit)
        .build(k);

    // 5. Simplified session list: readable title + @profile badge + active-work
    //    indicator + metadata line (source · turns · model / active task).
    let mut session_list = col().key("sidebar-session-list").w_full().gap(4.0);
    if !state.ui_config.sidebar_channels_visible {
        // Layout hid the session list entirely.
    } else if filtered.visible.is_empty() {
        session_list = session_list.child(
            col()
                .padding(12.0)
                .radius(8.0)
                .fill(theme.bg.alpha(0.45))
                .border(0.5, theme.border)
                .gap(4.0)
                .child(
                    text("No sessions matching filter")
                        .font(TextStyle::new(12.0, 500.0))
                        .color(theme.text_muted),
                )
                .child(
                    text("Start a prompt on Home or click + New.")
                        .font(theme.mono_sm)
                        .color(theme.text_faint),
                ),
        );
    } else {
        for entry in &filtered.visible {
            let ch_id = entry.session.channel_id.clone();
            let is_active = ch_id == state.active_channel_id;
            let has_local_channel = state.channels.iter().any(|c| c.id == ch_id);
            let profile = entry
                .session
                .profile
                .clone()
                .filter(|p| !p.is_empty() && p != "default");

            let border_c = if entry.has_active_work {
                theme.amber.alpha(0.45)
            } else if is_active {
                theme.accent.alpha(0.48)
            } else {
                theme.border
            };
            let bg_c = if is_active {
                theme.accent.alpha(0.11)
            } else {
                theme.bg.alpha(0.45)
            };

            let mut title_row = row()
                .w_full()
                .gap(6.0)
                .items_center()
                .child(
                    el().w(6.0)
                        .h(6.0)
                        .radius(3.0)
                        .fill(if entry.has_active_work {
                            theme.amber
                        } else if is_active {
                            theme.accent
                        } else {
                            theme.text_faint
                        }),
                )
                .child(
                    text(entry.display_title.clone())
                        .font(TextStyle::new(12.5, if is_active { 600.0 } else { 500.0 }).lh(16.0))
                        .color(if is_active { theme.text } else { theme.text_muted }),
                );
            if let Some(p) = &profile {
                title_row = title_row.child(
                    row()
                        .key(format!("profile-badge-{ch_id}"))
                        .px(4.0)
                        .py(1.0)
                        .radius(3.0)
                        .fill(theme.accent.alpha(0.14))
                        .border(0.5, theme.accent.alpha(0.40))
                        .child(
                            text(format!("@{p}"))
                                .font(TextStyle::new(9.5, 600.0).lh(11.0).mono())
                                .color(theme.accent),
                        ),
                );
            }
            let right_label = if entry.has_active_work {
                Some((
                    if entry.running_count > 1 {
                        format!("{} active", entry.running_count)
                    } else {
                        "working".to_string()
                    },
                    theme.amber,
                ))
            } else if !entry.relative_time.is_empty() {
                Some((
                    entry.relative_time.clone(),
                    if is_active { theme.text_muted } else { theme.text_faint },
                ))
            } else {
                None
            };
            if let Some((label, color)) = right_label {
                title_row = title_row.child(
                    text(label)
                        .font(TextStyle::new(10.0, 500.0).lh(12.0).mono())
                        .color(color),
                );
            }

            // The badge chip already surfaces @profile; keep it out of the
            // metadata line to avoid duplication.
            let metadata = match &profile {
                Some(p) => entry.metadata_line.replace(&format!(" · @{p}"), ""),
                None => entry.metadata_line.clone(),
            };

            let entry_for_press = entry.clone();
            let ch_id_select = ch_id.clone();
            let mut row_el = row()
                .key(format!("session-row-{ch_id}"))
                .w_full()
                .padding(8.0)
                .radius(7.0)
                .fill(bg_c)
                .border(0.75, border_c)
                .items_center()
                .justify_between()
                .gap(6.0)
                .on_press(move |s: &mut AppState| {
                    // Gateway-history rows adopt into a local channel on first
                    // click so Thread/Canvas can address them.
                    if !s.channels.iter().any(|c| c.id == ch_id_select) {
                        s.channels.push(ChannelMeta {
                            id: ch_id_select.clone(),
                            name: entry_for_press.display_title.clone(),
                            category: "hermes".to_string(),
                            description: entry_for_press.source_label.clone(),
                            session_id: entry_for_press.session.hermes_session_id.clone(),
                            unread: 0,
                            origin: entry_for_press.origin,
                            has_local_history: false,
                            model: entry_for_press
                                .hermes_session
                                .as_ref()
                                .and_then(|h| h.model.clone()),
                            updated_at_ms: Some(entry_for_press.updated_at_ms),
                        });
                    }
                    select_sidebar_channel(s, &ch_id_select);
                })
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(title_row)
                        .child(
                            text(metadata)
                                .font(TextStyle::new(10.5, 400.0).lh(13.0).mono())
                                .color(if entry.has_active_work {
                                    theme.amber.alpha(0.85)
                                } else if is_active {
                                    theme.text_muted
                                } else {
                                    theme.text_faint
                                }),
                        ),
                );

            if has_local_channel && ch_id != "home" {
                let ch_id_dismiss = ch_id.clone();
                row_el = row_el.child(
                    Button::new("x")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Mini)
                        .on_press(move |s: &mut AppState| {
                            s.channels.retain(|c| c.id != ch_id_dismiss);
                            s.dots.retain(|d| d.channel_id != ch_id_dismiss);
                            if s.active_channel_id == ch_id_dismiss {
                                s.active_channel_id = "home".into();
                            }
                            s.pending_commands
                                .push(UiCommand::DismissChannel(ch_id_dismiss.clone()));
                        })
                        .build(k),
                );
            }

            session_list = session_list.child(row_el);
        }
    }

    // 6. Footer actions (Pin to Canvas + Phone Widget quick toggle).
    let footer = row()
        .key("sidebar-footer")
        .w_full()
        .justify_between()
        .gap(6.0)
        .child(
            Button::new("Pin to Canvas")
                .variant(ButtonVariant::Neutral)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| s.pin_active_channel_to_canvas())
                .build(k),
        )
        .child(
            Button::new(if state.phone_widget_open {
                "Phone Open"
            } else {
                "Phone HUD"
            })
            .variant(if state.phone_widget_open {
                ButtonVariant::Submit
            } else {
                ButtonVariant::Neutral
            })
            .control_size(ControlSize::Small)
            .on_press(|s: &mut AppState| {
                let next = !s.phone_widget_open;
                s.phone_widget_open = next;
                s.pending_commands.push(UiCommand::TogglePhoneWindow(next));
            })
            .build(k),
        );

    let mut root = col()
        .key("sidebar")
        .w(width)
        .h_full()
        .min_h(0.0)
        .padding(12.0)
        .gap(10.0)
        .fill(theme.glass_fill)
        .backdrop_blur(12.0)
        .border(1.0, theme.border)
        .child(header)
        .child(companion_card);

    if let Some(card) = attention_card {
        root = root.child(card);
    }

    let mut root = root.child(Divider::horizontal().build(k));
    if state.ui_config.sidebar_queue_visible {
        root = root.child(scope_tabs).child(search_bar);
    }
    root.child(
        col()
            .key("sidebar-scroll")
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .scroll(rune::ScrollSpec { stick_bottom: false })
            .child(session_list),
    )
    .child(Divider::horizontal().build(k))
    .child(footer)
}

// ---------------------------------------------------------------------------
// Headless Runtime tests (pattern: voxcell-app/src/main.rs bottom tests).
// Kept in this file per ownership map (app/main.rs belongs to theme-chrome).
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_main_window;
    use rune::{Env, Runtime};
    use voxcell_core::{
        ApprovalKind, BackgroundTask, ChannelApprovalRequest, SessionOrigin, TaskStepItem,
    };

    fn mk_channel(id: &str, name: &str, origin: SessionOrigin, updated: u64) -> ChannelMeta {
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

    fn mk_task(id: &str, channel_id: &str, title: &str) -> BackgroundTask {
        BackgroundTask {
            id: id.into(),
            channel_id: channel_id.into(),
            session_id: format!("s-{channel_id}"),
            kind: Some("Turn".into()),
            background: Some(false),
            title: title.into(),
            status: TaskLifecycleStatus::Running,
            raw_status: Some("running".into()),
            progress: 45.0,
            progress_label: Some("1/2".into()),
            last_step: Some("Scanning handlers".into()),
            steps: vec![
                TaskStepItem { text: "Inspect routes".into(), done: true, active: false },
                TaskStepItem { text: "Scanning handlers".into(), done: false, active: true },
            ],
            result: None,
            error_code: None,
            created_at_ms: Some(1_000),
            updated_at_ms: Some(9_000),
            agent_done_at_ms: None,
            confirmed_at_ms: None,
            updated_at: "live".into(),
        }
    }

    fn mk_approval(id: &str, request_id: &str, title: &str) -> ChannelApprovalRequest {
        ChannelApprovalRequest {
            id: id.into(),
            channel_id: "home".into(),
            run_id: Some("run-77".into()),
            request_id: request_id.into(),
            execution_generation: 1,
            kind: ApprovalKind::Approval,
            title: title.into(),
            tool_name: Some("terminal".into()),
            command: "systemctl restart hermes-gateway".into(),
            summary: "Restart gateway daemon".into(),
            destination: "ops".into(),
            choices: vec!["once".into(), "always".into(), "deny".into()],
            created_at: "just now".into(),
            resolved: false,
            resolution: None,
        }
    }

    fn boot(scene: &str) -> Runtime<AppState> {
        let mut s = AppState::new();
        s.apply_scene_preset(scene);
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);
        rt
    }

    fn click_text(rt: &mut Runtime<AppState>, label: &str) {
        let el = rt
            .elements()
            .into_iter()
            .find(|e| e.text.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("button with text {label:?} not found"));
        let x = el.target[0] + el.target[2] * 0.5;
        let y = el.target[1] + el.target[3] * 0.5;
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.016);
    }

    fn has_text(rt: &Runtime<AppState>, label: &str) -> bool {
        rt.elements().iter().any(|e| e.text.as_deref() == Some(label))
    }

    /// Direct state injection happens outside a handler, so mark the view
    /// dirty before the next frame (rune only re-runs the view on change).
    fn refresh(rt: &mut Runtime<AppState>) {
        rt.invalidate();
        rt.frame(0.016);
    }

    #[test]
    fn sidebar_scope_filter_switches_sessions_and_gateway() {
        let mut rt = boot("idle");
        rt.state
            .channels
            .push(mk_channel("cli-run-1", "Overnight CI sweep", SessionOrigin::External, 5_000));
        refresh(&mut rt);

        // Sessions scope: workspace rows only.
        assert!(rt.find("session-row-home").is_some());
        assert!(rt.find("session-row-cli-run-1").is_none());
        assert!(has_text(&rt, "Sessions (1)"));
        assert!(has_text(&rt, "Gateway (1)"));

        // Switch to Gateway scope.
        click_text(&mut rt, "Gateway (1)");
        assert_eq!(rt.state.session_scope, SessionScopeTab::Gateway);
        assert!(rt.find("session-row-cli-run-1").is_some());
        assert!(rt.find("session-row-home").is_none());

        // Search narrows the gateway list; a miss shows the empty state.
        rt.state.search_edit.replace_text("overnight");
        refresh(&mut rt);
        assert!(rt.find("session-row-cli-run-1").is_some());
        rt.state.search_edit.replace_text("zzz-no-match");
        refresh(&mut rt);
        assert!(rt.find("session-row-cli-run-1").is_none());
        assert!(has_text(&rt, "No sessions matching filter"));

        // Switching back clears the query (web parity) and restores rows.
        click_text(&mut rt, "Sessions (1)");
        assert_eq!(rt.state.session_scope, SessionScopeTab::Sessions);
        assert!(rt.state.search_edit.text.is_empty());
        assert!(rt.find("session-row-home").is_some());
    }

    #[test]
    fn sidebar_attention_strip_resolves_via_ui_command() {
        let mut rt = boot("attention");
        assert_eq!(rt.state.pending_approvals().len(), 1);
        assert!(rt.find("sidebar-attention-card").is_some());
        assert!(has_text(&rt, "1 pending"));
        // No running task in this scene -> companion card shows the approval
        // strip that deep-links into the Queue.
        assert!(rt.find("sidebar-approval-strip").is_some());
        let (sx, sy) = rt.find("sidebar-approval-strip").unwrap();
        rt.pointer_down(sx, sy);
        rt.pointer_up(sx, sy);
        rt.frame(0.016);
        assert_eq!(rt.state.active_view, ActiveView::Queue);

        // Resolve inline: Approve -> UiCommand::ResolveApproval { decision: once }.
        rt.state.active_view = ActiveView::Home;
        refresh(&mut rt);
        click_text(&mut rt, "Approve");
        assert_eq!(rt.state.pending_approvals().len(), 0);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::ResolveApproval { decision, request_id, is_clarify, .. }
                if decision == "once" && request_id == "req-77" && !*is_clarify
        )));
        rt.frame(0.016);
        assert!(rt.find("sidebar-attention-card").is_none());
        assert!(rt.find("sidebar-approval-strip").is_none());

        // A fresh request resolves via Deny.
        rt.state.approvals.push(mk_approval("apr-2", "req-78", "Drop stale cache"));
        refresh(&mut rt);
        assert!(rt.find("sidebar-attention-card").is_some());
        click_text(&mut rt, "Deny");
        assert_eq!(rt.state.pending_approvals().len(), 0);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::ResolveApproval { decision, request_id, .. }
                if decision == "deny" && request_id == "req-78"
        )));
    }

    #[test]
    fn sidebar_position_swap_and_collapse_toggles() {
        let mut rt = boot("idle");
        let (sb_x, _) = rt.find("sidebar").expect("sidebar rendered");
        let (main_x, _) = rt.find("main-pane").expect("main pane rendered");
        assert!(sb_x < main_x, "default position is left");

        // Swap docks the sidebar to the right and persists the config.
        click_text(&mut rt, "Swap");
        assert_eq!(rt.state.ui_config.sidebar_position, SidebarPosition::Right);
        assert!(rt
            .state
            .pending_commands
            .iter()
            .any(|c| matches!(c, UiCommand::PersistUiConfig)));
        rt.settle_all();
        rt.frame(0.016);
        let (sb_x, _) = rt.find("sidebar").expect("sidebar rendered");
        let (main_x, _) = rt.find("main-pane").expect("main pane rendered");
        assert!(sb_x > main_x, "swapped position is right");

        // Collapse hides the sidebar; the top-bar control restores it.
        click_text(&mut rt, "Hide");
        assert!(rt.state.ui_config.sidebar_collapsed);
        assert!(rt.find("sidebar").is_none());
        click_text(&mut rt, "Show Sidebar");
        assert!(!rt.state.ui_config.sidebar_collapsed);
        assert!(rt.find("sidebar").is_some());
    }

    #[test]
    fn sidebar_parallel_profile_badges_and_active_task_indicator() {
        let mut rt = boot("idle");
        rt.state.create_new_workspace_session(Some("Parser refactor"));
        let ch1 = rt.state.active_channel_id.clone();
        rt.state.create_new_workspace_session(Some("Ops deploy"));
        let ch2 = rt.state.active_channel_id.clone();
        rt.state.tasks.push(mk_task("t-1", &ch1, "Refactor parser module"));
        rt.state.tasks.push(mk_task("t-2", &ch2, "Deploy staging preview"));
        rt.state.session_profiles.insert(ch1.clone(), "coder".into());
        rt.state.session_profiles.insert(ch2.clone(), "ops".into());
        refresh(&mut rt);

        // Both parallel profiles surface badges at the same time.
        assert!(rt.find(&format!("profile-badge-{ch1}")).is_some());
        assert!(rt.find(&format!("profile-badge-{ch2}")).is_some());
        assert!(has_text(&rt, "@coder"));
        assert!(has_text(&rt, "@ops"));

        // Active-task indicators: per-row "working" labels + companion tray.
        assert!(has_text(&rt, "working"));
        assert!(rt.find("sidebar-task-tray").is_some());
        assert!(has_text(&rt, "Working"));
        assert!(rt
            .elements()
            .iter()
            .any(|e| e.text.as_deref().is_some_and(|t| t.contains("Refactor parser module"))));

        // Default profile renders no badge.
        rt.state.session_profiles.insert(ch2.clone(), "default".into());
        refresh(&mut rt);
        assert!(rt.find(&format!("profile-badge-{ch2}")).is_none());
        assert!(rt.find(&format!("profile-badge-{ch1}")).is_some());
    }

    #[test]
    fn sidebar_portrait_is_72x66_and_tracks_cursor_gaze() {
        let mut rt = boot("idle");
        let avatar = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("sidebar-avatar-hit"))
            .expect("sidebar avatar surface");
        assert!((avatar.target[2] - 72.0).abs() < 0.5, "avatar width 72");
        assert!((avatar.target[3] - 66.0).abs() < 0.5, "avatar height 66");

        // Hover across the companion card drives the normalized look target.
        let (cx, cy) = rt.find("sidebar-companion-card").expect("companion card");
        rt.pointer_move(cx + 24.0, cy + 6.0);
        rt.frame(0.016);
        let look = rt.state.pointer_sidebar.expect("gaze tracked on card hover");
        assert!(look.0.abs() <= 1.0 && look.1.abs() <= 1.0);
        assert!(look.0 != 0.0 || look.1 != 0.0, "gaze follows cursor off-center");

        // Leaving the card re-centers the gaze (web: lookTargetRef = null).
        rt.pointer_move(1200.0, 700.0);
        rt.frame(0.016);
        assert!(rt.state.pointer_sidebar.is_none(), "gaze resets on leave");

        // Pressing the portrait cycles the stage mood (web: onCycleMood).
        let before = rt.state.stage_mood;
        let (ax, ay) = rt.find("sidebar-avatar-hit").unwrap();
        rt.pointer_down(ax, ay);
        rt.pointer_up(ax, ay);
        rt.frame(0.016);
        assert_ne!(rt.state.stage_mood, before);
    }

    // ── P5 uphill-audit fixes: avatar accessible name + status contrast ────

    /// Walk the AccessKit JSON exactly like voxcell-uphill's `audit_a11y`:
    /// collect (role, name, key) for every node that carries a role.
    fn a11y_nodes(tree: &serde_json::Value, out: &mut Vec<(String, String, String)>) {
        match tree {
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
                        a11y_nodes(child, out);
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    a11y_nodes(child, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn sidebar_avatar_button_is_named_in_accesskit_tree() {
        let mut rt = boot("idle");
        rt.settle_all();
        rt.frame(0.016);

        // Element inspection: the hit-area button carries an explicit label.
        let avatar = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("sidebar-avatar-hit"))
            .expect("sidebar avatar surface");
        assert_eq!(avatar.role, "button");
        assert_eq!(avatar.label.as_deref(), Some("Cycle companion mood"));

        // Audit method: the name must surface in accessibility_json — the
        // uphill harness flags interactive nodes whose trimmed name is empty.
        let mut nodes = Vec::new();
        a11y_nodes(&rt.accessibility_json(), &mut nodes);
        let node = nodes
            .iter()
            .find(|(_, _, k)| k == "sidebar-avatar-hit")
            .expect("avatar button present in AccessKit tree");
        assert_eq!(node.0, "button");
        assert_eq!(node.1.trim(), "Cycle companion mood");

        // Naming must not break the press behavior (stage mood cycle).
        let before = rt.state.stage_mood;
        let (ax, ay) = rt.find("sidebar-avatar-hit").unwrap();
        rt.pointer_down(ax, ay);
        rt.pointer_up(ax, ay);
        rt.frame(0.016);
        assert_ne!(rt.state.stage_mood, before);
    }

    /// WCAG ratio of a text element against its painter-order composited
    /// background — the same model as voxcell-uphill `audit_contrast`:
    /// theme colors are gamma-encoded sRGB consumed verbatim, ancestor
    /// opacities fold into the text alpha, and every fill drawn BEFORE the
    /// text whose visible box covers its center composites over the clear
    /// color (captures sibling backdrops an ancestor walk would miss).
    fn status_contrast(rt: &Runtime<AppState>, idx: usize) -> f64 {
        let elements = rt.elements();
        let e = &elements[idx];
        let tc = e.text_color.expect("status label has a text color");
        let mut eff_opacity = e.opacity;
        let mut cur = e.parent;
        while let Some(p) = cur {
            eff_opacity *= elements[p].opacity;
            cur = elements[p].parent;
        }
        let clear = rt.clear_color();
        let cx = e.rect[0] + e.rect[2] * 0.5;
        let cy = e.rect[1] + e.rect[3] * 0.5;
        let mut bg = [clear[0], clear[1], clear[2], 1.0];
        for b in &elements[..idx] {
            if b.fill.a <= 0.001 || b.exiting || b.opacity < 0.05 || b.virtual_extent {
                continue;
            }
            let v = b.visible;
            if v[2] <= 0.0 || v[3] <= 0.0 {
                continue;
            }
            if cx >= v[0] && cx <= v[0] + v[2] && cy >= v[1] && cy <= v[1] + v[3] {
                let a = (b.fill.a * b.opacity).clamp(0.0, 1.0);
                bg = [
                    b.fill.r * a + bg[0] * (1.0 - a),
                    b.fill.g * a + bg[1] * (1.0 - a),
                    b.fill.b * a + bg[2] * (1.0 - a),
                    1.0,
                ];
            }
        }
        let a = (tc.a * eff_opacity).clamp(0.0, 1.0);
        let fg = [
            tc.r * a + bg[0] * (1.0 - a),
            tc.g * a + bg[1] * (1.0 - a),
            tc.b * a + bg[2] * (1.0 - a),
        ];
        let lum = |c: [f32; 3]| {
            0.2126 * c[0].clamp(0.0, 1.0) as f64
                + 0.7152 * c[1].clamp(0.0, 1.0) as f64
                + 0.0722 * c[2].clamp(0.0, 1.0) as f64
        };
        let (l1, l2) = (lum(fg), lum([bg[0], bg[1], bg[2]]));
        let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Assert the companion status label uses the bright slate token
    /// (accent.soft #CBD5E1, not the semantic tone color) and clears WCAG 3.0
    /// against its composited chip background. Settle first: mid-entrance
    /// fades read as false low contrast (uphill harness lesson).
    fn assert_status_label_contrast(rt: &mut Runtime<AppState>, label: &str) {
        rt.invalidate();
        rt.frame(0.016);
        rt.settle_all();
        rt.frame(0.016);
        let theme = VoxcellTheme::from_config(&rt.state.ui_config);
        let elements = rt.elements();
        let idx = elements
            .iter()
            .position(|e| e.text.as_deref() == Some(label) && !e.exiting)
            .unwrap_or_else(|| panic!("status label {label:?} not rendered"));
        let tc = elements[idx].text_color.expect("status label text color");
        let want = theme.accent_soft;
        for (got, exp) in [(tc.r, want.r), (tc.g, want.g), (tc.b, want.b), (tc.a, want.a)] {
            assert!(
                (got - exp).abs() < 1e-3,
                "status label {label:?} must use the accent.soft token: got {got}, want {exp}"
            );
        }
        let ratio = status_contrast(rt, idx);
        assert!(ratio >= 3.0, "status label {label:?} contrast {ratio:.2} < 3.0");
    }

    #[test]
    fn sidebar_status_labels_use_bright_slate_token_above_wcag_3() {
        // Slate tone (idle gateway): "Online".
        let mut rt = boot("idle");
        assert_status_label_contrast(&mut rt, "Online");

        // Amber tone (running task): "Working".
        let mut rt = boot("idle");
        rt.state.tasks.push(mk_task("t-a11y", "home", "Contrast probe task"));
        assert_status_label_contrast(&mut rt, "Working");

        // Rose tone (pending approval): "Approval needed".
        let mut rt = boot("attention");
        assert_status_label_contrast(&mut rt, "Approval needed");

        // Emerald tone (voice live): "Speaking" and "Listening".
        let mut rt = boot("idle");
        rt.state.audio.is_speaking = true;
        assert_status_label_contrast(&mut rt, "Speaking");
        let mut rt = boot("idle");
        rt.state.audio.is_listening = true;
        assert_status_label_contrast(&mut rt, "Listening");
    }
}
