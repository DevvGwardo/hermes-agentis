#!/usr/bin/env bash
# =============================================================================
# Voxcell Desktop — macOS packaging script (P4-05, packaging/README.md)
#
# Usage (from repo root OR voxcell-desktop/, cwd-independent):
#   bash packaging/macos/build.sh
#   bash voxcell-desktop/packaging/macos/build.sh
#   bash packaging/macos/build.sh --skip-build   # reuse target/release/voxcell
#
# MUST run on macOS (hdiutil/codesign/notarytool are macOS-only). The release-ci
# workflow runs this on macos-latest; on any other OS it exits early with a
# clear error. It never fakes macOS artifacts.
#
# Steps:
#   1. cargo build --release -p voxcell-app   (binary: target/release/voxcell)
#   2. Stage the app bundle:
#        Voxcell.app/Contents/MacOS/voxcell
#        Voxcell.app/Contents/Resources/assets/**  (fonts + character art,
#          whatever assets/ contains at build time — fonts-assets owns content)
#        Voxcell.app/Contents/Info.plist  (identifier/name/version/HiDPI)
#   3. dist/macos/Voxcell-<version>-<arch>.dmg via hdiutil
#      (create-dmg used as fallback if hdiutil is unavailable)
#   4. SHA256SUMS next to the artifact
#
# Version: VOXCELL_VERSION env overrides; default = workspace Cargo.toml
#          [workspace.package] version.
# Arch:    uname -m -> arm64 | x86_64 (aarch64 mapped to arm64). Note: under
#          Rosetta uname -m reports x86_64; run CI on native runners.
#
# Signing + notarization hooks (NO certificates exist in this environment —
# unsigned output is the expected result; we NEVER fake a signature):
#   APPLE_SIGN_IDENTITY  e.g. "Developer ID Application: Name (TEAMID)"
#     -> codesign --deep --force --options runtime --timestamp \
#          --sign "$APPLE_SIGN_IDENTITY" Voxcell.app
#        (then codesign --verify --deep --strict)
#   APPLE_ID + APPLE_TEAM_ID + APPLE_APP_PASSWORD (app-specific password,
#   all three required)
#     -> xcrun notarytool submit <dmg> --apple-id ... --team-id ... \
#          --password ... --wait
#        xcrun stapler staple <dmg>   (+ stapler validate)
#   When the env vars are absent the hooks print a notice and skip gracefully.
#
# No .icns icon ships yet (assets/ has fonts + character art only); add
# CFBundleIconFile + Contents/Resources/Voxcell.icns when an icon exists.
# =============================================================================
set -euo pipefail

step() { printf '==> %s\n' "$*"; }
note() { printf '    %s\n' "$*"; }
fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

# --- Platform guard (honesty: no macOS tooling elsewhere) --------------------
if [ "$(uname -s)" != "Darwin" ]; then
    fail "macOS packaging must run on macOS (uname -s = $(uname -s)); use a macos-latest CI runner"
fi

# --- Args ---------------------------------------------------------------------
SKIP_BUILD=0
for arg in "$@"; do
    case "$arg" in
        --skip-build) SKIP_BUILD=1 ;;
        *) fail "unknown argument: $arg (supported: --skip-build)" ;;
    esac
done

# --- Locate workspace root (cwd-independent: script lives at packaging/macos/) ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
if [ ! -f "$WORKSPACE_ROOT/Cargo.toml" ]; then
    # Fallback: invoked from a copy elsewhere; try cwd and cwd/voxcell-desktop.
    for cand in "$PWD" "$PWD/voxcell-desktop"; do
        if [ -f "$cand/Cargo.toml" ]; then
            WORKSPACE_ROOT="$(cd "$cand" && pwd)"
            break
        fi
    done
fi
CARGO_TOML="$WORKSPACE_ROOT/Cargo.toml"
if [ ! -f "$CARGO_TOML" ]; then
    fail "Cannot locate workspace Cargo.toml (looked in $WORKSPACE_ROOT)"
fi
step "Workspace: $WORKSPACE_ROOT"

# --- Version: env override, else [workspace.package] version ------------------
VERSION="${VOXCELL_VERSION:-}"
if [ -z "$VERSION" ]; then
    VERSION="$(awk '
        /^[[:space:]]*\[workspace\.package\]/ { inpkg = 1; next }
        /^[[:space:]]*\[/                     { inpkg = 0 }
        inpkg && /^[[:space:]]*version[[:space:]]*=/ {
            sub(/^[^"]*"/, ""); sub(/".*$/, ""); print; exit
        }' "$CARGO_TOML")"
fi
if [ -z "$VERSION" ]; then
    fail 'Could not determine version (set VOXCELL_VERSION or fix Cargo.toml)'
fi
step "Version: $VERSION"

# --- Arch ----------------------------------------------------------------------
RAW_ARCH="$(uname -m)"
case "$RAW_ARCH" in
    arm64|aarch64) ARCH=arm64 ;;
    x86_64)        ARCH=x86_64 ;;
    *) fail "unsupported architecture for macOS packaging: $RAW_ARCH" ;;
esac
step "Arch: $ARCH"

DIST_DIR="$WORKSPACE_ROOT/dist/macos"
STAGE_ROOT="$DIST_DIR/stage"
APP_NAME="Voxcell.app"
APP_PATH="$STAGE_ROOT/$APP_NAME"
DMG_NAME="Voxcell-$VERSION-$ARCH.dmg"
DMG_PATH="$DIST_DIR/$DMG_NAME"
mkdir -p "$DIST_DIR"

