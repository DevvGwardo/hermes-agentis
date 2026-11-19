# Voxcell Desktop — Packaging Contract (P4-05)

Parent-owned interface contract. Packaging workstreams (win/mac/linux) and release-ci
code against THIS file; do not edit it — report needed changes instead.

## Layout
```
packaging/
  README.md            <- this contract (parent-owned)
  windows/build.ps1    <- win-packaging
  macos/build.sh       <- mac-packaging
  linux/build.sh       <- linux-packaging
dist/                  <- build output (gitignored), per-platform subdirs
```

## Script interface (all three platforms)
- Input: run from repo root or voxcell-desktop/ (detect both). Env `VOXCELL_VERSION`
  overrides version; default = workspace Cargo.toml `version` (currently 0.1.0).
- Steps: `cargo build --release -p voxcell-app` (binary name: `voxcell`), then bundle
  `voxcell-desktop/assets/**` (fonts + character art) with the binary, then package.
- Output artifacts in `dist/<platform>/`:
  - windows: `Voxcell-<version>-x64.msi` (WiX/cargo-wix preferred) or
    `Voxcell-<version>-x64-setup.exe` (NSIS) — PLUS always `Voxcell-<version>-x64.zip`
    (portable fallback, must always be produced).
  - macos: `Voxcell-<version>-<arch>.dmg` (hdiutil/create-dmg; arch = arm64/x86_64).
  - linux: `Voxcell-<version>-x86_64.AppImage` (appimagetool) and
    `Voxcell-<version>-amd64.deb` (dpkg-deb/cargo-deb).
- Exit 0 only if the primary artifact exists; print artifact paths + sizes at the end.

## Signing (NO certificates available in this environment)
- Every script must contain a clearly-marked signing hook that activates ONLY when
  credentials are present, and skips gracefully otherwise:
  - windows: `signtool sign /fd SHA256 /tr <timestamp-url> /td SHA256` gated on
    `VOXCELL_SIGN_CERT` (pfx path) + `VOXCELL_SIGN_PASSWORD` env.
  - macos: `codesign --deep --options runtime` gated on `APPLE_SIGN_IDENTITY`, then
    `xcrun notarytool submit --wait` gated on `APPLE_ID`/`APPLE_TEAM_ID`/`APPLE_APP_PASSWORD`.
  - linux: no signing; publish SHA256SUMS next to artifacts.
- Unsigned artifacts are the expected output here. Document the cert steps in each
  script's header comment. NEVER fake a signature.

## Verification expectations (honesty rule)
- windows/build.ps1 MUST be executed for real on this Windows machine and produce at
  least the .zip (msi/exe if tooling can be installed: choco/cargo install are allowed;
  if WiX/NSIS cannot be installed, ship the zip + a ready-to-run .wxs/.nsi script and
  report the gap).
- macos/linux scripts cannot run natively here: verify with `bash -n` + shellcheck if
  available, and make them CI-runnable (release-ci executes them on native runners).
  Report exactly what was and wasn't executed.

## release-ci (.github/workflows/rune-release.yml)
- Triggers: push tag `v*` + workflow_dispatch.
- Job 1 (test): ubuntu-latest — `cargo test --workspace` in voxcell-desktop (submodule
  checkout with the pinned SHA; Rust stable; Swatinem/rust-cache or actions/cache).
- Job 2 (package): matrix [windows-latest, macos-latest, ubuntu-latest], needs test —
  run the platform script, upload dist artifacts (actions/upload-artifact).
- Job 3 (release): on tag — download artifacts, create GitHub Release (softprops/action-gh-release
  or gh CLI), attach artifacts + SHA256SUMS. Signing steps gated on repo secrets, skip if absent.
- Do NOT modify .github/workflows/rune-desktop.yml (existing CI).

## Assets
- `voxcell-desktop/assets/fonts/` — Plus Jakarta Sans (400/500/600), Archivo (600/700),
  JetBrains Mono (400/500) + OFL license files (fonts-assets workstream downloads these).
- `voxcell-desktop/assets/character/` — portrait_base.png + sprite_sheet.png copied from
  public/assets/aistudio/ (image_9fdb5ec4.png / image_f0b5dc2e.png per spec §5).
- Packaging bundles whatever assets/ contains at build time; fonts-assets owns content.