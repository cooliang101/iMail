!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Stopping and removing the iMail user service (mail data is preserved)..."
  ExecWait '"$SYSDIR\cmd.exe" /D /C ""$INSTDIR\imail-service-manager.exe" --imail-uninstall-cleanup"' $0
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "iMail could not stop the current-user background service. The uninstall has been cancelled so the service is not left in a broken state. Close iMail and try again." /SD IDOK
    Abort
  ${EndIf}
!macroend
