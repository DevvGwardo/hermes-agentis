//! P3-06 Settings modal — web parity with the TS settings surface:
//! gateway/profile/voice sections, `UiCustomizerStudio` (8 accent swatches,
//! 7 curated presets, dot profile, bloom, backdrop picker, NL intent input),
//! the Preferences Vault backup/restore UI (rolling 12 restore points,
//! manual/pre-update labels, import/export) and the `UpdateCenterPanel`
//! native updater flow (probe/apply/receipt states from `AppState`).
//!
//! All appearance mutations route through `voxcell_core::ui_config`
//! (`sanitize_ui_patch`/`apply_ui_patch` inside `apply_ui_patch`, NL via
//! `parse_natural_ui_intent`). Vault/updater side effects dispatch through
//! `AppState::settings_outbox` (`SettingsAction`) for app/bridge.rs to drain
//! via `voxcell_store::{vault, updater}` — no live network in tests.

use crate::state::{AppState, SettingsAction, UiCommand};
use crate::theme::{VoxcellTheme, parse_hex_color};
use rune::{El, ScrollSpec, TextStyle, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog, Divider, Switch, TextInput};
use voxcell_core::ui_config::{ACCENT_SWATCHES, curated_ui_presets};
use voxcell_core::{BackgroundPresetId, DensityMode, DotProfile, HorizonMode, SidebarPosition};

/// TS `formatCompactTime` parity (UpdateCenterPanel), deterministic against
/// `AppState::now_ms` so headless tests never depend on the wall clock.
fn format_compact_time(ts_ms: i64, now_ms: u64) -> String {
    if ts_ms <= 0 {
        return "just now".to_string();
    }
    let diff_sec = (((now_ms as i64 - ts_ms) as f64) / 1000.0).round().max(0.0) as i64;
    if diff_sec < 15 {
        return "just now".to_string();
    }
    if diff_sec < 60 {
        return format!("{diff_sec}s ago");
    }
    if diff_sec < 3600 {
        return format!("{}m ago", diff_sec / 60);
    }
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (month, day) = civil_from_days(ts_ms / 86_400_000);
    let secs_of_day = (ts_ms % 86_400_000) / 1000;
    format!(
        "{} {} {:02}:{:02}",
        MONTHS[month - 1],
        day,
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60
    )
}

/// Days-since-epoch -> (month, day) (Howard Hinnant's civil_from_days).
fn civil_from_days(z: i64) -> (usize, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as usize;
    (m, d)
}

fn section_title(theme: &VoxcellTheme, title: &str) -> El {
    text(title.to_string())
        .font(TextStyle::new(13.0 * theme.font_scale, 700.0))
        .color(theme.text)
}

fn sub_title(theme: &VoxcellTheme, title: &str) -> El {
    text(title.to_string())
        .font(TextStyle::new(12.0 * theme.font_scale, 600.0))
        .color(theme.text_muted)
}

/// Compact −/value/+ stepper (rune-kit has no Slider; bloom/opacity/blur use
/// discrete steps routed through the core patch pipeline).
fn stepper_row(
    theme: &VoxcellTheme,
    label: &str,
    minus_key: String,
    readout_key: String,
    plus_key: String,
    readout: String,
    on_minus: impl Fn(&mut AppState) + 'static,
    on_plus: impl Fn(&mut AppState) + 'static,
) -> El {
    let k = &theme.kit;
    row()
        .gap(6.0)
        .items_center()
        .child(text(label.to_string()).font(theme.mono_sm).color(theme.text_muted))
        .child(
            Button::new("-")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Mini)
                .on_press(on_minus)
                .build(k)
                .key(minus_key),
        )
        .child(
            text(readout)
                .key(readout_key)
                .font(theme.mono_sm)
                .color(theme.text),
        )
        .child(
            Button::new("+")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Mini)
                .on_press(on_plus)
                .build(k)
                .key(plus_key),
        )
}

/// Gateway URL + API key + parallel profile switcher (web settings gateway
/// section; profile switch reuses the queue workstream's dispatch helper).
fn render_gateway_section(state: &AppState, theme: &VoxcellTheme) -> El {
    let k = &theme.kit;
    let mut profiles = row()
        .gap(6.0)
        .items_center()
        .child(text("Profile:").font(theme.mono_sm).color(theme.text_muted));
    for p in &state.gateway_profiles {
        let active = state.gateway.active_profile == p.name;
        let name = p.name.clone();
        let online = p.online;
        profiles = profiles.child(
            row()
                .key(format!("gateway-profile-{}", p.name))
                .gap(5.0)
                .items_center()
                .px(8.0)
                .py(4.0)
                .radius(theme.radius_control.min(8.0))
                .fill(if active {
                    theme.accent.alpha(0.14)
                } else {
                    theme.glass_fill
                })
                .border(1.0, if active { theme.border_strong } else { theme.border_subtle })
                .role("button")
                .label(format!("Switch to profile {}", p.name))
                .checked(active)
                .on_press(move |s: &mut AppState| s.queue_switch_profile(&name))
                .child(
                    el().w(6.0).h(6.0).radius(3.0).fill(if online {
                        theme.semantic_live
                    } else {
                        theme.semantic_error.alpha(0.7)
                    }),
                )
                .child(
                    text(p.name.clone())
                        .font(theme.ui_sm)
                        .color(if active { theme.accent_soft } else { theme.text_muted }),
                ),
        );
    }

    col()
        .key("settings-section-gateway")
        .w_full()
        .gap(8.0)
        .child(section_title(theme, "Hermes Gateway & Profiles"))
        .child(profiles)
        .child(
            TextInput::new(&state.gateway_url_edit)
                .placeholder("http://127.0.0.1:8642")
                .label("Hermes Gateway URL")
                .width(None)
                .on_edit(|s: &mut AppState| &mut s.gateway_url_edit)
                .build(k),
        )
        .child(
            TextInput::new(&state.hermes_key_edit)
                .placeholder(if state.has_hermes_key {
                    "HERMES_API_KEY configured (enter to replace)"
                } else {
                    "HERMES_API_KEY (optional)"
                })
                .label("Hermes API Key")
                .width(None)
                .on_edit(|s: &mut AppState| &mut s.hermes_key_edit)
                .build(k),
        )
        .child(
            row().w_full().justify_between().items_center().child(
                Button::new("Save & Reconnect")
                    .variant(ButtonVariant::Submit)
                    .control_size(ControlSize::Small)
                    .on_press(|s: &mut AppState| {
                        let base_url = s.gateway_url_edit.text.trim().to_string();
                        let hermes_api_key = s.hermes_key_edit.text.trim().to_string();
                        let deepgram_api_key = s.deepgram_key_edit.text.trim().to_string();
                        if !base_url.is_empty() {
                            s.gateway_base_url = base_url.clone();
                            s.gateway.base_url = base_url.clone();
                        }
                        if !hermes_api_key.is_empty() {
                            s.has_hermes_key = true;
                        }
                        if !deepgram_api_key.is_empty() {
                            s.has_deepgram_key = true;
                        }
                        s.pending_commands.push(UiCommand::SaveGatewaySettings {
                            base_url,
                            hermes_api_key,
                            deepgram_api_key,
                        });
                        s.pending_commands.push(UiCommand::RefreshGateway);
                        s.settings_open = false;
                    })
                    .build(k),
            ),
        )
}

