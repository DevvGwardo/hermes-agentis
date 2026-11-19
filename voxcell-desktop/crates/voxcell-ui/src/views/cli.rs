//! P3-05 CLI terminal — web parity with `src/components/CommandTerminal.tsx`
//! and `SlashCommandPalette.tsx`.
//!
//! - JetBrains Mono (tabular figures inherent to the monospace family; rune
//!   exposes no OpenType feature API) with Cascadia/Consolas/Iosevka fallbacks.
//! - Command history (Up/Down) with draft stashing.
//! - Autocomplete popup over the core::cli slash set, incl. `/profile` slug,
//!   `/resume` session and `/ui` preset subcommands; Up/Down navigate, Tab or
//!   Enter accept, Esc dismisses.
//! - Submit dispatches through the TS-parity core engine (`execute_cli_input`)
//!   via `AppState::cli_submit_line`.
//! - Output renders as chronological trace blocks (echo starts a block below,
//!   never hoisted — same invariant as the thread view), collapsible and
//!   copyable per block; the whole log is also drag-selectable.
//! - slate-v1 tokens only; amber/rose reserved for working/error telemetry.

use crate::state::AppState;
use crate::theme::VoxcellTheme;
use rune::{Cx, El, Family, Key, KeyEvent, ScrollSpec, TextStyle, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Divider, TextInput};

/// Web `QUICK_COMMANDS` (CommandTerminal.tsx) — one-tap readouts.
pub const CLI_QUICK_COMMANDS: &[(&str, &str)] = &[
    ("/gateway", "Probe server health, profile & capabilities"),
    ("/skills", "List skills loaded from GET /v1/skills"),
    ("/toolsets", "Inspect enabled toolsets from GET /v1/toolsets"),
    ("/cron", "Show scheduled jobs from GET /api/jobs"),
    ("/sessions", "List active sessions on this gateway"),
    ("/tasks", "Inspect active & completed turn tasks"),
    ("/help", "Show all CLI & slash commands"),
];

/// JetBrains Mono terminal face. Monospace glyphs give tabular-nums alignment
/// for progress/latency columns; the named family falls back through the
/// platform monospace stack when JetBrains Mono is not installed.
fn cli_mono(size: f32, weight: f32, lh: f32) -> TextStyle {
    TextStyle::new(size, weight)
        .lh(lh)
        .family(Family::Named("JetBrains Mono, Cascadia Mono, Consolas, Iosevka"))
}

/// One chronological output block: a `> command` echo opens a new block;
/// following output lines belong to it. Lines before the first echo form a
/// system block. Order is preserved exactly (thread trace invariant).
#[derive(Clone, Debug, PartialEq)]
pub struct CliTraceBlock {
    pub id: String,
    pub command: Option<String>,
    pub lines: Vec<String>,
}

impl CliTraceBlock {
    /// Clipboard payload: echo + output, exactly as logged.
    pub fn copy_text(&self) -> String {
        match &self.command {
            Some(cmd) => format!("> {}\n{}", cmd, self.lines.join("\n")),
            None => self.lines.join("\n"),
        }
    }
}

pub fn cli_trace_blocks(log: &[String]) -> Vec<CliTraceBlock> {
    let mut blocks: Vec<CliTraceBlock> = Vec::new();
    for line in log {
        if let Some(cmd) = line.strip_prefix("> ") {
            blocks.push(CliTraceBlock {
                id: format!("cli-trace-{}", blocks.len()),
                command: Some(cmd.to_string()),
                lines: Vec::new(),
            });
        } else if let Some(last) = blocks.last_mut() {
            last.lines.push(line.clone());
        } else {
            blocks.push(CliTraceBlock {
                id: format!("cli-trace-{}", blocks.len()),
                command: None,
                lines: vec![line.clone()],
            });
        }
    }
    blocks
}

