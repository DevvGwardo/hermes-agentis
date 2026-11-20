//! Bundled fonts (P4-05 fonts-assets).
//!
//! The three OFL families are embedded via `include_bytes!` so the single
//! binary carries its own typography (no system-font dependency, no runtime
//! asset path). Wiring seam (verified against rune's own integration test
//! `crates/rune/tests/integration/font_swap.rs`): `AppSpec.fonts` takes the
//! embedded byte slices and `AppSpec.families` sets the default (sans, mono)
//! slots; `Family::Named(...)` in `TextStyle::family` resolves against every
//! loaded face, which is how theme.rs reaches Archivo for display headings.
//!
//! Licenses: assets/fonts/OFL-*.txt (bundled alongside in release packages).

/// Plus Jakarta Sans — variable weight axis (UI text, 400/500/600).
pub const FONT_UI_BYTES: &[u8] = include_bytes!("../../../assets/fonts/PlusJakartaSans[wght].ttf");
/// Archivo — variable width+weight axes (display headings, 600/700).
/// NOTE: the variable file's default instance is SemiBold; the typographic
/// family (name ID 16) is "Archivo", which is what fontdb registers.
pub const FONT_DISPLAY_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Archivo[wdth,wght].ttf");
/// JetBrains Mono — variable weight axis (CLI, timers, tabular readouts).
pub const FONT_MONO_BYTES: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono[wght].ttf");

/// All bundled faces, for `AppSpec.fonts`.
pub const APP_FONTS: &[&[u8]] = &[FONT_UI_BYTES, FONT_DISPLAY_BYTES, FONT_MONO_BYTES];

/// Default (sans, mono) slots, for `AppSpec.families`.
pub const FAMILIES: (&str, &str) = ("Plus Jakarta Sans", "JetBrains Mono");

#[cfg(test)]
mod tests {
    use super::*;
    use rune::text::TextSystem;
    use rune::{Cx, El, Env, Family, Runtime, TextStyle, col, row, text};

    #[test]
    fn bundled_fonts_are_valid_nonempty_ttfs() {
        for (name, bytes) in [
            ("PlusJakartaSans", FONT_UI_BYTES),
            ("Archivo", FONT_DISPLAY_BYTES),
            ("JetBrainsMono", FONT_MONO_BYTES),
        ] {
            assert!(bytes.len() > 10_000, "{name} too small: {}", bytes.len());
            // TrueType sfnt version 1.0 (variable TTFs included).
            assert_eq!(&bytes[..4], &[0x00, 0x01, 0x00, 0x00], "{name} bad sfnt magic");
        }
    }

    fn view(_: &(), _: &mut Cx<()>) -> El {
        let s = TextStyle::new(14.0, 400.0);
        col().children([
            row().child(
                text("Hamburgefonstiv 0123")
                    .key("ui")
                    .font(s.family(Family::Named("Plus Jakarta Sans"))),
            ),
            row().child(
                text("Hamburgefonstiv 0123")
                    .key("display")
                    .font(TextStyle::new(14.0, 700.0).family(Family::Named("Archivo"))),
            ),
            row().child(
                text("let mono = 42;")
                    .key("mono")
                    .font(s.family(Family::Named("JetBrains Mono"))),
            ),
            row().child(
                text("Hamburgefonstiv 0123")
                    .key("missing")
                    .font(s.family(Family::Named("Rune Missing Family XYZ"))),
            ),
        ])
    }

    fn width(rt: &Runtime<()>, key: &str) -> f32 {
        rt.elements()
            .iter()
            .find(|e| e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("{key} missing"))
            .target[2]
    }

    #[test]
    fn bundled_families_resolve_headless() {
        // Pattern from rune's font_swap integration test: TextSystem::new with
        // embedded bytes + Runtime::with_text, then compare rendered widths.
        // NOTE: an unknown Family::Named falls back to the SANS SLOT (font_swap
        // test 4), so "missing" is NOT a valid control — it equals the bundled
        // sans by design. Controls: (a) distinct bundled faces measure apart,
        // (b) a system-only TextSystem measures differently from the bundled one.
        let ts = TextSystem::new(APP_FONTS, FAMILIES);
        let mut rt = Runtime::with_text((), view, (600.0, 400.0), Env::new(true, 1.0), ts);
        rt.frame(0.0);
        let (ui, display, mono) = (width(&rt, "ui"), width(&rt, "display"), width(&rt, "mono"));
        assert!(ui > 0.0 && display > 0.0 && mono > 0.0, "all faces shape text");
        assert!(
            (ui - mono).abs() > 0.5,
            "Jakarta vs JetBrains Mono measure identical — faces not registered: {ui} vs {mono}"
        );
        assert!(
            (ui - display).abs() > 0.5,
            "Jakarta vs Archivo measure identical — Archivo not resolved (SemiBold name-ID caveat?): {ui} vs {display}"
        );
        assert!(rt.stats.shaped > 0, "text actually shaped");

        // System-only control: same families requested, NO embedded bytes.
        let ts_sys = TextSystem::new(&[], FAMILIES);
        let mut rt_sys = Runtime::with_text((), view, (600.0, 400.0), Env::new(true, 1.0), ts_sys);
        rt_sys.frame(0.0);
        let ui_sys = width(&rt_sys, "ui");
        assert!(
            (ui - ui_sys).abs() > 0.5,
            "bundled Plus Jakarta Sans measures like the system fallback — bytes not used: {ui} vs {ui_sys}"
        );
    }
}
