//! `slate-v1` VoxcellTheme — spec §6 (P3-01).
//!
//! Consumes `voxcell-core::ui_config` (8 accent swatches, 7 curated presets,
//! sanitizers/patch appliers) — nothing is duplicated here. Charcoal-Slate is
//! the default epoch: bg `#0B0D12`, surface `#11141B`, accent `#94A3B8`.
//! Emerald/Amber/Rose survive ONLY as semantic live/working/error telemetry.
//!
//! Typography (§6.3): Plus Jakarta Sans UI (12–14px, 400/500/600), Archivo
//! display headings (600/700), JetBrains Mono with `tabular-nums` (`'tnum' 1`)
//! for timers, counters, FFT readouts and CLI. Radius modes: `rounded`
//! (16px cards / 12px controls), `sharp` (4px), `pill` (26px / 18px).
//! Reduced motion zeroes stagger and lands springs at once.

use rune::{Color, El, Family, Spring, TextStyle, el};
use rune_kit::{Kit, PaletteFocus, PaletteSeed};
use voxcell_core::motion::{StaggerOptions, compute_stagger_delay_ms};
use voxcell_core::ui_config::slate_v1;
use voxcell_core::{
    BackgroundPresetId, BackgroundSettings, CornerRadius, DensityMode, FontPreset,
    UiCustomizationConfig,
};

/// Web `--font-sans` (src/index.css).
pub const FONT_UI: &str = "Plus Jakarta Sans";
/// Web `--font-display`.
pub const FONT_DISPLAY: &str = "Archivo";
/// Web `--font-mono`.
pub const FONT_MONO: &str = "JetBrains Mono";
/// OpenType tabular figures for mono readouts (web `font-variant-numeric`).
pub const MONO_FEATURES: &str = "'tnum' 1";

pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color { r, g, b, a }
}

pub fn parse_hex_color(hex: &str, fallback: Color) -> Color {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&clean[0..2], 16),
            u8::from_str_radix(&clean[2..4], 16),
            u8::from_str_radix(&clean[4..6], 16),
        ) {
            return rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0);
        }
    }
    fallback
}

/// Card/control corner radii for a radius mode (§6.3: rounded 16/12, sharp 4,
/// pill 26/18).
pub fn radius_pair(mode: CornerRadius) -> (f32, f32) {
    match mode {
        CornerRadius::Sharp => (4.0, 4.0),
        CornerRadius::Rounded => (16.0, 12.0),
        CornerRadius::Pill => (26.0, 18.0),
    }
}

#[derive(Clone)]
pub struct VoxcellTheme {
    pub kit: Kit,
    pub bg: Color,
    pub surface: Color,
    pub surface_elevated: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub accent_bright: Color,
    /// Semantic telemetry only — live/voice/completed (never ambient chrome).
    pub semantic_live: Color,
    pub semantic_live_soft: Color,
    /// Semantic telemetry only — running/thinking tasks.
    pub semantic_working: Color,
    pub semantic_working_soft: Color,
    /// Semantic telemetry only — offline/error/stop gates.
    pub semantic_error: Color,
    pub semantic_error_soft: Color,
    /// Legacy aliases kept for drafted views: amber = working, rose = error.
    pub amber: Color,
    pub rose: Color,
    pub text: Color,
    pub text_muted: Color,
    pub text_faint: Color,
    /// Active border: accent at `border_opacity` (web `--ui-border-alpha`).
    pub border: Color,
    /// Active-tab / focused border (web active tab 0.52).
    pub border_strong: Color,
    /// Hairline structural divider (web `rgba(255,255,255,0.08)`).
    pub border_subtle: Color,
    pub glass_fill: Color,
    pub card_fill: Color,
    /// Ambient glow color: accent at `glow_intensity * 0.1` (web 0.03 default).
    pub glow: Color,

