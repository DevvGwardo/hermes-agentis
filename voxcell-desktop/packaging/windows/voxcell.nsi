; =============================================================================
; Voxcell Desktop — NSIS installer source (P4-05)
; Ready-to-run. Normally invoked by build.ps1, which passes:
;   makensis /DVERSION=<v> /DSTAGEDIR=<abs stage dir> /DOUTFILE=<abs out.exe> voxcell.nsi
; Manual run (defaults below assume repo layout, run from packaging/windows/):
;   makensis /DVERSION=0.1.0 voxcell.nsi
; Requires: NSIS 3.x (choco install nsis -y  OR  scoop install nsis)
; Signing: build.ps1 signs the produced setup.exe with signtool when
; VOXCELL_SIGN_CERT + VOXCELL_SIGN_PASSWORD are set (never faked).
; =============================================================================

!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!ifndef STAGEDIR
  ; default: dist/windows/stage/Voxcell-<version>-x64 relative to this script
  !define STAGEDIR "..\..\dist\windows\stage\Voxcell-${VERSION}-x64"
!endif
!ifndef OUTFILE
  !define OUTFILE "..\..\dist\windows\Voxcell-${VERSION}-x64-setup.exe"
!endif

Unicode true
SetCompressor /SOLID lzma

!include "MUI2.nsh"

!define APPNAME "Voxcell"
!define PUBLISHER "Voxcell"
!define REGKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"

Name "${APPNAME} ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\${APPNAME}"
InstallDirRegKey HKLM "${REGKEY}" "InstallLocation"
RequestExecutionLevel admin
ShowInstDetails show
ShowUnInstDetails show

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  ; staged layout: voxcell.exe + assets/** (fonts + character art)
  File "${STAGEDIR}\voxcell.exe"
  File /r "${STAGEDIR}\assets"

  ; uninstaller + Add/Remove Programs entry
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "${REGKEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKLM "${REGKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${REGKEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKLM "${REGKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${REGKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
  WriteRegDWORD HKLM "${REGKEY}" "NoModify" 1
  WriteRegDWORD HKLM "${REGKEY}" "NoRepair" 1

  ; shortcuts
  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\voxcell.exe"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\Uninstall.lnk" "$INSTDIR\uninstall.exe"
  CreateShortcut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\voxcell.exe"
SectionEnd

Section "Uninstall"
  Delete "$DESKTOP\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  Delete "$SMPROGRAMS\${APPNAME}\Uninstall.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  RMDir /r "$INSTDIR\assets"
  Delete "$INSTDIR\voxcell.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "${REGKEY}"
SectionEnd
