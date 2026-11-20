#!/usr/bin/env bash
# =============================================================================
# Voxcell Desktop — Linux packaging script (P4-05, packaging/README.md)
#
# Usage (from repo root OR voxcell-desktop/, cwd-independent):
#   bash packaging/linux/build.sh
#   bash voxcell-desktop/packaging/linux/build.sh
#   bash packaging/linux/build.sh --skip-build   # reuse target/release/voxcell
#   bash packaging/linux/build.sh --offline      # never download appimagetool
#
# MUST run on Linux (release-ci executes this on ubuntu-latest). On any other
# OS it exits early with a clear error. It never fakes Linux artifacts.
#
# Steps:
#   1. cargo build --release -p voxcell-app   (binary: target/release/voxcell)
#   2. Stage an AppDir:
#        AppDir/AppRun                    (launcher)
#        AppDir/voxcell.desktop           (+ usr/share/applications copy)
#        AppDir/voxcell.svg               (+ hicolor icon copy)
#        AppDir/usr/bin/voxcell
#        AppDir/usr/bin/assets/**         (fonts + character art — whatever
#                                          assets/ contains at build time;
#                                          fonts-assets owns content)
#   3. dist/linux/Voxcell-<version>-x86_64.AppImage via appimagetool.
#      Tool resolution: $VOXCELL_APPIMAGETOOL -> PATH -> download from the
#      official GitHub releases (AppImage/appimagetool) into dist/linux/.tools/.
#      If the tool cannot be obtained (--offline, no network, no curl/wget),
#      the staged AppDir is shipped and the exact ready-to-run appimagetool
#      command is printed as an explicit gap.
#   4. dist/linux/Voxcell-<version>-amd64.deb via dpkg-deb --root-owner-group
#      --build (DEBIAN/control + DEBIAN/postinst; payload in /opt/voxcell with
#      a /usr/bin/voxcell wrapper so exe-relative asset lookup keeps working).
#      If dpkg-deb is missing, the .deb ar archive is assembled manually:
#      debian-binary + control.tar.gz + data.tar.gz, via `ar` or a pure-shell
#      ar writer (no external tools beyond tar/gzip).
#   5. SHA256SUMS next to the artifacts (Linux has NO signing per contract;
#      integrity is provided by SHA256SUMS. We never fake a signature).
#
# Version: VOXCELL_VERSION env overrides; default = workspace Cargo.toml
#          [workspace.package] version. A leading 'v' is stripped.
# Arch:    uname -m -> x86_64 (AppImage name) / amd64 (deb); aarch64 ->
#          aarch64 / arm64. Other architectures fail loudly.
#
# Exit policy (contract: "Exit 0 only if the primary artifact exists"):
#   primary = the AppImage when produced, else the .deb. If NEITHER artifact
#   can be produced the script exits non-zero. Tooling gaps are printed
#   explicitly and never silently swallowed.
#
# Runtime dependencies (deb Depends, best-effort for a wgpu/winit app):
#   libvulkan1, libegl1, libgl1, libx11-6, libxcb1, libxkbcommon0,
#   libwayland-client0, libfontconfig1. Adjust after a native smoke test.
#
# Icon: packaging/linux/voxcell.svg is a placeholder mark owned by this
# workstream (assets/ ships fonts + character art only). Override with
# VOXCELL_ICON=/path/to/icon.svg|png when a brand icon exists.
# =============================================================================
set -euo pipefail

step() { printf '==> %s\n' "$*"; }
note() { printf '    %s\n' "$*"; }
fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

GAPS=()
gap() { GAPS+=("$*"); note "GAP: $*"; }

# --- Platform guard (honesty: no Linux packaging elsewhere) ------------------
if [ "$(uname -s)" != "Linux" ]; then
    fail "Linux packaging must run on Linux (uname -s = $(uname -s)); use an ubuntu-latest CI runner"
fi

# --- Args ---------------------------------------------------------------------
SKIP_BUILD=0
OFFLINE=0
for arg in "$@"; do
    case "$arg" in
        --skip-build) SKIP_BUILD=1 ;;
        --offline)    OFFLINE=1 ;;
        -h|--help)    sed -n '2,45p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) fail "unknown argument: $arg (supported: --skip-build, --offline)" ;;
    esac
done

# --- Locate workspace root (cwd-independent: script lives at packaging/linux/) ---
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
VERSION="${VERSION#v}"
if [ -z "$VERSION" ]; then
    fail 'Could not determine version (set VOXCELL_VERSION or fix Cargo.toml)'
fi
# dpkg version policy: must start with a digit; allowed chars afterwards.
if ! printf '%s' "$VERSION" | grep -Eq '^[0-9][A-Za-z0-9.+:~-]*$'; then
    fail "Version '$VERSION' is not a valid Debian version (must start with a digit)"
