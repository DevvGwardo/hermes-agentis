//! P3-05 Queue // Operations — web parity with `src/components/HermesOperationsPanel.tsx`.
//!
//! Layout: gateway health strip -> active-run stop/steer rail -> underline tabs
//! (Gateway & Tools / Approvals / Jobs) -> scrollable tab body.
//! - Parallel Profiles: redacted `HermesProfileSummary` health cards (boolean
//!   `has_api_key` only — raw bearer tokens never reach this view) + default
//!   switcher dispatching `UiCommand::SwitchProfile`.
//! - Approvals/Clarify action cards: run/req/gen meta, status badges, command
//!   preview, once/always/deny choices and free-text clarify replies, all routed
//!   through `AppState::resolve_approval` -> `UiCommand::ResolveApproval`
//!   (carries `request_id` + `execution_generation`).
//! - Skills / Toolsets / Cron manager lists from AppState gateway data; cron
//!   actions dispatch `UiCommand::ManageCronJob`.
//! - Stop/steer rail uses the existing `UiCommand::StopRun` / `SteerRun`.
//!
//! slate-v1 tokens only; amber = running/thinking/pending gates, rose =
//! errors/offline, emerald (`semantic_live`) = live/connected/done telemetry.

use crate::state::{AppState, UiCommand};
use crate::theme::{FONT_UI, VoxcellTheme};
use rune::{Color, Cx, El, ScrollSpec, TextStyle, col, edit_access, el, row, text, text_input};
use rune_kit::{Button, ButtonVariant, ControlSize, Divider, Kit};
use voxcell_core::{ApprovalKind, GatewayTransport};
use voxcell_hermes::profiles::{HermesProfileMode, HermesProfileSource};

/// Section title weight on the UI font stack (slate-v1 §6.3).
fn section_font() -> TextStyle {
    TextStyle::new(13.0, 700.0).family(rune::Family::Named(FONT_UI))
}

fn card_title_font() -> TextStyle {
    TextStyle::new(13.0, 600.0).family(rune::Family::Named(FONT_UI))
}

/// Semantic status pulse dot (live/working/error telemetry only).
fn pulse_dot(color: Color) -> El {
    el().w(8.0).h(8.0).radius(4.0).fill(color)
}

fn section_header(theme: &VoxcellTheme, title: &str, meta: &str) -> El {
    row()
        .w_full()
        .justify_between()
        .items_center()
        .gap(8.0)
        .child(text(title.to_string()).font(section_font()).color(theme.text))
        .child(text(meta.to_string()).font(theme.mono_sm).color(theme.text_faint))
}

fn empty_note(theme: &VoxcellTheme, msg: &str) -> El {
    el().w_full()
        .padding(12.0)
        .radius(theme.radius_control)
        .fill(theme.bg.alpha(0.45))
        .border(0.75, theme.border_subtle)
        .child(text(msg.to_string()).font(theme.mono_sm).color(theme.text_faint))
}

fn telemetry_cell(theme: &VoxcellTheme, label: &str, value: &str, ink: Color) -> El {
    col()
        .grow(1.0)
        .min_w(0.0)
        .gap(2.0)
        .child(text(label.to_string()).font(theme.mono_sm).color(theme.text_faint))
        .child(text(value.to_string()).font(theme.mono_sm).color(ink))
}

fn transport_label(t: GatewayTransport) -> &'static str {
    match t {
        GatewayTransport::Runs => "/v1/runs",
        GatewayTransport::SessionStream => "/sessions/stream",
        GatewayTransport::ChatCompletions => "/v1/chat/completions",
    }
}

// ---------------------------------------------------------------------------
// Gateway health strip (web: Gateway Status & Route + telemetry row).
// ---------------------------------------------------------------------------
fn gateway_card(state: &AppState, theme: &VoxcellTheme, k: &Kit) -> El {
    let online = state.gateway.online;
    let status_ink = if online { theme.semantic_live } else { theme.semantic_error };
    let active_route = state
        .queue_profile_summaries()
        .into_iter()
        .find(|p| p.is_default)
        .map(|p| format!("{}{}", p.base_url, p.route_prefix))
        .unwrap_or_else(|| state.gateway.base_url.clone());
    let endpoint_display = active_route
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_string();

    let mut header = row()
        .w_full()
        .justify_between()
        .items_center()
        .gap(8.0)
        .child(
            row()
                .gap(8.0)
                .items_center()
                .min_w(0.0)
                .child(pulse_dot(status_ink))
                .child(
                    text("Hermes Agent API Server")
                        .font(card_title_font())
                        .color(theme.text),
                )
                .child(
                    text(if online { "Connected" } else { "Offline" })
                        .font(theme.mono_sm)
                        .color(status_ink),
                ),
        );
    if !online {
        if let Some(err) = &state.gateway.last_error {
            header = header.child(
                text(err.clone())
                    .font(theme.mono_sm)
                    .color(theme.semantic_error.alpha(0.8)),
            );
        }
    }
    header = header.child(
        Button::new(if online { "Refresh" } else { "Retry" })
            .variant(ButtonVariant::Neutral)
            .control_size(ControlSize::Small)
            .on_press(|s: &mut AppState| {
                s.pending_commands.push(UiCommand::RefreshGateway);
            })
            .build(k),
    );

    let caps = &state.gateway.capabilities;
    let mut chips: Vec<&str> = Vec::new();
    if caps.runs {
        chips.push("runs");
    }
    if caps.approvals {
        chips.push("approvals");
    }
    if caps.clarify {
        chips.push("clarify");
    }
    if caps.steering {
        chips.push("steering");
    }
    if caps.session_stream {
        chips.push("session_stream");
    }
    if caps.skills {
        chips.push("skills");
    }
    if caps.toolsets {
        chips.push("toolsets");
    }
    if caps.cron_jobs {
        chips.push("cron");
    }

    col()
        .key("queue-gateway-card")
        .w_full()
        .padding(12.0)
        .gap(8.0)
        .radius(theme.radius_card)
        .fill(theme.card_fill)
        .border(1.0, theme.border)
        .child(header)
        .child(
            row()
                .w_full()
                .gap(12.0)
                .padding(6.0)
                .border(0.75, theme.border_subtle)
                .child(telemetry_cell(theme, "Endpoint", &endpoint_display, theme.text))
                .child(telemetry_cell(theme, "Model", &state.gateway.model, theme.accent_soft))
                .child(telemetry_cell(
                    theme,
                    "Profile",
                    &format!("@{}", state.gateway.active_profile),
                    theme.text,
                ))
                .child(telemetry_cell(
                    theme,
                    "Transport",
                    transport_label(state.gateway.active_transport),
                    theme.text,
                )),
        )
        .child(
            text(format!("Capabilities: {}", chips.join(" · ")))
                .font(theme.mono_sm)
                .color(theme.text_muted),
        )
}

