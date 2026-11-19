//! UI customization — port of `src/utils/uiCustomization.ts` (spec P1-03, §6).
//!
//! `slate-v1` Charcoal-Slate epoch: defaults are bg `#0B0D12`, surface
//! `#11141B`, accent `#94A3B8` (soft `#CBD5E1`, bright `#E2E8F0`, text
//! `#F1F5F9`). Emerald `#34D399` / Amber `#F59E0B` / Rose `#F43F5E` remain
//! ONLY as semantic live/working/error telemetry colors and opt-in accents —
//! never ambient chrome defaults.
//!
//! Two API layers live here:
//! - **Web parity** (`normalize_hex`, `sanitize_ui_patch`, `apply_ui_patch`,
//!   `parse_natural_ui_intent`, `ACCENT_SWATCHES`, `curated_ui_presets`) —
//!   mirrors the TypeScript module 1:1, including clamp ranges, blend
//!   fallbacks and the natural-language intent grammar.
//! - **Legacy scaffold** (`sanitize_hex_color`, `apply_ui_customization_patch`,
//!   `parse_natural_language_ui_request`, `apply_preset`, `BUILT_IN_PRESETS`,
//!   `decode_frontier_step`) — kept so existing `voxcell-ui` consumers and
//!   their tests keep compiling and passing.

use crate::types::{
    AccentPreset, ComposerStyle, CornerRadius, DensityMode, DotProfile, FontPreset,
    HorizonMode, SidebarPosition, UiCustomizationConfig,
};
use serde_json::{json, Value};
use std::sync::LazyLock;

/// Theme epoch marker (web `UI_THEME_EPOCH_VALUE`).
pub const THEME_EPOCH: &str = "slate-v1";

/// `slate-v1` core tokens (spec §6.1 / `src/index.css`).
pub mod slate_v1 {
    pub const BG_CANVAS: &str = "#0B0D12";
    pub const BG_SURFACE: &str = "#11141B";
    pub const BG_ELEVATED: &str = "#1A202C";
    pub const ACCENT_PRIMARY: &str = "#94A3B8";
    pub const ACCENT_SOFT: &str = "#CBD5E1";
    pub const ACCENT_BRIGHT: &str = "#E2E8F0";
    pub const TEXT_PRIMARY: &str = "#F1F5F9";
    /// Semantic telemetry only — live/voice/completed.
    pub const SEMANTIC_LIVE: &str = "#34D399";
    pub const SEMANTIC_LIVE_SOFT: &str = "#6EE7B7";
    /// Semantic telemetry only — running/thinking tasks.
    pub const SEMANTIC_WORKING: &str = "#F59E0B";
    pub const SEMANTIC_WORKING_SOFT: &str = "#FCD34D";
    /// Semantic telemetry only — offline/error/stop gates.
    pub const SEMANTIC_ERROR: &str = "#F43F5E";
    pub const SEMANTIC_ERROR_SOFT: &str = "#FDA4AF";
}

// ---------------------------------------------------------------------------
// Accent swatches (8) — exact port of ACCENT_SWATCHES
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct AccentSwatch {
    pub id: &'static str,
    pub label: &'static str,
    pub accent_hex: &'static str,
    pub accent_soft_hex: &'static str,
    pub accent_bright_hex: &'static str,
    pub surface_hex: &'static str,
}

pub const ACCENT_SWATCHES: &[AccentSwatch] = &[
    AccentSwatch {
        id: "slate",
        label: "Slate",
        accent_hex: "#94A3B8",
        accent_soft_hex: "#CBD5E1",
        accent_bright_hex: "#E2E8F0",
        surface_hex: "#11141B",
    },
    AccentSwatch {
        id: "emerald",
        label: "Emerald",
        accent_hex: "#34D399",
        accent_soft_hex: "#6EE7B7",
        accent_bright_hex: "#4ADE80",
        surface_hex: "#0A1613",
    },
    AccentSwatch {
        id: "cyan",
        label: "Glacier Cyan",
        accent_hex: "#38BDF8",
        accent_soft_hex: "#7DD3FC",
        accent_bright_hex: "#22D3EE",
        surface_hex: "#08141D",
    },
    AccentSwatch {
        id: "violet",
        label: "Cyber Violet",
        accent_hex: "#A855F7",
        accent_soft_hex: "#D8B4FE",
        accent_bright_hex: "#C084FC",
        surface_hex: "#120B1E",
    },
    AccentSwatch {
        id: "amber",
        label: "Solaris Amber",
        accent_hex: "#F59E0B",
        accent_soft_hex: "#FCD34D",
        accent_bright_hex: "#FBBF24",
        surface_hex: "#161108",
    },
    AccentSwatch {
        id: "rose",
        label: "Sakura Rose",
        accent_hex: "#FB7185",
        accent_soft_hex: "#FDA4AF",
        accent_bright_hex: "#F472B6",
        surface_hex: "#190B12",
    },
    AccentSwatch {
        id: "crimson",
        label: "Crimson",
        accent_hex: "#EF4444",
        accent_soft_hex: "#FCA5A5",
        accent_bright_hex: "#F87171",
        surface_hex: "#19090B",
    },
    AccentSwatch {
        id: "mono",
        label: "Monograph",
        accent_hex: "#E2E8F0",
        accent_soft_hex: "#F8FAFC",
        accent_bright_hex: "#CBD5E1",
        surface_hex: "#090B0E",
    },
];

pub fn get_accent_swatch(id: &str) -> Option<&'static AccentSwatch> {
    ACCENT_SWATCHES.iter().find(|s| s.id == id)
}

// ---------------------------------------------------------------------------
// Curated presets (7) — exact port of CURATED_UI_PRESETS
// ---------------------------------------------------------------------------

/// Full curated preset definition. `config` is a *partial* camelCase config
/// object, merged verbatim over the current config exactly like the TS
/// `{ ...next, ...preset.config }` spread (no re-sanitization).
#[derive(Clone, Debug)]
pub struct CuratedUiPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub subtitle: &'static str,
    pub background_preset_id: &'static str,
    pub background_opacity: f32,
    pub glass_blur_px: f32,
    pub mood: Option<&'static str>,
    pub config: Value,
}

pub fn curated_ui_presets() -> &'static [CuratedUiPreset] {
    static PRESETS: LazyLock<Vec<CuratedUiPreset>> = LazyLock::new(|| {
        vec![
            CuratedUiPreset {
                id: "slate-terminal",
                name: "Slate Monochrome",
                subtitle: "Modern graphite glass & neutral telemetry",
                background_preset_id: "aurora-drift",
                background_opacity: 0.58,
                glass_blur_px: 14.0,
                mood: Some("focused"),
                config: json!({
                    "accentPreset": "slate",
                    "accentHex": "#94A3B8",
                    "accentSoftHex": "#CBD5E1",
                    "accentBrightHex": "#E2E8F0",
                    "surfaceHex": "#11141B",
                    "textHex": "#F1F5F9",
                    "panelAlpha": 0.80,
                    "borderOpacity": 0.22,
                    "glowIntensity": 0.30,
                    "cornerRadius": "rounded",
                    "density": "comfortable",
                    "fontPreset": "jakarta",
                    "fontScale": 1.0,
                    "sidebarVisible": true,
                    "sidebarPosition": "left",
                    "sidebarWidthPx": 296,
                    "sidebarQueueVisible": true,
                    "sidebarChannelsVisible": true,
                    "showTopNav": true,
                    "composerStyle": "docked",
                    "workspaceTitle": "Hermes",
                    "portraitVisible": true,
                    "portraitScale": 1.0,
                    "portraitTintHex": null,
                    "brailleDotProfile": "balanced",
                    "brailleBloom": 0.75,
                    "horizonVisible": true,
                    "horizonOpacity": 0.8,
                    "horizonStyle": "silk",
                    "statusReadoutVisible": true,
                    "animationSpeed": 1.0,
                    "reducedMotion": false
                }),
            },
            CuratedUiPreset {
                id: "emerald-terminal",
                name: "Emerald Terminal",
                subtitle: "Signature smoked-emerald Hermes cockpit",
                background_preset_id: "aurora-drift",
                background_opacity: 0.64,
                glass_blur_px: 12.0,
                mood: Some("happy"),
                config: json!({
                    "accentPreset": "emerald",
                    "accentHex": "#34D399",
                    "accentSoftHex": "#6EE7B7",
                    "accentBrightHex": "#4ADE80",
                    "surfaceHex": "#0A1613",
                    "textHex": "#ECF3F0",
                    "panelAlpha": 0.76,
                    "borderOpacity": 0.42,
                    "glowIntensity": 1.0,
                    "cornerRadius": "rounded",
                    "density": "comfortable",
                    "fontPreset": "jakarta",
                    "fontScale": 1.0,
                    "sidebarVisible": true,
                    "sidebarPosition": "left",
                    "sidebarWidthPx": 296,
                    "sidebarQueueVisible": true,
                    "sidebarChannelsVisible": true,
                    "showTopNav": true,
                    "composerStyle": "docked",
                    "workspaceTitle": "Hermes",
                    "portraitVisible": true,
                    "portraitScale": 1.0,
                    "portraitTintHex": null,
                    "brailleDotProfile": "balanced",
                    "brailleBloom": 0.75,
                    "horizonVisible": true,
                    "horizonOpacity": 0.8,
                    "horizonStyle": "silk",
                    "statusReadoutVisible": true,
                    "animationSpeed": 1.0,
                    "reducedMotion": false
                }),
            },
            CuratedUiPreset {
                id: "cyber-violet",
                name: "Cyber Violet",
                subtitle: "Neon plasma nebula & ultraviolet glass",
                background_preset_id: "nebula",
                background_opacity: 0.72,
                glass_blur_px: 16.0,
                mood: Some("focused"),
                config: json!({
                    "accentPreset": "violet",
                    "accentHex": "#A855F7",
                    "accentSoftHex": "#D8B4FE",
                    "accentBrightHex": "#C084FC",
                    "surfaceHex": "#120B1E",
                    "textHex": "#F3EEFA",
                    "panelAlpha": 0.74,
                    "borderOpacity": 0.54,
                    "glowIntensity": 1.35,
                    "cornerRadius": "rounded",
                    "density": "comfortable",
                    "fontPreset": "display",
                    "fontScale": 1.0,
                    "portraitTintHex": "#E9D5FF",
                    "brailleDotProfile": "phosphor",
                    "brailleBloom": 1.1,
                    "horizonVisible": true,
                    "horizonOpacity": 0.85,
                    "horizonStyle": "pulse"
                }),
            },
            CuratedUiPreset {
                id: "solaris-amber",
                name: "Solaris Amber",
                subtitle: "Retro phosphor CRT monospace telemetry",
                background_preset_id: "scanline-shimmer",
                background_opacity: 0.78,
                glass_blur_px: 8.0,
                mood: Some("focused"),
                config: json!({
                    "accentPreset": "amber",
                    "accentHex": "#F59E0B",
                    "accentSoftHex": "#FCD34D",
                    "accentBrightHex": "#FBBF24",
                    "surfaceHex": "#141007",
                    "textHex": "#FEF3C7",
                    "panelAlpha": 0.82,
                    "borderOpacity": 0.58,
                    "glowIntensity": 1.25,
                    "cornerRadius": "sharp",
                    "density": "compact",
                    "fontPreset": "mono",
                    "fontScale": 0.98,
                    "composerStyle": "docked",
                    "portraitTintHex": "#FDE68A",
                    "brailleDotProfile": "crisp",
                    "brailleBloom": 0.85,
                    "horizonVisible": true,
                    "horizonOpacity": 0.72,
                    "horizonStyle": "peaks"
                }),
            },
            CuratedUiPreset {
                id: "glacier-cyan",
                name: "Glacier Ice",
                subtitle: "Frosted Arctic blue glass & orbital nodes",
                background_preset_id: "constellation-drift",
                background_opacity: 0.68,
                glass_blur_px: 20.0,
                mood: Some("neutral"),
                config: json!({
                    "accentPreset": "cyan",
                    "accentHex": "#38BDF8",
                    "accentSoftHex": "#7DD3FC",
                    "accentBrightHex": "#22D3EE",
                    "surfaceHex": "#07131C",
                    "textHex": "#E0F2FE",
                    "panelAlpha": 0.68,
                    "borderOpacity": 0.48,
                    "glowIntensity": 1.15,
                    "cornerRadius": "rounded",
                    "density": "airy",
                    "fontPreset": "jakarta",
                    "fontScale": 1.02,
                    "portraitTintHex": "#BAE6FD",
                    "horizonVisible": true,
                    "horizonOpacity": 0.78
                }),
            },
            CuratedUiPreset {
                id: "sakura-rose",
                name: "Sakura Nocturne",
                subtitle: "Soft petal pills & bioluminescent tide",
                background_preset_id: "horizon-tide",
                background_opacity: 0.7,
                glass_blur_px: 14.0,
                mood: Some("happy"),
                config: json!({
                    "accentPreset": "rose",
                    "accentHex": "#FB7185",
                    "accentSoftHex": "#FDA4AF",
                    "accentBrightHex": "#F472B6",
                    "surfaceHex": "#180B12",
                    "textHex": "#FFF1F2",
                    "panelAlpha": 0.74,
                    "borderOpacity": 0.5,
                    "glowIntensity": 1.2,
                    "cornerRadius": "pill",
                    "density": "comfortable",
                    "fontPreset": "jakarta",
                    "fontScale": 1.0,
                    "portraitScale": 1.1,
                    "portraitTintHex": "#FECDD3",
                    "horizonVisible": true,
                    "horizonOpacity": 0.88
                }),
            },
            CuratedUiPreset {
                id: "zen-mono",
                name: "Stealth Monograph",
                subtitle: "Zero-clutter minimalist focus shell",
                background_preset_id: "braille-rain",
                background_opacity: 0.42,
                glass_blur_px: 10.0,
                mood: Some("neutral"),
                config: json!({
                    "accentPreset": "mono",
                    "accentHex": "#E2E8F0",
                    "accentSoftHex": "#F8FAFC",
                    "accentBrightHex": "#CBD5E1",
                    "surfaceHex": "#090B0E",
                    "textHex": "#F1F5F9",
                    "panelAlpha": 0.84,
                    "borderOpacity": 0.24,
                    "glowIntensity": 0.35,
                    "cornerRadius": "sharp",
                    "density": "compact",
                    "fontPreset": "mono",
                    "fontScale": 0.98,
                    "sidebarVisible": false,
                    "showTopNav": true,
                    "composerStyle": "minimal",
                    "portraitVisible": true,
                    "portraitScale": 0.96,
                    "portraitTintHex": "#F1F5F9",
                    "horizonVisible": false,
                    "horizonOpacity": 0.35,
                    "statusReadoutVisible": true
                }),
            },
        ]
    });
    &PRESETS
}

