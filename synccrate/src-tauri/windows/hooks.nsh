; NSIS installer hooks (bundle.windows.nsis.installerHooks in tauri.conf.json).

; Tauri's "Delete the application data" checkbox only removes
; %APPDATA%\com.synccrate.app and %LOCALAPPDATA%\com.synccrate.app (the bundle
; identifier), but SyncCrate keeps everything (profiles, backups, file history,
; crews, the network key, settings) in %APPDATA%\synccrate, so it survived an
; uninstall (GitHub issue #1). Same condition as Tauri's own removal: only when
; the box is ticked, and never during an update (UpdateMode = 1).
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "$APPDATA\synccrate"
    ; Pre-rename versions of the app stored the same data here.
    RmDir /r "$APPDATA\simshare"
  ${EndIf}
!macroend