// ---------------------------------------------------------------------------
// Active runs rail: stop/steer via existing UiCommands.
// ---------------------------------------------------------------------------
fn active_runs_section(state: &AppState, theme: &VoxcellTheme, k: &Kit) -> Option<El> {
    if state.active_run_ids.is_empty() {
        return None;
    }
    let mut runs: Vec<(&String, &String)> = state.active_run_ids.iter().collect();
    runs.sort_by(|a, b| a.0.cmp(b.0));

    let mut sec = col()
        .key("queue-active-runs")
        .w_full()
        .gap(6.0)
        .child(section_header(
            theme,
            &format!("Active Runs ({})", runs.len()),
            "stop / steer",
        ));

    for (ch_id, run_id) in runs {
        let task = state
            .tasks
            .iter()
            .rev()
            .find(|t| &t.channel_id == ch_id && t.status == voxcell_core::TaskLifecycleStatus::Running);
        let title = task
            .map(|t| t.title.clone())
            .or_else(|| {
                state
                    .channels
                    .iter()
                    .find(|c| &c.id == ch_id)
                    .map(|c| c.name.clone())
            })
            .unwrap_or_else(|| ch_id.clone());
        let progress = task
            .and_then(|t| t.progress_label.clone())
            .unwrap_or_else(|| "live".to_string());
        let thinking = state
            .thinking_by_channel
            .get(ch_id)
            .copied()
            .unwrap_or(false);

        let ch_stop = ch_id.clone();
        let run_steer = run_id.clone();
        let mut actions = row().gap(6.0).items_center().child(
            Button::new("Stop")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut AppState| s.queue_stop_run(&ch_stop))
                .build(k),
        );
        if state.gateway.capabilities.steering {
            actions = actions.child(
                Button::new("Steer")
                    .variant(ButtonVariant::Neutral)
                    .control_size(ControlSize::Small)
                    .on_press(move |s: &mut AppState| {
                        s.queue_steer_run = Some(run_steer.clone());
                    })
                    .build(k),
            );
        }

        let mut run_col = col()
            .key(format!("queue-run-{ch_id}"))
            .w_full()
            .padding(10.0)
            .gap(6.0)
            .radius(theme.radius_control)
            .fill(theme.card_fill)
            .border(0.75, theme.border_subtle)
            .child(
                row()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .gap(8.0)
                    .child(
                        col()
                            .gap(2.0)
                            .grow(1.0)
                            .min_w(0.0)
                            .child(text(title).font(card_title_font()).color(theme.text))
                            .child(
                                text(format!("run:{run_id} · #{ch_id} · {progress}"))
                                    .font(theme.mono_sm)
                                    .color(if thinking {
                                        theme.semantic_working
                                    } else {
                                        theme.text_muted
                                    }),
                            ),
                    )
                    .child(actions),
            );

        if state.queue_steer_run.as_deref() == Some(run_id.as_str()) {
            run_col = run_col.child(
                row()
                    .key("queue-steer-composer")
                    .w_full()
                    .gap(8.0)
                    .items_center()
                    .child(
                        el().grow(1.0)
                            .min_w(0.0)
                            .padding(8.0)
                            .radius(theme.radius_control)
                            .fill(theme.bg.alpha(0.35))
                            .border(0.75, theme.border)
                            .child(
                                text_input(&state.queue_steer_edit, theme.mono_sm)
                                    .key("queue-steer-input")
                                    .single_line()
                                    .placeholder("Steer this run...", theme.text_faint)
                                    .on_edit(|s: &mut AppState| &mut s.queue_steer_edit)
                                    .on_submit(|s: &mut AppState, _| s.queue_send_steer())
                                    .w_full(),
                            ),
                    )
                    .child(
                        Button::new("Send Steer")
                            .variant(ButtonVariant::Submit)
                            .control_size(ControlSize::Small)
                            .on_press(|s: &mut AppState| s.queue_send_steer())
                            .build(k),
                    ),
            );
        }
        sec = sec.child(run_col);
    }
    Some(sec)
}

// ---------------------------------------------------------------------------
// Underline tab strip (web MorphTabs: runtime / approvals / jobs).
// ---------------------------------------------------------------------------
fn tabs_row(state: &AppState, _theme: &VoxcellTheme, k: &Kit) -> El {
    let active = state.queue_active_tab().to_string();
    let pending = state.approvals.iter().filter(|a| !a.resolved).count();
    let tabs = [
        ("runtime", "Gateway & Tools".to_string()),
        ("approvals", format!("Approvals ({pending})")),
        ("jobs", format!("Jobs ({})", state.cron_jobs.len())),
    ];
    let mut r = row().key("queue-tabs").w_full().gap(4.0).items_center();
    for (id, label) in tabs {
        let is_active = active == id;
        let idc = id.to_string();
        r = r.child(
            el().key(format!("queue-tab-{id}")).child(
                Button::new(label)
                    .variant(if is_active {
                        ButtonVariant::Submit
                    } else {
                        ButtonVariant::Ghost
                    })
                    .control_size(ControlSize::Small)
                    .on_press(move |s: &mut AppState| {
                        s.queue_tab = Some(idc.clone());
                    })
                    .build(k),
            ),
        );
    }
    r
}