fi
step "Version: $VERSION"

# --- Arch ----------------------------------------------------------------------
RAW_ARCH="$(uname -m)"
case "$RAW_ARCH" in
    x86_64)  APPIMAGE_ARCH=x86_64;  DEB_ARCH=amd64 ;;
    aarch64) APPIMAGE_ARCH=aarch64; DEB_ARCH=arm64 ;;
    *) fail "unsupported architecture for Linux packaging: $RAW_ARCH" ;;
esac
step "Arch: $APPIMAGE_ARCH (deb: $DEB_ARCH)"

DIST_DIR="$WORKSPACE_ROOT/dist/linux"
APPDIR="$DIST_DIR/AppDir"
DEB_ROOT="$DIST_DIR/debroot"
APPIMAGE_NAME="Voxcell-$VERSION-$APPIMAGE_ARCH.AppImage"
APPIMAGE_PATH="$DIST_DIR/$APPIMAGE_NAME"
DEB_NAME="Voxcell-$VERSION-$DEB_ARCH.deb"
DEB_PATH="$DIST_DIR/$DEB_NAME"
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

# --- 2. Stage the AppDir ----------------------------------------------------------
step "Staging $APPDIR"
rm -rf "$APPDIR" "$DEB_ROOT"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/scalable/apps"
cp "$BIN_PATH" "$APPDIR/usr/bin/voxcell"
chmod 755 "$APPDIR/usr/bin/voxcell"

ASSETS_SRC="$WORKSPACE_ROOT/assets"
if [ -d "$ASSETS_SRC" ]; then
    cp -R "$ASSETS_SRC" "$APPDIR/usr/bin/assets"
    N_ASSETS="$(find "$APPDIR/usr/bin/assets" -type f | wc -l | tr -d '[:space:]')"
    note "Bundled $N_ASSETS asset files"
else
    note 'WARNING: assets/ directory not found — shipping binary only'
fi

# Icon: explicit override > workstream placeholder next to this script.
ICON_SRC="${VOXCELL_ICON:-$SCRIPT_DIR/voxcell.svg}"
if [ ! -f "$ICON_SRC" ]; then
    fail "Icon not found: $ICON_SRC (set VOXCELL_ICON or restore packaging/linux/voxcell.svg)"
fi
ICON_EXT="${ICON_SRC##*.}"
case "$ICON_EXT" in
    svg|png|xpm) : ;;
    *) fail "Unsupported icon type .$ICON_EXT (use svg, png or xpm)" ;;
esac
cp "$ICON_SRC" "$APPDIR/voxcell.$ICON_EXT"
cp "$ICON_SRC" "$APPDIR/usr/share/icons/hicolor/scalable/apps/voxcell.$ICON_EXT"

cat > "$APPDIR/voxcell.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Voxcell
Comment=Voxcell desktop application
Exec=voxcell
Icon=voxcell
Terminal=false
Categories=Graphics;Utility;
StartupWMClass=voxcell
X-AppImage-Version=$VERSION
EOF
cp "$APPDIR/voxcell.desktop" "$APPDIR/usr/share/applications/voxcell.desktop"

cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
# AppImage launcher: resolve the mounted AppDir and exec the real binary.
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/voxcell" "$@"
EOF
chmod 755 "$APPDIR/AppRun"

# --- 3. AppImage -------------------------------------------------------------------
# Resolve appimagetool: env override > PATH > official GitHub release download.
APPIMAGETOOL=""
if [ -n "${VOXCELL_APPIMAGETOOL:-}" ]; then
    if [ ! -x "$VOXCELL_APPIMAGETOOL" ]; then
        fail "VOXCELL_APPIMAGETOOL is not an executable file: $VOXCELL_APPIMAGETOOL"
    fi
    APPIMAGETOOL="$VOXCELL_APPIMAGETOOL"
elif command -v appimagetool >/dev/null 2>&1; then
    APPIMAGETOOL="$(command -v appimagetool)"
elif [ "$OFFLINE" -eq 0 ]; then
    TOOL_DIR="$DIST_DIR/.tools"
    mkdir -p "$TOOL_DIR"
    APPIMAGETOOL_DL="$TOOL_DIR/appimagetool-$RAW_ARCH.AppImage"
    TAG="${VOXCELL_APPIMAGETOOL_TAG:-continuous}"
    URL="https://github.com/AppImage/appimagetool/releases/download/$TAG/appimagetool-$RAW_ARCH.AppImage"
    step "Downloading appimagetool: $URL"
    if command -v curl >/dev/null 2>&1; then
        curl -fL --retry 3 -o "$APPIMAGETOOL_DL" "$URL" || APPIMAGETOOL_DL=""
    elif command -v wget >/dev/null 2>&1; then
        wget -O "$APPIMAGETOOL_DL" "$URL" || APPIMAGETOOL_DL=""
    else
        note 'Neither curl nor wget found — cannot download appimagetool'
        APPIMAGETOOL_DL=""
    fi
    if [ -n "$APPIMAGETOOL_DL" ] && [ -s "$APPIMAGETOOL_DL" ]; then
        chmod 755 "$APPIMAGETOOL_DL"
        APPIMAGETOOL="$APPIMAGETOOL_DL"
    else
        rm -f "$APPIMAGETOOL_DL"
    fi
