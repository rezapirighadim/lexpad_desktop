; "Open with Lexpad" for videos and subtitles, and a Default Apps entry;
; never the default player unless the learner picks it in Settings.
; Tauri's own fileAssociations would write each extension's default value
; (making Lexpad the handler where no user choice exists), so the installer
; only adds Lexpad to each type's OpenWithProgids and to the app's
; SupportedTypes: Lexpad shows in "Open with", and Windows never picks it
; by itself. The uninstaller removes exactly what was added.
; The Microsoft Store's MSIX declares the same types in its manifest
; (msix/AppxManifest.xml), which Windows treats the same way.
; Hooked in by tauri.conf.json, bundle.windows.nsis.installerHooks.
;
; It also adds the second Start menu entry, "Lexpad Player": the same
; Lexpad.exe started with --player, which opens straight into the Player
; (owner, 26 Oct 2026: one app, two ways in). The shortcut carries the
; Player's own AppUserModelID (Lexpad.Player, as its window does:
; player_window.rs, taskbar::own_button), so it groups with the Player's
; taskbar button and not with Lexpad's.

!define LEXPAD_PLAYER_LNK "Lexpad Player.lnk"
!define LEXPAD_PLAYER_AUMID "Lexpad.Player"

; The template's SetLnkAppUserModelId, with an id of our own.
!macro LexpadSetLnkAumid shortcut id
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ""
  ${If} $0 P<> 0
    ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}",.r1)'
    ${If} $1 P<> 0
      ${IPersistFile::Load} $1 '("${shortcut}", ${STGM_READWRITE})'
      ${IUnknown::QueryInterface} $0 '("${IID_IPropertyStore}",.r2)'
      ${If} $2 P<> 0
        System::Call 'Oleaut32::SysAllocString(w "${id}") i.r3'
        System::Call '*${SYSSTRUCT_PROPERTYKEY}(${PKEY_AppUserModel_ID})p.r4'
        System::Call '*${SYSSTRUCT_PROPVARIANT}(${VT_BSTR},,&i4 $3)p.r5'
        ${IPropertyStore::SetValue} $2 '($4,$5)'
        System::Call 'Oleaut32::SysFreeString($3)'
        System::Free $4
        System::Free $5
        ${IPropertyStore::Commit} $2 ""
        ${IUnknown::Release} $2 ""
        ${IPersistFile::Save} $1 '("${shortcut}",1)'
      ${EndIf}
      ${IUnknown::Release} $1 ""
    ${EndIf}
    ${IUnknown::Release} $0 ""
  ${EndIf}
!macroend

!define LEXPAD_PROGID "Lexpad.Media"
!macro LexpadOpenWith EXT
  WriteRegStr SHCTX "Software\Classes\.${EXT}\OpenWithProgids" "${LEXPAD_PROGID}" ""
  WriteRegStr SHCTX "Software\Lexpad\Capabilities\FileAssociations" ".${EXT}" "${LEXPAD_PROGID}"
  WriteRegStr SHCTX "Software\Classes\Applications\${MAINBINARYNAME}.exe\SupportedTypes" ".${EXT}" ""
!macroend