// ---------------------------------------------------------------------------
// Runtime tab: Parallel Profiles + Toolsets + Skills + memory scoping note.
// ---------------------------------------------------------------------------
fn runtime_tab(state: &AppState, theme: &VoxcellTheme, k: &Kit) -> El {
    let summaries = state.queue_profile_summaries();
    let mut profiles = col()
        .key("queue-profiles")
        .w_full()
        .gap(6.0)
        .child(section_header(
            theme,
            "Parallel Profiles",
            "isolated memories & tools per profile",
        ));
    if summaries.is_empty() {
        profiles = profiles.child(empty_note(theme, "No gateway profiles discovered."));
    }
    for p in &summaries {
        let connected = p.connected.unwrap_or(false);
        let lanes = state.queue_profile_lanes(&p.id);
        let mode_label = match p.mode {
            HermesProfileMode::Standalone => "standalone".to_string(),
            HermesProfileMode::Multiplexed => {
                if p.route_prefix.is_empty() {
                    "/".to_string()
                } else {
                    p.route_prefix.clone()
                }
            }
        };
        let route_label = match p.mode {
            HermesProfileMode::Standalone => p.base_url.clone(),
            HermesProfileMode::Multiplexed => format!("{}{}", p.base_url, p.route_prefix),
        };
        let source_label = match p.source {
            HermesProfileSource::Custom => "custom",
            HermesProfileSource::Env => "env",
            HermesProfileSource::Discovered => "discovered",
        };
        // Redacted key indicator: boolean presence only, never the key itself.
        let key_label = if p.has_api_key { "key: set" } else { "key: inherit" };

        let mut meta = row()
            .gap(6.0)
            .items_center()
            .child(pulse_dot(if connected {
                theme.semantic_live
            } else {
                theme.text_faint
            }))
            .child(
                text(format!("@{}", p.id))
                    .font(theme.mono_sm)
                    .color(theme.text),
            )
            .child(text(mode_label).font(theme.mono_sm).color(theme.text_faint));
        if let Some(model) = &p.active_model {
            meta = meta.child(
                text(format!("· {model}"))
                    .font(theme.mono_sm)
                    .color(theme.accent_soft),
            );
        }
        if lanes > 0 {
            meta = meta.child(
                text(format!(
                    "· {lanes} {}",
                    if lanes == 1 { "lane" } else { "lanes" }
                ))
                .font(theme.mono_sm)
                .color(theme.text_muted),
            );
        }

        let name = p.id.clone();
        profiles = profiles.child(
            row()
                .key(format!("queue-profile-{}", p.id))
                .w_full()
                .padding(10.0)
                .gap(8.0)
                .radius(theme.radius_control)
                .fill(theme.card_fill)
                .border(
                    0.75,
                    if p.is_default {
                        theme.border_strong
                    } else {
                        theme.border_subtle
                    },
                )
                .justify_between()
                .items_center()
                .child(
                    col()
                        .gap(2.0)
                        .grow(1.0)
                        .min_w(0.0)
                        .child(meta)
                        .child(
                            text(format!("{route_label} · {source_label} · {key_label}"))
                                .font(theme.mono_sm)
                                .color(theme.text_faint),
                        ),
                )
                .child(
                    el().key(format!("queue-profile-use-{}", p.id)).child(
                        Button::new(if p.is_default { "Default" } else { "Use default" })
                            .variant(if p.is_default {
                                ButtonVariant::Submit
                            } else {
                                ButtonVariant::Neutral
                            })
                            .control_size(ControlSize::Small)
                            .on_press(move |s: &mut AppState| s.queue_switch_profile(&name))
                            .build(k),
                    ),
                ),
        );
    }

    // Toolsets (GET /v1/toolsets) — expandable tool lists.
    let mut toolsets = col()
        .key("queue-toolsets")
        .w_full()
        .gap(6.0)
        .child(section_header(
            theme,
            "Toolsets",
            &format!("{} reported · GET /v1/toolsets", state.toolsets.len()),
        ));
    if state.toolsets.is_empty() {
        toolsets = toolsets.child(empty_note(
            theme,
            if state.gateway.online {
                "0 toolsets returned by gateway."
            } else {
                "Offline — start hermes gateway to inspect toolsets."
            },
        ));
    }
    for ts in &state.toolsets {
        let expanded = state.queue_expanded_toolset.as_deref() == Some(ts.name.as_str());
        let tname = ts.name.clone();
        let header = row()
            .key(format!("queue-toolset-{}", ts.name))
            .w_full()
            .justify_between()
            .items_center()
            .gap(8.0)
            .on_press(move |s: &mut AppState| {
                s.queue_expanded_toolset =
                    if s.queue_expanded_toolset.as_deref() == Some(tname.as_str()) {
                        None
                    } else {
                        Some(tname.clone())
                    };
            })
            .child(
                text(ts.name.clone())
                    .font(theme.mono_sm)
                    .color(if ts.enabled {
                        theme.accent_soft
                    } else {
                        theme.text_faint
                    }),
            )
            .child(
                text(format!(
                    "{} · {} tools",
                    if ts.enabled { "enabled" } else { "disabled" },
                    ts.tools.len()
                ))
                .font(theme.mono_sm)
                .color(theme.text_muted),
            );
        let mut card = col()
            .w_full()
            .gap(2.0)
            .padding(8.0)
            .radius(theme.radius_control)
            .fill(theme.card_fill)
            .border(0.5, theme.border_subtle)
            .child(header);
        if !ts.description.is_empty() {
            card = card.child(
                text(ts.description.clone())
                    .font(theme.ui_sm)
                    .color(theme.text_muted),
            );
        }
        if expanded && !ts.tools.is_empty() {
            card = card.child(
                el().key(format!("queue-toolset-tools-{}", ts.name))
                    .w_full()
                    .padding(6.0)
                    .radius(6.0)
                    .fill(theme.bg.alpha(0.3))
                    .child(text(ts.tools.join(", ")).font(theme.mono_sm).color(theme.text)),
            );
        }
        toolsets = toolsets.child(card);
    }

    // Skills (GET /v1/skills).
    let mut skills = col()
        .key("queue-skills")
        .w_full()
        .gap(6.0)
        .child(section_header(
            theme,
            "Skills",
            &format!("{} loaded · GET /v1/skills", state.skills.len()),
        ));
    if state.skills.is_empty() {
        skills = skills.child(empty_note(
            theme,
            if state.gateway.online {
                "0 skills returned by gateway."
            } else {
                "Offline — start hermes gateway to inspect /v1/skills."
            },
        ));
    }
    for sk in &state.skills {
        skills = skills.child(
            row()
                .key(format!("queue-skill-{}", sk.name))
                .w_full()
                .padding(8.0)
                .gap(8.0)
                .radius(theme.radius_control)
                .fill(theme.card_fill)
                .border(0.5, theme.border_subtle)
                .justify_between()
                .items_center()
                .child(
                    col()
                        .gap(2.0)
                        .grow(1.0)
                        .min_w(0.0)
                        .child(
                            text(sk.name.clone())
                                .font(theme.mono_sm)
                                .color(theme.accent_soft),
                        )
                        .child(
                            text(sk.description.clone())
                                .font(theme.ui_sm)
                                .color(theme.text_muted),
                        ),
                )
                .child(
                    col()
                        .gap(2.0)
                        .items_end()
                        .child(
                            text(sk.category.clone().unwrap_or_else(|| "gateway".to_string()))
                                .font(theme.mono_sm)
                                .color(theme.text_faint),
                        )
                        .child(
                            text(if sk.enabled { "enabled" } else { "disabled" })
                                .font(theme.mono_sm)
                                .color(if sk.enabled {
                                    theme.semantic_live
                                } else {
                                    theme.text_faint
                                }),
                        ),
                ),
        );
    }

    let memory_note = col()
        .key("queue-memory-note")
        .w_full()
        .gap(2.0)
        .child(
            text("Memory & Session Continuity")
                .font(card_title_font())
                .color(theme.text),
        )
        .child(
            text(
                "Transcript continuity uses X-Hermes-Session-Id; isolated long-term memory scoping sends X-Hermes-Session-Key per lane. Hermes API Server does not expose a separate /v1/memory endpoint.",
            )
            .font(theme.mono_sm)
            .color(theme.text_muted),
        );

    col()
        .key("queue-runtime-tab")
        .w_full()
        .gap(14.0)
        .child(profiles)
        .child(Divider::horizontal().build(k))
        .child(toolsets)
        .child(Divider::horizontal().build(k))
        .child(skills)
        .child(memory_note)
}