fn line_color(line: &str, theme: &VoxcellTheme) -> rune::Color {
    if line.starts_with("[gateway error]")
        || line.starts_with("Unknown")
        || line.contains("Gateway offline")
    {
        theme.rose
    } else {
        theme.text
    }
}

/// Focused-input key handling, shared by the view and headless tests.
/// Returns true when the key is consumed (editor default suppressed).
pub fn handle_cli_key(s: &mut AppState, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    // Leave chords (copy/paste/etc.) to the editor and app shortcuts.
    if ev.mods.cmd || ev.mods.ctrl || ev.mods.alt {
        return false;
    }
    let suggestions = s.cli_suggestions();
    let popup = !suggestions.is_empty();
    match &ev.key {
        Key::Up => {
            if popup {
                s.cli_suggestion_move(-1);
                true
            } else {
                s.cli_history_prev()
            }
        }
        Key::Down => {
            if popup {
                s.cli_suggestion_move(1);
                true
            } else {
                s.cli_history_next()
            }
        }
        Key::Tab => {
            if popup {
                let idx = s.cli_suggestion_selected.min(suggestions.len() - 1);
                s.cli_accept_suggestion(idx);
                true
            } else {
                false
            }
        }
        Key::Escape => {
            if popup {
                s.cli_suggestions_dismissed = true;
                true
            } else {
                false
            }
        }
        Key::Enter => {
            if popup {
                let idx = s.cli_suggestion_selected.min(suggestions.len() - 1);
                let current = s.cli_edit.text.trim().to_ascii_lowercase();
                // `/theme` normalizes to `/ui` for comparison so a fully
                // typed alias submits instead of re-inserting.
                let current_norm = if current.starts_with("/theme") {
                    format!("/ui{}", &current["/theme".len()..])
                } else {
                    current.clone()
                };
                if suggestions[idx].insert.trim().eq_ignore_ascii_case(&current_norm) {
                    false // complete command: let on_submit dispatch it
                } else {
                    s.cli_accept_suggestion(idx);
                    true
                }
            } else {
                false
            }
        }
        _ => false,
    }
}

