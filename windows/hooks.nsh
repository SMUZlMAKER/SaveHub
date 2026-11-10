; The marker preserves the user's choice when upgrading or reinstalling.
!macro NSIS_HOOK_POSTINSTALL
  ReadRegDWORD $0 HKCU "Software\SaveHub" "AutostartInitialized"
  ${If} $0 != 1
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SaveHub" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --autostart'
    WriteRegDWORD HKCU "Software\SaveHub" "AutostartInitialized" 1
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Keep the preference during updater-driven uninstall.
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SaveHub"
    DeleteRegKey HKCU "Software\SaveHub"
  ${EndIf}
!macroend
