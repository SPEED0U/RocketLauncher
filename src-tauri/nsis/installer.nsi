; Tauri NSIS One-Click Installer
; Installation automatique style Discord/Slack

!include MUI2.nsh
!include FileFunc.nsh
!include x64.nsh

; ===== Configuration =====
Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel admin

; Remove branding
BrandingText " "

; ===== UI Configuration - One Click Mode =====
; No welcome page, no directory selection, just install!
!define MUI_CUSTOMFUNCTION_GUIINIT onGUIInit
!define MUI_INSTFILESPAGE_COLORS "FFFFFF 09090b"
!define MUI_INSTFILESPAGE_PROGRESSBAR "smooth"

; Finish page - option to launch app
!define MUI_FINISHPAGE_NOAUTOCLOSE
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_NOTCHECKED
!define MUI_FINISHPAGE_RUN_TEXT "Launch RocketLauncher"
!define MUI_FINISHPAGE_RUN_FUNCTION "LaunchApp"

; ===== Pages - Only progress and finish =====
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

; ===== Language =====
!insertmacro MUI_LANGUAGE "English"

; ===== Variables =====
Var /GLOBAL INSTALL_DIR

; ===== Init Function - Auto-start installation =====
Function onGUIInit
  ; Set installation directory
  StrCpy $INSTALL_DIR "$PROGRAMFILES64\RocketLauncher"
  StrCpy $INSTDIR $INSTALL_DIR
  
  ; Start installation automatically
  ; The installer will show only the progress bar
FunctionEnd

; ===== Main Installation Section =====
Section "Install" SecInstall
  SectionIn RO
  
  SetOutPath "$INSTDIR"
  
  ; Extract application files
  File /r "${__TAURI_BUNDLE_RESOURCES__}\*"
  
  ; Write uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"
  
  ; Create desktop shortcut
  CreateShortcut "$DESKTOP\RocketLauncher.lnk" "$INSTDIR\${__TAURI_MAIN_BINARY_NAME__}.exe"
  
  ; Create start menu shortcut
  CreateDirectory "$SMPROGRAMS\RocketLauncher"
  CreateShortcut "$SMPROGRAMS\RocketLauncher\RocketLauncher.lnk" "$INSTDIR\${__TAURI_MAIN_BINARY_NAME__}.exe"
  CreateShortcut "$SMPROGRAMS\RocketLauncher\Uninstall.lnk" "$INSTDIR\uninstall.exe"
  
  ; Write registry for Add/Remove Programs
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RocketLauncher" "DisplayName" "RocketLauncher"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RocketLauncher" "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RocketLauncher" "DisplayIcon" "$INSTDIR\${__TAURI_MAIN_BINARY_NAME__}.exe,0"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RocketLauncher" "Publisher" "speedou"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RocketLauncher" "DisplayVersion" "${__TAURI_BUNDLE_VERSION__}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RocketLauncher" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RocketLauncher" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RocketLauncher" "NoRepair" 1
  
  ; Calculate and write estimated size
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RocketLauncher" "EstimatedSize" "$0"
SectionEnd

; ===== Launch App Function =====
Function LaunchApp
  Exec '"$INSTDIR\${__TAURI_MAIN_BINARY_NAME__}.exe"'
FunctionEnd

; Auto-restart the launcher after silent update installation.
; Interactive installs keep the existing finish-page behavior.
Function .onInstSuccess
  IfSilent 0 done
  Exec '"$INSTDIR\${__TAURI_MAIN_BINARY_NAME__}.exe"'
done:
FunctionEnd

; ===== Uninstaller Section =====
Section "Uninstall"
  ; Kill running process
  ExecWait 'taskkill /F /IM "${__TAURI_MAIN_BINARY_NAME__}.exe" /T'
  Sleep 500
  
  ; Remove files and directories
  RMDir /r "$INSTDIR"
  
  ; Remove shortcuts
  Delete "$DESKTOP\RocketLauncher.lnk"
  RMDir /r "$SMPROGRAMS\RocketLauncher"
  
  ; Remove registry keys
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RocketLauncher"
SectionEnd
