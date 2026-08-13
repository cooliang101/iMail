!macro NSIS_HOOK_POSTINSTALL
  ; Windows desktop toasts require a Start Menu shortcut carrying the bundle
  ; AppUserModelId. Keep this identity even when an update/no-shortcut install
  ; would otherwise skip Tauri's normal shortcut creation.
  IfFileExists "$SMPROGRAMS\${PRODUCTNAME}.lnk" notification_identity_ready
  CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  notification_identity_ready:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing retained legacy service files (mail data and migration snapshots are preserved)..."
  ExecWait '"$INSTDIR\imail.exe" --imail-uninstall-cleanup' $0
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "iMail could not finish legacy service cleanup. The uninstall has been cancelled so mail data is not left in an unsafe state. Close iMail and try again." /SD IDOK
    Abort
  ${EndIf}
!macroend