pub fn render_cli(state: &AppState, theme: &VoxcellTheme, _cx: &mut Cx<AppState>) -> El {
    let k = &theme.kit;
    let blocks = cli_trace_blocks(&state.cli_log);
    let suggestions = state.cli_suggestions();
    let popup_open = !suggestions.is_empty();
    let selected = state
        .cli_suggestion_selected
        .min(suggestions.len().saturating_sub(1));

    // ── Header: title + session label + quick commands (web parity) ──
    let session_label = state.active_session_id();
    let mut header_right = row().gap(4.0).items_center();
    if blocks.is_empty() {
        header_right = header_right.child(
            Button::new("/help")
                .variant(ButtonVariant::Neutral)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| s.cli_submit_line("/help"))
                .build(k),
        );
    } else {
        for (cmd, summary) in CLI_QUICK_COMMANDS {
            let cmd = *cmd;
            let summary = *summary;
            header_right = header_right.child(
                Button::new(cmd)
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Mini)
                    .on_press(move |s: &mut AppState| s.cli_submit_line(cmd))
                    .build(k)
                    .description(summary),
            );
        }
    }
    header_right = header_right.child(
        Button::new("Clear")
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Mini)
            .on_press(|s: &mut AppState| s.cli_log.clear())
            .build(k),
    );

    let header = row()
        .key("cli-header")
        .w_full()
        .justify_between()
        .items_center()
        .gap(8.0)
        .child(
            row()
                .gap(6.0)
                .items_center()
                .child(text("Hermes CLI").font(theme.heading).color(theme.text))
                .child(text("·").font(theme.mono_sm).color(theme.text_faint))
                .child(
                    text(session_label.clone())
                        .key("cli-session-label")
                        .font(theme.mono_sm)
                        .color(theme.text_muted),
                ),
        )
        .child(header_right);

    // ── Output stream: chronological trace blocks ──
    let mut log_col = col()
        .key("cli-log-lines")
        .w_full()
        .gap(10.0)
        .padding(12.0)
        .selection_group(true);

    if blocks.is_empty() {
        let mut empty = col()
            .key("cli-empty-state")
            .w_full()
            .gap(8.0)
            .padding(16.0);
        empty = empty
            .child(text("New session ready").font(theme.heading).color(theme.text))
            .child(
                text("Run a quick command below or enter a prompt in the composer.")
                    .font(theme.mono_sm)
                    .color(theme.text_muted),
            );
        for (cmd, summary) in CLI_QUICK_COMMANDS.iter().take(6) {
            let cmd = *cmd;
            let summary = *summary;
            empty = empty.child(
                row()
                    .key(format!("cli-empty-{}", &cmd[1..]))
                    .w_full()
                    .justify_between()
                    .items_center()
                    .gap(12.0)
                    .padding(6.0)
                    .radius(6.0)
                    .on_press(move |s: &mut AppState| s.cli_submit_line(cmd))
                    .child(text(cmd).font(cli_mono(12.0, 600.0, 17.0)).color(theme.accent))
                    .child(
                        text(summary)
                            .font(theme.mono_sm)
                            .color(theme.text_muted),
                    ),
            );
        }
        log_col = log_col.child(empty);
    }

    for block in &blocks {
        let collapsed = state.cli_collapsed_traces.contains(&block.id);
        let is_command = block.command.is_some();
        let header_label = match &block.command {
            Some(cmd) => format!("$ {cmd}"),
            None => "SYSTEM".to_string(),
        };
        let copy_payload = block.copy_text();
        let toggle_id = block.id.clone();

        let mut block_col = col()
            .key(block.id.clone())
            .w_full()
            .gap(4.0)
            .padding(8.0)
            .radius(8.0)
            .fill(if is_command {
                theme.card_fill
            } else {
                theme.surface.alpha(0.55)
            })
            .border(1.0, theme.border);

        let header_row = row()
            .key(format!("{}-header", block.id))
            .w_full()
            .justify_between()
            .items_center()
            .gap(8.0)
            .child(
                text(header_label)
                    .font(cli_mono(12.0, 600.0, 17.0))
                    .color(if is_command { theme.accent } else { theme.text_muted }),
            )
            .child(
                row()
                    .gap(4.0)
                    .items_center()
                    .child(
                        Button::new(if collapsed { "Show" } else { "Hide" })
                            .variant(ButtonVariant::Ghost)
                            .control_size(ControlSize::Mini)
                            .on_press(move |s: &mut AppState| {
                                if !s.cli_collapsed_traces.insert(toggle_id.clone()) {
                                    s.cli_collapsed_traces.remove(&toggle_id);
                                }
                            })
                            .build(k)
                            .key(format!("{}-toggle", block.id)),
                    )
                    .child(
                        Button::new("Copy")
                            .variant(ButtonVariant::Ghost)
                            .control_size(ControlSize::Mini)
                            .on_press_tx(move |_s: &mut AppState, tx: &mut Tx| {
                                tx.copy(copy_payload.clone());
                            })
                            .build(k)
                            .key(format!("{}-copy", block.id)),
                    ),
            );
        block_col = block_col.child(header_row);

        if !collapsed {
            for (j, line) in block.lines.iter().enumerate() {
                block_col = block_col.child(
                    text(line.clone())
                        .key(format!("{}-line-{j}", block.id))
                        .selectable(true)
                        .font(cli_mono(12.5, 400.0, 18.0))
                        .color(if is_command {
                            line_color(line, theme)
                        } else {
                            theme.text_muted
                        }),
                );
            }
        }
        log_col = log_col.child(block_col);
    }

    if state.active_is_thinking() {
        log_col = log_col.child(
            row()
                .key("cli-thinking")
                .gap(6.0)
                .items_center()
                .child(
                    el()
                        .w(8.0)
                        .h(8.0)
                        .radius(4.0)
                        .fill(theme.amber),
                )
                .child(
                    text("HERMES · Routing through active lane...")
                        .font(cli_mono(11.5, 500.0, 16.0))
                        .color(theme.amber),
                ),
        );
    }

    let scroll = col()
        .key("cli-scroll")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .radius(10.0)
        .fill(theme.bg.alpha(0.78))
        .border(1.0, theme.border)
        .scroll(ScrollSpec { stick_bottom: true })
        .child(log_col);

    // ── Footer: keyboard hints + tabular entry count (web parity) ──
    let entry_count = state.cli_log.len();
    let footer = row()
        .key("cli-footer")
        .w_full()
        .justify_between()
        .items_center()
        .child(
            text("Tab autocomplete · ↑↓ history")
                .font(theme.mono_sm)
                .color(theme.text_faint),
        )
        .child(
            text(format!(
                "{entry_count} {}",
                if entry_count == 1 { "entry" } else { "entries" }
            ))
            .key("cli-entry-count")
            .font(theme.mono_sm)
            .color(theme.text_faint),
        );

    // ── Autocomplete popup (SlashCommandPalette parity) ──
    let mut popup = col()
        .key("cli-autocomplete-popup")
        .w_full()
        .radius(10.0)
        .fill(theme.glass_fill)
        .border(1.0, theme.border_strong);
    if popup_open {
        popup = popup.child(
            row()
                .key("cli-autocomplete-header")
                .w_full()
                .justify_between()
                .items_center()
                .padding(8.0)
                .border(1.0, theme.border)
                .child(
                    text(format!("Hermes Commands · {}", suggestions.len()))
                        .font(cli_mono(10.5, 600.0, 15.0))
                        .color(theme.text_muted),
                )
                .child(
                    text("↑↓ navigate · Enter select · Esc close")
                        .font(cli_mono(10.0, 400.0, 14.0))
                        .color(theme.text_faint),
                ),
        );
        let mut list = col()
            .key("cli-autocomplete-list")
            .w_full()
            .gap(2.0)
            .padding(6.0)
            .scroll(ScrollSpec { stick_bottom: false });
        if suggestions.len() > 4 {
            list = list.h(200.0);
        }
        for (i, item) in suggestions.iter().enumerate().take(8) {
            let is_sel = i == selected;
            list = list.child(
                col()
                    .key(format!("cli-suggestion-{i}"))
                    .w_full()
                    .gap(2.0)
                    .padding(7.0)
                    .radius(6.0)
                    .fill(if is_sel {
                        theme.accent.alpha(0.14)
                    } else {
                        theme.bg.alpha(0.0)
                    })
                    .border(1.0, if is_sel {
                        theme.accent.alpha(0.42)
                    } else {
                        theme.border.alpha(0.0)
                    })
                    .on_press(move |s: &mut AppState| {
                        s.cli_accept_suggestion(i);
                    })
                    .child(
                        text(item.label.clone())
                            .font(cli_mono(12.0, 600.0, 17.0))
                            .color(theme.text),
                    )
                    .child(
                        text(item.detail.clone())
                            .font(theme.mono_sm)
                            .color(theme.text_muted),
                    ),
            );
        }
        popup = popup.child(list);
    }

    // ── Input bar ──
    let input_bar = row()
        .key("cli-input-bar")
        .w_full()
        .padding(8.0)
        .gap(8.0)
        .radius(8.0)
        .fill(theme.card_fill)
        .border(1.0, theme.border_strong)
        .items_center()
        .child(text("$").font(cli_mono(13.0, 700.0, 18.0)).color(theme.accent))
        .child(
            el().key("cli-input-wrap").grow(1.0).min_w(0.0).child(
                TextInput::new(&state.cli_edit)
                    .placeholder("Enter /command or prompt...")
                    .label("CLI command")
                    .plain()
                    .width(None)
                    .text_style(cli_mono(12.5, 400.0, 18.0))
                    .on_edit(|s: &mut AppState| &mut s.cli_edit)
                    .on_change(|s: &mut AppState, _tx: &mut Tx| {
                        s.cli_suggestion_selected = 0;
                        s.cli_suggestions_dismissed = false;
                    })
                    .on_key(|s: &mut AppState, ev: &KeyEvent, tx: &mut Tx| {
                        // Manual typing leaves history browse mode.
                        if matches!(ev.key, Key::Text(_) | Key::Backspace | Key::Delete) {
                            s.cli_history_cursor = None;
                        }
                        handle_cli_key(s, ev, tx)
                    })
                    .on_submit(|s: &mut AppState, _tx: &mut Tx| {
                        let line = s.cli_edit.text.clone();
                        s.cli_edit.clear();
                        s.cli_submit_line(&line);
                    })
                    .build(k),
            ),
        )
        .child(
            Button::new("Run")
                .variant(ButtonVariant::Submit)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| {
                    let line = s.cli_edit.text.clone();
                    s.cli_edit.clear();
                    s.cli_submit_line(&line);
                })
                .build(k),
        );

    let mut root = col()
        .key("cli-view")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .h_full()
        .padding(14.0)
        .gap(10.0)
        .child(header)
        .child(scroll)
        .child(footer);
    if popup_open {
        root = root.child(popup);
    }
    root.child(Divider::horizontal().build(k)).child(input_bar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::UiCommand;
    use crate::views::render_main_window;
    use rune::{Env, Mods, Runtime};
    use voxcell_core::{ActiveView, UiCustomizationConfig};

    fn cli_runtime() -> Runtime<AppState> {
        let mut s = AppState::new();
        s.apply_scene_preset("idle");
        s.active_view = ActiveView::Cli;
        let mut rt = Runtime::new(s, render_main_window, (1280.0, 800.0), Env::new(true, 1.0));
        rt.frame(0.0);
        rt
    }

    fn focus_input(rt: &mut Runtime<AppState>) {
        let (x, y) = rt.find("cli-input-wrap").expect("cli input wrap");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.016);
    }

    fn type_text(rt: &mut Runtime<AppState>, text: &str) {
        for ch in text.chars() {
            rt.key(KeyEvent {
                key: Key::Text(ch.to_string()),
                mods: Mods::default(),
            });
        }
        rt.frame(0.016);
    }

    fn press(rt: &mut Runtime<AppState>, key: Key) {
        rt.key(KeyEvent {
            key,
            mods: Mods::default(),
        });
        rt.frame(0.016);
    }

    fn click(rt: &mut Runtime<AppState>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.016);
    }

    #[test]
    fn trace_blocks_group_chronologically_without_hoisting() {
        let log: Vec<String> = [
            "boot banner",
            "> /status",
            "SESSION LANE: #home",
            "GATEWAY: CONNECTED",
            "> /skills",
            "0 skills reported",
            "late diagnostic",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let blocks = cli_trace_blocks(&log);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].command, None);
        assert_eq!(blocks[0].lines, vec!["boot banner".to_string()]);
        assert_eq!(blocks[1].command.as_deref(), Some("/status"));
        assert_eq!(blocks[1].lines.len(), 2);
        assert_eq!(blocks[2].command.as_deref(), Some("/skills"));
        // Post-command activity stays in the block below, never hoisted up.
        assert_eq!(
            blocks[2].lines,
            vec!["0 skills reported".to_string(), "late diagnostic".to_string()]
        );
        assert_eq!(
            blocks[1].copy_text(),
            "> /status\nSESSION LANE: #home\nGATEWAY: CONNECTED"
        );
    }

    #[test]
    fn typing_enter_dispatches_through_core_engine() {
        let mut rt = cli_runtime();
        focus_input(&mut rt);
        type_text(&mut rt, "/status");
        press(&mut rt, Key::Enter);

        // TS-parity engine output (the old draft parser printed "Gateway: …").
        assert!(rt.state.cli_log.iter().any(|l| l.contains("SESSION LANE: #home")));
        assert!(rt.state.cli_log.iter().any(|l| l.starts_with("GATEWAY:")));
        assert!(rt.state.cli_log.iter().any(|l| l == "> /status"));
        assert_eq!(rt.state.cli_edit.text, "");
        assert_eq!(rt.state.cli_history, vec!["/status".to_string()]);

        // Natural language routes to Chat -> SendPrompt through the engine.
        type_text(&mut rt, "hello hermes");
        press(&mut rt, Key::Enter);
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::SendPrompt { prompt, .. } if prompt == "hello hermes"
        )));
        assert!(rt.state.entries.iter().any(|e| e.text == "hello hermes"));
    }

    #[test]
    fn history_up_down_recalls_and_restores_draft() {
        let mut rt = cli_runtime();
        rt.state.cli_submit_line("/status");
        rt.state.cli_submit_line("/help");
        rt.frame(0.016);

        focus_input(&mut rt);
        type_text(&mut rt, "my draft");
        press(&mut rt, Key::Up);
        assert_eq!(rt.state.cli_edit.text, "/help");
        press(&mut rt, Key::Up);
        assert_eq!(rt.state.cli_edit.text, "/status");
        press(&mut rt, Key::Up); // oldest: stays put
        assert_eq!(rt.state.cli_edit.text, "/status");
        press(&mut rt, Key::Down);
        assert_eq!(rt.state.cli_edit.text, "/help");
        press(&mut rt, Key::Down); // past newest: draft restored
        assert_eq!(rt.state.cli_edit.text, "my draft");
        assert_eq!(rt.state.cli_history_cursor, None);
        press(&mut rt, Key::Down); // no browse active: unchanged
        assert_eq!(rt.state.cli_edit.text, "my draft");
    }

    #[test]
    fn autocomplete_filters_and_selection_dispatches_profile_subcommand() {
        let mut rt = cli_runtime();
        focus_input(&mut rt);
        type_text(&mut rt, "/pro");
        rt.frame(0.016);
        assert!(rt.find("cli-autocomplete-popup").is_some());
        let items = rt.state.cli_suggestions();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "/profile [name]");
        assert!(rt.find("cli-suggestion-0").is_some());

        // Tab accepts the filtered command into the input.
        press(&mut rt, Key::Tab);
        assert_eq!(rt.state.cli_edit.text, "/profile ");

        // `/profile` subcommands: registry slugs filtered by the arg prefix.
        type_text(&mut rt, "o");
        let items = rt.state.cli_suggestions();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "@ops");

        // Enter accepts the incomplete slug, second Enter submits it.
        press(&mut rt, Key::Enter);
        assert_eq!(rt.state.cli_edit.text, "/profile ops");
        press(&mut rt, Key::Enter);
        assert_eq!(rt.state.gateway.active_profile, "ops");
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::SwitchProfile { profile } if profile == "ops"
        )));
        assert!(rt.state.cli_log.iter().any(|l| l.contains("launch on @ops")));
    }

    #[test]
    fn autocomplete_arrow_navigation_and_escape_dismissal() {
        let mut rt = cli_runtime();
        focus_input(&mut rt);
        type_text(&mut rt, "/");
        assert!(rt.state.cli_suggestions().len() > 3);
        press(&mut rt, Key::Down);
        assert_eq!(rt.state.cli_suggestion_selected, 1);
        press(&mut rt, Key::Down);
        assert_eq!(rt.state.cli_suggestion_selected, 2);
        press(&mut rt, Key::Up);
        assert_eq!(rt.state.cli_suggestion_selected, 1);
        press(&mut rt, Key::Escape);
        assert!(rt.state.cli_suggestions_dismissed);
        assert!(rt.find("cli-autocomplete-popup").is_none());
        // Typing again re-opens the palette.
        type_text(&mut rt, "s");
        assert!(!rt.state.cli_suggestions_dismissed);
        assert!(rt.find("cli-autocomplete-popup").is_some());
    }

    #[test]
    fn theme_and_ui_apply_via_ui_config() {
        let mut rt = cli_runtime();
        focus_input(&mut rt);

        // /theme alias normalizes to /ui and applies a curated preset.
        type_text(&mut rt, "/theme cyber-violet");
        press(&mut rt, Key::Enter);
        assert!(rt.state.cli_log.iter().any(|l| l.contains("UI PRESET // Applied")));
        assert_ne!(rt.state.ui_config, UiCustomizationConfig::default());
        assert!(rt.state.pending_commands.iter().any(|c| matches!(c, UiCommand::PersistUiConfig)));

        // Natural-language /ui edit lands in ui_config enums.
        rt.state.cli_edit.clear();
        type_text(&mut rt, "/ui amber mono sharp right sidebar");
        press(&mut rt, Key::Enter);
        let cfg = &rt.state.ui_config;
        assert_eq!(cfg.accent_preset, voxcell_core::AccentPreset::from_id("amber").unwrap());
        assert_eq!(cfg.font_preset, voxcell_core::FontPreset::from_id("mono").unwrap());
        assert_eq!(cfg.corner_radius, voxcell_core::CornerRadius::from_id("sharp").unwrap());
        assert_eq!(cfg.sidebar_position, voxcell_core::SidebarPosition::Right);

        // /ui reset restores defaults through ui_config.
        rt.state.cli_edit.clear();
        type_text(&mut rt, "/ui reset");
        press(&mut rt, Key::Enter);
        assert_eq!(rt.state.ui_config, UiCustomizationConfig::default());
    }

    #[test]
    fn copy_block_writes_clipboard() {
        let mut rt = cli_runtime();
        let clip = rune::clipboard::memory();
        rt.use_clipboard(&clip);
        rt.state.cli_submit_line("/status");
        rt.frame(0.016);

        // Block 0 is the boot banner; block 1 is the /status trace.
        click(&mut rt, "cli-trace-1-copy");
        let text = clip.text().expect("clipboard payload");
        assert!(text.contains("> /status"));
        assert!(text.contains("GATEWAY:"));
    }

    #[test]
    fn trace_block_collapse_toggle() {
        let mut rt = cli_runtime();
        rt.state.cli_submit_line("/status");
        rt.frame(0.016);
        assert!(rt.find("cli-trace-1-line-0").is_some());
        click(&mut rt, "cli-trace-1-toggle");
        assert!(rt.state.cli_collapsed_traces.contains("cli-trace-1"));
        assert!(rt.find("cli-trace-1-line-0").is_none());
        click(&mut rt, "cli-trace-1-toggle");
        assert!(!rt.state.cli_collapsed_traces.contains("cli-trace-1"));
        assert!(rt.find("cli-trace-1-line-0").is_some());
    }

    #[test]
    fn slash_set_dispatches_engine_readouts() {
        let mut rt = cli_runtime();
        let cases: &[(&str, &str)] = &[
            ("/gateway", "HERMES AGENT DOCTOR // GATEWAY CHECK"),
            ("/profile", "HERMES AGENT // PARALLEL PROFILES"),
            ("/skills", "GET /v1/skills"),
            ("/toolsets", "GET /v1/toolsets"),
            ("/cron", "GET /api/jobs"),
            ("/sessions", "ACTIVE SESSION LANE: #home"),
            ("/tasks", "HERMES AGENT // SESSION TASKS"),
            ("/resume home", "Resumed session #home."),
        ];
        for (cmd, marker) in cases {
            rt.state.cli_log.clear();
            rt.state.cli_submit_line(cmd);
            assert!(
                rt.state.cli_log.iter().any(|l| l.contains(marker)),
                "{cmd} should produce engine marker {marker:?}, got {:?}",
                rt.state.cli_log
            );
        }
        // /resume switched the active lane and opened the thread view.
        assert_eq!(rt.state.active_channel_id, "home");
        assert_eq!(rt.state.active_view, ActiveView::Thread);
    }
}