/// Deepgram voice credentials + TTS/VAD/mute toggles (web voice section).
fn render_voice_section(state: &AppState, theme: &VoxcellTheme) -> El {
    let k = &theme.kit;
    col()
        .key("settings-section-voice")
        .w_full()
        .gap(8.0)
        .child(section_title(theme, "Deepgram Voice"))
        .child(
            TextInput::new(&state.deepgram_key_edit)
                .placeholder(if state.has_deepgram_key {
                    "DEEPGRAM_API_KEY configured (enter to replace)"
                } else {
                    "DEEPGRAM_API_KEY (Aura-2 TTS / Nova-2 STT)"
                })
                .label("Deepgram API Key")
                .width(None)
                .on_edit(|s: &mut AppState| &mut s.deepgram_key_edit)
                .build(k),
        )
        .child(
            row().gap(12.0).items_center()
                .child(
                    Switch::new(state.voice.tts_enabled)
                        .label("Aura-2 TTS")
                        .web()
                        .on_toggle(|s: &mut AppState| {
                            s.voice.tts_enabled = !s.voice.tts_enabled;
                            s.pending_commands.push(UiCommand::PersistVoiceSettings);
                        })
                        .build(k),
                )
                .child(
                    Switch::new(state.voice.hands_free_vad)
                        .label("Hands-Free VAD")
                        .web()
                        .on_toggle(|s: &mut AppState| {
                            let next = !s.voice.hands_free_vad;
                            s.voice.hands_free_vad = next;
                            s.pending_commands.push(UiCommand::ToggleMic(next));
                            s.pending_commands.push(UiCommand::PersistVoiceSettings);
                        })
                        .build(k),
                )
                .child(
                    Switch::new(state.voice.muted)
                        .label("Mute voice")
                        .web()
                        .on_toggle(|s: &mut AppState| {
                            let next = !s.voice.muted;
                            s.voice.muted = next;
                            s.pending_commands.push(UiCommand::ToggleMute(next));
                            s.pending_commands.push(UiCommand::PersistVoiceSettings);
                        })
                        .build(k),
                ),
        )
}

/// `UiCustomizerStudio` parity: NL intent input, 8 accent swatches, 7 curated
/// presets, dot profile + bloom, horizon mode, layout toggles. Every change
/// routes through `core::ui_config::apply_ui_patch` (sanitize + apply).
fn render_studio_section(state: &AppState, theme: &VoxcellTheme) -> El {
    let k = &theme.kit;

    // Natural-language intent (web `parseNaturalUiIntent`).
    let nl_row = row()
        .w_full()
        .gap(8.0)
        .items_center()
        .child(
            el().grow(1.0).min_w(0.0).child(
                TextInput::new(&state.ui_prompt_edit)
                    .placeholder(
                        "Describe a UI change (e.g. \"move sidebar right\", \"amber CRT\", \"zen mode\")…",
                    )
                    .label("Natural language UI tweak")
                    .width(None)
                    .on_edit(|s: &mut AppState| &mut s.ui_prompt_edit)
                    .on_submit(|s: &mut AppState, _tx| {
                        let req = s.ui_prompt_edit.text.clone();
                        s.settings_apply_intent(&req);
                    })
                    .build(k),
            ),
        )
        .child(
            Button::new("Apply Style")
                .variant(ButtonVariant::Submit)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| {
                    let req = s.ui_prompt_edit.text.clone();
                    s.settings_apply_intent(&req);
                })
                .build(k)
                .key("ui-intent-apply"),
        );

    // 8 accent swatches (web ACCENT_SWATCHES).
    let mut swatch_row = row().gap(8.0).items_center();
    for sw in ACCENT_SWATCHES {
        let active = state.ui_config.accent_hex.eq_ignore_ascii_case(sw.accent_hex);
        swatch_row = swatch_row.child(
            el()
                .key(format!("accent-swatch-{}", sw.id))
                .w(24.0)
                .h(24.0)
                .radius(12.0)
                .fill(parse_hex_color(sw.accent_hex, theme.accent))
                .border(
                    if active { 2.0 } else { 1.0 },
                    if active { theme.text } else { theme.border_subtle },
                )
                .role("button")
                .label(format!("Set accent color to {}", sw.label))
                .checked(active)
                .on_press(move |s: &mut AppState| {
                    s.settings_apply_swatch(sw.id);
                }),
        );
    }

    // 7 curated presets (web curatedUiPresets), two rows.
    let presets = curated_ui_presets();
    let mut preset_row_1 = row().gap(6.0).items_center();
    let mut preset_row_2 = row().gap(6.0).items_center();
    for (i, preset) in presets.iter().enumerate() {
        let active = state.ui_config.preset_name == preset.name;
        let btn = Button::new(preset.name)
            .variant(if active { ButtonVariant::Submit } else { ButtonVariant::Neutral })
            .control_size(ControlSize::Mini)
            .on_press(move |s: &mut AppState| {
                s.settings_apply_curated_preset(preset.id);
            })
            .build(k)
            .key(format!("ui-preset-{}", preset.id))
            .description(preset.subtitle);
        if i < 4 {
            preset_row_1 = preset_row_1.child(btn);
        } else {
            preset_row_2 = preset_row_2.child(btn);
        }
    }

    // Dot profile (bloom defaults mirror the drafted desktop behavior).
    let mut dot_row = row()
        .gap(6.0)
        .items_center()
        .child(text("Dot Profile:").font(theme.mono_sm).color(theme.text_muted));
    for (profile, id, bloom_default) in [
        (DotProfile::Crisp, "crisp", 0.22_f32),
        (DotProfile::Balanced, "balanced", 0.38),
        (DotProfile::Phosphor, "phosphor", 0.65),
    ] {
        let active = state.ui_config.dot_profile == profile;
        dot_row = dot_row.child(
            Button::new(match profile {
                DotProfile::Crisp => "Crisp",
                DotProfile::Balanced => "Balanced",
                DotProfile::Phosphor => "Phosphor",
            })
            .variant(if active { ButtonVariant::Submit } else { ButtonVariant::Ghost })
            .control_size(ControlSize::Mini)
            .on_press(move |s: &mut AppState| {
                s.settings_apply_json_patch(&serde_json::json!({
                    "brailleDotProfile": id,
                    "brailleBloom": bloom_default,
                }));
            })
            .build(k)
            .key(format!("dot-profile-{id}")),
        );
    }

    // Bloom stepper (web "Phosphor dot bloom" slider parity).
    let bloom_row = stepper_row(
        theme,
        "Phosphor dot bloom:",
        "bloom-minus".to_string(),
        "bloom-readout".to_string(),
        "bloom-plus".to_string(),
        format!("{:.0}%", (state.ui_config.bloom * 100.0).round()),
        |s: &mut AppState| {
            let next = (((s.ui_config.bloom - 0.05) * 100.0).round() / 100.0).clamp(0.0, 1.0);
            s.settings_apply_json_patch(&serde_json::json!({ "brailleBloom": next }));
        },
        |s: &mut AppState| {
            let next = (((s.ui_config.bloom + 0.05) * 100.0).round() / 100.0).clamp(0.0, 1.0);
            s.settings_apply_json_patch(&serde_json::json!({ "brailleBloom": next }));
        },
    );

    // Horizon mode.
    let mut horizon_row = row()
        .gap(6.0)
        .items_center()
        .child(text("Horizon:").font(theme.mono_sm).color(theme.text_muted));
    for (mode, id, label) in [
        (HorizonMode::Silk, "silk", "Silk"),
        (HorizonMode::Peaks, "peaks", "Peaks"),
        (HorizonMode::Pulse, "pulse", "Pulse"),
        (HorizonMode::Ribbon, "ribbon", "Ribbon"),
    ] {
        let active = state.ui_config.horizon_mode == mode;
        horizon_row = horizon_row.child(
            Button::new(label)
                .variant(if active { ButtonVariant::Submit } else { ButtonVariant::Ghost })
                .control_size(ControlSize::Mini)
                .on_press(move |s: &mut AppState| {
                    s.settings_apply_json_patch(&serde_json::json!({ "horizonStyle": id }));
                })
                .build(k)
                .key(format!("horizon-mode-{id}")),
        );
    }

    // Layout toggles: sidebar side, density, reduced motion.
    let layout_row = row()
        .gap(10.0)
        .items_center()
        .child(
            Button::new(match state.ui_config.sidebar_position {
                SidebarPosition::Left => "Sidebar: Left",
                SidebarPosition::Right => "Sidebar: Right",
            })
            .variant(ButtonVariant::Neutral)
            .control_size(ControlSize::Mini)
            .on_press(|s: &mut AppState| {
                let next = match s.ui_config.sidebar_position {
                    SidebarPosition::Left => "right",
                    SidebarPosition::Right => "left",
                };
                s.settings_apply_json_patch(&serde_json::json!({ "sidebarPosition": next }));
            })
            .build(k)
            .key("layout-sidebar-side"),
        )
        .child(
            Button::new(match state.ui_config.density {
                DensityMode::Compact => "Density: Compact",
                DensityMode::Comfortable => "Density: Comfortable",
                DensityMode::Airy => "Density: Airy",
            })
            .variant(ButtonVariant::Neutral)
            .control_size(ControlSize::Mini)
            .on_press(|s: &mut AppState| {
                let next = match s.ui_config.density {
                    DensityMode::Compact => "comfortable",
                    DensityMode::Comfortable => "airy",
                    DensityMode::Airy => "compact",
                };
                s.settings_apply_json_patch(&serde_json::json!({ "density": next }));
            })
            .build(k)
            .key("layout-density"),
        )
        .child(
            Switch::new(state.ui_config.reduced_motion)
                .label("Reduced Motion")
                .web()
                .on_toggle(|s: &mut AppState| {
                    let next = !s.ui_config.reduced_motion;
                    s.settings_apply_json_patch(&serde_json::json!({ "reducedMotion": next }));
                })
                .build(k),
        );

    // P5 native-chrome (spec P3-01 gap): frameless OS-chrome switch. The frame
    // swap applies at next launch (rune's `App::titlebar` is creation-time);
    // main.rs persists the flag through its local config seam, so no ui_config
    // patch / bridge command is needed here.
    let native_chrome_row = row()
        .key("native-chrome-row")
        .gap(10.0)
        .items_center()
        .child(
            Switch::new(state.native_chrome)
                .label("Native Window Chrome")
                .web()
                .on_toggle(|s: &mut AppState| {
                    s.native_chrome = !s.native_chrome;
                })
                .build(k)
                .key("native-chrome-toggle")
                .label("Native Window Chrome"),
        )
        .child(
            text("Frameless OS titlebar — applies on next launch")
                .font(theme.mono_sm)
                .color(theme.text_faint),
        );

    let mut section = col()
        .key("settings-section-studio")
        .w_full()
        .gap(8.0)
        .child(
            text("Appearance Studio & Natural Language Styler")
                .font(TextStyle::new(13.0 * theme.font_scale, 700.0))
                .color(theme.accent),
        )
        .child(nl_row);
    if let Some(feedback) = &state.ui_studio_feedback {
        section = section.child(
            text(feedback.clone())
                .key("ui-studio-feedback")
                .font(theme.mono_sm)
                .color(theme.text_muted),
        );
    }
    section
        .child(swatch_row)
        .child(preset_row_1)
        .child(preset_row_2)
        .child(dot_row)
        .child(bloom_row)
        .child(horizon_row)
        .child(layout_row)
        .child(native_chrome_row)
}

