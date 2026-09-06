; Compile-time Modern UI settings only. Tauri owns installation and updates.
!define MUI_BGCOLOR "070D1F"
!define MUI_TEXTCOLOR "DFE4FE"
!define MUI_INSTFILESPAGE_COLORS "DFE4FE 070D1F"
!define MUI_HEADERIMAGE_RIGHT
!define MUI_WELCOMEFINISHPAGE_BITMAP_NOSTRETCH
!define MUI_HEADERIMAGE_BITMAP_NOSTRETCH

; Versions through 26.9.5 used "ZAPRET" as PRODUCTNAME. Tauri derives the
; install directory and uninstall registry key from that value, so changing it
; to "ZAPRET UI" would otherwise create a second installation with an empty
; sibling binaries directory. Prefer the legacy location when it still contains
; the application, preserving the downloaded core, user lists and old shortcut.
!macro NSIS_HOOK_PREINSTALL
  Push $R0
  Push $R1

  StrCpy $R0 ""
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZAPRET" "InstallLocation"
  ${If} $R0 != ""
    nsis_tauri_utils::StrReplace "$R0" "$\"" ""
    Pop $R0
    ${If} ${FileExists} "$R0\${MAINBINARYNAME}.exe"
      SetShellVarContext current
    ${Else}
      StrCpy $R0 ""
    ${EndIf}
  ${EndIf}

  ${If} $R0 == ""
    ReadRegStr $R0 HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZAPRET" "InstallLocation"
    ${If} $R0 != ""
      nsis_tauri_utils::StrReplace "$R0" "$\"" ""
      Pop $R0
      ${If} ${FileExists} "$R0\${MAINBINARYNAME}.exe"
        SetShellVarContext all
      ${Else}
        StrCpy $R0 ""
      ${EndIf}
    ${EndIf}
  ${EndIf}

  ${If} $R0 != ""
    StrCpy $INSTDIR "$R0"
    SetOutPath "$INSTDIR"
  ${EndIf}

  Pop $R1
  Pop $R0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Push $R0

  ; Preserve the user's existing shortcut, but migrate its visible name.
  ${If} ${FileExists} "$DESKTOP\ZAPRET.lnk"
    Delete "$DESKTOP\ZAPRET.lnk"
    CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  ${EndIf}
  ${If} ${FileExists} "$SMPROGRAMS\ZAPRET.lnk"
    Delete "$SMPROGRAMS\ZAPRET.lnk"
    CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  ${EndIf}

  ; The new PRODUCTNAME entry now owns the migrated installation.
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZAPRET"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZAPRET"
  DeleteRegKey HKCU "Software\${MANUFACTURER}\ZAPRET"
  DeleteRegKey HKLM "Software\${MANUFACTURER}\ZAPRET"

  Pop $R0
!macroend