    pub radius_mode: CornerRadius,
    pub radius_card: f32,
    pub radius_control: f32,
    pub density: DensityMode,
    pub font_scale: f32,
    pub reduced_motion: bool,
    pub animation_speed: f32,

    /// UI body text — Plus Jakarta Sans (or the active font preset), 13px/500.
    pub ui: TextStyle,
    /// Small UI text — 12px/400.
    pub ui_sm: TextStyle,
    /// Display headings — Archivo 700.
    pub heading: TextStyle,
    /// Large display title — Archivo 700.
    pub title: TextStyle,
    /// JetBrains Mono, tabular-nums — timers/counters/CLI.
    pub mono: TextStyle,
    pub mono_sm: TextStyle,
}

impl VoxcellTheme {
    pub fn from_config(cfg: &UiCustomizationConfig) -> Self {
        // slate-v1 fallbacks come from core tokens, not hardcoded emerald.
        let bg = parse_hex_color(&cfg.bg_hex, parse_hex_color(slate_v1::BG_CANVAS, rgba(0.043, 0.051, 0.071, 1.0)));
        let surface = parse_hex_color(&cfg.surface_hex, parse_hex_color(slate_v1::BG_SURFACE, rgba(0.067, 0.078, 0.106, 1.0)));
        let fg = parse_hex_color(&cfg.text_hex, parse_hex_color(slate_v1::TEXT_PRIMARY, rgba(0.945, 0.961, 0.976, 1.0)));
        let accent = parse_hex_color(&cfg.accent_hex, parse_hex_color(slate_v1::ACCENT_PRIMARY, rgba(0.58, 0.64, 0.72, 1.0)));
        let accent_soft = parse_hex_color(&cfg.accent_soft_hex, parse_hex_color(slate_v1::ACCENT_SOFT, accent));
        let accent_bright = parse_hex_color(&cfg.accent_bright_hex, parse_hex_color(slate_v1::ACCENT_BRIGHT, accent));
        let amber = parse_hex_color(&cfg.amber_hex, parse_hex_color(slate_v1::SEMANTIC_WORKING, rgba(0.961, 0.620, 0.043, 1.0)));
        let rose = parse_hex_color(&cfg.rose_hex, parse_hex_color(slate_v1::SEMANTIC_ERROR, rgba(0.957, 0.247, 0.369, 1.0)));
        let live = parse_hex_color(slate_v1::SEMANTIC_LIVE, rgba(0.204, 0.827, 0.60, 1.0));
        let live_soft = parse_hex_color(slate_v1::SEMANTIC_LIVE_SOFT, live);
        let working_soft = parse_hex_color(slate_v1::SEMANTIC_WORKING_SOFT, amber);
        let error_soft = parse_hex_color(slate_v1::SEMANTIC_ERROR_SOFT, rose);

        let mut kit = Kit::from_palette_focus(
            PaletteSeed { bg, fg, accent },
            PaletteFocus::Accent,
        );
        // Elevated surface: surface lifted toward the accent (web #1A202C /
        // rgba(148,163,184,0.12) over #11141B at slate defaults).
        let lift = |t: f32| {
            rgba(
                surface.r + (accent.r - surface.r) * t,
                surface.g + (accent.g - surface.g) * t,
                surface.b + (accent.b - surface.b) * t,
                1.0,
            )
        };
        let elevated = lift(0.12);
        kit.c.layer_01 = surface;
        kit.c.layer_02 = elevated;
        kit.c.layer_03 = lift(0.20);
        kit.c.warning.fg = amber;
        kit.c.danger.fg = rose;
        kit.c.success.fg = accent;
        kit.c.text_base = fg;
        kit.c.text_accent = accent;

        let panel_alpha = cfg.panel_alpha.clamp(0.55, 0.98);
        let glass_fill = rgba(surface.r, surface.g, surface.b, panel_alpha);
        let card_fill = rgba(elevated.r, elevated.g, elevated.b, (panel_alpha * 0.88).clamp(0.45, 0.96));
        let border = accent.alpha(cfg.border_opacity.clamp(0.04, 0.8));
        let border_strong = accent.alpha(0.52);
        let border_subtle = rgba(1.0, 1.0, 1.0, 0.08);
        let glow = accent.alpha((cfg.glow_intensity.clamp(0.0, 2.0) * 0.1).clamp(0.0, 0.25));

        let (radius_card, radius_control) = radius_pair(cfg.corner_radius);
        let scale = cfg.font_scale.clamp(0.75, 1.5);

        // Font preset selects the active UI stack (web --ui-font-active);
        // display headings stay Archivo and mono stays JetBrains Mono.
        let ui_family = match cfg.font_preset {
            FontPreset::Jakarta => Family::Named(FONT_UI),
            FontPreset::Mono => Family::Named(FONT_MONO),
            FontPreset::Display => Family::Named(FONT_DISPLAY),
            FontPreset::System => Family::Sans,
        };
        let ui = TextStyle::new(13.0 * scale, 500.0).lh(18.0 * scale).family(ui_family);
        let ui_sm = TextStyle::new(12.0 * scale, 400.0).lh(16.0 * scale).family(ui_family);
        let heading = TextStyle::new(16.0 * scale, 700.0).lh(22.0 * scale).family(Family::Named(FONT_DISPLAY));
        let title = TextStyle::new(20.0 * scale, 700.0).lh(26.0 * scale).family(Family::Named(FONT_DISPLAY));
        let mono = TextStyle::new(12.5 * scale, 400.0).lh(18.0 * scale).family(Family::Named(FONT_MONO)).features(MONO_FEATURES);
        let mono_sm = TextStyle::new(11.0 * scale, 400.0).lh(15.0 * scale).family(Family::Named(FONT_MONO)).features(MONO_FEATURES);

        Self {
            text_muted: kit.c.text_muted,
            text_faint: kit.c.text_faint,
            surface_elevated: elevated,
            kit,
            bg,
            surface,
            accent,
            accent_soft,
            accent_bright,
            semantic_live: live,
            semantic_live_soft: live_soft,
            semantic_working: amber,
            semantic_working_soft: working_soft,
            semantic_error: rose,
            semantic_error_soft: error_soft,
            amber,
            rose,
            text: fg,
            border,
            border_strong,
            border_subtle,
            glass_fill,
            card_fill,
            glow,
            radius_mode: cfg.corner_radius,
            radius_card,
            radius_control,
            density: cfg.density,
            font_scale: scale,
            reduced_motion: cfg.reduced_motion,
            animation_speed: cfg.animation_speed.clamp(0.25, 2.5),
            ui,
            ui_sm,
            heading,
            title,
            mono,
            mono_sm,
        }
    }