/// Ambient backdrop picker: all 7 web presets + opacity/blur steppers.
fn render_backdrop_section(state: &AppState, theme: &VoxcellTheme) -> El {
    let k = &theme.kit;
    let mut bg_row_1 = row().gap(6.0).items_center();
    let mut bg_row_2 = row().gap(6.0).items_center();
    for (i, &preset) in BackgroundPresetId::all().iter().enumerate() {
        let active = state.bg_settings.preset == preset;
        let btn = Button::new(preset.label())
            .variant(if active { ButtonVariant::Submit } else { ButtonVariant::Ghost })
            .control_size(ControlSize::Mini)
            .on_press(move |s: &mut AppState| {
                s.bg_settings.preset = preset;
                s.bg_settings.active_id = preset.web_id().to_string();
                s.pending_commands.push(UiCommand::PersistBackgroundSettings);
            })
            .build(k)
            .key(format!("bg-preset-{}", preset.web_id()));
        if i < 4 {
            bg_row_1 = bg_row_1.child(btn);
        } else {
            bg_row_2 = bg_row_2.child(btn);
        }
    }

    let opacity_row = stepper_row(
        theme,
        "Backdrop opacity:",
        "bg-opacity-minus".to_string(),
        "bg-opacity-readout".to_string(),
        "bg-opacity-plus".to_string(),
        format!("{:.0}%", (state.bg_settings.opacity * 100.0).round()),
        |s: &mut AppState| {
            let next = (((s.bg_settings.opacity - 0.05) * 100.0).round() / 100.0).clamp(0.05, 1.0);
            s.settings_apply_json_patch(&serde_json::json!({ "backgroundOpacity": next }));
        },
        |s: &mut AppState| {
            let next = (((s.bg_settings.opacity + 0.05) * 100.0).round() / 100.0).clamp(0.05, 1.0);
            s.settings_apply_json_patch(&serde_json::json!({ "backgroundOpacity": next }));
        },
    );
    let blur_row = stepper_row(
        theme,
        "Glass blur:",
        "bg-blur-minus".to_string(),
        "bg-blur-readout".to_string(),
        "bg-blur-plus".to_string(),
        format!("{:.0}px", state.bg_settings.blur_px.round()),
        |s: &mut AppState| {
            let next = (s.bg_settings.blur_px - 2.0).clamp(0.0, 40.0);
            s.settings_apply_json_patch(&serde_json::json!({ "glassBlurPx": next }));
        },
        |s: &mut AppState| {
            let next = (s.bg_settings.blur_px + 2.0).clamp(0.0, 40.0);
            s.settings_apply_json_patch(&serde_json::json!({ "glassBlurPx": next }));
        },
    );

    col()
        .key("settings-section-backdrop")
        .w_full()
        .gap(8.0)
        .child(section_title(theme, "Ambient Glass Backdrop"))
        .child(bg_row_1)
        .child(bg_row_2)
        .child(opacity_row)
        .child(blur_row)
}

