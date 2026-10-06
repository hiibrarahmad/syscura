; Syscura installer hooks (Tauri NSIS bundler). The installer runs as
; administrator (per-machine install), so it can set up the service.

!macro NSIS_HOOK_PREINSTALL
  ; Updating: stop the running service so its files can be replaced.
  nsExec::ExecToLog '"$SYSDIR\sc.exe" stop Syscura'
  Sleep 1500
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /IM syscura-agent.exe'
  Sleep 500
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Background protection as a Windows service that starts with Windows.
  ; `install` replaces an older Syscura service (for example one from a
  ; copy in Downloads) so it runs this copy.
  DetailPrint "Starting Syscura's background protection..."
  nsExec::ExecToLog '"$INSTDIR\syscura-agent.exe" install'
  ; The tray icon starts with Windows for every user, quietly in the tray.
  SetShellVarContext all
  CreateShortCut "$SMSTARTUP\Syscura.lnk" "$INSTDIR\Syscura.exe" "--tray" "$INSTDIR\Syscura.exe" 0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing Syscura's background protection..."
  nsExec::ExecToLog '"$INSTDIR\syscura-agent.exe" uninstall'
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /IM syscura-agent.exe'
  SetShellVarContext all
  Delete "$SMSTARTUP\Syscura.lnk"
!macroend