    /// Layout/color spring for chrome transitions; lands at once under
    /// reduced motion (§6.3 "disables choreography springs").
    pub fn spring(&self) -> Spring {
        if self.reduced_motion {
            Spring::NONE
        } else {
            Spring::from_motion(0.30, 0.0)
        }
    }

    /// Spring for the gliding titlebar tab indicator (a touch snappier).
    pub fn tab_spring(&self) -> Spring {
        if self.reduced_motion {
            Spring::NONE
        } else {
            Spring::from_motion(0.24, 0.0)
        }
    }

    /// Stagger delay (ms) for item `index` via `core::motion` — speed-scaled,
    /// capped, and zeroed under reduced motion.
    pub fn stagger_ms(&self, index: usize) -> f64 {
        compute_stagger_delay_ms(
            index as f64,
            StaggerOptions {
                speed_scale: Some(self.animation_speed as f64),
                reduced_motion: Some(self.reduced_motion),
                ..StaggerOptions::default()
            },
        )
    }

    /// Entrance transition for staggered chrome items (fade + small rise).
    pub fn stagger_transition(&self) -> rune::Transition {
        rune::Transition::offset(0.0, 6.0).combined(rune::Transition::opacity())
    }
}

/// Atmospheric charcoal-slate backdrop field (§6.1: slate 700/800 radial
/// washes over `#0B0D12`). Preset moods tint with the active accent; the
/// default slate accent keeps the field fully monochrome. Reduced motion
/// freezes the drift.
pub fn render_ambient_backdrop(
    theme: &VoxcellTheme,
    bg_settings: &BackgroundSettings,
    time_s: f32,
    reduced_motion: bool,
) -> El {
    let t = if reduced_motion || theme.reduced_motion {
        0.0
    } else {
        time_s * bg_settings.motion_speed.clamp(0.1, 2.5) * theme.animation_speed
    };
    let op = bg_settings.opacity.clamp(0.1, 1.0);
    let inten = bg_settings.intensity.clamp(0.1, 1.5);

    // Slate wash constants from src/index.css .emerald-ambient-bg:
    // slate-600 #475569 (71,85,105), slate-700 #334155 (51,65,85),
    // slate-800 #1E293B (30,41,59).
    let slate600 = rgba(0.278, 0.333, 0.412, 1.0);
    let slate700 = rgba(0.2, 0.255, 0.333, 1.0);
    let slate800 = rgba(0.118, 0.161, 0.231, 1.0);

    let (primary, secondary, drift_x, drift_y) = match bg_settings.preset {
        BackgroundPresetId::EmeraldAurora => (
            slate600.alpha(0.28 * op * inten),
            slate800.alpha(0.45 * op * inten),
            0.52 + (t * 0.22).sin() * 0.08,
            0.34 + (t * 0.17).cos() * 0.06,
        ),
        BackgroundPresetId::AbyssGrid => (
            slate700.alpha(0.24 * op * inten),
            theme.accent.alpha(0.10 * op * inten),
            0.50,
            0.48,
        ),
        BackgroundPresetId::CyberMist => (
            theme.accent.alpha(0.16 * op * inten),
            slate700.alpha(0.22 * op * inten),
            0.44 + (t * 0.28).sin() * 0.10,
            0.32 + (t * 0.21).cos() * 0.08,
        ),
        BackgroundPresetId::SolarFlare => (
            theme.semantic_working.alpha(0.16 * op * inten),
            slate800.alpha(0.30 * op * inten),
            0.56 + (t * 0.24).cos() * 0.09,
            0.30 + (t * 0.19).sin() * 0.07,
        ),
        BackgroundPresetId::MidnightPulse => (
            slate700.alpha(0.30 * op * inten),
            theme.accent.alpha(0.10 * op * inten),
            0.48 + (t * 0.32).sin() * 0.07,
            0.42 + (t * 0.27).cos() * 0.07,
        ),
        BackgroundPresetId::ObsidianSilk => (
            slate600.alpha(0.14 * op * inten),
            theme.accent.alpha(0.06 * op * inten),
            0.50 + (t * 0.14).sin() * 0.04,
            0.38,
        ),
        BackgroundPresetId::PhosphorGrain => (
            theme.accent.alpha(0.18 * op * inten),
            slate800.alpha(0.28 * op * inten),
            0.50 + (t * 0.35).sin() * 0.05,
            0.45 + (t * 0.25).cos() * 0.05,
        ),
    };

    el().key("ambient-backdrop")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .fill(theme.bg)
        .radial_gradient(
            [drift_x, drift_y],
            560.0,
            [
                (0.0, primary),
                (0.45, secondary),
                (1.0, theme.bg.alpha(0.0)),
            ],
        )
        .a11y_hidden()
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxcell_core::ui_config::{
        ACCENT_SWATCHES, apply_ui_patch, curated_ui_presets, get_accent_swatch,
    };

    fn theme_with(cfg: UiCustomizationConfig) -> VoxcellTheme {
        VoxcellTheme::from_config(&cfg)
    }

    #[test]
    fn default_theme_is_slate_v1_not_emerald() {
        let t = theme_with(UiCustomizationConfig::default());
        assert_eq!(t.bg, parse_hex_color("#0B0D12", t.bg));
        assert_eq!(t.surface, parse_hex_color("#11141B", t.surface));
        assert_eq!(t.accent, parse_hex_color("#94A3B8", t.accent));
        assert_eq!(t.accent_soft, parse_hex_color("#CBD5E1", t.accent));
        assert_eq!(t.accent_bright, parse_hex_color("#E2E8F0", t.accent));
        assert_eq!(t.text, parse_hex_color("#F1F5F9", t.text));
        // Emerald/amber/rose remain semantic telemetry, not chrome.
        assert_eq!(t.semantic_live, parse_hex_color("#34D399", t.accent));
        assert_eq!(t.semantic_working, parse_hex_color("#F59E0B", t.accent));
        assert_eq!(t.semantic_error, parse_hex_color("#F43F5E", t.accent));
        assert_ne!(t.accent, t.semantic_live);
        // Default rounded radii: 16px cards / 12px controls.
        assert_eq!((t.radius_card, t.radius_control), (16.0, 12.0));
        assert_eq!(t.radius_mode, CornerRadius::Rounded);
    }

    #[test]
    fn typography_stacks_and_tabular_nums() {
        let t = theme_with(UiCustomizationConfig::default());
        assert_eq!(t.ui.family, Family::Named(FONT_UI));
        assert_eq!(t.ui_sm.family, Family::Named(FONT_UI));
        assert_eq!(t.heading.family, Family::Named(FONT_DISPLAY));
        assert_eq!(t.title.family, Family::Named(FONT_DISPLAY));
        assert_eq!(t.mono.family, Family::Named(FONT_MONO));
        assert_eq!(t.mono_sm.family, Family::Named(FONT_MONO));
        assert!(t.mono.features.contains("'tnum' 1"));
        assert!(t.mono_sm.features.contains("'tnum' 1"));
        // UI weights/sizes in the §6.3 band.
        assert_eq!(t.ui.size, 13.0);
        assert_eq!(t.ui.weight, 500.0);
        assert_eq!(t.heading.weight, 700.0);

        // Font presets switch the active UI stack; mono/display stay put.
        let mut cfg = UiCustomizationConfig::default();
        cfg.font_preset = FontPreset::Mono;
        assert_eq!(theme_with(cfg.clone()).ui.family, Family::Named(FONT_MONO));
        cfg.font_preset = FontPreset::Display;
        assert_eq!(theme_with(cfg.clone()).ui.family, Family::Named(FONT_DISPLAY));
        cfg.font_preset = FontPreset::System;
        assert_eq!(theme_with(cfg.clone()).ui.family, Family::Sans);
        // font_scale scales sizes.
        cfg.font_preset = FontPreset::Jakarta;
        cfg.font_scale = 1.2;
        let scaled = theme_with(cfg);
        assert!((scaled.ui.size - 13.0 * 1.2).abs() < 1e-4);
    }

    #[test]
    fn radius_modes_rounded_sharp_pill() {
        let mut cfg = UiCustomizationConfig::default();
        cfg.corner_radius = CornerRadius::Sharp;
        let t = theme_with(cfg.clone());
        assert_eq!((t.radius_card, t.radius_control), (4.0, 4.0));
        cfg.corner_radius = CornerRadius::Pill;
        let t = theme_with(cfg.clone());
        assert_eq!((t.radius_card, t.radius_control), (26.0, 18.0));
        cfg.corner_radius = CornerRadius::Rounded;
        let t = theme_with(cfg);
        assert_eq!((t.radius_card, t.radius_control), (16.0, 12.0));
    }

    #[test]
    fn all_eight_core_swatchescarry_into_theme() {
        assert_eq!(ACCENT_SWATCHES.len(), 8);
        for sw in ACCENT_SWATCHES {
            let mut cfg = UiCustomizationConfig::default();
            cfg.accent_hex = sw.accent_hex.to_string();
            cfg.accent_soft_hex = sw.accent_soft_hex.to_string();
            cfg.accent_bright_hex = sw.accent_bright_hex.to_string();
            cfg.surface_hex = sw.surface_hex.to_string();
            let t = theme_with(cfg);
            assert_eq!(t.accent, parse_hex_color(sw.accent_hex, t.bg), "swatch {}", sw.id);
            assert_eq!(t.accent_soft, parse_hex_color(sw.accent_soft_hex, t.bg));
            assert_eq!(t.accent_bright, parse_hex_color(sw.accent_bright_hex, t.bg));
            assert_eq!(t.surface, parse_hex_color(sw.surface_hex, t.bg));
            assert!(get_accent_swatch(sw.id).is_some());
        }
    }

    #[test]
    fn all_seven_curated_presets_apply_through_core() {
        let presets = curated_ui_presets();
        assert_eq!(presets.len(), 7);
        for p in presets {
            // Core patch applier merges the partial preset config (TS spread).
            let outcome = apply_ui_patch(&UiCustomizationConfig::default(), &p.config);
            let t = theme_with(outcome.next.clone());
            let obj = p.config.as_object().expect("preset config object");
            if let Some(hex) = obj.get("accentHex").and_then(|v| v.as_str()) {
                assert_eq!(t.accent, parse_hex_color(hex, t.bg), "preset {}", p.id);
            }
            if let Some(hex) = obj.get("surfaceHex").and_then(|v| v.as_str()) {
                assert_eq!(t.surface, parse_hex_color(hex, t.bg), "preset {}", p.id);
            }
            if let Some(mode) = obj.get("cornerRadius").and_then(|v| v.as_str()) {
                assert_eq!(t.radius_mode.as_str(), mode, "preset {}", p.id);
            }
            if let Some(fp) = obj.get("fontPreset").and_then(|v| v.as_str()) {
                let expect = match fp {
                    "mono" => Family::Named(FONT_MONO),
                    "display" => Family::Named(FONT_DISPLAY),
                    "system" => Family::Sans,
                    _ => Family::Named(FONT_UI),
                };
                assert_eq!(t.ui.family, expect, "preset {}", p.id);
            }
        }
        // The default preset is slate monochrome.
        assert_eq!(presets[0].id, "slate-terminal");
    }

    #[test]
    fn reduced_motion_zeroes_stagger_and_springs() {
        let mut cfg = UiCustomizationConfig::default();
        let lively = theme_with(cfg.clone());
        assert!(!lively.reduced_motion);
        assert_ne!(lively.spring(), Spring::NONE);
        assert_ne!(lively.tab_spring(), Spring::NONE);
        assert_eq!(lively.stagger_ms(0), 0.0);
        assert_eq!(lively.stagger_ms(3), 90.0); // core default step 30ms
        assert_eq!(lively.stagger_ms(100), 240.0); // core cap

        cfg.reduced_motion = true;
        let calm = theme_with(cfg);
        assert!(calm.reduced_motion);
        assert_eq!(calm.spring(), Spring::NONE);
        assert_eq!(calm.tab_spring(), Spring::NONE);
        for i in 0..8 {
            assert_eq!(calm.stagger_ms(i), 0.0);
        }
    }

    #[test]
    fn panel_border_glow_tokens_follow_config() {
        let mut cfg = UiCustomizationConfig::default();
        cfg.panel_alpha = 0.76;
        cfg.border_opacity = 0.42;
        cfg.glow_intensity = 1.0;
        let t = theme_with(cfg);
        assert!((t.glass_fill.a - 0.76).abs() < 1e-4);
        assert!((t.border.a - 0.42).abs() < 1e-4);
        assert!((t.glow.a - 0.10).abs() < 1e-4);
        assert_eq!(t.border_subtle, rgba(1.0, 1.0, 1.0, 0.08));
    }
}