/// Preferences Vault (web `UpdateCenterPanel` lower half): rolling 12 restore
/// points with manual/pre-update labels, save/export/import actions. Rows
/// render from `AppState::vault_backups`; actions drain via `settings_outbox`.
fn render_vault_section(state: &AppState, theme: &VoxcellTheme) -> El {
    let k = &theme.kit;
    let shown = state.vault_backups.len().min(12);
    let storage = if state.vault_storage_label.is_empty() {
        "Local preferences vault".to_string()
    } else {
        state.vault_storage_label.clone()
    };

    let actions = row()
        .gap(8.0)
        .items_center()
        .child(
            Button::new("Save backup")
                .variant(ButtonVariant::Submit)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| {
                    s.vault_busy = true;
                    s.settings_outbox.push(SettingsAction::VaultCreateBackup {
                        reason: "manual".to_string(),
                    });
                })
                .build(k)
                .key("vault-save-backup"),
        )
        .child(
            Button::new("Export")
                .variant(ButtonVariant::Neutral)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| {
                    s.settings_outbox.push(SettingsAction::VaultExport);
                })
                .build(k)
                .key("vault-export"),
        );

    let import_row = row()
        .w_full()
        .gap(8.0)
        .items_center()
        .child(
            el().grow(1.0).min_w(0.0).child(
                TextInput::new(&state.vault_import_edit)
                    .placeholder("Paste settings backup JSON…")
                    .label("Import settings backup JSON")
                    .width(None)
                    .on_edit(|s: &mut AppState| &mut s.vault_import_edit)
                    .build(k),
            ),
        )
        .child(
            Button::new("Import")
                .variant(ButtonVariant::Neutral)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| {
                    let json = s.vault_import_edit.text.trim().to_string();
                    if json.is_empty() {
                        s.vault_feedback = Some("Paste a settings backup JSON first.".to_string());
                        return;
                    }
                    s.vault_import_edit.clear();
                    s.vault_busy = true;
                    s.settings_outbox.push(SettingsAction::VaultImport { json });
                })
                .build(k)
                .key("vault-import"),
        );

    let mut section = col()
        .key("settings-section-vault")
        .w_full()
        .gap(8.0)
        .child(section_title(theme, "Protected Settings Vault"))
        .child(
            row().gap(10.0).items_center()
                .child(
                    text(storage)
                        .key("vault-storage-label")
                        .font(theme.mono_sm)
                        .color(theme.text_faint),
                )
                .child(
                    text(format!("{shown}/12 restore points"))
                        .key("vault-count")
                        .font(theme.mono_sm)
                        .color(theme.text_faint),
                ),
        )
        .child(sub_title(theme, "Restore Points & Portable Backup"))
        .child(actions)
        .child(import_row);
    if let Some(feedback) = &state.vault_feedback {
        section = section.child(
            text(feedback.clone())
                .key("vault-feedback")
                .font(theme.mono_sm)
                .color(theme.semantic_error_soft),
        );
    }

    if state.vault_backups.is_empty() {
        section = section.child(
            text("No restore points yet — save a backup before updating.")
                .key("vault-empty")
                .font(theme.ui_sm)
                .color(theme.text_faint),
        );
    }
    for backup in state.vault_backups.iter().take(12) {
        let id = backup.id.clone();
        let meta = format!(
            "v{} · {}",
            backup.app_version,
            format_compact_time(backup.created_at, state.now_ms)
        );
        section = section.child(
            row()
                .key(format!("vault-backup-row-{}", backup.id))
                .w_full()
                .gap(8.0)
                .items_center()
                .px(8.0)
                .py(6.0)
                .radius(theme.radius_control.min(10.0))
                .fill(theme.card_fill)
                .border(1.0, theme.border_subtle)
                .child(
                    el().w(10.0).h(10.0).radius(5.0).fill(parse_hex_color(
                        &backup.accent_hex,
                        theme.accent,
                    )),
                )
                .child(
                    col().grow(1.0).min_w(0.0).gap(2.0)
                        .child(
                            text(if backup.workspace_title.is_empty() {
                                "Hermes".to_string()
                            } else {
                                backup.workspace_title.clone()
                            })
                            .font(theme.ui_sm)
                            .color(theme.text),
                        )
                        .child(text(meta).font(theme.mono_sm).color(theme.text_faint)),
                )
                .child(
                    el().px(6.0).py(2.0).radius(6.0)
                        .fill(theme.glass_fill)
                        .border(1.0, theme.border_subtle)
                        .child(
                            text(backup.reason.clone())
                                .key(format!("vault-reason-{}", backup.id))
                                .font(theme.mono_sm)
                                .color(theme.text_muted),
                        ),
                )
                .child(
                    Button::new("Restore")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Mini)
                        .on_press(move |s: &mut AppState| {
                            s.settings_outbox.push(SettingsAction::VaultRestore {
                                backup_id: id.clone(),
                            });
                        })
                        .build(k)
                        .key(format!("vault-restore-{}", backup.id)),
                ),
        );
    }
    section
}