fi

if [ -n "$APPIMAGETOOL" ]; then
    step "Creating $APPIMAGE_PATH (appimagetool: $APPIMAGETOOL)"
    # APPIMAGE_EXTRACT_AND_RUN avoids the FUSE requirement (CI containers);
    # ARCH tells appimagetool which runtime to embed.
    if APPIMAGE_EXTRACT_AND_RUN=1 ARCH="$APPIMAGE_ARCH" \
        "$APPIMAGETOOL" "$APPDIR" "$APPIMAGE_PATH"; then
        :
    else
        rm -f "$APPIMAGE_PATH"
        gap "appimagetool failed — AppDir kept at $APPDIR"
    fi
    if [ -f "$APPIMAGE_PATH" ]; then
        chmod 755 "$APPIMAGE_PATH"
    else
        gap "AppImage was not produced: $APPIMAGE_PATH"
    fi
else
    gap "appimagetool unavailable (offline or download failed) — AppDir kept at $APPDIR"
fi
if [ ! -f "$APPIMAGE_PATH" ]; then
    note 'Ready-to-run command once appimagetool is available:'
    note "  ARCH=$APPIMAGE_ARCH APPIMAGE_EXTRACT_AND_RUN=1 appimagetool '$APPDIR' '$APPIMAGE_PATH'"
    note 'Get appimagetool: https://github.com/AppImage/appimagetool/releases'
fi

# --- 4. .deb ------------------------------------------------------------------------
step "Staging $DEB_ROOT"
mkdir -p "$DEB_ROOT/DEBIAN" "$DEB_ROOT/opt/voxcell" "$DEB_ROOT/usr/bin" \
         "$DEB_ROOT/usr/share/applications" \
         "$DEB_ROOT/usr/share/icons/hicolor/scalable/apps"
cp "$BIN_PATH" "$DEB_ROOT/opt/voxcell/voxcell"
if [ -d "$ASSETS_SRC" ]; then
    cp -R "$ASSETS_SRC" "$DEB_ROOT/opt/voxcell/assets"
fi
cp "$APPDIR/voxcell.desktop" "$DEB_ROOT/usr/share/applications/voxcell.desktop"
cp "$ICON_SRC" "$DEB_ROOT/usr/share/icons/hicolor/scalable/apps/voxcell.$ICON_EXT"

# Wrapper keeps exe-relative asset resolution working (/opt/voxcell/assets).
cat > "$DEB_ROOT/usr/bin/voxcell" <<'EOF'
#!/bin/sh
exec /opt/voxcell/voxcell "$@"
EOF

INSTALLED_SIZE="$(du -skc "$DEB_ROOT/opt" "$DEB_ROOT/usr" | awk 'END { print $1 }')"
cat > "$DEB_ROOT/DEBIAN/control" <<EOF
Package: voxcell
Version: $VERSION
Section: utils
Priority: optional
Architecture: $DEB_ARCH
Maintainer: Voxcell Desktop <voxcell@localhost>
Installed-Size: $INSTALLED_SIZE
Depends: libvulkan1, libegl1, libgl1, libx11-6, libxcb1, libxkbcommon0, libwayland-client0, libfontconfig1
Description: Voxcell desktop application
 RUNE desktop client built with the voxcell Rust workspace.
 Bundles the voxcell binary plus fonts and character art under /opt/voxcell.
EOF

cat > "$DEB_ROOT/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
chmod 755 /opt/voxcell/voxcell
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q /usr/share/icons/hicolor || true
fi
exit 0
EOF

# Normalize permissions/ownership for the package payload.
find "$DEB_ROOT" -type d -exec chmod 755 {} +
find "$DEB_ROOT" -type f -exec chmod 644 {} +
chmod 755 "$DEB_ROOT/opt/voxcell/voxcell" "$DEB_ROOT/usr/bin/voxcell" \
          "$DEB_ROOT/DEBIAN/postinst"