!macro LexpadOpenWithRemove EXT
  DeleteRegValue SHCTX "Software\Classes\.${EXT}\OpenWithProgids" "${LEXPAD_PROGID}"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "Software\Classes\${LEXPAD_PROGID}" "" "Video (Lexpad)"
  WriteRegStr SHCTX "Software\Classes\${LEXPAD_PROGID}\DefaultIcon" "" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr SHCTX "Software\Classes\${LEXPAD_PROGID}\shell\open\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" "%1"'
  WriteRegStr SHCTX "Software\Classes\Applications\${MAINBINARYNAME}.exe" "FriendlyAppName" "Lexpad"
  ; Default Apps: Lexpad gets a page in Settings > Default apps
  ; (ms-settings:defaultapps?registeredAppUser=Lexpad), where the learner may
  ; choose it per type. Registering sets nothing by itself.
  WriteRegStr SHCTX "Software\Lexpad\Capabilities" "ApplicationName" "Lexpad"
  WriteRegStr SHCTX "Software\Lexpad\Capabilities" "ApplicationDescription" "Watch videos with subtitles and save the words you meet."
  WriteRegStr SHCTX "Software\RegisteredApplications" "Lexpad" "Software\Lexpad\Capabilities"
  WriteRegStr SHCTX "Software\Classes\Applications\${MAINBINARYNAME}.exe\shell\open\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" "%1"'
  !insertmacro LexpadOpenWith "mp4"
  !insertmacro LexpadOpenWith "m4v"
  !insertmacro LexpadOpenWith "mov"
  !insertmacro LexpadOpenWith "mkv"
  !insertmacro LexpadOpenWith "webm"
  !insertmacro LexpadOpenWith "avi"
  !insertmacro LexpadOpenWith "wmv"
  !insertmacro LexpadOpenWith "asf"
  !insertmacro LexpadOpenWith "flv"
  !insertmacro LexpadOpenWith "f4v"
  !insertmacro LexpadOpenWith "ts"
  !insertmacro LexpadOpenWith "m2ts"
  !insertmacro LexpadOpenWith "mts"
  !insertmacro LexpadOpenWith "mpg"
  !insertmacro LexpadOpenWith "mpeg"
  !insertmacro LexpadOpenWith "vob"
  !insertmacro LexpadOpenWith "ogv"
  !insertmacro LexpadOpenWith "3gp"
  !insertmacro LexpadOpenWith "3g2"
  !insertmacro LexpadOpenWith "divx"
  !insertmacro LexpadOpenWith "rm"
  !insertmacro LexpadOpenWith "rmvb"
  !insertmacro LexpadOpenWith "mxf"
  !insertmacro LexpadOpenWith "srt"
  !insertmacro LexpadOpenWith "vtt"
  !insertmacro LexpadOpenWith "webvtt"
  !insertmacro LexpadOpenWith "ass"
  !insertmacro LexpadOpenWith "ssa"
  ; "Lexpad Player" in the Start menu, beside "Lexpad" (not with /NS).
  ${If} $NoShortcutMode <> 1
    CreateShortcut "$SMPROGRAMS\${LEXPAD_PLAYER_LNK}" "$INSTDIR\${MAINBINARYNAME}.exe" "--player" "$INSTDIR\${MAINBINARYNAME}.exe" 0
    !insertmacro LexpadSetLnkAumid "$SMPROGRAMS\${LEXPAD_PLAYER_LNK}" "${LEXPAD_PLAYER_AUMID}"
  ${EndIf}
  ; SHCNE_ASSOCCHANGED: Explorer reads the new "Open with" entries now.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro LexpadOpenWithRemove "mp4"
  !insertmacro LexpadOpenWithRemove "m4v"
  !insertmacro LexpadOpenWithRemove "mov"
  !insertmacro LexpadOpenWithRemove "mkv"
  !insertmacro LexpadOpenWithRemove "webm"
  !insertmacro LexpadOpenWithRemove "avi"
  !insertmacro LexpadOpenWithRemove "wmv"
  !insertmacro LexpadOpenWithRemove "asf"
  !insertmacro LexpadOpenWithRemove "flv"
  !insertmacro LexpadOpenWithRemove "f4v"
  !insertmacro LexpadOpenWithRemove "ts"
  !insertmacro LexpadOpenWithRemove "m2ts"
  !insertmacro LexpadOpenWithRemove "mts"
  !insertmacro LexpadOpenWithRemove "mpg"
  !insertmacro LexpadOpenWithRemove "mpeg"
  !insertmacro LexpadOpenWithRemove "vob"
  !insertmacro LexpadOpenWithRemove "ogv"
  !insertmacro LexpadOpenWithRemove "3gp"
  !insertmacro LexpadOpenWithRemove "3g2"
  !insertmacro LexpadOpenWithRemove "divx"
  !insertmacro LexpadOpenWithRemove "rm"
  !insertmacro LexpadOpenWithRemove "rmvb"
  !insertmacro LexpadOpenWithRemove "mxf"
  !insertmacro LexpadOpenWithRemove "srt"
  !insertmacro LexpadOpenWithRemove "vtt"
  !insertmacro LexpadOpenWithRemove "webvtt"
  !insertmacro LexpadOpenWithRemove "ass"
  !insertmacro LexpadOpenWithRemove "ssa"
  DeleteRegKey SHCTX "Software\Classes\${LEXPAD_PROGID}"
  DeleteRegValue SHCTX "Software\RegisteredApplications" "Lexpad"
  DeleteRegKey SHCTX "Software\Lexpad\Capabilities"
  DeleteRegKey SHCTX "Software\Classes\Applications\${MAINBINARYNAME}.exe"
  ; The Player's Start menu entry, unless this is an update (it comes back).
  ${If} $UpdateMode <> 1
    Delete "$SMPROGRAMS\${LEXPAD_PLAYER_LNK}"
  ${EndIf}
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