/// `UpdateCenterPanel` parity: receipt banner, version card, probe/apply
/// buttons, release row, dirty-worktree warning, error banner, step list.
/// All states come from `AppState` (bridge injects probe/apply results).
fn render_update_section(state: &AppState, theme: &VoxcellTheme) -> El {
    let k = &theme.kit;
    let status = state.update_status.as_ref();
    let mut section = col()
        .key("settings-section-updater")
        .w_full()
        .gap(8.0)
        .child(section_title(theme, "Update Center"));

    // Post-update receipt banner (shown on next launch until dismissed).
    if let Some(receipt) = status.and_then(|s| s.pending_receipt.as_ref()) {
        let mut version_line = format!("v{} → v{}", receipt.previous_version, receipt.new_version);
        if !receipt.backup_id.is_empty() {
            version_line.push_str(&format!(" · backup {}", receipt.backup_id));
        }
        let mut chips = row().gap(4.0).items_center();
        for (i, cat) in receipt.preserved_categories.iter().enumerate() {
            chips = chips.child(
                el().px(6.0).py(2.0).radius(6.0)
                    .fill(theme.glass_fill)
                    .border(1.0, theme.border_subtle)
                    .child(
                        text(cat.clone())
                            .key(format!("receipt-cat-{i}"))
                            .font(theme.mono_sm)
                            .color(theme.text_muted),
                    ),
            );
        }
        section = section.child(
            col()
                .key("update-receipt")
                .w_full()
                .gap(6.0)
                .px(10.0)
                .py(8.0)
                .radius(theme.radius_card.min(12.0))
                .fill(theme.card_fill)
                .border(1.0, theme.border)
                .child(
                    row().w_full().justify_between().items_center()
                        .child(
                            text("Updated cleanly — themes & settings preserved")
                                .font(TextStyle::new(12.0 * theme.font_scale, 600.0))
                                .color(theme.text),
                        )
                        .child(
                            Button::new("Dismiss")
                                .variant(ButtonVariant::Ghost)
                                .control_size(ControlSize::Mini)
                                .on_press(|s: &mut AppState| {
                                    if let Some(st) = &mut s.update_status {
                                        st.pending_receipt = None;
                                    }
                                    s.settings_outbox.push(SettingsAction::UpdaterDismissReceipt);
                                })
                                .build(k)
                                .key("update-receipt-dismiss"),
                        ),
                )
                .child(
                    text(version_line)
                        .key("update-receipt-version")
                        .font(theme.mono_sm)
                        .color(theme.text_muted),
                )
                .child(chips),
        );
    }

    // Version & click-to-update card.
    let current_version = status
        .map(|s| s.current_version.clone())
        .unwrap_or_else(|| "0.1.0".to_string());
    let mut version_chip = format!("v{current_version}");
    if let Some(commit) = status.and_then(|s| s.current_commit.as_deref()) {
        version_chip.push_str(&format!(" · {}", &commit[..commit.len().min(7)]));
    }
    let available = status.map(|s| s.update_available).unwrap_or(false);
    let chip_text = if available {
        format!(
            "Update available{}",
            status
                .and_then(|s| s.latest_version.as_deref())
                .map(|v| format!(" · v{v}"))
                .unwrap_or_default()
        )
    } else if state.update_checking || status.is_none() {
        "Checking…".to_string()
    } else {
        "Ready".to_string()
    };
    let mode_desc = match status.map(|s| s.update_mode.as_str()) {
        Some("git-pull") => format!(
            "Git workspace{} · snapshots themes & settings before pulling and rebuilding",
            status
                .and_then(|s| s.branch.as_deref())
                .map(|b| format!(" ({b})"))
                .unwrap_or_default()
        ),
        _ => "Snapshots your active theme, custom wallpapers, layout & workspace settings before applying update".to_string(),
    };

    let mut card = col()
        .key("update-version-card")
        .w_full()
        .gap(8.0)
        .px(10.0)
        .py(8.0)
        .radius(theme.radius_card.min(12.0))
        .fill(theme.card_fill)
        .border(1.0, theme.border_subtle)
        .child(
            row().gap(8.0).items_center()
                .child(
                    text("App Version")
                        .font(TextStyle::new(12.0 * theme.font_scale, 600.0))
                        .color(theme.text),
                )
                .child(
                    el().px(6.0).py(2.0).radius(6.0)
                        .fill(theme.glass_fill)
                        .border(1.0, theme.border_subtle)
                        .child(
                            text(version_chip)
                                .key("update-current-version")
                                .font(theme.mono_sm)
                                .color(theme.text_muted),
                        ),
                )
                .child(
                    el().px(6.0).py(2.0).radius(6.0)
                        .fill(if available {
                            theme.semantic_working.alpha(0.15)
                        } else {
                            theme.glass_fill
                        })
                        .border(1.0, if available {
                            theme.semantic_working.alpha(0.3)
                        } else {
                            theme.border_subtle
                        })
                        .child(
                            text(chip_text)
                                .key("update-status-chip")
                                .font(theme.mono_sm)
                                .color(if available {
                                    theme.semantic_working_soft
                                } else {
                                    theme.text_muted
                                }),
                        ),
                ),
        )
        .child(text(mode_desc).font(theme.ui_sm).color(theme.text_faint));

    if let Some(title) = status.and_then(|s| s.release_title.as_deref()) {
        let commit_suffix = status
            .and_then(|s| s.latest_commit.as_deref())
            .map(|c| format!(" ({})", &c[..c.len().min(7)]))
            .unwrap_or_default();
        let mut release_row = row().gap(8.0).items_center().child(
            text(format!("Upstream: {title}{commit_suffix}"))
                .font(theme.mono_sm)
                .color(theme.text_muted),
        );
        if let Some(url) = status.and_then(|s| s.release_url.as_deref()) {
            release_row = release_row.child(
                text("Notes")
                    .key("update-release-notes")
                    .font(theme.mono_sm)
                    .color(theme.accent_soft)
                    .href(url.to_string()),
            );
        }
        card = card.child(
            el().key("update-release-row").w_full().px(8.0).py(6.0)
                .radius(8.0)
                .fill(theme.glass_fill)
                .border(1.0, theme.border_subtle)
                .child(release_row),
        );
    }

    if status.map(|s| s.dirty_worktree).unwrap_or(false) {
        let reason = status
            .and_then(|s| s.dirty_reason.as_deref())
            .unwrap_or("local edits");
        card = card.child(
            el().key("update-dirty-banner").w_full().px(8.0).py(6.0).radius(8.0)
                .fill(theme.semantic_working.alpha(0.07))
                .border(1.0, theme.semantic_working.alpha(0.25))
                .child(
                    text(format!(
                        "Local repository has uncommitted changes ({reason}). Commit or stash local edits before pulling via git — your settings backup still works anytime."
                    ))
                    .font(theme.ui_sm)
                    .color(theme.semantic_working_soft),
                ),
        );
    }

    let error = state
        .update_error
        .as_deref()
        .or_else(|| status.and_then(|s| s.upstream_error.as_deref()));
    if let Some(err) = error {
        card = card.child(
            el().key("update-error-banner").w_full().px(8.0).py(6.0).radius(8.0)
                .fill(theme.semantic_error.alpha(0.09))
                .border(1.0, theme.semantic_error.alpha(0.3))
                .child(
                    text(err.to_string())
                        .font(theme.ui_sm)
                        .color(theme.semantic_error_soft),
                ),
        );
    }

    if !state.update_steps.is_empty() {
        let mut steps = col().key("update-steps-list").w_full().gap(4.0);
        for step in &state.update_steps {
            let dot_color = match step.status.as_str() {
                "failed" => theme.semantic_error,
                "skipped" => theme.text_faint,
                _ => theme.accent_soft,
            };
            let mut step_row = row()
                .key(format!("update-step-{}", step.id))
                .gap(6.0)
                .items_center()
                .child(el().w(8.0).h(8.0).radius(4.0).fill(dot_color))
                .child(text(step.label.clone()).font(theme.ui_sm).color(theme.text));
            if let Some(detail) = &step.detail {
                step_row = step_row.child(
                    text(detail.clone()).font(theme.mono_sm).color(theme.text_faint),
                );
            }
            steps = steps.child(step_row);
        }
        card = card.child(steps);
    }

    let checked_label = match status {
        Some(s) if s.checked_at > 0 => {
            format!("Checked {}", format_compact_time(s.checked_at, state.now_ms))
        }
        _ => "Checking status…".to_string(),
    };
    let mut button_row = row().gap(8.0).items_center().child(
        Button::new(if state.update_checking { "Checking…" } else { "Check updates" })
            .variant(ButtonVariant::Neutral)
            .control_size(ControlSize::Small)
            .on_press(|s: &mut AppState| {
                if s.update_applying {
                    return;
                }
                s.update_checking = true;
                s.settings_outbox.push(SettingsAction::UpdaterProbe);
            })
            .build(k)
            .key("check-updates-btn"),
    );
    if status.map(|s| s.update_available && s.can_self_update).unwrap_or(false) {
        button_row = button_row.child(
            Button::new(if state.update_applying { "Applying…" } else { "Apply update" })
                .variant(ButtonVariant::Submit)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut AppState| {
                    if s.update_applying {
                        return;
                    }
                    s.update_applying = true;
                    s.settings_outbox.push(SettingsAction::UpdaterApply);
                })
                .build(k)
                .key("apply-update-btn"),
        );
    }
    card = card.child(
        row().w_full().justify_between().items_center()
            .child(text(checked_label).font(theme.mono_sm).color(theme.text_faint))
            .child(button_row),
    );

    section.child(card)
}

