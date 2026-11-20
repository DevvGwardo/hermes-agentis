# =============================================================================
# Voxcell Desktop — Windows packaging script (P4-05, packaging/README.md)
#
# Usage (from repo root OR voxcell-desktop/, cwd-independent):
#   powershell -NoProfile -ExecutionPolicy Bypass -File packaging/windows/build.ps1
#   powershell ... -File voxcell-desktop/packaging/windows/build.ps1
#
# Steps:
#   1. cargo build --release -p voxcell-app   (binary: target/release/voxcell.exe)
#   2. Stage binary + voxcell-desktop/assets/** into dist/windows/stage/
#   3. ALWAYS produce dist/windows/Voxcell-<version>-x64.zip (portable fallback)
#   4. If NSIS (makensis) or WiX (candle/light or cargo-wix) is available, also
#      produce Voxcell-<version>-x64-setup.exe / Voxcell-<version>-x64.msi.
#      Ready-to-run installer sources live next to this script:
#        packaging/windows/voxcell.nsi   (NSIS:  makensis /DVERSION=... voxcell.nsi)
#        packaging/windows/voxcell.wxs   (WiX:   candle+light, or cargo wix)
#
# Version: $env:VOXCELL_VERSION overrides; default = workspace Cargo.toml
#          [workspace.package] version.
#
# Signing hook (NO certificates exist in this environment — unsigned output is
# the expected result; we NEVER fake a signature):
#   Set BOTH env vars to activate:
#     VOXCELL_SIGN_CERT     = path to a .pfx code-signing certificate
#     VOXCELL_SIGN_PASSWORD = password for that .pfx
#   Then every produced .exe (app binary + installer) is signed with:
#     signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /f <pfx> /p <pw>
#   signtool is located on PATH or under the Windows 10/11 SDK bin dirs.
#   When the env vars are absent the hook prints a notice and skips gracefully.
#
# Switches:
#   -SkipBuild   reuse an existing target/release/voxcell.exe (no cargo build)
#   -NoInstaller skip installer tooling detection/attempt (zip only)
# =============================================================================
[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$NoInstaller
)

$ErrorActionPreference = 'Stop'

function Write-Step($msg)  { Write-Host "==> $msg" -ForegroundColor Cyan }
function Write-Note($msg)  { Write-Host "    $msg" -ForegroundColor DarkGray }
function Fail($msg)        { Write-Host "ERROR: $msg" -ForegroundColor Red; exit 1 }

# --- Locate workspace root (cwd-independent: script lives at packaging/windows/) ---
$WorkspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not (Test-Path (Join-Path $WorkspaceRoot 'Cargo.toml'))) {
    # Fallback: maybe invoked from a copy elsewhere; try cwd and cwd/voxcell-desktop
    foreach ($cand in @((Get-Location).Path, (Join-Path (Get-Location).Path 'voxcell-desktop'))) {
        if (Test-Path (Join-Path $cand 'Cargo.toml')) { $WorkspaceRoot = $cand; break }
    }
}
$CargoToml = Join-Path $WorkspaceRoot 'Cargo.toml'
if (-not (Test-Path $CargoToml)) { Fail "Cannot locate workspace Cargo.toml (looked in $WorkspaceRoot)" }
Write-Step "Workspace: $WorkspaceRoot"

# --- Version: env override, else [workspace.package] version ---
$Version = $env:VOXCELL_VERSION
if (-not $Version) {
    $inPkg = $false
    foreach ($line in Get-Content $CargoToml) {
        if ($line -match '^\s*\[workspace\.package\]') { $inPkg = $true; continue }
        if ($line -match '^\s*\[') { $inPkg = $false }
        if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
    }
}
if (-not $Version) { Fail 'Could not determine version (set VOXCELL_VERSION or fix Cargo.toml)' }
Write-Step "Version: $Version"

$DistDir   = Join-Path $WorkspaceRoot 'dist\windows'
$StageRoot = Join-Path $DistDir 'stage'
$StageName = "Voxcell-$Version-x64"
$StageDir  = Join-Path $StageRoot $StageName
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