pub fn get_curated_preset(id: &str) -> Option<&'static CuratedUiPreset> {
    curated_ui_presets().iter().find(|p| p.id == id)
}

/// Legacy display-facing preset list consumed by `voxcell-ui::views::settings`
/// (uses `.id` / `.name`). Now the 7 curated `slate-v1` presets; `bg_hex` stays
/// the constant charcoal canvas for every preset (web has no per-preset bg).
#[derive(Clone, Debug, PartialEq)]
pub struct UiPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub accent_hex: &'static str,
    pub bg_hex: &'static str,
    pub surface_hex: &'static str,
    pub dot_profile: DotProfile,
    pub bloom: f32,
    pub horizon_mode: HorizonMode,
}

pub const BUILT_IN_PRESETS: &[UiPreset] = &[
    UiPreset {
        id: "slate-terminal",
        name: "Slate Monochrome",
        accent_hex: "#94A3B8",
        bg_hex: "#0B0D12",
        surface_hex: "#11141B",
        dot_profile: DotProfile::Balanced,
        bloom: 0.75,
        horizon_mode: HorizonMode::Silk,
    },
    UiPreset {
        id: "emerald-terminal",
        name: "Emerald Terminal",
        accent_hex: "#34D399",
        bg_hex: "#0B0D12",
        surface_hex: "#0A1613",
        dot_profile: DotProfile::Balanced,
        bloom: 0.75,
        horizon_mode: HorizonMode::Silk,
    },
    UiPreset {
        id: "cyber-violet",
        name: "Cyber Violet",
        accent_hex: "#A855F7",
        bg_hex: "#0B0D12",
        surface_hex: "#120B1E",
        dot_profile: DotProfile::Phosphor,
        bloom: 1.1,
        horizon_mode: HorizonMode::Pulse,
    },
    UiPreset {
        id: "solaris-amber",
        name: "Solaris Amber",
        accent_hex: "#F59E0B",
        bg_hex: "#0B0D12",
        surface_hex: "#141007",
        dot_profile: DotProfile::Crisp,
        bloom: 0.85,
        horizon_mode: HorizonMode::Peaks,
    },
    UiPreset {
        id: "glacier-cyan",
        name: "Glacier Ice",
        accent_hex: "#38BDF8",
        bg_hex: "#0B0D12",
        surface_hex: "#07131C",
        dot_profile: DotProfile::Balanced,
        bloom: 0.75,
        horizon_mode: HorizonMode::Silk,
    },
    UiPreset {
        id: "sakura-rose",
        name: "Sakura Nocturne",
        accent_hex: "#FB7185",
        bg_hex: "#0B0D12",
        surface_hex: "#180B12",
        dot_profile: DotProfile::Balanced,
        bloom: 0.75,
        horizon_mode: HorizonMode::Silk,
    },
    UiPreset {
        id: "zen-mono",
        name: "Stealth Monograph",
        accent_hex: "#E2E8F0",
        bg_hex: "#0B0D12",
        surface_hex: "#090B0E",
        dot_profile: DotProfile::Balanced,
        bloom: 0.75,
        horizon_mode: HorizonMode::Silk,
    },
];

// ---------------------------------------------------------------------------
// Patch type (superset of the legacy scaffold patch + full web patch surface)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiCustomizationPatch {
    // Legacy scaffold fields (kept for compatibility).
    pub accent_hex: Option<String>,
    pub surface_hex: Option<String>,
    pub bg_hex: Option<String>,
    pub portrait_tint_hex: Option<Option<String>>,
    pub dot_profile: Option<DotProfile>,
    pub bloom: Option<f32>,
    pub horizon_mode: Option<HorizonMode>,
    pub density: Option<DensityMode>,
    pub sidebar_position: Option<SidebarPosition>,
    pub sidebar_width: Option<f32>,
    pub reduced_motion: Option<bool>,
    pub preset_name: Option<String>,
    // Web-parity fields (`UiCustomizationPatch` in uiCustomization.ts).
    pub preset_id: Option<String>,
    pub accent_preset: Option<AccentPreset>,
    pub accent_soft_hex: Option<String>,
    pub accent_bright_hex: Option<String>,
    pub text_hex: Option<String>,
    pub panel_alpha: Option<f32>,
    pub border_opacity: Option<f32>,
    pub glow_intensity: Option<f32>,
    pub corner_radius: Option<CornerRadius>,
    pub font_preset: Option<FontPreset>,
    pub font_scale: Option<f32>,
    pub sidebar_visible: Option<bool>,
    pub sidebar_queue_visible: Option<bool>,
    pub sidebar_channels_visible: Option<bool>,
    pub show_top_nav: Option<bool>,
    pub composer_style: Option<ComposerStyle>,
    pub workspace_title: Option<String>,
    pub portrait_visible: Option<bool>,
    pub portrait_scale: Option<f32>,
    pub horizon_visible: Option<bool>,
    pub horizon_opacity: Option<f32>,
    pub status_readout_visible: Option<bool>,
    pub animation_speed: Option<f32>,
    pub background_preset_id: Option<String>,
    pub background_opacity: Option<f32>,
    pub glass_blur_px: Option<f32>,
    pub active_view: Option<String>,
    pub mood: Option<String>,
    pub show_thinking: Option<bool>,
    pub voice_muted: Option<bool>,
    pub open_customizer: Option<bool>,
    pub close_customizer: Option<bool>,
    pub canvas_auto_arrange: Option<bool>,
    pub canvas_mood_all: Option<String>,
    pub reset_all: bool,
}

// ---------------------------------------------------------------------------
// Color & numeric helpers (port of normalizeHex / clampNumber / blendHex)
// ---------------------------------------------------------------------------

/// TS `normalizeHex`: trim, prepend `#` when missing, expand 3-digit shorthand,
/// uppercase 6-digit form; `None` for anything else.
pub fn normalize_hex(input: &str) -> Option<String> {
    let clean = input.trim();
    if clean.is_empty() {
        return None;
    }
    let with_hash = if clean.starts_with('#') {
        clean.to_string()
    } else {
        format!("#{clean}")
    };
    let digits = &with_hash[1..];
    if digits.len() == 3 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        let expanded: String = digits.chars().flat_map(|c| [c, c]).collect();
        return Some(format!("#{}", expanded.to_ascii_uppercase()));
    }
    if digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(format!("#{}", digits.to_ascii_uppercase()));
    }
    None
}

/// Legacy scaffold hex sanitizer (behavior preserved for existing callers).
pub fn sanitize_hex_color(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_start_matches('#');
    if trimmed.len() == 3 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        let expanded: String = trimmed
            .chars()
            .flat_map(|c| [c, c])
            .collect::<String>()
            .to_ascii_uppercase();
        return Some(format!("#{expanded}"));
    }
    if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(format!("#{}", trimmed.to_ascii_uppercase()));
    }
    None
}

/// TS `clampNumber`: non-finite/non-number falls back, then clamp + round to
/// `precision` decimal places.
pub fn clamp_number(
    value: Option<&Value>,
    min: f64,
    max: f64,
    fallback: f64,
    precision: u32,
) -> f32 {
    let Some(n) = value.and_then(Value::as_f64) else {
        return fallback as f32;
    };
    if !n.is_finite() {
        return fallback as f32;
    }
    let clamped = n.max(min).min(max);
    let factor = 10f64.powi(precision as i32);
    ((clamped * factor).round() / factor) as f32
}

pub fn hex_to_rgb_tuple(hex: &str) -> (u8, u8, u8) {
    let norm = normalize_hex(hex).unwrap_or_else(|| "#94A3B8".to_string());
    let r = u8::from_str_radix(&norm[1..3], 16).unwrap_or(0x94);
    let g = u8::from_str_radix(&norm[3..5], 16).unwrap_or(0xA3);
    let b = u8::from_str_radix(&norm[5..7], 16).unwrap_or(0xB8);
    (r, g, b)
}

pub fn blend_hex(hex_a: &str, hex_b: &str, t: f32) -> String {
    let (r1, g1, b1) = hex_to_rgb_tuple(hex_a);
    let (r2, g2, b2) = hex_to_rgb_tuple(hex_b);
    let w = t.clamp(0.0, 1.0) as f64;
    let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * w).round() as u8;
    format!("#{:02X}{:02X}{:02X}", mix(r1, r2), mix(g1, g2), mix(b1, b2))
}

// ---------------------------------------------------------------------------
// sanitizeUiPatch — validates/clamps any raw JSON patch
// ---------------------------------------------------------------------------

fn kv<'a>(raw: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    let obj = raw.as_object()?;
    obj.get(camel).or_else(|| obj.get(snake))
}

fn as_str(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}

fn as_bool(v: Option<&Value>) -> Option<bool> {
    v.and_then(Value::as_bool)
}

fn is_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

const VALID_MOODS: &[&str] = &[
    "neutral", "listening", "thinking", "speaking", "happy", "focused", "alert",
];
const VALID_VIEWS: &[&str] = &["home", "canvas", "voice", "queue", "cli"];
pub const VALID_BACKGROUND_PRESET_IDS: &[&str] = &[
    "aurora-drift",
    "mesh-gradient",
    "braille-rain",
    "scanline-shimmer",
    "nebula",
    "constellation-drift",
    "horizon-tide",
];

fn valid_enum<T>(raw: Option<&Value>, parse: impl Fn(&str) -> Option<T>) -> Option<T> {
    parse(as_str(raw)?)
}