// ---------------------------------------------------------------------------
// Approvals tab: action cards (pending gates + resolved history).
// ---------------------------------------------------------------------------
fn approvals_tab(state: &AppState, theme: &VoxcellTheme, k: &Kit) -> El {
    let pending = state.approvals.iter().filter(|a| !a.resolved).count();
    let mut sec = col()
        .key("queue-approvals")
        .w_full()
        .gap(8.0)
        .child(section_header(
            theme,
            &format!("Approvals & Clarifications ({pending})"),
            "pending_controls · /v1/runs/{id}/events",
        ));
    if state.approvals.is_empty() {
        return sec.child(empty_note(
            theme,
            "No pending run approvals. Execution gates from /v1/runs/{id}/events and pending_controls appear here when a run requests confirmation.",
        ));
    }

    for req in &state.approvals {
        let is_clarify = req.kind == ApprovalKind::Clarify;
        let status = if req.resolved {
            if req.resolution.as_deref() == Some("deny") {
                "rejected"
            } else {
                "approved"
            }
        } else {
            "pending"
        };
        let status_ink = match status {
            "pending" => theme.semantic_working,
            "approved" => theme.accent_soft,
            _ => theme.semantic_error,
        };
        let (fill, border) = if req.resolved {
            (theme.card_fill, theme.border_subtle)
        } else {
            (
                theme.semantic_working.alpha(0.09),
                theme.semantic_working.alpha(0.4),
            )
        };

        let mut meta = format!(
            "run:{} · req:{} · gen:{} · #{}",
            req.run_id.as_deref().unwrap_or(&req.id),
            req.request_id,
            req.execution_generation,
            req.channel_id
        );
        if let Some(tool) = &req.tool_name {
            meta.push_str(&format!(" · {tool}"));
        }

        let mut badge = row().gap(6.0).items_center();
        if !req.resolved {
            badge = badge.child(pulse_dot(theme.semantic_working));
        }
        badge = badge.child(
            text(status.to_uppercase())
                .font(theme.mono_sm)
                .color(status_ink),
        );

        let mut card = col()
            .key(format!("queue-apr-{}", req.id))
            .w_full()
            .padding(12.0)
            .gap(6.0)
            .radius(theme.radius_card)
            .fill(fill)
            .border(1.0, border)
            .child(
                row()
                    .w_full()
                    .justify_between()
                    .items_start()
                    .gap(8.0)
                    .child(
                        col()
                            .gap(2.0)
                            .grow(1.0)
                            .min_w(0.0)
                            .child(text(meta).font(theme.mono_sm).color(theme.text_faint))
                            .child(text(req.title.clone()).font(card_title_font()).color(theme.text)),
                    )
                    .child(badge),
            );
        if !req.summary.is_empty() {
            card = card.child(text(req.summary.clone()).font(theme.ui_sm).color(theme.text));
        }
        if !req.command.is_empty() {
            card = card.child(
                el().key(format!("queue-apr-cmd-{}", req.id))
                    .w_full()
                    .padding(8.0)
                    .radius(8.0)
                    .fill(theme.bg.alpha(0.45))
                    .border(0.75, theme.border_subtle)
                    .child(
                        text(req.command.clone())
                            .font(theme.mono_sm)
                            .color(theme.accent_soft),
                    ),
            );
        }

        if !req.resolved {
            if is_clarify {
                let edit = state
                    .queue_clarify_edits
                    .get(&req.id)
                    .cloned()
                    .unwrap_or_default();
                let cid_edit = req.id.clone();
                let cid_submit = req.id.clone();
                let cid_btn = req.id.clone();
                card = card.child(
                    row()
                        .key(format!("queue-clarify-row-{}", req.id))
                        .w_full()
                        .gap(8.0)
                        .items_center()
                        .child(
                            el().grow(1.0)
                                .min_w(0.0)
                                .padding(8.0)
                                .radius(theme.radius_control)
                                .fill(theme.bg.alpha(0.35))
                                .border(0.75, theme.border)
                                .child(
                                    text_input(&edit, theme.mono_sm)
                                        .key(format!("queue-clarify-{}", req.id))
                                        .single_line()
                                        .placeholder(
                                            "Enter clarification answer...",
                                            theme.text_faint,
                                        )
                                        .on_edit(edit_access(move |s: &mut AppState| {
                                            s.queue_clarify_edits
                                                .entry(cid_edit.clone())
                                                .or_default()
                                        }))
                                        .on_submit(move |s: &mut AppState, _| {
                                            s.queue_reply_clarify(&cid_submit);
                                        })
                                        .w_full(),
                                ),
                        )
                        .child(
                            Button::new("Reply")
                                .variant(ButtonVariant::Submit)
                                .control_size(ControlSize::Small)
                                .on_press(move |s: &mut AppState| {
                                    s.queue_reply_clarify(&cid_btn);
                                })
                                .build(k),
                        ),
                );
            } else {
                let choices = if req.choices.is_empty() {
                    vec!["once".to_string(), "always".to_string(), "deny".to_string()]
                } else {
                    req.choices.clone()
                };
                let mut actions = row().key(format!("queue-apr-actions-{}", req.id)).gap(8.0);
                for choice in choices {
                    let (label, variant) = match choice.as_str() {
                        "once" => ("Approve Once", ButtonVariant::Submit),
                        "always" => ("Always Allow", ButtonVariant::Neutral),
                        "deny" => ("Deny", ButtonVariant::Ghost),
                        other => (other, ButtonVariant::Neutral),
                    };
                    let aid = req.id.clone();
                    let c = choice.clone();
                    actions = actions.child(
                        Button::new(label)
                            .variant(variant)
                            .control_size(ControlSize::Small)
                            .on_press(move |s: &mut AppState| s.resolve_approval(&aid, &c))
                            .build(k),
                    );
                }
                card = card.child(actions);
            }
        } else if let Some(resolution) = &req.resolution {
            card = card.child(
                text(format!("resolved: {resolution}"))
                    .font(theme.mono_sm)
                    .color(status_ink),
            );
        }
        sec = sec.child(card);
    }
    sec
}

