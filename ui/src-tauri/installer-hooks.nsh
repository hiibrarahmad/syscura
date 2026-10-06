!include "FileFunc.nsh"

; Syscura installer hooks (Tauri NSIS bundler). The installer runs as
; administrator (per-machine install), so it can set up the service.
; Housekeeping commands run with nsExec::Exec (output hidden): on a fresh
; install there is nothing to stop, and their "not found" replies would
; only look like errors.

!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Checking for an older Syscura..."
  ; Updating: stop the running service so its files can be replaced.
  nsExec::Exec '"$SYSDIR\sc.exe" stop Syscura'
  Pop $0
  ${If} $0 == 0
    DetailPrint "Stopped the older Syscura service."
    Sleep 1500
  ${EndIf}
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM syscura-agent.exe'
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Background protection as a Windows service that starts with Windows.
  ; `install` replaces an older Syscura service (for example one from a
  ; copy in Downloads) so it runs this copy.
  DetailPrint "Starting Syscura's background protection..."
  nsExec::Exec '"$INSTDIR\syscura-agent.exe" install'
  Pop $0
  ${If} $0 == 0
    DetailPrint "Background protection is on and starts with Windows."
  ${Else}
    DetailPrint "Background protection could not start now. Open Syscura and press Start."
  ${EndIf}
  ; The tray icon starts with Windows for every user, quietly in the tray.
  SetShellVarContext all
  CreateShortCut "$SMSTARTUP\Syscura.lnk" "$INSTDIR\Syscura.exe" "--tray" "$INSTDIR\Syscura.exe" 0
  DetailPrint "Syscura will start in the taskbar tray after every restart."
  ; An update started from inside Syscura: open it again, as the signed-in
  ; user (not as administrator).
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/SYSCURA_RELAUNCH" $R1
  ${IfNot} ${Errors}
    nsis_tauri_utils::RunAsUser "$INSTDIR\Syscura.exe" ""
    Pop $R1
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing Syscura's background protection..."
  nsExec::Exec '"$INSTDIR\syscura-agent.exe" uninstall'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM syscura-agent.exe'
  Pop $0
  SetShellVarContext all
  Delete "$SMSTARTUP\Syscura.lnk"
!macroend