/// Port of `sanitizeUiPatch(raw)`. Accepts camelCase keys (web parity) and
/// snake_case aliases (desktop-native JSON). Unknown/invalid entries dropped.
pub fn sanitize_ui_patch(raw: &Value) -> UiCustomizationPatch {
    let mut out = UiCustomizationPatch::default();
    if !raw.is_object() {
        return out;
    }

    if raw.get("resetAll").and_then(Value::as_bool) == Some(true)
        || raw.get("reset_all").and_then(Value::as_bool) == Some(true)
    {
        out.reset_all = true;
        return out;
    }

    if let Some(pid) = as_str(kv(raw, "presetId", "preset_id")) {
        if let Some(preset) = get_curated_preset(&pid.trim().to_ascii_lowercase()) {
            out.preset_id = Some(preset.id.to_string());
        }
    }

    // Desktop-only extension (no web counterpart): preserve the display name
    // of the active preset across export/import roundtrips.
    if let Some(name) = as_str(kv(raw, "presetName", "preset_name")) {
        if !name.trim().is_empty() {
            out.preset_name = Some(name.to_string());
        }
    }

    if let Some(ap) = valid_enum(kv(raw, "accentPreset", "accent_preset"), AccentPreset::from_id) {
        out.accent_preset = Some(ap);
        if let Some(swatch) = get_accent_swatch(ap.as_str()) {
            out.accent_hex = Some(swatch.accent_hex.to_string());
            out.accent_soft_hex = Some(swatch.accent_soft_hex.to_string());
            out.accent_bright_hex = Some(swatch.accent_bright_hex.to_string());
            if !is_truthy(kv(raw, "surfaceHex", "surface_hex")) {
                out.surface_hex = Some(swatch.surface_hex.to_string());
            }
        }
    }

    if let Some(accent) = as_str(kv(raw, "accentHex", "accent_hex")).and_then(normalize_hex) {
        out.accent_hex = Some(accent.clone());
        if out.accent_preset.is_none() {
            let matched = ACCENT_SWATCHES.iter().find(|s| s.accent_hex == accent);
            out.accent_preset = Some(matched.map(|s| AccentPreset::from_id(s.id).unwrap()).unwrap_or(AccentPreset::Custom));
        }
        if as_str(kv(raw, "accentSoftHex", "accent_soft_hex")).and_then(normalize_hex).is_none() {
            out.accent_soft_hex = Some(blend_hex(&accent, "#FFFFFF", 0.42));
        }
        if as_str(kv(raw, "accentBrightHex", "accent_bright_hex")).and_then(normalize_hex).is_none() {
            out.accent_bright_hex = Some(blend_hex(&accent, "#FFFFFF", 0.22));
        }
        if as_str(kv(raw, "surfaceHex", "surface_hex")).and_then(normalize_hex).is_none()
            && out.accent_preset == Some(AccentPreset::Custom)
        {
            out.surface_hex = Some(blend_hex(slate_v1::BG_CANVAS, &accent, 0.08));
        }
    }

    if let Some(soft) = as_str(kv(raw, "accentSoftHex", "accent_soft_hex")).and_then(normalize_hex) {
        out.accent_soft_hex = Some(soft);
    }
    if let Some(bright) = as_str(kv(raw, "accentBrightHex", "accent_bright_hex")).and_then(normalize_hex) {
        out.accent_bright_hex = Some(bright);
    }
    if let Some(surface) = as_str(kv(raw, "surfaceHex", "surface_hex")).and_then(normalize_hex) {
        // Keep surface dark enough for crisp readable contrast.
        let (r, g, b) = hex_to_rgb_tuple(&surface);
        let lum = (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) / 255.0;
        out.surface_hex = Some(if lum > 0.22 {
            blend_hex("#06090B", &surface, 0.28)
        } else {
            surface
        });
    }
    if let Some(text) = as_str(kv(raw, "textHex", "text_hex")).and_then(normalize_hex) {
        out.text_hex = Some(text);
    }

    let d = UiCustomizationConfig::default();
    if kv(raw, "panelAlpha", "panel_alpha").and_then(Value::as_f64).is_some() {
        out.panel_alpha = Some(clamp_number(kv(raw, "panelAlpha", "panel_alpha"), 0.32, 0.95, d.panel_alpha as f64, 2));
    }
    if kv(raw, "borderOpacity", "border_opacity").and_then(Value::as_f64).is_some() {
        out.border_opacity = Some(clamp_number(kv(raw, "borderOpacity", "border_opacity"), 0.08, 0.9, d.border_opacity as f64, 2));
    }
    if kv(raw, "glowIntensity", "glow_intensity").and_then(Value::as_f64).is_some() {
        out.glow_intensity = Some(clamp_number(kv(raw, "glowIntensity", "glow_intensity"), 0.0, 2.0, d.glow_intensity as f64, 2));
    }

    out.corner_radius = valid_enum(kv(raw, "cornerRadius", "corner_radius"), CornerRadius::from_id);
    out.density = valid_enum(kv(raw, "density", "density"), |s| match s {
        "compact" => Some(DensityMode::Compact),
        "comfortable" => Some(DensityMode::Comfortable),
        "airy" => Some(DensityMode::Airy),
        _ => None,
    });
    out.font_preset = valid_enum(kv(raw, "fontPreset", "font_preset"), FontPreset::from_id);
    if kv(raw, "fontScale", "font_scale").and_then(Value::as_f64).is_some() {
        out.font_scale = Some(clamp_number(kv(raw, "fontScale", "font_scale"), 0.85, 1.25, d.font_scale as f64, 2));
    }

    out.sidebar_visible = as_bool(kv(raw, "sidebarVisible", "sidebar_visible"));
    out.sidebar_position = valid_enum(kv(raw, "sidebarPosition", "sidebar_position"), |s| match s {
        "left" => Some(SidebarPosition::Left),
        "right" => Some(SidebarPosition::Right),
        _ => None,
    });
    if kv(raw, "sidebarWidthPx", "sidebar_width_px").or_else(|| kv(raw, "sidebarWidth", "sidebar_width")).and_then(Value::as_f64).is_some() {
        let v = kv(raw, "sidebarWidthPx", "sidebar_width_px").or_else(|| kv(raw, "sidebarWidth", "sidebar_width"));
        out.sidebar_width = Some(clamp_number(v, 236.0, 420.0, d.sidebar_width as f64, 0));
    }
    out.sidebar_queue_visible = as_bool(kv(raw, "sidebarQueueVisible", "sidebar_queue_visible"));
    out.sidebar_channels_visible = as_bool(kv(raw, "sidebarChannelsVisible", "sidebar_channels_visible"));
    out.show_top_nav = as_bool(kv(raw, "showTopNav", "show_top_nav"));
    out.composer_style = valid_enum(kv(raw, "composerStyle", "composer_style"), ComposerStyle::from_id);
    if let Some(title) = as_str(kv(raw, "workspaceTitle", "workspace_title")) {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            out.workspace_title = Some(trimmed.chars().take(28).collect());
        }
    }

    out.portrait_visible = as_bool(kv(raw, "portraitVisible", "portrait_visible"));
    if kv(raw, "portraitScale", "portrait_scale").and_then(Value::as_f64).is_some() {
        out.portrait_scale = Some(clamp_number(kv(raw, "portraitScale", "portrait_scale"), 0.6, 1.45, d.portrait_scale as f64, 2));
    }
    match kv(raw, "portraitTintHex", "portrait_tint_hex") {
        Some(Value::Null) => out.portrait_tint_hex = Some(None),
        Some(Value::String(s)) if s == "auto" || s.is_empty() => out.portrait_tint_hex = Some(None),
        Some(v) => {
            if let Some(tint) = v.as_str().and_then(normalize_hex) {
                out.portrait_tint_hex = Some(Some(tint));
            }
        }
        None => {}
    }

    out.dot_profile = valid_enum(kv(raw, "brailleDotProfile", "braille_dot_profile").or_else(|| kv(raw, "dotProfile", "dot_profile")), |s| match s {
        "crisp" => Some(DotProfile::Crisp),
        "balanced" => Some(DotProfile::Balanced),
        "phosphor" => Some(DotProfile::Phosphor),
        _ => None,
    });
    if kv(raw, "brailleBloom", "braille_bloom").or_else(|| kv(raw, "bloom", "bloom")).and_then(Value::as_f64).is_some() {
        let v = kv(raw, "brailleBloom", "braille_bloom").or_else(|| kv(raw, "bloom", "bloom"));
        out.bloom = Some(clamp_number(v, 0.0, 1.5, d.bloom as f64, 2));
    }
    out.horizon_visible = as_bool(kv(raw, "horizonVisible", "horizon_visible"));
    if kv(raw, "horizonOpacity", "horizon_opacity").and_then(Value::as_f64).is_some() {
        out.horizon_opacity = Some(clamp_number(kv(raw, "horizonOpacity", "horizon_opacity"), 0.0, 1.0, d.horizon_opacity as f64, 2));
    }
    // TS VALID_HORIZON_STYLES = silk | peaks | pulse (ribbon is a legacy
    // desktop-only mode handled by the scaffold parser, not the web sanitizer).
    out.horizon_mode = valid_enum(kv(raw, "horizonStyle", "horizon_style").or_else(|| kv(raw, "horizonMode", "horizon_mode")), |s| match s {
        "silk" => Some(HorizonMode::Silk),
        "peaks" => Some(HorizonMode::Peaks),
        "pulse" => Some(HorizonMode::Pulse),
        _ => None,
    });
    out.status_readout_visible = as_bool(kv(raw, "statusReadoutVisible", "status_readout_visible"));
    if kv(raw, "animationSpeed", "animation_speed").and_then(Value::as_f64).is_some() {
        out.animation_speed = Some(clamp_number(kv(raw, "animationSpeed", "animation_speed"), 0.3, 1.8, d.animation_speed as f64, 2));
    }
    out.reduced_motion = as_bool(kv(raw, "reducedMotion", "reduced_motion"));

    if let Some(bg) = as_str(kv(raw, "backgroundPresetId", "background_preset_id")) {
        let bg_id = bg.trim().to_ascii_lowercase();
        if VALID_BACKGROUND_PRESET_IDS.contains(&bg_id.as_str()) || bg_id.starts_with("custom-") {
            out.background_preset_id = Some(bg_id);
        }
    }
    if kv(raw, "backgroundOpacity", "background_opacity").and_then(Value::as_f64).is_some() {
        out.background_opacity = Some(clamp_number(kv(raw, "backgroundOpacity", "background_opacity"), 0.15, 0.85, 0.64, 2));
    }
    if kv(raw, "glassBlurPx", "glass_blur_px").and_then(Value::as_f64).is_some() {
        out.glass_blur_px = Some(clamp_number(kv(raw, "glassBlurPx", "glass_blur_px"), 0.0, 28.0, 12.0, 0));
    }

    out.active_view = valid_enum(kv(raw, "activeView", "active_view"), |s| {
        VALID_VIEWS.contains(&s).then(|| s.to_string())
    });
    out.mood = valid_enum(kv(raw, "mood", "mood"), |s| {
        VALID_MOODS.contains(&s).then(|| s.to_string())
    });
    out.show_thinking = as_bool(kv(raw, "showThinking", "show_thinking"));
    out.voice_muted = as_bool(kv(raw, "voiceMuted", "voice_muted"));
    out.open_customizer = as_bool(kv(raw, "openCustomizer", "open_customizer"));
    out.close_customizer = as_bool(kv(raw, "closeCustomizer", "close_customizer"));
    out.canvas_auto_arrange = as_bool(kv(raw, "canvasAutoArrange", "canvas_auto_arrange"));
    out.canvas_mood_all = valid_enum(kv(raw, "canvasMoodAll", "canvas_mood_all"), |s| {
        VALID_MOODS.contains(&s).then(|| s.to_string())
    });

    out
}

// ---------------------------------------------------------------------------
// applyUiPatch — sanitized merge with changedKeys + summary
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct UiPatchOutcome {
    pub next: UiCustomizationConfig,
    pub sanitized: UiCustomizationPatch,
    pub changed_keys: Vec<String>,
    pub summary: String,
}

fn push_unique(keys: &mut Vec<String>, key: impl Into<String>) {
    let key = key.into();
    if !keys.contains(&key) {
        keys.push(key);
    }
}

/// Merge a curated preset's partial config verbatim (TS spread semantics).
fn merge_preset_config(cfg: &mut UiCustomizationConfig, config: &Value) {
    let Some(obj) = config.as_object() else { return };
    let str_of = |k: &str| obj.get(k).and_then(Value::as_str).map(str::to_string);
    let f32_of = |k: &str| obj.get(k).and_then(Value::as_f64).map(|v| v as f32);
    let bool_of = |k: &str| obj.get(k).and_then(Value::as_bool);

    if let Some(v) = str_of("accentPreset").as_deref().and_then(AccentPreset::from_id) { cfg.accent_preset = v; }
    if let Some(v) = str_of("accentHex") { cfg.accent_hex = v; }
    if let Some(v) = str_of("accentSoftHex") { cfg.accent_soft_hex = v; }
    if let Some(v) = str_of("accentBrightHex") { cfg.accent_bright_hex = v; }
    if let Some(v) = str_of("surfaceHex") { cfg.surface_hex = v; }
    if let Some(v) = str_of("textHex") { cfg.text_hex = v; }
    if let Some(v) = f32_of("panelAlpha") { cfg.panel_alpha = v; cfg.glass_opacity = v; }
    if let Some(v) = f32_of("borderOpacity") { cfg.border_opacity = v; }
    if let Some(v) = f32_of("glowIntensity") { cfg.glow_intensity = v; }
    if let Some(v) = str_of("cornerRadius").as_deref().and_then(CornerRadius::from_id) { cfg.corner_radius = v; }
    if let Some(v) = str_of("density") {
        cfg.density = match v.as_str() { "compact" => DensityMode::Compact, "airy" => DensityMode::Airy, _ => DensityMode::Comfortable };
    }
    if let Some(v) = str_of("fontPreset").as_deref().and_then(FontPreset::from_id) { cfg.font_preset = v; }
    if let Some(v) = f32_of("fontScale") { cfg.font_scale = v; }
    if let Some(v) = bool_of("sidebarVisible") { cfg.sidebar_visible = v; }
    if let Some(v) = str_of("sidebarPosition") { cfg.sidebar_position = if v == "right" { SidebarPosition::Right } else { SidebarPosition::Left }; }
    if let Some(v) = f32_of("sidebarWidthPx") { cfg.sidebar_width = v; }
    if let Some(v) = bool_of("sidebarQueueVisible") { cfg.sidebar_queue_visible = v; }
    if let Some(v) = bool_of("sidebarChannelsVisible") { cfg.sidebar_channels_visible = v; }
    if let Some(v) = bool_of("showTopNav") { cfg.show_top_nav = v; }
    if let Some(v) = str_of("composerStyle").as_deref().and_then(ComposerStyle::from_id) { cfg.composer_style = v; }
    if let Some(v) = str_of("workspaceTitle") { cfg.workspace_title = v; }
    if let Some(v) = bool_of("portraitVisible") { cfg.portrait_visible = v; }
    if let Some(v) = f32_of("portraitScale") { cfg.portrait_scale = v; }
    if let Some(v) = obj.get("portraitTintHex") {
        cfg.portrait_tint_hex = v.as_str().map(str::to_string);
    }
    if let Some(v) = str_of("brailleDotProfile") {
        cfg.dot_profile = match v.as_str() { "crisp" => DotProfile::Crisp, "phosphor" => DotProfile::Phosphor, _ => DotProfile::Balanced };
    }
    if let Some(v) = f32_of("brailleBloom") { cfg.bloom = v; }
    if let Some(v) = bool_of("horizonVisible") { cfg.horizon_visible = v; }
    if let Some(v) = f32_of("horizonOpacity") { cfg.horizon_opacity = v; }
    if let Some(v) = str_of("horizonStyle") {
        cfg.horizon_mode = match v.as_str() { "peaks" => HorizonMode::Peaks, "pulse" => HorizonMode::Pulse, "ribbon" => HorizonMode::Ribbon, _ => HorizonMode::Silk };
    }
    if let Some(v) = bool_of("statusReadoutVisible") { cfg.status_readout_visible = v; }
    if let Some(v) = f32_of("animationSpeed") { cfg.animation_speed = v; }
    if let Some(v) = bool_of("reducedMotion") { cfg.reduced_motion = v; }
}