pub fn render_settings_dialog(
    state: &AppState,
    theme: &VoxcellTheme,
    overlay_layer: &mut Vec<El>,
) {
    if !state.settings_open {
        return;
    }
    let k = &theme.kit;
    let content = col()
        .w_full()
        .padding(16.0)
        .gap(12.0)
        .child(render_gateway_section(state, theme))
        .child(Divider::horizontal().build(k))
        .child(render_voice_section(state, theme))
        .child(Divider::horizontal().build(k))
        .child(render_studio_section(state, theme))
        .child(Divider::horizontal().build(k))
        .child(render_backdrop_section(state, theme))
        .child(Divider::horizontal().build(k))
        .child(render_vault_section(state, theme))
        .child(Divider::horizontal().build(k))
        .child(render_update_section(state, theme));

    Dialog::new("settings", "Voxcell Settings & UI Customizer")
        .open(true)
        .description(
            "Gateway & profiles, Deepgram voice, appearance studio, glass backdrop, settings vault and native updates.",
        )
        .width(720.0)
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec { stick_bottom: false })
                .child(content),
        )
        .on_close(|s: &mut AppState| s.settings_open = false)
        .build(k, overlay_layer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        UpdaterStatus, UpdaterStep, VaultBackupEntry, VaultUpdateReceipt, settings_mood_from_id,
    };
    use rune::{Env, Runtime, TextEdit};
    use voxcell_core::ui_config::{get_curated_preset, ACCENT_SWATCHES as SWATCHES};
    use voxcell_core::{GatewayProfile, SidebarPosition};

    fn boot_settings() -> Runtime<AppState> {
        let mut s = AppState::new();
        s.apply_scene_preset("idle");
        s.settings_open = true;
        s.ui_config.reduced_motion = true;
        let mut rt = Runtime::new(
            s,
            crate::views::render_main_window,
            (1280.0, 800.0),
            Env::new(true, 1.0),
        );
        rt.frame(0.0);
        rt
    }

    fn settle(rt: &mut Runtime<AppState>, frames: usize) {
        for _ in 0..frames {
            rt.frame(0.016);
        }
    }

    /// Visible content band of the settings dialog (header + padding excluded).
    fn dialog_band(rt: &Runtime<AppState>) -> (f32, f32) {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("settings-dialog"))
            .map(|e| (e.rect[1] + 100.0, e.rect[1] + e.rect[3] - 20.0))
            .unwrap_or((316.0, 564.0))
    }

    /// The settings dialog scrolls; below-fold elements are laid out but not
    /// hit-testable (a click there lands on the scrim and closes the dialog).
    /// Wheel the dialog until the target sits inside the visible band.
    /// rune wheel: negative dy scrolls DOWN; reduced motion lands it at once.
    fn bring_into_view(rt: &mut Runtime<AppState>, key: &str) {
        let mut sign = 1.0_f32;
        let mut prev: Option<f32> = None;
        for _ in 0..80 {
            let Some((_, y)) = rt.find(key) else { return };
            let (lo, hi) = dialog_band(rt);
            if y >= lo && y <= hi {
                return;
            }
            let want = if y > hi { 1.0 } else { -1.0 };
            if let Some(py) = prev {
                if (y - py) * want > 0.0 {
                    sign = -sign; // scrolled the wrong way — flip
                }
            }
            prev = Some(y);
            rt.wheel(640.0, 400.0, -(want * sign * 200.0));
            rt.frame(0.016);
        }
    }

    fn click_key(rt: &mut Runtime<AppState>, key: &str) {
        bring_into_view(rt, key);
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("element {key:?}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.016);
    }

    fn click_text(rt: &mut Runtime<AppState>, label: &str) {
        let center = |rt: &Runtime<AppState>| -> (f32, f32) {
            let e = rt
                .elements()
                .into_iter()
                .find(|e| e.text.as_deref() == Some(label))
                .unwrap_or_else(|| panic!("element with text {label:?}"));
            // Use the laid-out rect: `.target` lags one frame behind wheel scrolls.
            (e.rect[0] + e.rect[2] * 0.5, e.rect[1] + e.rect[3] * 0.5)
        };
        let mut sign = 1.0_f32;
        let mut prev: Option<f32> = None;
        for _ in 0..80 {
            let (_, cy) = center(rt);
            let (lo, hi) = dialog_band(rt);
            if cy >= lo && cy <= hi {
                break;
            }
            let want = if cy > hi { 1.0 } else { -1.0 };
            if let Some(py) = prev {
                if (cy - py) * want > 0.0 {
                    sign = -sign;
                }
            }
            prev = Some(cy);
            rt.wheel(640.0, 400.0, -(want * sign * 200.0));
            rt.frame(0.016);
        }
        let (cx, cy) = center(rt);
        rt.pointer_down(cx, cy);
        rt.pointer_up(cx, cy);
        rt.frame(0.016);
    }

    fn has_text(rt: &Runtime<AppState>, needle: &str) -> bool {
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref().map(|t| t.contains(needle)).unwrap_or(false))
    }

    fn persist_ui_pushed(rt: &Runtime<AppState>) -> bool {
        rt.state
            .pending_commands
            .iter()
            .any(|c| matches!(c, UiCommand::PersistUiConfig))
    }

    fn sample_backup(id: &str, reason: &str, created_at: i64) -> VaultBackupEntry {
        VaultBackupEntry {
            id: id.to_string(),
            created_at,
            reason: reason.to_string(),
            app_version: "0.1.0".to_string(),
            accent_hex: "#38BDF8".to_string(),
            accent_preset: "cyan".to_string(),
            background_id: "nebula".to_string(),
            custom_slot_count: 0,
            workspace_title: "Hermes".to_string(),
        }
    }



    #[test]
    fn settings_sections_and_studio_controls_render() {
        let mut rt = boot_settings();
        settle(&mut rt, 2);
        for key in [
            "settings-section-gateway",
            "settings-section-voice",
            "settings-section-studio",
            "settings-section-backdrop",
            "settings-section-vault",
            "settings-section-updater",
        ] {
            assert!(rt.find(key).is_some(), "section {key}");
        }
        // 8 accent swatches + 7 curated presets + 7 backdrop presets.
        let els = rt.elements();
        let swatches = els
            .iter()
            .filter(|e| e.key.as_deref().map(|k| k.starts_with("accent-swatch-")).unwrap_or(false))
            .count();
        let presets = els
            .iter()
            .filter(|e| e.key.as_deref().map(|k| k.starts_with("ui-preset-")).unwrap_or(false))
            .count();
        let backdrops = els
            .iter()
            .filter(|e| e.key.as_deref().map(|k| k.starts_with("bg-preset-")).unwrap_or(false))
            .count();
        assert_eq!(swatches, 8, "8 accent swatches");
        assert_eq!(presets, 7, "7 curated presets");
        assert_eq!(backdrops, 7, "7 backdrop presets");
        assert_eq!(SWATCHES.len(), 8);
        assert!(rt.find("check-updates-btn").is_some());
        assert!(rt.find("vault-save-backup").is_some());
    }

    #[test]
    fn accent_swatch_apply_changes_theme() {
        let mut rt = boot_settings();
        settle(&mut rt, 2);
        let before = VoxcellTheme::from_config(&rt.state.ui_config);
        click_key(&mut rt, "accent-swatch-cyan");
        assert_eq!(rt.state.ui_config.accent_hex, "#38BDF8");
        assert_eq!(rt.state.ui_config.surface_hex, "#08141D");
        let after = VoxcellTheme::from_config(&rt.state.ui_config);
        assert_ne!(after.accent, before.accent, "theme accent changed");
        assert!(persist_ui_pushed(&rt));
        assert!(rt.state.ui_studio_feedback.is_some());
        // Swatch highlight follows the active accent.
        settle(&mut rt, 2);
        assert!(has_text(&rt, "Applied"), "studio feedback rendered");
    }

    #[test]
    fn curated_preset_apply_changes_theme_and_backdrop() {
        let mut rt = boot_settings();
        settle(&mut rt, 2);
        click_key(&mut rt, "ui-preset-cyber-violet");
        let preset = get_curated_preset("cyber-violet").expect("preset exists");
        assert_eq!(rt.state.ui_config.accent_hex, "#A855F7");
        assert_eq!(rt.state.ui_config.preset_name, "Cyber Violet");
        assert_eq!(rt.state.bg_settings.active_id, preset.background_preset_id);
        assert_eq!(
            rt.state.bg_settings.preset,
            BackgroundPresetId::from_web_id(preset.background_preset_id).unwrap()
        );
        if let Some(mood) = preset.mood {
            assert_eq!(rt.state.stage_mood, settings_mood_from_id(mood).unwrap());
        }
        assert!(persist_ui_pushed(&rt));
        assert!(rt
            .state
            .pending_commands
            .iter()
            .any(|c| matches!(c, UiCommand::PersistBackgroundSettings)));
    }

    #[test]
    fn nl_intent_input_applies_patch() {
        let mut rt = boot_settings();
        settle(&mut rt, 2);
        rt.update(|s, _tx| s.ui_prompt_edit = TextEdit::new("move sidebar to the right"));
        click_text(&mut rt, "Apply Style");
        assert_eq!(rt.state.ui_config.sidebar_position, SidebarPosition::Right);
        assert!(persist_ui_pushed(&rt));
        assert!(rt.state.ui_studio_feedback.is_some());
    }

    #[test]
    fn nl_intent_direct_phrases_and_rejection() {
        let mut rt = boot_settings();
        rt.update(|s, _tx| {
            assert!(s.settings_apply_intent("solaris-amber"));
        });
        assert_eq!(rt.state.ui_config.preset_name, "Solaris Amber");
        rt.update(|s, _tx| {
            assert!(s.settings_apply_intent("zen-mono"));
        });
        assert_eq!(rt.state.ui_config.preset_name, "Stealth Monograph");
        rt.update(|s, _tx| {
            assert!(!s.settings_apply_intent("what is the weather today"));
        });
        assert!(rt
            .state
            .ui_studio_feedback
            .as_deref()
            .unwrap_or_default()
            .contains("No UI intent recognized"));
    }

    #[test]
    fn bloom_stepper_routes_through_core_patch() {
        let mut rt = boot_settings();
        settle(&mut rt, 2);
        rt.update(|s, _tx| s.ui_config.bloom = 0.5);
        rt.frame(0.016);
        click_key(&mut rt, "bloom-plus");
        assert!((rt.state.ui_config.bloom - 0.55).abs() < 1e-4, "bloom +0.05");
        assert!(persist_ui_pushed(&rt));
        click_key(&mut rt, "bloom-minus");
        assert!((rt.state.ui_config.bloom - 0.5).abs() < 1e-4, "bloom -0.05");
        // Clamped at 1.0.
        rt.update(|s, _tx| s.ui_config.bloom = 1.0);
        rt.frame(0.016);
        click_key(&mut rt, "bloom-plus");
        assert!((rt.state.ui_config.bloom - 1.0).abs() < 1e-4);
    }

    #[test]
    fn backdrop_picker_applies_and_persists() {
        let mut rt = boot_settings();
        settle(&mut rt, 2);
        click_key(&mut rt, "bg-preset-nebula");
        assert_eq!(rt.state.bg_settings.preset, BackgroundPresetId::CyberMist);
        assert_eq!(rt.state.bg_settings.active_id, "nebula");
        assert!(rt
            .state
            .pending_commands
            .iter()
            .any(|c| matches!(c, UiCommand::PersistBackgroundSettings)));
        // Opacity stepper routes through the core patch + bg side effects.
        let before = rt.state.bg_settings.opacity;
        click_key(&mut rt, "bg-opacity-plus");
        assert!(rt.state.bg_settings.opacity > before);
    }

    #[test]
    fn vault_backup_list_labels_and_restore_dispatch() {
        let mut rt = boot_settings();
        rt.update(|s, _tx| {
            s.now_ms = 1_000_000;
            let now = s.now_ms as i64;
            s.vault_backups = vec![
                sample_backup("manual-1", "manual", now - 30_000),
                sample_backup("pre-update-2", "pre-update", now - 120_000),
            ];
        });
        settle(&mut rt, 2);
        assert!(rt.find("vault-backup-row-manual-1").is_some());
        assert!(rt.find("vault-backup-row-pre-update-2").is_some());
        let reason = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("vault-reason-pre-update-2"))
            .expect("reason chip");
        assert_eq!(reason.text.as_deref(), Some("pre-update"));
        assert!(has_text(&rt, "30s ago"), "relative created time");
        assert!(has_text(&rt, "2/12 restore points"));
        click_key(&mut rt, "vault-restore-pre-update-2");
        assert!(rt.state.settings_outbox.iter().any(|a| matches!(
            a,
            SettingsAction::VaultRestore { backup_id } if backup_id == "pre-update-2"
        )));
    }

    #[test]
    fn vault_save_export_import_dispatch() {
        let mut rt = boot_settings();
        settle(&mut rt, 2);
        click_key(&mut rt, "vault-save-backup");
        assert!(rt.state.settings_outbox.iter().any(|a| matches!(
            a,
            SettingsAction::VaultCreateBackup { reason } if reason == "manual"
        )));
        click_key(&mut rt, "vault-export");
        assert!(rt
            .state
            .settings_outbox
            .iter()
            .any(|a| matches!(a, SettingsAction::VaultExport)));
        // Empty import is rejected with feedback, no dispatch.
        click_key(&mut rt, "vault-import");
        assert!(rt.state.vault_feedback.is_some());
        assert!(!rt
            .state
            .settings_outbox
            .iter()
            .any(|a| matches!(a, SettingsAction::VaultImport { .. })));
        // Paste-JSON import dispatches the trimmed payload.
        rt.update(|s, _tx| s.vault_import_edit = TextEdit::new("  {\"schemaVersion\":1}  "));
        click_key(&mut rt, "vault-import");
        assert!(rt.state.settings_outbox.iter().any(|a| matches!(
            a,
            SettingsAction::VaultImport { json } if json == "{\"schemaVersion\":1}"
        )));
    }

    #[test]
    fn vault_rolling_12_display_cap() {
        let mut rt = boot_settings();
        rt.update(|s, _tx| {
            s.now_ms = 1_000_000;
            let now = s.now_ms as i64;
            s.vault_backups = (0..14)
                .map(|i| sample_backup(&format!("b{i:02}"), "manual", now - i as i64 * 60_000))
                .collect();
        });
        settle(&mut rt, 2);
        let rows = rt
            .elements()
            .into_iter()
            .filter(|e| {
                e.key
                    .as_deref()
                    .map(|k| k.starts_with("vault-backup-row-"))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(rows, 12, "rolling 12 restore points");
        assert!(has_text(&rt, "12/12 restore points"));
    }

    #[test]
    fn updater_receipt_rendering_and_dismiss() {
        let mut rt = boot_settings();
        rt.update(|s, _tx| {
            s.update_status = Some(UpdaterStatus {
                current_version: "0.2.0".to_string(),
                pending_receipt: Some(VaultUpdateReceipt {
                    updated_at: s.now_ms as i64 - 60_000,
                    previous_version: "0.1.0".to_string(),
                    new_version: "0.2.0".to_string(),
                    previous_commit: None,
                    new_commit: None,
                    backup_id: "pre-update-9".to_string(),
                    mode: "git-pull".to_string(),
                    preserved_categories: vec![
                        "Theme & accent".to_string(),
                        "Workspace layout".to_string(),
                    ],
                }),
                ..Default::default()
            });
        });
        settle(&mut rt, 2);
        assert!(rt.find("update-receipt").is_some());
        assert!(has_text(&rt, "Updated cleanly — themes & settings preserved"));
        let ver = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("update-receipt-version"))
            .expect("receipt version line");
        assert_eq!(
            ver.text.as_deref(),
            Some("v0.1.0 → v0.2.0 · backup pre-update-9")
        );
        assert!(rt.find("receipt-cat-0").is_some());
        assert!(rt.find("receipt-cat-1").is_some());
        click_key(&mut rt, "update-receipt-dismiss");
        assert!(rt
            .state
            .settings_outbox
            .iter()
            .any(|a| matches!(a, SettingsAction::UpdaterDismissReceipt)));
        assert!(rt.state.update_status.as_ref().unwrap().pending_receipt.is_none());
        settle(&mut rt, 2);
        assert!(rt.find("update-receipt").is_none(), "banner dismissed");
    }

    #[test]
    fn updater_probe_apply_steps_and_error() {
        let mut rt = boot_settings();
        rt.update(|s, _tx| {
            s.now_ms = 1_000_000;
            s.update_status = Some(UpdaterStatus {
                current_version: "0.1.0".to_string(),
                current_commit: Some("abcdef1234567".to_string()),
                branch: Some("rune-desktop".to_string()),
                latest_version: Some("0.2.0".to_string()),
                update_available: true,
                can_self_update: true,
                dirty_worktree: true,
                dirty_reason: Some("2 files".to_string()),
                release_title: Some("v0.2.0 — spatial canvas".to_string()),
                checked_at: s.now_ms as i64 - 30_000,
                ..Default::default()
            });
            s.update_steps = vec![
                UpdaterStep {
                    id: "backup".to_string(),
                    label: "Settings snapshot saved".to_string(),
                    status: "ok".to_string(),
                    detail: Some("pre-update-1".to_string()),
                },
                UpdaterStep {
                    id: "pull".to_string(),
                    label: "git pull --ff-only".to_string(),
                    status: "failed".to_string(),
                    detail: Some("exit 1".to_string()),
                },
            ];
            s.update_error = Some("git pull failed: local changes".to_string());
        });
        settle(&mut rt, 2);
        let chip = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("update-status-chip"))
            .expect("status chip");
        assert_eq!(chip.text.as_deref(), Some("Update available · v0.2.0"));
        assert!(has_text(&rt, "v0.1.0 · abcdef1"));
        assert!(rt.find("update-dirty-banner").is_some());
        assert!(rt.find("update-error-banner").is_some());
        assert!(rt.find("update-steps-list").is_some());
        assert!(rt.find("update-step-backup").is_some());
        assert!(has_text(&rt, "Settings snapshot saved"));
        assert!(has_text(&rt, "Upstream: v0.2.0 — spatial canvas"));
        assert!(has_text(&rt, "Checked 30s ago"));

        click_key(&mut rt, "check-updates-btn");
        assert!(rt.state.update_checking);
        assert!(rt
            .state
            .settings_outbox
            .iter()
            .any(|a| matches!(a, SettingsAction::UpdaterProbe)));
        click_key(&mut rt, "apply-update-btn");
        assert!(rt.state.update_applying);
        assert!(rt
            .state
            .settings_outbox
            .iter()
            .any(|a| matches!(a, SettingsAction::UpdaterApply)));
    }

    #[test]
    fn gateway_profile_switch_dispatch() {
        let mut rt = boot_settings();
        rt.update(|s, _tx| {
            s.gateway_profiles.push(GatewayProfile {
                name: "ops".to_string(),
                base_url: "http://127.0.0.1:8643".to_string(),
                active: false,
                online: true,
                model: Some("hermes-agent".to_string()),
                source: "profile".to_string(),
            });
        });
        settle(&mut rt, 2);
        click_key(&mut rt, "gateway-profile-ops");
        assert_eq!(rt.state.gateway.active_profile, "ops");
        assert!(rt.state.pending_commands.iter().any(|c| matches!(
            c,
            UiCommand::SwitchProfile { profile } if profile == "ops"
        )));
    }

    #[test]
    fn compact_time_format_parity() {
        let now = 1_800_000_000_000_u64;
        assert_eq!(format_compact_time(0, now), "just now");
        assert_eq!(format_compact_time(now as i64 - 5_000, now), "just now");
        assert_eq!(format_compact_time(now as i64 - 30_000, now), "30s ago");
        assert_eq!(format_compact_time(now as i64 - 120_000, now), "2m ago");
        // 1_700_000_000_000 ms = 2023-11-14 22:13:20 UTC.
        assert_eq!(format_compact_time(1_700_000_000_000, now), "Nov 14 22:13");
    }

    // ---- P5 native-chrome toggle (headless) ----

    #[test]
    fn native_chrome_toggle_flips_state_and_renders_both_modes() {
        let mut rt = boot_settings();
        settle(&mut rt, 2);
        assert!(!rt.state.native_chrome, "default off");
        assert!(rt.find("native-chrome-toggle").is_some(), "toggle row present");
        assert!(rt.find("native-chrome-row").is_some());
        // A11y: the switch surfaces its name in the accessibility tree.
        let a11y = rt.accessibility_json().to_string();
        assert!(a11y.contains("Native Window Chrome"), "switch named for a11y");
        click_key(&mut rt, "native-chrome-toggle");
        assert!(rt.state.native_chrome, "toggle flips on");
        settle(&mut rt, 2);
        assert!(rt.find("settings-dialog").is_some(), "chrome-on mode renders");
        assert!(rt.find("native-chrome-toggle").is_some());
        click_key(&mut rt, "native-chrome-toggle");
        assert!(!rt.state.native_chrome, "toggle flips back off");
        settle(&mut rt, 2);
        assert!(rt.find("settings-dialog").is_some(), "chrome-off mode renders");
    }
}