build_deb_manual() {
    # Assemble a .deb without dpkg-deb: ar archive of
    # debian-binary ("2.0\n") + control.tar.gz (./control, ./postinst, ...)
    # + data.tar.gz (./opt/..., ./usr/...). Member order matters.
    local work deb_tmp
    work="$(mktemp -d)"
    deb_tmp="$work/voxcell.deb"
    printf '2.0\n' > "$work/debian-binary"
    tar -czf "$work/control.tar.gz" --owner=0 --group=0 --numeric-owner \
        -C "$DEB_ROOT/DEBIAN" .
    tar -czf "$work/data.tar.gz" --owner=0 --group=0 --numeric-owner \
        --exclude='./DEBIAN' -C "$DEB_ROOT" .
    if command -v ar >/dev/null 2>&1; then
        step 'dpkg-deb missing — assembling .deb with ar'
        (cd "$work" && ar rc "$deb_tmp" debian-binary control.tar.gz data.tar.gz)
    else
        step 'dpkg-deb and ar missing — assembling .deb with pure-shell ar writer'
        printf '!<arch>\n' > "$deb_tmp"
        local m size
        for m in debian-binary control.tar.gz data.tar.gz; do
            size="$(wc -c < "$work/$m" | tr -d '[:space:]')"
            # 60-byte ar member header: name/ mtime uid gid mode size `magic
            printf '%-16s%-12s%-6s%-6s%-8s%-10s`\n' \
                "$m/" 0 0 0 100644 "$size" >> "$deb_tmp"
            cat "$work/$m" >> "$deb_tmp"
            if [ $((size % 2)) -ne 0 ]; then printf '\n' >> "$deb_tmp"; fi
        done
        # Self-check: expected total size = 8-byte global header + members.
        local expected=8
        for m in debian-binary control.tar.gz data.tar.gz; do
            size="$(wc -c < "$work/$m" | tr -d '[:space:]')"
            expected=$((expected + 60 + size + size % 2))
        done
        actual="$(wc -c < "$deb_tmp" | tr -d '[:space:]')"
        if [ "$expected" -ne "$actual" ]; then
            rm -rf "$work"
            fail "manual .deb size mismatch (expected $expected, got $actual)"
        fi
    fi
    mv "$deb_tmp" "$DEB_PATH"
    rm -rf "$work"
}

if command -v dpkg-deb >/dev/null 2>&1; then
    step "Creating $DEB_PATH (dpkg-deb)"
    dpkg-deb --root-owner-group --build "$DEB_ROOT" "$DEB_PATH"
elif command -v tar >/dev/null 2>&1; then
    build_deb_manual
else
    gap 'dpkg-deb and tar missing — cannot build .deb'
    note 'Ready-to-run command once dpkg-deb is available:'
    note "  dpkg-deb --root-owner-group --build '$DEB_ROOT' '$DEB_PATH'"
fi
if [ -f "$DEB_PATH" ] && command -v dpkg-deb >/dev/null 2>&1; then
    dpkg-deb --info "$DEB_PATH" >/dev/null || fail "dpkg-deb cannot read back $DEB_PATH"
fi

# --- 5. SHA256SUMS ---------------------------------------------------------------------
step 'Writing SHA256SUMS'
(
    cd "$DIST_DIR"
    : > SHA256SUMS
    for f in "$APPIMAGE_NAME" "$DEB_NAME" "$APPIMAGE_NAME.zsync"; do
        if [ -f "$f" ]; then
            sha256sum "$f" >> SHA256SUMS
        fi
    done
    if [ ! -s SHA256SUMS ]; then
        fail 'No artifacts to hash — neither AppImage nor .deb was produced'
    fi
)

# --- 6. Report ----------------------------------------------------------------------------
step 'Artifacts:'
ARTIFACTS=()
[ -f "$APPIMAGE_PATH" ] && ARTIFACTS+=("$APPIMAGE_PATH")
[ -f "$DEB_PATH" ] && ARTIFACTS+=("$DEB_PATH")
ARTIFACTS+=("$DIST_DIR/SHA256SUMS")
for f in "${ARTIFACTS[@]}"; do
    SIZE="$(wc -c < "$f" | tr -d '[:space:]')"
    printf '    %s  (%s bytes)\n' "$f" "$SIZE"
done
if [ ${#GAPS[@]} -gt 0 ]; then
    step 'Gaps (tooling missing on this machine):'
    for g in "${GAPS[@]}"; do note "$g"; done
fi
# Primary artifact: AppImage when produced, else the .deb (contract: exit 0
# only if the primary artifact exists).
if [ -f "$APPIMAGE_PATH" ]; then
    PRIMARY="$APPIMAGE_PATH"
elif [ -f "$DEB_PATH" ]; then
    PRIMARY="$DEB_PATH"
    note 'Primary artifact fell back to the .deb (AppImage tooling gap above)'
else
    fail "Neither primary artifact exists: $APPIMAGE_PATH / $DEB_PATH"
fi
step "OK — primary artifact: $PRIMARY"
exit 0
