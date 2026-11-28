Var AppNameExe
Var AppNameDesktopLink

!macro NSIS_HOOK_POSTINSTALL
  StrCpy $AppNameExe "$INSTDIR\${MAINBINARYNAME}.exe"
  StrCpy $AppNameDesktopLink "$DESKTOP\${PRODUCTNAME}.lnk"
  System::Call 'shlwapi::SHLoadIndirectString(w "@$AppNameExe,-1001", w .r0, i ${NSIS_MAX_STRLEN}, p 0) i.r1'
  ${If} $1 = 0
    WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "$0"
  ${EndIf}
  !insertmacro LocalizeAppShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !insertmacro LocalizeAppShortcut "$AppNameDesktopLink"
!macroend

!macro LocalizeAppShortcut path
  !insertmacro IsShortcutTarget "${path}" "$AppNameExe"
  Pop $0
  ${If} $0 = 1
    System::Call 'shell32::SHSetLocalizedName(w "${path}", w "$AppNameExe", i 1001)'
  ${EndIf}
!macroend

; The optional desktop shortcut is created on the finish page.
Function .onGUIEnd
  ${If} $AppNameExe != ""
    !insertmacro LocalizeAppShortcut "$AppNameDesktopLink"
  ${EndIf}
FunctionEnd
