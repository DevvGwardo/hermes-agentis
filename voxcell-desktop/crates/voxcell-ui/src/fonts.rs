//! Bundled-font registration (P4-05 fonts-assets workstream).
//!
//! PRE-WIRED STUB by the parent so no agent edits shared files: `main.rs`
//! already calls `register_fonts()` before `App::new`. The fonts-assets
//! workstream owns this file: load the bundled OFL fonts from
//! `voxcell-desktop/assets/fonts/` into rune's text system (see
//! `third_party/rune/crates/rune/src/text/fonts.rs` for the registration API)
//! so `Family::Named("Plus Jakarta Sans" / "Archivo" / "JetBrains Mono")`
//! resolves to the bundled faces instead of system fallbacks. Keep the
//! `register_fonts()` signature stable (main.rs call site is foreign-owned);
//! if a different seam is required, report it instead of editing main.rs.

/// Register bundled fonts with rune's text system. No-op until implemented.
pub fn register_fonts() {}