// ---------------------------------------------------------------------------
// Jobs tab: cron manager (GET /api/jobs) with run/pause/resume/delete.
// ---------------------------------------------------------------------------
fn jobs_tab(state: &AppState, theme: &VoxcellTheme, k: &Kit) -> El {
    let mut sec = col()
        .key("queue-cron")
        .w_full()
        .gap(8.0)
        .child(
            row()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    text(format!("Scheduled Jobs ({})", state.cron_jobs.len()))
                        .font(section_font())
                        .color(theme.text),
                )
                .child(
                    Button::new("Refresh")
                        .variant(ButtonVariant::Neutral)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut AppState| {
                            s.pending_commands.push(UiCommand::RefreshGateway);
                        })
                        .build(k),
                ),
        );
    if state.cron_jobs.is_empty() {
        return sec.child(empty_note(
            theme,
            if state.gateway.online {
                "No scheduled jobs on gateway. Live jobs from GET /api/jobs appear here."
            } else {
                "Gateway offline — connect to the Hermes Agent API Server to manage /api/jobs."
            },
        ));
    }

    for job in &state.cron_jobs {
        let paused = !job.enabled || job.last_status.as_deref() == Some("paused");
        let failed = matches!(job.last_status.as_deref(), Some("error") | Some("failed"));
        let status_label = job
            .last_status
            .clone()
            .unwrap_or_else(|| if paused { "paused" } else { "active" }.to_string());
        let status_ink = if failed {
            theme.semantic_error
        } else if paused {
            theme.semantic_working
        } else {
            theme.accent_soft
        };

        let jid_run = job.id.clone();
        let jid_toggle = job.id.clone();
        let jid_del = job.id.clone();
        let next_act = if paused { "resume" } else { "pause" };
        let next_label = if paused { "Resume" } else { "Pause" };

        sec = sec.child(
            row()
                .key(format!("cron-{}", job.id))
                .w_full()
                .padding(10.0)
                .gap(8.0)
                .radius(theme.radius_control)
                .fill(theme.card_fill)
                .border(0.75, theme.border_subtle)
                .justify_between()
                .items_center()
                .child(
                    col()
                        .gap(2.0)
                        .grow(1.0)
                        .min_w(0.0)
                        .child(
                            row()
                                .gap(6.0)
                                .items_center()
                                .child(
                                    text(job.schedule.clone())
                                        .font(theme.mono_sm)
                                        .color(theme.accent_soft),
                                )
                                .child(text("·").font(theme.mono_sm).color(theme.text_faint))
                                .child(text(status_label).font(theme.mono_sm).color(status_ink))
                                .child(text("·").font(theme.mono_sm).color(theme.text_faint))
                                .child(text(job.id.clone()).font(theme.mono_sm).color(theme.text_faint)),
                        )
                        .child(
                            text(format!("{} — {}", job.name, job.prompt))
                                .font(theme.ui_sm)
                                .color(theme.text),
                        )
                        .child(
                            text(format!(
                                "last={} · next={}",
                                job.last_run_at.as_deref().unwrap_or("-"),
                                job.next_run_at.as_deref().unwrap_or("-")
                            ))
                            .font(theme.mono_sm)
                            .color(theme.text_faint),
                        ),
                )
                .child(
                    row()
                        .gap(6.0)
                        .child(
                            Button::new("Run Now")
                                .variant(ButtonVariant::Neutral)
                                .control_size(ControlSize::Mini)
                                .on_press(move |s: &mut AppState| {
                                    s.pending_commands.push(UiCommand::ManageCronJob {
                                        job_id: jid_run.clone(),
                                        action: "run".into(),
                                    });
                                })
                                .build(k),
                        )
                        .child(
                            Button::new(next_label)
                                .variant(ButtonVariant::Ghost)
                                .control_size(ControlSize::Mini)
                                .on_press(move |s: &mut AppState| {
                                    if let Some(j) =
                                        s.cron_jobs.iter_mut().find(|c| c.id == jid_toggle)
                                    {
                                        j.enabled = !j.enabled;
                                    }
                                    s.pending_commands.push(UiCommand::ManageCronJob {
                                        job_id: jid_toggle.clone(),
                                        action: next_act.into(),
                                    });
                                })
                                .build(k),
                        )
                        .child(
                            Button::new("Delete")
                                .variant(ButtonVariant::Ghost)
                                .control_size(ControlSize::Mini)
                                .on_press(move |s: &mut AppState| {
                                    s.cron_jobs.retain(|c| c.id != jid_del);
                                    s.pending_commands.push(UiCommand::ManageCronJob {
                                        job_id: jid_del.clone(),
                                        action: "delete".into(),
                                    });
                                })
                                .build(k),
                        ),
                ),
        );
    }
    sec
}