/// Apply an already-sanitized patch (mirrors `applyUiPatch` after sanitize).
pub fn apply_sanitized_patch(
    current: &UiCustomizationConfig,
    sanitized: &UiCustomizationPatch,
) -> UiPatchOutcome {
    let mut sanitized = sanitized.clone();

    if sanitized.reset_all {
        let def_bg = &curated_ui_presets()[0];
        sanitized.background_preset_id = Some(def_bg.background_preset_id.to_string());
        sanitized.background_opacity = Some(def_bg.background_opacity);
        sanitized.glass_blur_px = Some(def_bg.glass_blur_px);
        return UiPatchOutcome {
            next: UiCustomizationConfig::default(),
            sanitized,
            changed_keys: vec!["resetAll".to_string()],
            summary: "Restored default Slate Monochrome UI & backdrop".to_string(),
        };
    }

    let mut next = current.clone();
    let mut changed: Vec<String> = Vec::new();
    let mut preset_applied = false;

    if let Some(pid) = &sanitized.preset_id {
        if let Some(preset) = get_curated_preset(pid) {
            merge_preset_config(&mut next, &preset.config);
            preset_applied = true;
            push_unique(&mut changed, format!("preset:{}", preset.name));
            if sanitized.background_preset_id.is_none() {
                sanitized.background_preset_id = Some(preset.background_preset_id.to_string());
            }
            if sanitized.background_opacity.is_none() {
                sanitized.background_opacity = Some(preset.background_opacity);
            }
            if sanitized.glass_blur_px.is_none() {
                sanitized.glass_blur_px = Some(preset.glass_blur_px);
            }
            if sanitized.mood.is_none() {
                sanitized.mood = preset.mood.map(str::to_string);
            }
            // Desktop bridge: keep the settings-dialog highlight in sync.
            next.preset_name = preset.name.to_string();
        }
    }

    // CONFIG_KEYS iteration order from DEFAULT_UI_CONFIG (TS object order).
    macro_rules! apply_keys {
        ($(($key:literal, $pfield:ident, $cfield:ident)),+ $(,)?) => {
            $(
                if let Some(val) = &sanitized.$pfield {
                    if &next.$cfield != val {
                        next.$cfield = val.clone();
                        push_unique(&mut changed, $key);
                    }
                }
            )+
        };
    }
    apply_keys!(
        ("accentPreset", accent_preset, accent_preset),
        ("accentHex", accent_hex, accent_hex),
        ("accentSoftHex", accent_soft_hex, accent_soft_hex),
        ("accentBrightHex", accent_bright_hex, accent_bright_hex),
        ("surfaceHex", surface_hex, surface_hex),
        ("textHex", text_hex, text_hex),
        ("panelAlpha", panel_alpha, panel_alpha),
        ("borderOpacity", border_opacity, border_opacity),
        ("glowIntensity", glow_intensity, glow_intensity),
        ("cornerRadius", corner_radius, corner_radius),
        ("density", density, density),
        ("fontPreset", font_preset, font_preset),
        ("fontScale", font_scale, font_scale),
        ("sidebarVisible", sidebar_visible, sidebar_visible),
        ("sidebarPosition", sidebar_position, sidebar_position),
        ("sidebarWidthPx", sidebar_width, sidebar_width),
        ("sidebarQueueVisible", sidebar_queue_visible, sidebar_queue_visible),
        ("sidebarChannelsVisible", sidebar_channels_visible, sidebar_channels_visible),
        ("showTopNav", show_top_nav, show_top_nav),
        ("composerStyle", composer_style, composer_style),
        ("workspaceTitle", workspace_title, workspace_title),
        ("portraitVisible", portrait_visible, portrait_visible),
        ("portraitScale", portrait_scale, portrait_scale),
        ("portraitTintHex", portrait_tint_hex, portrait_tint_hex),
        ("brailleDotProfile", dot_profile, dot_profile),
        ("brailleBloom", bloom, bloom),
        ("horizonVisible", horizon_visible, horizon_visible),
        ("horizonOpacity", horizon_opacity, horizon_opacity),
        ("horizonStyle", horizon_mode, horizon_mode),
        ("statusReadoutVisible", status_readout_visible, status_readout_visible),
        ("animationSpeed", animation_speed, animation_speed),
        ("reducedMotion", reduced_motion, reduced_motion),
    );

    // Desktop bridges (no web counterpart): legacy glass alpha tracks panel
    // alpha; a non-preset accent change marks the config as custom so the
    // settings dialog does not highlight a stale preset.
    if sanitized.panel_alpha.is_some() {
        next.glass_opacity = next.panel_alpha;
    }
    if let Some(name) = &sanitized.preset_name {
        next.preset_name = name.clone();
    } else if !preset_applied && sanitized.accent_hex.is_some() && next.accent_hex != current.accent_hex {
        next.preset_name = "Custom".to_string();
    }

    if let Some(bg) = &sanitized.background_preset_id {
        push_unique(&mut changed, format!("background:{bg}"));
    }
    if let Some(blur) = sanitized.glass_blur_px {
        push_unique(&mut changed, format!("blur:{}px", blur.round() as i64));
    }
    if let Some(op) = sanitized.background_opacity {
        push_unique(&mut changed, format!("bgOpacity:{}%", (op as f64 * 100.0).round() as i64));
    }
    if let Some(view) = &sanitized.active_view {
        push_unique(&mut changed, format!("view:{view}"));
    }
    if let Some(mood) = &sanitized.mood {
        push_unique(&mut changed, format!("mood:{mood}"));
    }
    if sanitized.canvas_auto_arrange == Some(true) {
        push_unique(&mut changed, "canvas:arrange");
    }

    let summary = if changed.is_empty() {
        "No UI changes needed".to_string()
    } else {
        let head = changed.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
        if changed.len() > 5 {
            format!("Applied {head} (+{} more)", changed.len() - 5)
        } else {
            format!("Applied {head}")
        }
    };

    UiPatchOutcome { next, sanitized, changed_keys: changed, summary }
}

/// Port of `applyUiPatch(current, rawPatch)` — sanitizes then applies.
pub fn apply_ui_patch(current: &UiCustomizationConfig, raw_patch: &Value) -> UiPatchOutcome {
    let sanitized = sanitize_ui_patch(raw_patch);
    apply_sanitized_patch(current, &sanitized)
}

// ---------------------------------------------------------------------------
// Legacy scaffold API (kept for voxcell-ui compatibility)
// ---------------------------------------------------------------------------

/// Legacy mutating patch applier used by `voxcell-ui::state`. Existing field
/// behavior is preserved; new web-parity fields are applied when present.
pub fn apply_ui_customization_patch(cfg: &mut UiCustomizationConfig, patch: &UiCustomizationPatch) {
    if patch.reset_all {
        *cfg = UiCustomizationConfig::default();
        return;
    }
    if let Some(pid) = &patch.preset_id {
        apply_preset(cfg, pid);
    }
    if let Some(hex) = &patch.accent_hex {
        if let Some(clean) = sanitize_hex_color(hex) {
            cfg.accent_hex = clean;
        }
    }
    if let Some(hex) = &patch.surface_hex {
        if let Some(clean) = sanitize_hex_color(hex) {
            cfg.surface_hex = clean;
        }
    }
    if let Some(hex) = &patch.bg_hex {
        if let Some(clean) = sanitize_hex_color(hex) {
            cfg.bg_hex = clean;
        }
    }
    if let Some(tint) = &patch.portrait_tint_hex {
        cfg.portrait_tint_hex = tint.as_deref().and_then(sanitize_hex_color);
    }
    if let Some(dp) = patch.dot_profile {
        cfg.dot_profile = dp;
    }
    if let Some(b) = patch.bloom {
        if b.is_finite() {
            cfg.bloom = b.clamp(0.0, 1.0);
        }
    }
    if let Some(hm) = patch.horizon_mode {
        cfg.horizon_mode = hm;
    }
    if let Some(d) = patch.density {
        cfg.density = d;
    }
    if let Some(sp) = patch.sidebar_position {
        cfg.sidebar_position = sp;
    }
    if let Some(w) = patch.sidebar_width {
        if w.is_finite() {
            cfg.sidebar_width = w.clamp(220.0, 420.0);
        }
    }
    if let Some(rm) = patch.reduced_motion {
        cfg.reduced_motion = rm;
    }
    if let Some(name) = &patch.preset_name {
        cfg.preset_name = name.clone();
    }
    // New web-parity fields.
    if let Some(ap) = patch.accent_preset {
        cfg.accent_preset = ap;
    }
    if let Some(hex) = &patch.accent_soft_hex {
        if let Some(clean) = sanitize_hex_color(hex) {
            cfg.accent_soft_hex = clean;
        }
    }
    if let Some(hex) = &patch.accent_bright_hex {
        if let Some(clean) = sanitize_hex_color(hex) {
            cfg.accent_bright_hex = clean;
        }
    }
    if let Some(hex) = &patch.text_hex {
        if let Some(clean) = sanitize_hex_color(hex) {
            cfg.text_hex = clean;
        }
    }
    if let Some(v) = patch.panel_alpha {
        if v.is_finite() {
            cfg.panel_alpha = v.clamp(0.32, 0.95);
            cfg.glass_opacity = cfg.panel_alpha;
        }
    }
    if let Some(v) = patch.border_opacity {
        if v.is_finite() {
            cfg.border_opacity = v.clamp(0.08, 0.9);
        }
    }
    if let Some(v) = patch.glow_intensity {
        if v.is_finite() {
            cfg.glow_intensity = v.clamp(0.0, 2.0);
        }
    }
    if let Some(v) = patch.corner_radius {
        cfg.corner_radius = v;
    }
    if let Some(v) = patch.font_preset {
        cfg.font_preset = v;
    }
    if let Some(v) = patch.font_scale {
        if v.is_finite() {
            cfg.font_scale = v.clamp(0.85, 1.25);
        }
    }
    if let Some(v) = patch.sidebar_visible {
        cfg.sidebar_visible = v;
    }
    if let Some(v) = patch.sidebar_queue_visible {
        cfg.sidebar_queue_visible = v;
    }
    if let Some(v) = patch.sidebar_channels_visible {
        cfg.sidebar_channels_visible = v;
    }
    if let Some(v) = patch.show_top_nav {
        cfg.show_top_nav = v;
    }
    if let Some(v) = patch.composer_style {
        cfg.composer_style = v;
    }
    if let Some(v) = &patch.workspace_title {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            cfg.workspace_title = trimmed.chars().take(28).collect();
        }
    }
    if let Some(v) = patch.portrait_visible {
        cfg.portrait_visible = v;
    }
    if let Some(v) = patch.portrait_scale {
        if v.is_finite() {
            cfg.portrait_scale = v.clamp(0.6, 1.45);
        }
    }
    if let Some(v) = patch.horizon_visible {
        cfg.horizon_visible = v;
    }
    if let Some(v) = patch.horizon_opacity {
        if v.is_finite() {
            cfg.horizon_opacity = v.clamp(0.0, 1.0);
        }
    }
    if let Some(v) = patch.status_readout_visible {
        cfg.status_readout_visible = v;
    }
    if let Some(v) = patch.animation_speed {
        if v.is_finite() {
            cfg.animation_speed = v.clamp(0.3, 1.8);
        }
    }
}

/// Apply a curated preset by id or display name (case-insensitive). Legacy
/// scaffold ids are aliased onto their closest curated successor.
pub fn apply_preset(cfg: &mut UiCustomizationConfig, preset_id: &str) -> bool {
    let needle = preset_id.trim().to_ascii_lowercase();
    let alias = match needle.as_str() {
        "abyss" | "voxcell abyss" => Some("emerald-terminal"),
        "amber-ops" | "amber ops" => Some("solaris-amber"),
        "cyan-wire" | "cyan wire" => Some("glacier-cyan"),
        "obsidian-mono" | "obsidian mono" => Some("zen-mono"),
        _ => None,
    };
    let needle = alias.unwrap_or(needle.as_str());
    let Some(preset) = curated_ui_presets()
        .iter()
        .find(|p| p.id == needle || p.name.to_ascii_lowercase() == needle)
    else {
        return false;
    };
    merge_preset_config(cfg, &preset.config);
    cfg.preset_name = preset.name.to_string();
    true
}