# --- 1. Build ---
$ExePath = Join-Path $WorkspaceRoot 'target\release\voxcell.exe'
if ($SkipBuild) {
    if (-not (Test-Path $ExePath)) { Fail "-SkipBuild given but $ExePath does not exist" }
    Write-Step "Skipping cargo build (reusing $ExePath)"
} else {
    Write-Step 'cargo build --release -p voxcell-app'
    Push-Location $WorkspaceRoot
    try {
        & cargo build --release -p voxcell-app
        if ($LASTEXITCODE -ne 0) { Fail "cargo build failed (exit $LASTEXITCODE)" }
    } finally { Pop-Location }
}
if (-not (Test-Path $ExePath)) { Fail "Build reported success but $ExePath is missing" }

# --- Signing hook (gated on VOXCELL_SIGN_CERT + VOXCELL_SIGN_PASSWORD) ---
$script:SignTool = $null
function Find-SignTool {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $kits = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
    if (Test-Path $kits) {
        $hit = Get-ChildItem -Path $kits -Recurse -Filter 'signtool.exe' -ErrorAction SilentlyContinue |
               Where-Object { $_.FullName -match 'x64' } |
               Sort-Object FullName -Descending | Select-Object -First 1
        if ($hit) { return $hit.FullName }
    }
    return $null
}
function Invoke-Sign([string]$Path) {
    if (-not $env:VOXCELL_SIGN_CERT -or -not $env:VOXCELL_SIGN_PASSWORD) {
        Write-Note "Signing skipped for $(Split-Path $Path -Leaf) (VOXCELL_SIGN_CERT/VOXCELL_SIGN_PASSWORD not set)"
        return
    }
    if (-not (Test-Path $env:VOXCELL_SIGN_CERT)) { Fail "VOXCELL_SIGN_CERT points to missing file: $env:VOXCELL_SIGN_CERT" }
    if (-not $script:SignTool) { $script:SignTool = Find-SignTool }
    if (-not $script:SignTool) { Fail 'Signing requested but signtool.exe not found (install Windows SDK)' }
    Write-Step "Signing $Path"
    & $script:SignTool sign /fd SHA256 /tr 'http://timestamp.digicert.com' /td SHA256 `
        /f $env:VOXCELL_SIGN_CERT /p $env:VOXCELL_SIGN_PASSWORD $Path
    if ($LASTEXITCODE -ne 0) { Fail "signtool failed (exit $LASTEXITCODE)" }
}

# Sign the app binary in place BEFORE staging so zip + installer carry the signed exe.
Invoke-Sign $ExePath

# --- 2. Stage binary + assets ---
Write-Step "Staging into $StageDir"
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
Copy-Item $ExePath (Join-Path $StageDir 'voxcell.exe')
$AssetsSrc = Join-Path $WorkspaceRoot 'assets'
if (Test-Path $AssetsSrc) {
    Copy-Item $AssetsSrc (Join-Path $StageDir 'assets') -Recurse
    $n = (Get-ChildItem (Join-Path $StageDir 'assets') -Recurse -File).Count
    Write-Note "Bundled $n asset files"
} else {
    Write-Note 'WARNING: assets/ directory not found — shipping binary only'
}

# --- 3. ALWAYS produce the portable zip ---
$ZipPath = Join-Path $DistDir "$StageName.zip"
Write-Step "Creating $ZipPath"
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
# Prefer Windows-native bsdtar (System32). A PATH-inherited MSYS/git tar
# misparses "C:..." as a remote host ("Cannot connect to C: resolve failed").
$tarExe = Join-Path $env:SystemRoot 'System32\tar.exe'
if (-not (Test-Path $tarExe)) { $tarExe = $null }
if ($tarExe) {
    # bsdtar (ships with Windows 10+): proper forward-slash zip entries
    Push-Location $StageRoot
    try { & $tarExe -a -c -f $ZipPath $StageName; if ($LASTEXITCODE -ne 0) { Fail "tar zip failed (exit $LASTEXITCODE)" } }
    finally { Pop-Location }
} else {
    Compress-Archive -Path $StageDir -DestinationPath $ZipPath
}
if (-not (Test-Path $ZipPath)) { Fail "zip was not produced: $ZipPath" }

# --- 4. Installer (NSIS preferred-light, WiX fallback) ---
$InstallerPath = $null
$InstallerKind = $null
if (-not $NoInstaller) {
    $makensis = Get-Command makensis.exe -ErrorAction SilentlyContinue
    if (-not $makensis) {
        foreach ($p in @("$env:ProgramFiles\NSIS\makensis.exe", "${env:ProgramFiles(x86)}\NSIS\makensis.exe")) {
            if (Test-Path $p) { $makensis = @{ Source = $p }; break }
        }
    }
    $candle = Get-Command candle.exe -ErrorAction SilentlyContinue
    $light  = Get-Command light.exe  -ErrorAction SilentlyContinue
    $cargoWix = $null
    if (-not $candle) {
        $cw = Get-Command cargo-wix.exe -ErrorAction SilentlyContinue
        if ($cw) { $cargoWix = $cw.Source }
    }

    if ($makensis) {
        $InstallerKind = 'nsis'
        $InstallerPath = Join-Path $DistDir "$StageName-setup.exe"
        $nsi = Join-Path $PSScriptRoot 'voxcell.nsi'
        Write-Step "Building NSIS installer: $InstallerPath"
        & $makensis.Source /NOLOGO /V2 "/DVERSION=$Version" "/DSTAGEDIR=$StageDir" "/DOUTFILE=$InstallerPath" $nsi
        if ($LASTEXITCODE -ne 0) { Fail "makensis failed (exit $LASTEXITCODE)" }
    } elseif ($candle -and $light) {
        $InstallerKind = 'wix'
        $InstallerPath = Join-Path $DistDir "$StageName.msi"
        $wxs = Join-Path $PSScriptRoot 'voxcell.wxs'
        $obj = Join-Path $DistDir 'voxcell.wixobj'
        Write-Step "Building WiX installer: $InstallerPath"
        & $candle.Source -nologo "-dVersion=$Version" "-dStageDir=$StageDir" -out $obj $wxs
        if ($LASTEXITCODE -ne 0) { Fail "candle failed (exit $LASTEXITCODE)" }
        & $light.Source -nologo -out $InstallerPath $obj
        if ($LASTEXITCODE -ne 0) { Fail "light failed (exit $LASTEXITCODE)" }
        Remove-Item -Force $obj -ErrorAction SilentlyContinue
    } elseif ($cargoWix) {
        $InstallerKind = 'cargo-wix'
        $InstallerPath = Join-Path $DistDir "$StageName.msi"
        Write-Step "Building installer via cargo wix"
        Push-Location $WorkspaceRoot
        try {
            & cargo wix --no-build --nocapture -p voxcell-app --output $InstallerPath `
                --install-version $Version
            if ($LASTEXITCODE -ne 0) { Fail "cargo wix failed (exit $LASTEXITCODE)" }
        } finally { Pop-Location }
    } else {
        Write-Note 'No installer tooling (NSIS/WiX/cargo-wix) found.'
        Write-Note "Zip is the deliverable; ready-to-run sources: packaging/windows/voxcell.nsi + voxcell.wxs"
        Write-Note 'Install with: choco install nsis -y   (or: choco install wixtoolset -y)'
    }
    if ($InstallerPath) {
        if (-not (Test-Path $InstallerPath)) { Fail "$InstallerKind reported success but installer missing: $InstallerPath" }
        Invoke-Sign $InstallerPath
    }
}

# --- 5. Report ---
Write-Step 'Artifacts:'
$artifacts = @($ZipPath)
if ($InstallerPath) { $artifacts += $InstallerPath }
foreach ($a in $artifacts) {
    $size = (Get-Item $a).Length
    Write-Host ("    {0}  ({1:N0} bytes, {2:N2} MiB)" -f $a, $size, ($size / 1MB))
}
# Primary artifact = installer when tooling produced one, else the portable zip.
$primary = if ($InstallerPath) { $InstallerPath } else { $ZipPath }
if (-not (Test-Path $primary)) { Fail "Primary artifact missing: $primary" }
Write-Step "OK — primary artifact: $primary"
exit 0