# --- 1. Build -------------------------------------------------------------------
BIN_PATH="$WORKSPACE_ROOT/target/release/voxcell"
if [ "$SKIP_BUILD" -eq 1 ]; then
    if [ ! -f "$BIN_PATH" ]; then
        fail "--skip-build given but $BIN_PATH does not exist"
    fi
    step "Skipping cargo build (reusing $BIN_PATH)"
else
    step 'cargo build --release -p voxcell-app'
    (cd "$WORKSPACE_ROOT" && cargo build --release -p voxcell-app)
fi
if [ ! -f "$BIN_PATH" ]; then
    fail "Build reported success but $BIN_PATH is missing"
fi

# --- 2. Stage the .app bundle -----------------------------------------------------
step "Staging $APP_PATH"
rm -rf "$STAGE_ROOT"
mkdir -p "$APP_PATH/Contents/MacOS" "$APP_PATH/Contents/Resources"
cp "$BIN_PATH" "$APP_PATH/Contents/MacOS/voxcell"
chmod +x "$APP_PATH/Contents/MacOS/voxcell"

ASSETS_SRC="$WORKSPACE_ROOT/assets"
if [ -d "$ASSETS_SRC" ]; then
    cp -R "$ASSETS_SRC" "$APP_PATH/Contents/Resources/assets"
    N_ASSETS="$(find "$APP_PATH/Contents/Resources/assets" -type f | wc -l | tr -d '[:space:]')"
    note "Bundled $N_ASSETS asset files"
else
    note 'WARNING: assets/ directory not found — shipping binary only'
fi

cat > "$APP_PATH/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.voxcell.desktop</string>
    <key>CFBundleName</key>
    <string>Voxcell</string>
    <key>CFBundleDisplayName</key>
    <string>Voxcell</string>
    <key>CFBundleExecutable</key>
    <string>voxcell</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
</dict>
</plist>
EOF
printf 'APPL????' > "$APP_PATH/Contents/PkgInfo"

# --- Signing hook (gated on APPLE_SIGN_IDENTITY) ----------------------------------
# Sign the whole .app AFTER staging so the bundled binary carries the signature
# into the DMG. --options runtime enables the hardened runtime (notarization
# prerequisite); --timestamp gives a secure timestamp.
if [ -n "${APPLE_SIGN_IDENTITY:-}" ]; then
    command -v codesign >/dev/null 2>&1 || fail 'APPLE_SIGN_IDENTITY set but codesign not found'
    step "Signing $APP_NAME (identity: $APPLE_SIGN_IDENTITY)"
    codesign --deep --force --options runtime --timestamp \
        --sign "$APPLE_SIGN_IDENTITY" "$APP_PATH"
    codesign --verify --deep --strict "$APP_PATH"
else
    note 'Signing skipped (APPLE_SIGN_IDENTITY not set) — unsigned artifact is the expected output here'
fi

# --- 3. DMG ------------------------------------------------------------------------
# Drag-install layout: the volume holds Voxcell.app + an /Applications symlink.
ln -s /Applications "$STAGE_ROOT/Applications"
if command -v hdiutil >/dev/null 2>&1; then
    step "Creating $DMG_PATH (hdiutil)"
    hdiutil create -volname "Voxcell $VERSION" -srcfolder "$STAGE_ROOT" \
        -ov -format UDZO "$DMG_PATH"
elif command -v create-dmg >/dev/null 2>&1; then
    step "Creating $DMG_PATH (create-dmg fallback)"
    create-dmg --volname "Voxcell $VERSION" "$DMG_PATH" "$STAGE_ROOT"
else
    fail 'Neither hdiutil nor create-dmg found — cannot produce a .dmg on this machine'
fi
if [ ! -f "$DMG_PATH" ]; then
    fail "DMG was not produced: $DMG_PATH"
fi

# --- Notarization hook (gated on APPLE_ID + APPLE_TEAM_ID + APPLE_APP_PASSWORD) -----
if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ]; then
    if [ -z "${APPLE_SIGN_IDENTITY:-}" ]; then
        fail 'Notarization credentials set but APPLE_SIGN_IDENTITY is missing — an unsigned DMG cannot be notarized'
    fi
    command -v xcrun >/dev/null 2>&1 || fail 'Notarization requested but xcrun not found'
    step "Notarizing $DMG_NAME (notarytool submit --wait)"
    xcrun notarytool submit "$DMG_PATH" \
        --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" \
        --wait
    xcrun stapler staple "$DMG_PATH"
    xcrun stapler validate "$DMG_PATH"
else
    note 'Notarization skipped (APPLE_ID/APPLE_TEAM_ID/APPLE_APP_PASSWORD not all set)'
fi

# --- 4. SHA256SUMS ---------------------------------------------------------------------
step 'Writing SHA256SUMS'
(
    cd "$DIST_DIR"
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$DMG_NAME" > SHA256SUMS
    elif command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$DMG_NAME" > SHA256SUMS
    else
        fail 'Neither shasum nor sha256sum found — cannot write SHA256SUMS'
    fi
)

# --- 5. Report ----------------------------------------------------------------------------
step 'Artifacts:'
for f in "$DMG_PATH" "$DIST_DIR/SHA256SUMS"; do
    SIZE="$(wc -c < "$f" | tr -d '[:space:]')"
    printf '    %s  (%s bytes)\n' "$f" "$SIZE"
done
if [ ! -f "$DMG_PATH" ]; then
    fail "Primary artifact missing: $DMG_PATH"
fi
step "OK — primary artifact: $DMG_PATH"
exit 0