/// Legacy quick NL parser (scaffold behavior preserved for existing callers
/// and tests). New code should prefer [`parse_natural_ui_intent`].
pub fn parse_natural_language_ui_request(prompt: &str) -> UiCustomizationPatch {
    let lower = prompt.trim().to_ascii_lowercase();
    let mut patch = UiCustomizationPatch::default();

    for preset in BUILT_IN_PRESETS {
        if lower.contains(preset.id) || lower.contains(&preset.name.to_ascii_lowercase()) {
            patch.accent_hex = Some(preset.accent_hex.to_string());
            patch.bg_hex = Some(preset.bg_hex.to_string());
            patch.surface_hex = Some(preset.surface_hex.to_string());
            patch.dot_profile = Some(preset.dot_profile);
            patch.bloom = Some(preset.bloom);
            patch.horizon_mode = Some(preset.horizon_mode);
            patch.preset_name = Some(preset.name.to_string());
        }
    }

    for token in lower.split_whitespace() {
        if token.starts_with('#') {
            if let Some(clean) = sanitize_hex_color(token) {
                patch.accent_hex = Some(clean.clone());
                patch.portrait_tint_hex = Some(Some(clean));
            }
        }
    }

    if lower.contains("emerald") || lower.contains("green") {
        patch.accent_hex = Some("#34D399".to_string());
    } else if lower.contains("amber") || lower.contains("gold") || lower.contains("orange") {
        patch.accent_hex = Some("#F59E0B".to_string());
    } else if lower.contains("cyan") || lower.contains("blue") {
        patch.accent_hex = Some("#22D3EE".to_string());
    } else if lower.contains("rose") || lower.contains("red") || lower.contains("pink") {
        patch.accent_hex = Some("#F43F5E".to_string());
    }

    if lower.contains("crisp") || lower.contains("sharp") {
        patch.dot_profile = Some(DotProfile::Crisp);
    } else if lower.contains("phosphor") || lower.contains("glow") {
        patch.dot_profile = Some(DotProfile::Phosphor);
        patch.bloom = Some(0.62);
    } else if lower.contains("balanced") {
        patch.dot_profile = Some(DotProfile::Balanced);
    }

    if lower.contains("no bloom") || lower.contains("less glow") {
        patch.bloom = Some(0.05);
    } else if lower.contains("high bloom") || lower.contains("more glow") {
        patch.bloom = Some(0.75);
    }

    if lower.contains("peaks") {
        patch.horizon_mode = Some(HorizonMode::Peaks);
    } else if lower.contains("pulse") {
        patch.horizon_mode = Some(HorizonMode::Pulse);
    } else if lower.contains("ribbon") {
        patch.horizon_mode = Some(HorizonMode::Ribbon);
    } else if lower.contains("silk") {
        patch.horizon_mode = Some(HorizonMode::Silk);
    }

    if lower.contains("compact") {
        patch.density = Some(DensityMode::Compact);
    } else if lower.contains("airy") || lower.contains("spacious") {
        patch.density = Some(DensityMode::Airy);
    } else if lower.contains("comfortable") {
        patch.density = Some(DensityMode::Comfortable);
    }

    if lower.contains("sidebar right") || lower.contains("right sidebar") {
        patch.sidebar_position = Some(SidebarPosition::Right);
    } else if lower.contains("sidebar left") || lower.contains("left sidebar") {
        patch.sidebar_position = Some(SidebarPosition::Left);
    }

    if lower.contains("reduce motion") || lower.contains("less motion") {
        patch.reduced_motion = Some(true);
    } else if lower.contains("full motion") {
        patch.reduced_motion = Some(false);
    }

    patch
}

// ---------------------------------------------------------------------------
// parseNaturalUiIntent — deterministic NL/slash intent parser (full port)
// ---------------------------------------------------------------------------
// The TS module uses regexes with \b word boundaries and \s* / \s+ separators.
// To keep voxcell-core dependency-free we implement the same semantics over
// ASCII bytes with the small pattern helpers below.

#[derive(Clone, Copy, PartialEq)]
enum Sep {
    /// Single word, no separator.
    Exact,
    /// `\s*` — zero or more ASCII whitespace.
    Star,
    /// `\s+` — one or more ASCII whitespace.
    Plus,
    /// Literal single space (TS patterns like `more blur`).
    Space,
    /// `[- ]?` — optional single dash or space.
    DashSpaceOpt,
    /// `[- ]` — exactly one dash or space.
    DashSpaceReq,
}

#[derive(Clone, Copy)]
struct Pat {
    parts: &'static [&'static str],
    sep: Sep,
}

macro_rules! word {
    ($s:literal) => {
        Pat { parts: &[$s], sep: Sep::Exact }
    };
}
const fn ph(parts: &'static [&'static str], sep: Sep) -> Pat {
    Pat { parts, sep }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' || b == 0x0b || b == 0x0c
}

/// All `\bpattern\b` matches as (start, end) byte ranges over lowercased text.
fn pat_matches(text: &str, pat: &Pat) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    for i in 0..b.len() {
        if i > 0 && is_word_byte(b[i - 1]) {
            continue;
        }
        let mut k = i;
        let mut ok = true;
        for (pi, part) in pat.parts.iter().enumerate() {
            if pi > 0 {
                match pat.sep {
                    Sep::Exact | Sep::Space => {
                        if pat.sep == Sep::Space {
                            if k < b.len() && b[k] == b' ' { k += 1; } else { ok = false; break; }
                        }
                    }
                    Sep::Star => {
                        while k < b.len() && is_ws(b[k]) { k += 1; }
                    }
                    Sep::Plus => {
                        let start = k;
                        while k < b.len() && is_ws(b[k]) { k += 1; }
                        if k == start { ok = false; break; }
                    }
                    Sep::DashSpaceOpt => {
                        if k < b.len() && (b[k] == b'-' || b[k] == b' ') { k += 1; }
                    }
                    Sep::DashSpaceReq => {
                        if k < b.len() && (b[k] == b'-' || b[k] == b' ') { k += 1; } else { ok = false; break; }
                    }
                }
            }
            let pb = part.as_bytes();
            if k + pb.len() <= b.len() && &b[k..k + pb.len()] == pb {
                k += pb.len();
            } else {
                ok = false;
                break;
            }
        }
        if ok && (k == b.len() || !is_word_byte(b[k])) {
            out.push((i, k));
        }
    }
    out
}

fn any_pat(text: &str, pats: &[Pat]) -> bool {
    pats.iter().any(|p| !pat_matches(text, p).is_empty())
}

/// `\bA\b.*\bB\b` — some A match ends at/before some B match starts.
fn pat_before(text: &str, a: &[Pat], b: &[Pat]) -> bool {
    let bs: Vec<(usize, usize)> = b.iter().flat_map(|p| pat_matches(text, p)).collect();
    a.iter().any(|p| {
        pat_matches(text, p).iter().any(|(_, ae)| bs.iter().any(|(bs, _)| ae <= bs))
    })
}

/// Like [`pat_before`] but the gap must not contain ',' or '.' (`[^,.]*`).
fn pat_before_nodotcomma(text: &str, a: &[Pat], b: &[Pat]) -> bool {
    let bs: Vec<(usize, usize)> = b.iter().flat_map(|p| pat_matches(text, p)).collect();
    a.iter().any(|p| {
        pat_matches(text, p).iter().any(|(_, ae)| {
            bs.iter().any(|(bs, _)| {
                ae <= bs && !text[*ae..*bs].contains(',') && !text[*ae..*bs].contains('.')
            })
        })
    })
}

/// `/#[0-9a-fA-F]{3,6}\b/g` — greedy hex run with trailing word boundary.
fn hex_tokens(text: &str) -> Vec<String> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'#' {
            let mut j = i + 1;
            while j < b.len() && (b[j] as char).is_ascii_hexdigit() {
                j += 1;
            }
            let run = j - (i + 1);
            if (3..=6).contains(&run) && (j == b.len() || !is_word_byte(b[j])) {
                out.push(text[i..j].to_string());
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `\bword\b[^0-9]*([0-9]{1,max})` — first occurrence with digits following.
fn capture_digits_after(text: &str, pats: &[Pat], max_digits: usize) -> Option<u32> {
    let b = text.as_bytes();
    for pat in pats {
        for (_, end) in pat_matches(text, pat) {
            let mut k = end;
            while k < b.len() && !b[k].is_ascii_digit() {
                k += 1;
            }
            if k >= b.len() {
                continue;
            }
            let start = k;
            let mut count = 0;
            while k < b.len() && b[k].is_ascii_digit() && count < max_digits {
                k += 1;
                count += 1;
            }
            if count > 0 {
                return text[start..k].parse().ok();
            }
        }
    }
    None
}

fn is_rename_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b' ' | b'_' | b'>' | b'.' | b'#' | b'/' | b'-')
}

/// `\b(?:rename|set)\s+(?:workspace|header|brand|title)\s+(?:to\s+)?["']?([a-zA-Z0-9 _>.#/-]{2,26})["']?`
/// Capture preserves original casing from `trimmed`.
fn capture_rename(trimmed: &str, lower: &str) -> Option<String> {
    let b = lower.as_bytes();
    for (_, end) in [word!("rename"), word!("set")].iter().flat_map(|p| pat_matches(lower, p)) {
        let mut k = end;
        let ws = k;
        while k < b.len() && is_ws(b[k]) { k += 1; }
        if k == ws { continue; }
        let mut noun_end = None;
        for noun in ["workspace", "header", "brand", "title"] {
            let nb = noun.as_bytes();
            if k + nb.len() <= b.len()
                && &b[k..k + nb.len()] == nb
                && (k + nb.len() == b.len() || !is_word_byte(b[k + nb.len()]))
            {
                noun_end = Some(k + nb.len());
                break;
            }
        }
        let Some(ne) = noun_end else { continue };
        k = ne;
        let ws2 = k;
        while k < b.len() && is_ws(b[k]) { k += 1; }
        if k == ws2 { continue; }
        // Optional `to\s+` — only consumed when followed by whitespace.
        if k + 2 <= b.len() && &b[k..k + 2] == b"to" && (k + 2 == b.len() || !is_word_byte(b[k + 2])) {
            let mut m = k + 2;
            let ws3 = m;
            while m < b.len() && is_ws(b[m]) { m += 1; }
            if m > ws3 { k = m; }
        }
        if k < b.len() && (b[k] == b'"' || b[k] == b'\'') { k += 1; }
        let start = k;
        let mut count = 0;
        while k < b.len() && count < 26 && is_rename_char(b[k]) {
            k += 1;
            count += 1;
        }
        if count >= 2 {
            return Some(trimmed[start..k].trim().to_string());
        }
    }
    None
}

const UI_VERBS: &[Pat] = &[
    word!("make"), word!("set"), word!("change"), word!("switch"), word!("turn"), word!("hide"),
    word!("show"), word!("toggle"), word!("move"), word!("put"), word!("resize"), word!("scale"),
    word!("rename"), word!("style"), word!("theme"), word!("customize"), word!("reset"),
    word!("restore"), word!("open"), word!("close"), word!("go"), word!("enable"), word!("disable"),
    word!("increase"), word!("decrease"), word!("boost"), word!("reduce"), word!("arrange"),
    word!("apply"),
];

const UI_NOUNS: &[Pat] = &[
    word!("ui"), word!("interface"), word!("theme"), word!("preset"), word!("color"), word!("colour"),
    word!("accent"), word!("palette"), word!("slate"), word!("grey"), word!("gray"), word!("violet"),
    word!("purple"), word!("amber"), word!("gold"), word!("cyan"), word!("blue"), word!("rose"),
    word!("pink"), word!("crimson"), word!("red"), word!("emerald"), word!("green"), word!("mono"),
    word!("monochrome"), word!("stealth"), word!("zen"), word!("sakura"), word!("solaris"),
    word!("glacier"), word!("sidebar"), word!("panel"), word!("nav"), word!("tabs"), word!("header"),
    word!("title"), word!("brand"), word!("portrait"), word!("character"), word!("avatar"),
    word!("horizon"), word!("wave"), word!("footer"), word!("composer"), word!("input"), word!("font"),
    word!("monospace"), word!("jakarta"), word!("text"), word!("corner"), word!("corners"),
    word!("sharp"), word!("pill"), word!("rounded"), word!("blur"), word!("opacity"), word!("glow"),
    word!("border"), word!("compact"), word!("airy"), word!("comfortable"), word!("motion"),
    word!("animation"), word!("speed"), word!("background"), word!("backdrop"), word!("aurora"),
    word!("braille"), word!("dots"), word!("bloom"), word!("phosphor"), word!("peaks"), word!("silk"),
    word!("pulse"), word!("rain"), word!("matrix"), word!("scanline"), word!("nebula"),
    word!("constellation"), word!("mesh"), word!("canvas"), word!("queue"), word!("cli"),
    word!("thread"), word!("studio"), word!("customizer"),
];

