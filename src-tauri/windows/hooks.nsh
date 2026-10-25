; Versailles' NSIS hooks (bundle.windows.nsis.installerHooks in tauri.conf.json).

; Uninstalling while Versailles hides the Windows taskbar: the uninstaller
; stops every versailles.exe, the taskbar guard included, so nothing would
; bring the taskbar back. Stop them here, before the files go, and run the
; app's own restore (it puts back the saved auto-hide state, and does nothing
; when the taskbar was not hidden). Not on an update: the updater has already
; handed the taskbar back, and the new version takes it again.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    nsExec::Exec 'taskkill /F /IM "${MAINBINARYNAME}.exe"'
    Pop $0
    Sleep 1500
    nsExec::Exec '"$INSTDIR\${MAINBINARYNAME}.exe" --restore-taskbar'
    Pop $0
    ; The logon fallback the app sets while it hides the taskbar.
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\RunOnce" "VersaillesRestoreTaskbar"
  ${EndIf}
!macroend