// ---------------------------------------------------------------------------
// View entry point.
// ---------------------------------------------------------------------------
pub fn render_queue(state: &AppState, theme: &VoxcellTheme, _cx: &mut Cx<AppState>) -> El {
    let k = &theme.kit;
    let tab = state.queue_active_tab().to_string();
    let body = match tab.as_str() {
        "approvals" => approvals_tab(state, theme, k),
        "jobs" => jobs_tab(state, theme, k),
        _ => runtime_tab(state, theme, k),
    };

    let mut root = col()
        .key("queue-view")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .h_full()
        .padding(14.0)
        .gap(12.0)
        .child(gateway_card(state, theme, k));
    if let Some(runs) = active_runs_section(state, theme, k) {
        root = root.child(runs);
    }
    root.child(tabs_row(state, theme, k)).child(
        col()
            .key("queue-scroll")
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .scroll(ScrollSpec { stick_bottom: false })
            .child(body),
    )
}

// ---------------------------------------------------------------------------
// P3-05 headless tests (Runtime pattern from app/main.rs + sidebar.rs tests).
// Kept in this file per ownership map (app/main.rs belongs to theme-chrome).
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_main_window;
    use rune::{Env, Runtime};
    use voxcell_core::{
        ApprovalKind, BackgroundTask, ChannelApprovalRequest, HermesCronJob, HermesSkill,
        HermesToolset, TaskLifecycleStatus, TaskStepItem,
    };

    fn boot(scene: &str) -> Runtime<AppState> {
        let mut s = AppState::new();
        s.apply_scene_preset(scene);
        boot_state(s)
    }

    fn boot_state(s: AppState) -> Runtime<AppState> {
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);
        rt
    }

    /// Direct state injection happens outside a handler — mark the view dirty.
    fn refresh(rt: &mut Runtime<AppState>) {
        rt.invalidate();
        rt.frame(0.016);
    }

    fn has_text(rt: &Runtime<AppState>, label: &str) -> bool {
        rt.elements().iter().any(|e| e.text.as_deref() == Some(label))
    }

    fn click_at(rt: &mut Runtime<AppState>, x: f32, y: f32) {
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.016);
    }

    fn click_key(rt: &mut Runtime<AppState>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("key {key:?} not found"));
        click_at(rt, x, y);
    }

    /// Click a button by exact text, scoped to the queue view rect so sidebar
    /// or titlebar duplicates can never steal the click.
    fn click_in_queue(rt: &mut Runtime<AppState>, label: &str) {
        let view = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("queue-view"))
            .expect("queue-view mounted");
        let [vx, vy, vw, vh] = view.target;
        let el = rt
            .elements()
            .into_iter()
            .filter(|e| e.text.as_deref() == Some(label))
            .find(|e| {
                let cx = e.target[0] + e.target[2] * 0.5;
                let cy = e.target[1] + e.target[3] * 0.5;
                cx >= vx && cx <= vx + vw && cy >= vy && cy <= vy + vh
            })
            .unwrap_or_else(|| panic!("button {label:?} not found inside queue view"));
        let x = el.target[0] + el.target[2] * 0.5;
        let y = el.target[1] + el.target[3] * 0.5;
        click_at(rt, x, y);
    }

    fn mk_approval(id: &str, request_id: &str, generation: u32, kind: ApprovalKind) -> ChannelApprovalRequest {
        ChannelApprovalRequest {
            id: id.into(),
            channel_id: "home".into(),
            run_id: Some(format!("run-{request_id}")),
            request_id: request_id.into(),
            execution_generation: generation,
            kind,
            title: "Execute Shell Command".into(),
            tool_name: Some("terminal".into()),
            command: "cargo test --workspace".into(),
            summary: "Run the full desktop test suite".into(),
            destination: "default".into(),
            choices: vec!["once".into(), "always".into(), "deny".into()],
            created_at: "now".into(),
            resolved: false,
            resolution: None,
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
            last_step: Some("Running tests".into()),
            steps: vec![TaskStepItem {
                text: "Running tests".into(),
                done: false,
                active: true,
            }],
            result: None,
            error_code: None,
            created_at_ms: Some(1_000),
            updated_at_ms: Some(9_000),
            agent_done_at_ms: None,
            confirmed_at_ms: None,
            updated_at: "live".into(),
        }
    }

    fn resolve_commands(rt: &Runtime<AppState>) -> Vec<&UiCommand> {
        rt.state
            .pending_commands
            .iter()
            .filter(|c| matches!(c, UiCommand::ResolveApproval { .. }))
            .collect()
    }

    #[test]
    fn queue_auto_tab_opens_on_pending_approvals() {
        let mut rt = boot("queue");
        // Web parity: pending gates auto-select the approvals tab.
        assert_eq!(rt.state.queue_active_tab(), "approvals");
        assert!(rt.find("queue-apr-apr-q-1").is_some());
        assert!(has_text(&rt, "Approve Once"));
        assert!(has_text(&rt, "PENDING"));
        // Explicit tab choice wins over auto.
        click_key(&mut rt, "queue-tab-jobs");
        assert_eq!(rt.state.queue_active_tab(), "jobs");
        assert!(rt.find("queue-cron").is_some());
        // Resolving the last gate falls back to runtime when unset.
        rt.state.queue_tab = None;
        rt.state.resolve_approval("apr-q-1", "once");
        refresh(&mut rt);
        assert_eq!(rt.state.queue_active_tab(), "runtime");
        assert!(rt.find("queue-profiles").is_some());
    }

    #[test]
    fn queue_approval_once_always_deny_dispatch_with_generation() {
        let mut rt = boot("queue");
        // Pin the tab: with no pending gates left, auto would fall back to runtime.
        rt.state.queue_tab = Some("approvals".into());
        refresh(&mut rt);
        click_in_queue(&mut rt, "Approve Once");
        rt.settle_all();
        rt.frame(0.016);
        assert_eq!(rt.state.pending_approvals().len(), 0);
        assert!(rt.state.approvals[0].resolved);
        assert_eq!(rt.state.approvals[0].resolution.as_deref(), Some("once"));
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::ResolveApproval {
                run_id: Some(run),
                request_id,
                execution_generation: 2,
                decision,
                is_clarify: false,
            } if run == "run-88" && request_id == "req-88" && decision == "once"
        )));
        // Resolved card keeps its history badge, loses its action buttons.
        assert!(has_text(&rt, "APPROVED"));
        assert!(!has_text(&rt, "Approve Once"));

        // Always + deny paths carry their own request ids / generations.
        rt.state.approvals.push(mk_approval("apr-a2", "req-a2", 5, ApprovalKind::Approval));
        rt.state.approvals.push(mk_approval("apr-a3", "req-a3", 7, ApprovalKind::Approval));
        refresh(&mut rt);
        click_in_queue(&mut rt, "Always Allow");
        rt.settle_all();
        rt.frame(0.016);
        click_in_queue(&mut rt, "Deny");
        rt.settle_all();
        rt.frame(0.016);
        assert_eq!(rt.state.pending_approvals().len(), 0);
        let cmds = resolve_commands(&rt);
        assert_eq!(cmds.len(), 3);
        assert!(cmds.iter().any(|c| matches!(
            c,
            UiCommand::ResolveApproval { request_id, execution_generation: 5, decision, .. }
                if request_id == "req-a2" && decision == "always"
        )));
        assert!(cmds.iter().any(|c| matches!(
            c,
            UiCommand::ResolveApproval { request_id, execution_generation: 7, decision, .. }
                if request_id == "req-a3" && decision == "deny"
        )));
        assert!(has_text(&rt, "REJECTED"));
    }

    #[test]
    fn queue_clarify_reply_dispatches_answer_as_clarify() {
        let mut rt = boot("queue");
        rt.state.approvals.push(mk_approval("apr-cl", "req-cl", 3, ApprovalKind::Clarify));
        refresh(&mut rt);
        assert!(rt.find("queue-clarify-apr-cl").is_some());

        // Empty answer is ignored (web disables Reply until text exists).
        click_in_queue(&mut rt, "Reply");
        assert_eq!(rt.state.pending_approvals().len(), 2);
        assert!(resolve_commands(&rt).is_empty());

        rt.state
            .queue_clarify_edits
            .entry("apr-cl".to_string())
            .or_default()
            .replace_text("use the staging gateway");
        refresh(&mut rt);
        click_in_queue(&mut rt, "Reply");
        assert!(rt.state.approvals.iter().find(|a| a.id == "apr-cl").unwrap().resolved);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::ResolveApproval {
                request_id,
                execution_generation: 3,
                decision,
                is_clarify: true,
                ..
            } if request_id == "req-cl" && decision == "use the staging gateway"
        )));
        // Draft is consumed after send.
        assert!(!rt.state.queue_clarify_edits.contains_key("apr-cl"));
    }

    #[test]
    fn queue_profile_switch_dispatches_and_counts_lanes() {
        let mut rt = boot("queue");
        click_key(&mut rt, "queue-tab-runtime");
        rt.state.session_profiles.insert("ch-a".into(), "ops".into());
        rt.state.session_profiles.insert("ch-b".into(), "ops".into());
        refresh(&mut rt);

        assert!(rt.find("queue-profile-default").is_some());
        assert!(rt.find("queue-profile-ops").is_some());
        assert!(has_text(&rt, "· 2 lanes"));
        assert!(has_text(&rt, "@ops"));
        assert!(has_text(&rt, "/p/ops"));
        assert!(has_text(&rt, "Default"));
        assert!(has_text(&rt, "Use default"));

        click_key(&mut rt, "queue-profile-use-ops");
        assert_eq!(rt.state.gateway.active_profile, "ops");
        assert!(rt.state.gateway_profiles.iter().find(|p| p.name == "ops").unwrap().active);
        assert!(!rt.state.gateway_profiles.iter().find(|p| p.name == "default").unwrap().active);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::SwitchProfile { profile } if profile == "ops"
        )));
        // The ops card now wears the Default badge.
        assert!(has_text(&rt, "Default"));
    }

    #[test]
    fn queue_skills_and_toolsets_render_and_expand() {
        let mut rt = boot("queue");
        click_key(&mut rt, "queue-tab-runtime");
        assert!(has_text(&rt, "2 loaded · GET /v1/skills"));
        assert!(has_text(&rt, "rust-diagnostics"));
        assert!(has_text(&rt, "ui-screenshot-audit"));
        assert!(rt.find("queue-skill-rust-diagnostics").is_some());
        assert!(has_text(&rt, "2 reported · GET /v1/toolsets"));
        assert!(rt.find("queue-toolset-terminal").is_some());
        assert!(has_text(&rt, "enabled · 2 tools"));

        // Toolset rows expand to reveal their tool list, then collapse again.
        assert!(!has_text(&rt, "exec, process"));
        click_key(&mut rt, "queue-toolset-terminal");
        assert_eq!(rt.state.queue_expanded_toolset.as_deref(), Some("terminal"));
        assert!(has_text(&rt, "exec, process"));
        click_key(&mut rt, "queue-toolset-terminal");
        assert_eq!(rt.state.queue_expanded_toolset, None);
        rt.settle_all();
        rt.frame(0.016);
        assert!(!has_text(&rt, "exec, process"));
    }

    #[test]
    fn queue_cron_list_renders_and_actions_dispatch() {
        let mut rt = boot("queue");
        click_key(&mut rt, "queue-tab-jobs");
        assert!(rt.find("cron-cron-health").is_some());
        assert!(has_text(&rt, "*/5 * * * *"));
        assert!(has_text(&rt, "Gateway Heartbeat — Check local gateway metrics"));
        assert!(has_text(&rt, "last=2m ago · next=in 3m"));

        click_in_queue(&mut rt, "Run Now");
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::ManageCronJob { job_id, action } if job_id == "cron-health" && action == "run"
        )));

        click_in_queue(&mut rt, "Pause");
        assert!(!rt.state.cron_jobs[0].enabled);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::ManageCronJob { job_id, action } if job_id == "cron-health" && action == "pause"
        )));
        assert!(has_text(&rt, "Resume"));

        click_in_queue(&mut rt, "Delete");
        assert!(rt.state.cron_jobs.is_empty());
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::ManageCronJob { job_id, action } if job_id == "cron-health" && action == "delete"
        )));
        assert!(has_text(&rt, "No scheduled jobs on gateway. Live jobs from GET /api/jobs appear here."));
    }

    #[test]
    fn queue_stop_and_steer_dispatch_existing_commands() {
        let mut rt = boot("queue");
        rt.state.active_run_ids.insert("home".into(), "run-88".into());
        rt.state.thinking_by_channel.insert("home".into(), true);
        rt.state.tasks.push(mk_task("t-q", "home", "Audit RUNE shader pipeline"));
        refresh(&mut rt);

        assert!(rt.find("queue-run-home").is_some());
        assert!(has_text(&rt, "run:run-88 · #home · 1/2"));

        click_in_queue(&mut rt, "Stop");
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::StopRun { run_id, channel_id } if run_id == "run-88" && channel_id == "home"
        )));
        assert!(!rt.state.thinking_by_channel.get("home").copied().unwrap_or(false));

        // Steer opens the composer; empty drafts never dispatch.
        click_in_queue(&mut rt, "Steer");
        assert_eq!(rt.state.queue_steer_run.as_deref(), Some("run-88"));
        assert!(rt.find("queue-steer-input").is_some());
        click_in_queue(&mut rt, "Send Steer");
        assert!(!rt.state.pending_commands.iter().any(|c| matches!(c, UiCommand::SteerRun { .. })));

        rt.state.queue_steer_edit.replace_text("focus on the failing test");
        refresh(&mut rt);
        click_in_queue(&mut rt, "Send Steer");
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::SteerRun { run_id, text } if run_id == "run-88" && text == "focus on the failing test"
        )));
        assert!(rt.state.queue_steer_edit.text.is_empty());
        assert_eq!(rt.state.queue_steer_run, None);
        assert!(rt.find("queue-steer-input").is_none());
    }

    #[test]
    fn queue_profile_cards_never_render_raw_api_keys() {
        let mut rt = boot("queue");
        rt.state.hermes_key_edit.replace_text("sk-live-SECRET-9f3e2d1c");
        rt.state.has_hermes_key = true;
        click_key(&mut rt, "queue-tab-runtime");
        refresh(&mut rt);

        // Redacted summary: boolean presence indicator only.
        assert!(rt.elements().iter().any(|e| {
            e.text.as_deref().map(|t| t.contains("key: set")).unwrap_or(false)
        }));
        for e in rt.elements() {
            if let Some(t) = &e.text {
                assert!(!t.contains("SECRET"), "raw key leaked into element text: {t}");
                assert!(!t.contains("sk-live"), "raw key leaked into element text: {t}");
            }
        }
        let a11y = rt.accessibility_json().to_string();
        assert!(!a11y.contains("sk-live-SECRET"));
    }

    #[test]
    fn queue_empty_and_offline_states_render() {
        let mut s = AppState::new();
        s.active_view = voxcell_core::ActiveView::Queue;
        let mut rt = boot_state(s);
        // No gates, no gateway data: runtime tab auto-selected, honest empties.
        assert_eq!(rt.state.queue_active_tab(), "runtime");
        assert!(has_text(&rt, "Offline"));
        assert!(!has_text(&rt, "0 toolsets returned by gateway."));
        assert!(has_text(&rt, "Offline — start hermes gateway to inspect toolsets."));
        assert!(has_text(&rt, "Offline — start hermes gateway to inspect /v1/skills."));

        click_key(&mut rt, "queue-tab-approvals");
        assert!(has_text(
            &rt,
            "No pending run approvals. Execution gates from /v1/runs/{id}/events and pending_controls appear here when a run requests confirmation."
        ));
        click_key(&mut rt, "queue-tab-jobs");
        assert!(has_text(
            &rt,
            "Gateway offline — connect to the Hermes Agent API Server to manage /api/jobs."
        ));
    }

    #[test]
    fn queue_gateway_refresh_and_capability_chips() {
        let mut rt = boot("queue");
        assert!(rt.find("queue-gateway-card").is_some());
        assert!(has_text(&rt, "Connected"));
        assert!(has_text(&rt, "Capabilities: runs · approvals · clarify · steering · session_stream · skills · toolsets · cron"));
        click_in_queue(&mut rt, "Refresh");
        assert!(rt.state.pending_commands.iter().any(|c| matches!(c, UiCommand::RefreshGateway)));

        // Offline gateway flips the semantic error telemetry + Retry label.
        rt.state.gateway.online = false;
        rt.state.gateway.last_error = Some("connection refused".into());
        refresh(&mut rt);
        assert!(has_text(&rt, "Offline"));
        assert!(has_text(&rt, "connection refused"));
        assert!(has_text(&rt, "Retry"));
    }

    #[test]
    fn queue_lists_render_injected_gateway_data() {
        // Data injected straight into AppState (bridge/mapper shape) renders.
        let mut rt = boot("idle");
        rt.state.active_view = voxcell_core::ActiveView::Queue;
        rt.state.skills.push(HermesSkill {
            name: "maps".into(),
            description: "Geocode and route via OpenStreetMap".into(),
            category: Some("productivity".into()),
            enabled: true,
        });
        rt.state.toolsets.push(HermesToolset {
            name: "browser".into(),
            description: "Headless browser automation".into(),
            tools: vec!["navigate".into(), "extract".into(), "click".into()],
            enabled: false,
        });
        rt.state.cron_jobs.push(HermesCronJob {
            id: "cron-digest".into(),
            name: "Morning Digest".into(),
            schedule: "0 9 * * *".into(),
            prompt: "Summarize overnight runs".into(),
            channel_id: None,
            enabled: false,
            last_run_at: None,
            next_run_at: None,
            last_status: Some("paused".into()),
        });
        refresh(&mut rt);

        click_key(&mut rt, "queue-tab-runtime");
        assert!(has_text(&rt, "1 loaded · GET /v1/skills"));
        assert!(has_text(&rt, "maps"));
        assert!(has_text(&rt, "productivity"));
        assert!(has_text(&rt, "disabled · 3 tools"));
        click_key(&mut rt, "queue-toolset-browser");
        assert!(has_text(&rt, "navigate, extract, click"));

        click_key(&mut rt, "queue-tab-jobs");
        assert!(has_text(&rt, "0 9 * * *"));
        assert!(has_text(&rt, "paused"));
        assert!(has_text(&rt, "Resume"));
    }
}