/// Port of `parseNaturalUiIntent(rawText, current)` — returns `None` when the
/// text is not a clear UI customization/navigation request.
pub fn parse_natural_ui_intent(
    raw_text: &str,
    current: &UiCustomizationConfig,
) -> Option<UiCustomizationPatch> {
    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();

    let has_ui_verb = any_pat(&lower, UI_VERBS);
    let has_ui_noun = any_pat(&lower, UI_NOUNS);
    let is_direct = get_curated_preset(&lower).is_some() || !hex_tokens(&lower).is_empty();
    if !is_direct && !(has_ui_verb && has_ui_noun) {
        return None;
    }

    let mut patch = UiCustomizationPatch::default();
    let mut touched = false;

    // Reset
    if pat_before(&lower, &[word!("reset"), word!("restore"), word!("default")],
        &[word!("ui"), word!("theme"), word!("layout"), word!("style"), word!("everything"), word!("all"), word!("settings"), word!("interface")])
        || lower == "reset"
    {
        patch.reset_all = true;
        return Some(patch);
    }

    // Open customizer studio
    if pat_before(&lower, &[word!("open"), word!("show"), word!("launch")],
        &[word!("studio"), word!("customizer"), word!("customize"), ph(&["theme", "picker"], Sep::Space)])
    {
        patch.open_customizer = Some(true);
        touched = true;
    }

    // Curated presets
    let preset_hit = if any_pat(&lower, &[ph(&["cyber", "violet"], Sep::Star), word!("ultraviolet"), ph(&["violet", "theme"], Sep::Star)]) || lower == "cyber-violet" {
        Some("cyber-violet")
    } else if any_pat(&lower, &[word!("solaris"), ph(&["amber", "crt"], Sep::Star), ph(&["retro", "crt"], Sep::Star), ph(&["crt", "theme"], Sep::Star)]) || lower == "solaris-amber" {
        Some("solaris-amber")
    } else if any_pat(&lower, &[word!("glacier"), ph(&["ice", "cyan"], Sep::Star), word!("arctic")]) || lower == "glacier-cyan" {
        Some("glacier-cyan")
    } else if any_pat(&lower, &[word!("sakura"), word!("nocturne"), ph(&["cherry", "blossom"], Sep::Star)]) || lower == "sakura-rose" {
        Some("sakura-rose")
    } else if any_pat(&lower, &[ph(&["zen", "mode"], Sep::Star), word!("stealth"), ph(&["minimalist", "focus"], Sep::Star), word!("monograph")]) || lower == "zen-mono" {
        Some("zen-mono")
    } else if any_pat(&lower, &[ph(&["slate", "monochrome"], Sep::Star), ph(&["slate", "terminal"], Sep::Star), ph(&["slate", "theme"], Sep::Star)]) || lower == "slate-terminal" {
        Some("slate-terminal")
    } else if any_pat(&lower, &[ph(&["emerald", "terminal"], Sep::Star)]) || lower == "emerald-terminal" {
        Some("emerald-terminal")
    } else {
        None
    };
    if let Some(pid) = preset_hit {
        patch.preset_id = Some(pid.to_string());
        touched = true;
    }

    // Accent swatches / hex codes
    let hexes = hex_tokens(trimmed);
    if let Some(first) = hexes.first().and_then(|h| normalize_hex(h)) {
        if any_pat(&lower, &[word!("character"), word!("portrait"), word!("avatar")])
            && !any_pat(&lower, &[word!("ui"), word!("accent"), word!("theme")])
        {
            patch.portrait_tint_hex = Some(Some(first));
        } else {
            patch.accent_hex = Some(first);
        }
        touched = true;
    } else if patch.preset_id.is_none() {
        let swatch_hit = if any_pat(&lower, &[word!("slate"), word!("grey"), word!("gray"), word!("graphite"), word!("charcoal")]) {
            Some(AccentPreset::Slate)
        } else if any_pat(&lower, &[word!("violet"), word!("purple"), word!("lavender")]) {
            Some(AccentPreset::Violet)
        } else if any_pat(&lower, &[word!("cyan"), word!("ice"), word!("aqua"), word!("sky"), word!("blue")]) {
            Some(AccentPreset::Cyan)
        } else if any_pat(&lower, &[word!("amber"), word!("gold"), word!("yellow"), word!("orange")]) {
            Some(AccentPreset::Amber)
        } else if any_pat(&lower, &[word!("rose"), word!("pink"), word!("magenta"), word!("sakura")]) {
            Some(AccentPreset::Rose)
        } else if any_pat(&lower, &[word!("crimson"), word!("scarlet"), word!("red")]) {
            Some(AccentPreset::Crimson)
        } else if any_pat(&lower, &[word!("monochrome"), word!("monograph"), word!("silver"), word!("grayscale"), word!("greyscale")]) {
            Some(AccentPreset::Mono)
        } else if any_pat(&lower, &[word!("emerald"), word!("mint"), word!("green")]) {
            Some(AccentPreset::Emerald)
        } else {
            None
        };
        if let Some(ap) = swatch_hit {
            patch.accent_preset = Some(ap);
            touched = true;
        }
    }

    // Portrait tint explicit color keywords
    if pat_before(&lower, &[word!("portrait"), word!("character"), word!("hermes"), word!("avatar")],
        &[word!("pink"), word!("rose"), word!("violet"), word!("purple"), word!("cyan"), word!("blue"), word!("amber"), word!("gold"), word!("red"), word!("white"), word!("silver"), word!("green"), word!("emerald"), word!("auto")])
    {
        let tint = if any_pat(&lower, &[word!("auto")]) {
            None
        } else if any_pat(&lower, &[word!("pink"), word!("rose")]) {
            Some("#F472B6")
        } else if any_pat(&lower, &[word!("violet"), word!("purple")]) {
            Some("#D8B4FE")
        } else if any_pat(&lower, &[word!("cyan"), word!("blue")]) {
            Some("#7DD3FC")
        } else if any_pat(&lower, &[word!("amber"), word!("gold")]) {
            Some("#FCD34D")
        } else if any_pat(&lower, &[word!("white"), word!("silver")]) {
            Some("#FFFFFF")
        } else if any_pat(&lower, &[word!("red"), word!("crimson")]) {
            Some("#FCA5A5")
        } else if any_pat(&lower, &[word!("green"), word!("emerald")]) {
            Some("#6EE7B7")
        } else {
            None
        };
        patch.portrait_tint_hex = Some(tint.map(str::to_string));
        touched = true;
    }

    // Backgrounds
    let bg_hit = if any_pat(&lower, &[ph(&["braille", "rain"], Sep::Star), word!("matrix"), ph(&["glyph", "rain"], Sep::Star)]) {
        Some("braille-rain")
    } else if any_pat(&lower, &[word!("aurora"), ph(&["aurora", "drift"], Sep::Star)]) {
        Some("aurora-drift")
    } else if any_pat(&lower, &[word!("mesh"), ph(&["mesh", "gradient"], Sep::Star)]) {
        Some("mesh-gradient")
    } else if any_pat(&lower, &[word!("scanline"), word!("scanlines"), word!("crt")]) {
        Some("scanline-shimmer")
    } else if any_pat(&lower, &[word!("nebula"), word!("cosmos")]) {
        Some("nebula")
    } else if any_pat(&lower, &[word!("constellation"), word!("stars"), word!("orbital")]) {
        Some("constellation-drift")
    } else if any_pat(&lower, &[ph(&["horizon", "tide"], Sep::Star), ph(&["tide", "backdrop"], Sep::Star)]) {
        Some("horizon-tide")
    } else {
        None
    };
    if let Some(bg) = bg_hit {
        patch.background_preset_id = Some(bg.to_string());
        touched = true;
    }

    // Blur & opacity numbers
    if let Some(n) = capture_digits_after(&lower, &[word!("blur")], 2) {
        patch.glass_blur_px = Some(n as f32);
        touched = true;
    } else if any_pat(&lower, &[ph(&["more", "blur"], Sep::Space), ph(&["heavy", "blur"], Sep::Space), ph(&["extra", "blur"], Sep::Space), word!("frostier"), word!("frosted")]) {
        patch.glass_blur_px = Some(22.0);
        touched = true;
    } else if any_pat(&lower, &[ph(&["no", "blur"], Sep::Space), ph(&["remove", "blur"], Sep::Space), ph(&["zero", "blur"], Sep::Space), ph(&["crisp", "glass"], Sep::Space)]) {
        patch.glass_blur_px = Some(0.0);
        touched = true;
    }

    if let Some(n) = capture_digits_after(&lower, &[ph(&["background", "opacity"], Sep::Star), ph(&["backdrop", "opacity"], Sep::Star)], 3) {
        let val = n as f32;
        patch.background_opacity = Some(if val > 1.0 { val / 100.0 } else { val });
        touched = true;
    }

    if any_pat(&lower, &[ph(&["transparent", "panel"], Sep::Star), ph(&["transparent", "panels"], Sep::Star), ph(&["see", "through"], Sep::DashSpaceReq), ph(&["more", "glass"], Sep::Star)]) {
        patch.panel_alpha = Some(0.52);
        touched = true;
    } else if any_pat(&lower, &[ph(&["solid", "panel"], Sep::Star), ph(&["solid", "panels"], Sep::Star), ph(&["opaque", "panel"], Sep::Star), ph(&["opaque", "panels"], Sep::Star)]) {
        patch.panel_alpha = Some(0.92);
        touched = true;
    }

    // Sidebar controls
    if pat_before(&lower, &[word!("hide"), word!("remove"), word!("close"), word!("collapse")], &[word!("sidebar")]) {
        patch.sidebar_visible = Some(false);
        touched = true;
    } else if pat_before(&lower, &[word!("show"), word!("open"), word!("restore"), word!("display")], &[word!("sidebar")]) {
        patch.sidebar_visible = Some(true);
        touched = true;
    }
    if pat_before(&lower, &[word!("sidebar")], &[word!("right")]) || any_pat(&lower, &[ph(&["right", "sidebar"], Sep::Star)]) {
        patch.sidebar_position = Some(SidebarPosition::Right);
        patch.sidebar_visible = Some(true);
        touched = true;
    } else if pat_before(&lower, &[word!("sidebar")], &[word!("left")]) || any_pat(&lower, &[ph(&["left", "sidebar"], Sep::Star)]) {
        patch.sidebar_position = Some(SidebarPosition::Left);
        patch.sidebar_visible = Some(true);
        touched = true;
    }
    if pat_before(&lower, &[word!("wider"), word!("expand")], &[word!("sidebar")]) {
        patch.sidebar_width = Some((current.sidebar_width + 48.0).min(420.0));
        touched = true;
    } else if pat_before(&lower, &[word!("narrow"), word!("slimmer"), word!("shrink")], &[word!("sidebar")]) {
        patch.sidebar_width = Some((current.sidebar_width - 44.0).max(236.0));
        touched = true;
    }

    // Top nav
    let nav_nouns = &[ph(&["top", "nav"], Sep::Star), word!("tabs"), word!("navigation"), word!("toolbar")];
    if pat_before(&lower, &[word!("hide"), word!("remove")], nav_nouns) {
        patch.show_top_nav = Some(false);
        touched = true;
    } else if pat_before(&lower, &[word!("show"), word!("restore")], nav_nouns) {
        patch.show_top_nav = Some(true);
        touched = true;
    }

    // Horizon / wave & braille optics
    let horizon_nouns = &[word!("horizon"), word!("wave"), word!("waves")];
    if pat_before(&lower, &[word!("hide"), word!("remove"), ph(&["turn", "off"], Sep::Star), word!("disable")], horizon_nouns) {
        patch.horizon_visible = Some(false);
        touched = true;
    } else if pat_before(&lower, &[word!("show"), word!("enable"), ph(&["turn", "on"], Sep::Star)], horizon_nouns) {
        patch.horizon_visible = Some(true);
        touched = true;
    }
    if any_pat(&lower, &[ph(&["spectral", "peak"], Sep::Star), ph(&["spectral", "peaks"], Sep::Star), ph(&["peaks", "horizon"], Sep::Star), ph(&["bar", "horizon"], Sep::Star), ph(&["bars", "horizon"], Sep::Star)]) {
        patch.horizon_mode = Some(HorizonMode::Peaks);
        patch.horizon_visible = Some(true);
        touched = true;
    } else if any_pat(&lower, &[ph(&["pulse", "tide"], Sep::Star), ph(&["interference", "wave"], Sep::Star), ph(&["pulse", "horizon"], Sep::Star)]) {
        patch.horizon_mode = Some(HorizonMode::Pulse);
        patch.horizon_visible = Some(true);
        touched = true;
    } else if any_pat(&lower, &[ph(&["silk", "wave"], Sep::Star), ph(&["silk", "waves"], Sep::Star), ph(&["silk", "horizon"], Sep::Star), ph(&["fluid", "horizon"], Sep::Star)]) {
        patch.horizon_mode = Some(HorizonMode::Silk);
        patch.horizon_visible = Some(true);
        touched = true;
    }
    if any_pat(&lower, &[ph(&["crisp", "dot"], Sep::Star), ph(&["crisp", "dots"], Sep::Star), ph(&["crisp", "braille"], Sep::Star), ph(&["sharp", "dot"], Sep::Star), ph(&["sharp", "dots"], Sep::Star)]) {
        patch.dot_profile = Some(DotProfile::Crisp);
        touched = true;
    } else if any_pat(&lower, &[ph(&["phosphor", "dot"], Sep::Star), ph(&["phosphor", "dots"], Sep::Star), ph(&["phosphor", "braille"], Sep::Star), ph(&["phosphor", "bloom"], Sep::Star)]) {
        patch.dot_profile = Some(DotProfile::Phosphor);
        patch.bloom = Some(1.15);
        touched = true;
    } else if any_pat(&lower, &[ph(&["balanced", "dot"], Sep::Star), ph(&["balanced", "dots"], Sep::Star), ph(&["balanced", "braille"], Sep::Star)]) {
        patch.dot_profile = Some(DotProfile::Balanced);
        touched = true;
    }
    if any_pat(&lower, &[ph(&["no", "bloom"], Sep::Star), ph(&["disable", "bloom"], Sep::Star), ph(&["zero", "bloom"], Sep::Star)]) {
        patch.bloom = Some(0.0);
        touched = true;
    } else if any_pat(&lower, &[ph(&["high", "bloom"], Sep::Star), ph(&["max", "bloom"], Sep::Star), ph(&["extra", "bloom"], Sep::Star), ph(&["boost", "bloom"], Sep::Star)]) {
        patch.bloom = Some(1.35);
        touched = true;
    }

    // Character portrait visibility & scale
    if pat_before(&lower, &[word!("hide"), word!("remove"), ph(&["turn", "off"], Sep::Star)],
        &[word!("character"), word!("portrait"), word!("avatar"), ph(&["hermes", "face"], Sep::Star)])
    {
        patch.portrait_visible = Some(false);
        touched = true;
    } else if pat_before(&lower, &[word!("show"), word!("display"), word!("restore"), ph(&["turn", "on"], Sep::Star)],
        &[word!("character"), word!("portrait"), word!("avatar")])
    {
        patch.portrait_visible = Some(true);
        touched = true;
    }
    let bigger_words = &[word!("bigger"), word!("larger"), word!("huge"), word!("giant"), ph(&["scale", "up"], Sep::Star), ph(&["zoom", "in"], Sep::Star)];
    let smaller_words = &[word!("smaller"), word!("tiny"), word!("compact"), word!("mini"), ph(&["scale", "down"], Sep::Star)];
    let portrait_words = &[word!("character"), word!("portrait"), word!("avatar"), word!("hermes")];
    if pat_before_nodotcomma(&lower, bigger_words, portrait_words)
        || pat_before_nodotcomma(&lower, portrait_words, bigger_words)
    {
        patch.portrait_scale = Some(1.3);
        patch.portrait_visible = Some(true);
        touched = true;
    } else if pat_before_nodotcomma(&lower, smaller_words, portrait_words)
        || pat_before_nodotcomma(&lower, portrait_words, smaller_words)
    {
        patch.portrait_scale = Some(0.78);
        patch.portrait_visible = Some(true);
        touched = true;
    }

    // Status readout
    if pat_before(&lower, &[word!("hide"), word!("remove")], &[word!("status"), word!("readout"), word!("subtitles")]) {
        patch.status_readout_visible = Some(false);
        touched = true;
    } else if pat_before(&lower, &[word!("show"), word!("restore")], &[word!("status"), word!("readout"), word!("subtitles")]) {
        patch.status_readout_visible = Some(true);
        touched = true;
    }

    // Corner radius
    if any_pat(&lower, &[word!("sharp"), word!("square"), word!("boxy"), word!("brutalist"), word!("angular")]) {
        patch.corner_radius = Some(CornerRadius::Sharp);
        touched = true;
    } else if any_pat(&lower, &[word!("pill"), ph(&["super", "round"], Sep::Space), ph(&["extra", "round"], Sep::Space), word!("bubbly")]) {
        patch.corner_radius = Some(CornerRadius::Pill);
        touched = true;
    } else if any_pat(&lower, &[ph(&["rounded", "corners"], Sep::Star), ph(&["normal", "corners"], Sep::Star)]) {
        patch.corner_radius = Some(CornerRadius::Rounded);
        touched = true;
    }

    // Density & composer style
    if any_pat(&lower, &[word!("compact"), word!("dense"), word!("tight")]) {
        patch.density = Some(DensityMode::Compact);
        touched = true;
    } else if any_pat(&lower, &[word!("airy"), word!("spacious"), word!("relaxed")]) {
        patch.density = Some(DensityMode::Airy);
        touched = true;
    }
    if any_pat(&lower, &[ph(&["minimal", "composer"], Sep::Star), ph(&["minimal", "input"], Sep::Star), ph(&["clean", "input"], Sep::Star)]) {
        patch.composer_style = Some(ComposerStyle::Minimal);
        touched = true;
    } else if any_pat(&lower, &[ph(&["floating", "composer"], Sep::Star), ph(&["floating", "input"], Sep::Star)]) {
        patch.composer_style = Some(ComposerStyle::Floating);
        touched = true;
    } else if any_pat(&lower, &[ph(&["docked", "composer"], Sep::Star), ph(&["docked", "input"], Sep::Star)]) {
        patch.composer_style = Some(ComposerStyle::Docked);
        touched = true;
    }

    // Fonts & text size
    if any_pat(&lower, &[word!("mono"), word!("monospace"), ph(&["code", "font"], Sep::Star), ph(&["terminal", "font"], Sep::Star), word!("jetbrains")]) {
        patch.font_preset = Some(FontPreset::Mono);
        touched = true;
    } else if any_pat(&lower, &[ph(&["display", "font"], Sep::Star), word!("archivo"), ph(&["bold", "font"], Sep::Star)]) {
        patch.font_preset = Some(FontPreset::Display);
        touched = true;
    } else if any_pat(&lower, &[ph(&["system", "font"], Sep::Star)]) {
        patch.font_preset = Some(FontPreset::System);
        touched = true;
    } else if any_pat(&lower, &[ph(&["default", "font"], Sep::Star), word!("jakarta")]) {
        patch.font_preset = Some(FontPreset::Jakarta);
        touched = true;
    }
    if pat_before(&lower, &[word!("bigger"), word!("larger"), word!("increase")], &[word!("font"), word!("text"), word!("type")]) {
        patch.font_scale = Some(1.12);
        touched = true;
    } else if pat_before(&lower, &[word!("smaller"), word!("tiny"), word!("decrease")], &[word!("font"), word!("text"), word!("type")]) {
        patch.font_scale = Some(0.9);
        touched = true;
    }

    // Motion & speed
    if any_pat(&lower, &[ph(&["calm", "motion"], Sep::Star), ph(&["reduce", "motion"], Sep::Star), ph(&["reduced", "motion"], Sep::Star), ph(&["freeze", "animations"], Sep::Star), ph(&["stop", "moving"], Sep::Star)]) {
        patch.reduced_motion = Some(true);
        touched = true;
    } else if any_pat(&lower, &[ph(&["full", "motion"], Sep::Star), ph(&["enable", "animations"], Sep::Star)]) {
        patch.reduced_motion = Some(false);
        touched = true;
    }
    let motion_nouns = &[word!("animat"), word!("character"), word!("wave"), word!("motion")];
    if pat_before(&lower, &[word!("faster"), ph(&["speed", "up"], Sep::Star), word!("energetic")], motion_nouns) {
        patch.animation_speed = Some(1.45);
        patch.reduced_motion = Some(false);
        touched = true;
    } else if pat_before(&lower, &[word!("slower"), word!("calm"), word!("gentle")], motion_nouns) {
        patch.animation_speed = Some(0.6);
        touched = true;
    }

    // Glow
    if any_pat(&lower, &[ph(&["high", "glow"], Sep::Star), ph(&["extra", "glow"], Sep::Star), ph(&["neon", "glow"], Sep::Star), ph(&["boost", "glow"], Sep::Star)]) {
        patch.glow_intensity = Some(1.65);
        touched = true;
    } else if any_pat(&lower, &[ph(&["no", "glow"], Sep::Star), word!("flat"), ph(&["remove", "glow"], Sep::Star)]) {
        patch.glow_intensity = Some(0.0);
        touched = true;
    }

    // Rename workspace title
    if let Some(title) = capture_rename(trimmed, &lower) {
        patch.workspace_title = Some(title);
        touched = true;
    }

    // Active view navigation
    let nav_verbs = &[word!("open"), ph(&["switch", "to"], Sep::Plus), word!("show"), ph(&["go", "to"], Sep::Plus)];
    if pat_before(&lower, nav_verbs, &[word!("canvas")]) {
        patch.active_view = Some("canvas".to_string());
        touched = true;
    } else if pat_before(&lower, nav_verbs, &[word!("thread"), word!("transcript"), ph(&["chat", "log"], Sep::Star)]) {
        patch.active_view = Some("voice".to_string());
        touched = true;
    } else if pat_before(&lower, nav_verbs, &[word!("queue"), word!("approvals")]) {
        patch.active_view = Some("queue".to_string());
        touched = true;
    } else if pat_before(&lower, nav_verbs, &[word!("cli")]) {
        patch.active_view = Some("cli".to_string());
        touched = true;
    } else if any_pat(&lower, &[ph(&["go", "home"], Sep::Star), ph(&["open", "home"], Sep::Star), ph(&["home", "view"], Sep::Star)]) {
        patch.active_view = Some("home".to_string());
        touched = true;
    }

    // Canvas multi-session actions
    if pat_before(&lower, &[ph(&["auto", "arrange"], Sep::DashSpaceOpt), word!("tidy"), word!("organize")],
        &[word!("canvas"), word!("sessions"), word!("characters"), word!("shapes")])
    {
        patch.canvas_auto_arrange = Some(true);
        touched = true;
    }

    if touched { Some(patch) } else { None }
}

// ---------------------------------------------------------------------------
// Braille frontier decode (textDecodeEngine parity, unchanged)
// ---------------------------------------------------------------------------

pub fn decode_frontier_step(target: &str, progress: f32) -> String {
    const GLYPHS: &[char] = &['⠁', '⠃', '⠇', '⠏', '⠟', '⠿', '⣿', '⠛', '⠉'];
    let clamped = progress.clamp(0.0, 1.0);
    let chars: Vec<char> = target.chars().collect();
    let reveal = ((chars.len() as f32) * clamped).floor() as usize;
    chars
        .iter()
        .enumerate()
        .map(|(i, &ch)| {
            if i < reveal || ch.is_whitespace() {
                ch
            } else if i == reveal && clamped > 0.0 && clamped < 1.0 {
                let idx = ((clamped * 37.0) as usize + i) % GLYPHS.len();
                GLYPHS[idx]
            } else {
                ' '
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::BackgroundPresetId;

    // ---- Existing scaffold test (preserved) ----
    #[test]
    fn sanitizes_hex_and_parses_natural_language_request() {
        assert_eq!(sanitize_hex_color("#3f9"), Some("#33FF99".into()));
        assert_eq!(sanitize_hex_color("not-hex"), None);

        let mut cfg = UiCustomizationConfig::default();
        let patch = parse_natural_language_ui_request("switch to cyan crisp peaks compact sidebar right");
        apply_ui_customization_patch(&mut cfg, &patch);
        assert_eq!(cfg.accent_hex, "#22D3EE");
        assert_eq!(cfg.dot_profile, DotProfile::Crisp);
        assert_eq!(cfg.horizon_mode, HorizonMode::Peaks);
        assert_eq!(cfg.density, DensityMode::Compact);
        assert_eq!(cfg.sidebar_position, SidebarPosition::Right);
    }

    // ---- slate-v1 defaults & tokens ----
    #[test]
    fn default_config_is_slate_v1_charcoal_slate() {
        let cfg = UiCustomizationConfig::default();
        assert_eq!(cfg.bg_hex, "#0B0D12");
        assert_eq!(cfg.surface_hex, "#11141B");
        assert_eq!(cfg.accent_hex, "#94A3B8");
        assert_eq!(cfg.accent_soft_hex, "#CBD5E1");
        assert_eq!(cfg.accent_bright_hex, "#E2E8F0");
        assert_eq!(cfg.text_hex, "#F1F5F9");
        assert_eq!(cfg.accent_preset, AccentPreset::Slate);
        assert_eq!(cfg.preset_name, "Slate Monochrome");
        assert_eq!(cfg.panel_alpha, 0.80);
        assert_eq!(cfg.border_opacity, 0.22);
        assert_eq!(cfg.glow_intensity, 0.30);
        assert_eq!(cfg.bloom, 0.75);
        // Semantic telemetry colors stay untouched (never ambient chrome).
        assert_eq!(cfg.amber_hex, "#F59E0B");
        assert_eq!(cfg.rose_hex, "#F43F5E");
        assert_eq!(slate_v1::SEMANTIC_LIVE, "#34D399");
        assert_ne!(cfg.accent_hex, slate_v1::SEMANTIC_LIVE);
    }

    #[test]
    fn all_eight_accent_swatches_match_web_definitions() {
        assert_eq!(ACCENT_SWATCHES.len(), 8);
        let expected = [
            ("slate", "Slate", "#94A3B8", "#CBD5E1", "#E2E8F0", "#11141B"),
            ("emerald", "Emerald", "#34D399", "#6EE7B7", "#4ADE80", "#0A1613"),
            ("cyan", "Glacier Cyan", "#38BDF8", "#7DD3FC", "#22D3EE", "#08141D"),
            ("violet", "Cyber Violet", "#A855F7", "#D8B4FE", "#C084FC", "#120B1E"),
            ("amber", "Solaris Amber", "#F59E0B", "#FCD34D", "#FBBF24", "#161108"),
            ("rose", "Sakura Rose", "#FB7185", "#FDA4AF", "#F472B6", "#190B12"),
            ("crimson", "Crimson", "#EF4444", "#FCA5A5", "#F87171", "#19090B"),
            ("mono", "Monograph", "#E2E8F0", "#F8FAFC", "#CBD5E1", "#090B0E"),
        ];
        for (i, e) in expected.iter().enumerate() {
            let s = &ACCENT_SWATCHES[i];
            assert_eq!((s.id, s.label, s.accent_hex, s.accent_soft_hex, s.accent_bright_hex, s.surface_hex), *e);
        }
        assert_eq!(ACCENT_SWATCHES[0].id, "slate"); // default swatch first
        assert!(get_accent_swatch("custom").is_none());
    }

    // ---- Mirror: uiCustomization.test.ts #1 ----
    #[test]
    fn normalize_hex_and_sanitize_ui_patch_clamp_values_and_compute_safe_dark_surfaces() {
        assert_eq!(normalize_hex("34d399"), Some("#34D399".into()));
        assert_eq!(normalize_hex("#f0a"), Some("#FF00AA".into()));
        assert_eq!(normalize_hex("not-a-color"), None);

        let sanitized = sanitize_ui_patch(&json!({
            "accentHex": "#ff5500",
            "surfaceHex": "#FFFFFF",
            "panelAlpha": 5,
            "borderOpacity": -2,
            "glowIntensity": 99,
            "sidebarWidthPx": 1000,
            "fontScale": 0.1,
            "portraitScale": 4,
            "glassBlurPx": 99,
            "backgroundOpacity": 0.01,
        }));
        assert_eq!(sanitized.accent_hex.as_deref(), Some("#FF5500"));
        assert_eq!(sanitized.accent_preset, Some(AccentPreset::Custom));
        assert!(sanitized.accent_soft_hex.as_deref().unwrap().starts_with('#'));
        assert_ne!(sanitized.surface_hex.as_deref(), Some("#FFFFFF"));
        assert_eq!(sanitized.panel_alpha, Some(0.95));
        assert_eq!(sanitized.border_opacity, Some(0.08));
        assert_eq!(sanitized.glow_intensity, Some(2.0));
        assert_eq!(sanitized.sidebar_width, Some(420.0));
        assert_eq!(sanitized.font_scale, Some(0.85));
        assert_eq!(sanitized.portrait_scale, Some(1.45));
        assert_eq!(sanitized.glass_blur_px, Some(28.0));
        assert_eq!(sanitized.background_opacity, Some(0.15));
    }

    // ---- Mirror: uiCustomization.test.ts #2 ----
    #[test]
    fn apply_ui_patch_applies_curated_themes_individual_edits_and_reset_all() {
        let outcome = apply_ui_patch(
            &UiCustomizationConfig::default(),
            &json!({ "presetId": "cyber-violet", "sidebarPosition": "right" }),
        );
        let violet = outcome.next;
        assert_eq!(violet.accent_preset, AccentPreset::Violet);
        assert_eq!(violet.accent_hex, "#A855F7");
        assert_eq!(violet.font_preset, FontPreset::Display);
        assert_eq!(violet.sidebar_position, SidebarPosition::Right);
        assert_eq!(outcome.sanitized.background_preset_id.as_deref(), Some("nebula"));
        assert!(outcome.changed_keys.iter().any(|k| k.starts_with("preset:")));

        let reset = apply_ui_patch(&violet, &json!({ "resetAll": true }));
        assert_eq!(reset.next, UiCustomizationConfig::default());
        assert_eq!(reset.changed_keys, vec!["resetAll".to_string()]);
        assert_eq!(reset.sanitized.background_preset_id.as_deref(), Some("aurora-drift"));
        assert_eq!(reset.sanitized.background_opacity, Some(0.58));
        assert_eq!(reset.sanitized.glass_blur_px, Some(14.0));
    }

    // ---- Mirror: uiCustomization.test.ts #3 ----
    #[test]
    fn parse_natural_ui_intent_handles_multi_element_requests_without_false_triggers() {
        let def = UiCustomizationConfig::default();
        assert!(parse_natural_ui_intent("Summarize my morning brief and tasks", &def).is_none());
        assert!(parse_natural_ui_intent("Approve card-1 for Mail MCP", &def).is_none());

        let complex = parse_natural_ui_intent(
            "Make the UI cyberpunk purple with monospace font, sharp corners, move sidebar to the right, set background to braille rain with blur 20, make portrait bigger and pink, and hide the horizon wave",
            &def,
        ).expect("expected UI intent to parse");
        assert_eq!(complex.accent_preset, Some(AccentPreset::Violet));
        assert_eq!(complex.font_preset, Some(FontPreset::Mono));
        assert_eq!(complex.corner_radius, Some(CornerRadius::Sharp));
        assert_eq!(complex.sidebar_position, Some(SidebarPosition::Right));
        assert_eq!(complex.background_preset_id.as_deref(), Some("braille-rain"));
        assert_eq!(complex.glass_blur_px, Some(20.0));
        assert_eq!(complex.portrait_scale, Some(1.3));
        assert_eq!(complex.portrait_tint_hex, Some(Some("#F472B6".into())));
        assert_eq!(complex.horizon_visible, Some(false));

        let reset = parse_natural_ui_intent("reset all UI settings back to default", &def).unwrap();
        assert!(reset.reset_all);
        assert_eq!(reset, UiCustomizationPatch { reset_all: true, ..Default::default() });

        let custom_hex = parse_natural_ui_intent("set accent color to #00E5FF and rename brand to > atlas", &def).unwrap();
        assert_eq!(custom_hex.accent_hex.as_deref(), Some("#00E5FF"));
        assert_eq!(custom_hex.workspace_title.as_deref(), Some("> atlas"));
    }

    // ---- Mirror: uiCustomization.test.ts #4 ----
    #[test]
    fn all_curated_presets_have_unique_ids_and_valid_sanitized_configs() {
        let def = UiCustomizationConfig::default();
        assert_eq!(def.accent_preset, AccentPreset::Slate);
        assert_eq!(def.accent_hex, "#94A3B8");
        let presets = curated_ui_presets();
        assert_eq!(presets.len(), 7);
        assert_eq!(BUILT_IN_PRESETS.len(), 7);
        let mut ids = std::collections::HashSet::new();
        for preset in presets {
            assert!(ids.insert(preset.id), "duplicate preset id: {}", preset.id);
            let next = apply_ui_patch(&def, &json!({ "presetId": preset.id })).next;
            assert!(normalize_hex(&next.accent_hex).is_some());
            assert!(normalize_hex(&next.surface_hex).is_some());
        }
        // Opt-in emerald preset remains cleanly selectable.
        let emerald = apply_ui_patch(&def, &json!({ "presetId": "emerald-terminal" })).next;
        assert_eq!(emerald.accent_preset, AccentPreset::Emerald);
        assert_eq!(emerald.accent_hex, "#34D399");
    }

    // ---- Additional parity coverage ----
    #[test]
    fn sanitize_expands_swatch_and_respects_explicit_surface() {
        let p = sanitize_ui_patch(&json!({ "accentPreset": "violet" }));
        assert_eq!(p.accent_hex.as_deref(), Some("#A855F7"));
        assert_eq!(p.accent_soft_hex.as_deref(), Some("#D8B4FE"));
        assert_eq!(p.accent_bright_hex.as_deref(), Some("#C084FC"));
        assert_eq!(p.surface_hex.as_deref(), Some("#120B1E"));

        let p2 = sanitize_ui_patch(&json!({ "accentPreset": "violet", "surfaceHex": "#101010" }));
        assert_eq!(p2.surface_hex.as_deref(), Some("#101010"));
    }

    #[test]
    fn custom_hex_derives_soft_bright_and_dark_surface() {
        let p = sanitize_ui_patch(&json!({ "accentHex": "#00E5FF" }));
        assert_eq!(p.accent_preset, Some(AccentPreset::Custom));
        assert_eq!(p.accent_soft_hex.as_deref(), Some(blend_hex("#00E5FF", "#FFFFFF", 0.42).as_str()));
        assert_eq!(p.accent_bright_hex.as_deref(), Some(blend_hex("#00E5FF", "#FFFFFF", 0.22).as_str()));
        assert_eq!(p.surface_hex.as_deref(), Some(blend_hex("#0B0D12", "#00E5FF", 0.08).as_str()));
        // Known swatch hex maps back to its preset, no custom surface blend.
        let p2 = sanitize_ui_patch(&json!({ "accentHex": "#38BDF8" }));
        assert_eq!(p2.accent_preset, Some(AccentPreset::Cyan));
        assert!(p2.surface_hex.is_none());
    }

    #[test]
    fn sanitize_rejects_invalid_enum_values_and_null_tint_clears() {
        let p = sanitize_ui_patch(&json!({
            "cornerRadius": "bogus",
            "horizonStyle": "ribbon",
            "mood": "ecstatic",
            "composerStyle": "hovering",
            "portraitTintHex": null,
            "backgroundPresetId": "custom-upload",
        }));
        assert!(p.corner_radius.is_none());
        assert!(p.horizon_mode.is_none());
        assert!(p.mood.is_none());
        assert!(p.composer_style.is_none());
        assert_eq!(p.portrait_tint_hex, Some(None));
        assert_eq!(p.background_preset_id.as_deref(), Some("custom-upload"));
        let auto = sanitize_ui_patch(&json!({ "portraitTintHex": "auto" }));
        assert_eq!(auto.portrait_tint_hex, Some(None));
    }

    #[test]
    fn apply_preset_matches_id_name_and_legacy_aliases() {
        let mut cfg = UiCustomizationConfig::default();
        assert!(apply_preset(&mut cfg, "solaris-amber"));
        assert_eq!(cfg.preset_name, "Solaris Amber");
        assert_eq!(cfg.accent_hex, "#F59E0B");
        assert_eq!(cfg.corner_radius, CornerRadius::Sharp);
        assert!(apply_preset(&mut cfg, "Glacier Ice"));
        assert_eq!(cfg.accent_hex, "#38BDF8");
        assert!(apply_preset(&mut cfg, "abyss"));
        assert_eq!(cfg.preset_name, "Emerald Terminal");
        assert!(!apply_preset(&mut cfg, "does-not-exist"));
    }

    #[test]
    fn changed_keys_and_summary_follow_ts_format() {
        let def = UiCustomizationConfig::default();
        let none = apply_ui_patch(&def, &json!({}));
        assert_eq!(none.summary, "No UI changes needed");
        assert!(none.changed_keys.is_empty());

        let one = apply_ui_patch(&def, &json!({ "workspaceTitle": "Ops Rig" }));
        assert_eq!(one.changed_keys, vec!["workspaceTitle".to_string()]);
        assert_eq!(one.summary, "Applied workspaceTitle");
        assert_eq!(one.next.workspace_title, "Ops Rig");

        // TS applyUiPatch only diffs *sanitized patch* keys against next —
        // preset config values are merged verbatim, so a pure preset apply
        // yields exactly: preset marker + background/blur/bgOpacity/mood.
        let preset_only = apply_ui_patch(&def, &json!({ "presetId": "cyber-violet" }));
        assert_eq!(
            preset_only.changed_keys,
            vec![
                "preset:Cyber Violet".to_string(),
                "background:nebula".to_string(),
                "blur:16px".to_string(),
                "bgOpacity:72%".to_string(),
                "mood:focused".to_string(),
            ]
        );
        assert_eq!(
            preset_only.summary,
            "Applied preset:Cyber Violet, background:nebula, blur:16px, bgOpacity:72%, mood:focused"
        );

        let many = apply_ui_patch(&def, &json!({
            "accentHex": "#FF5500",
            "panelAlpha": 0.5,
            "borderOpacity": 0.3,
            "glowIntensity": 1.0,
            "fontScale": 1.1,
            "workspaceTitle": "Ops Rig"
        }));
        assert!(many.changed_keys.len() > 5);
        assert!(many.summary.starts_with("Applied accentPreset, accentHex, "));
        assert!(many.summary.contains(&format!("(+{} more)", many.changed_keys.len() - 5)));
    }

    #[test]
    fn ui_config_serde_roundtrips_camel_case_and_legacy_snake_case() {
        let cfg = UiCustomizationConfig::default();
        let json_str = serde_json::to_string(&cfg).unwrap();
        assert!(json_str.contains("\"accentHex\""));
        assert!(json_str.contains("\"panelAlpha\""));
        let back: UiCustomizationConfig = serde_json::from_str(&json_str).unwrap();
        assert_eq!(back, cfg);
        // Legacy desktop-persisted snake_case JSON still loads; missing new
        // fields fall back to slate-v1 defaults.
        let legacy: UiCustomizationConfig =
            serde_json::from_str(r##"{"accent_hex":"#123456","bg_hex":"#050C0A"}"##).unwrap();
        assert_eq!(legacy.accent_hex, "#123456");
        assert_eq!(legacy.bg_hex, "#050C0A");
        assert_eq!(legacy.accent_soft_hex, "#CBD5E1");
        assert_eq!(legacy.panel_alpha, 0.80);
    }

    #[test]
    fn nl_intent_direct_preset_and_navigation_and_misc_branches() {
        let def = UiCustomizationConfig::default();
        let p = parse_natural_ui_intent("zen-mono", &def).unwrap();
        assert_eq!(p.preset_id.as_deref(), Some("zen-mono"));

        let p = parse_natural_ui_intent("open the canvas view", &def).unwrap();
        assert_eq!(p.active_view.as_deref(), Some("canvas"));

        let p = parse_natural_ui_intent("hide the sidebar and reduce motion", &def).unwrap();
        assert_eq!(p.sidebar_visible, Some(false));
        assert_eq!(p.reduced_motion, Some(true));

        let p = parse_natural_ui_intent("make the portrait tint auto", &def).unwrap();
        assert_eq!(p.portrait_tint_hex, Some(None));

        let p = parse_natural_ui_intent("set background opacity to 45", &def).unwrap();
        assert_eq!(p.background_opacity, Some(0.45));

        let p = parse_natural_ui_intent("make a wider sidebar", &def).unwrap();
        assert_eq!(p.sidebar_width, Some(344.0));

        let p = parse_natural_ui_intent("auto-arrange the canvas sessions", &def).unwrap();
        assert_eq!(p.canvas_auto_arrange, Some(true));

        // Empty / non-UI input never fires.
        assert!(parse_natural_ui_intent("   ", &def).is_none());
        assert!(parse_natural_ui_intent("what is the weather today", &def).is_none());
    }

    #[test]
    fn decode_frontier_step_reveals_progressively() {
        assert_eq!(decode_frontier_step("abc", 0.0), "   ");
        assert_eq!(decode_frontier_step("abc", 1.0), "abc");
        let mid = decode_frontier_step("abcd", 0.5);
        assert!(mid.starts_with("ab"));
        assert_eq!(mid.chars().count(), 4);
    }

    #[test]
    fn background_settings_default_is_web_parities() {
        let bg = crate::types::BackgroundSettings::default();
        assert_eq!(bg.active_id, "aurora-drift");
        assert_eq!(bg.opacity, 0.64);
        assert_eq!(bg.blur_px, 12.0);
        assert!(bg.custom_slots.is_empty());
        assert_eq!(bg.preset.web_id(), "aurora-drift");
        assert_eq!(BackgroundPresetId::from_web_id("braille-rain"), Some(BackgroundPresetId::PhosphorGrain));
    }
}
