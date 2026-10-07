# Windows manual test (about 20 minutes)

Use the installer from the latest `build` run on GitHub (Actions → build → artifact
`lexpad-desktop-Windows`). It is unsigned, so SmartScreen warns: More info → Run anyway.

1. **Install.** Run `Lexpad_<version>_x64-setup.exe`. It installs for this user only (no admin
   prompt). Lexpad's icon appears in the system tray (it may sit under the ^ arrow). The Settings
   window opens on first run.
2. **Single instance.** Start Lexpad again from the Start menu. No second tray icon appears; the
   Settings window comes forward.
3. **Connect.** In Settings, Connect to Lexpad. The browser opens app.lexpad.app/connect-desktop.
   Sign in there if needed, press Allow. The tab says "Lexpad is connected"; Settings shows your
   e-mail. In the web app, Settings → Account → Signed-in devices lists "Desktop app · Windows".
4. **Credential Manager.** Control Panel → Credential Manager → Windows Credentials lists
   `app.lexpad.desktop` (a generic credential). No password is stored, only the session.
5. **Word (UI Automation).** In Word or WordPad, type "Her answer was candid and kind." Select
   "candid" and press Ctrl+Shift+L. The card opens near the pointer: part of speech, level, up to
   three meanings, and "Seen in Microsoft Word" (or "WordPad") with the sentence. Press Enter:
   "Added to your notebook". In the web app the word has the sentence as its first example and
   the note "Seen in Microsoft Word".
6. **Clipboard path.** Copy something to the clipboard first (an image from Paint is a good
   test). In an app without UI Automation text support (for example a terminal or an Electron
   app), select a word and press Ctrl+Shift+L. The card shows the word, with no sentence. Paste
   afterwards: the clipboard still holds what you copied before (the image), not the word.
7. **Nothing selected.** Click somewhere with no selection, press Ctrl+Shift+L. The "Add a word"
   box appears; type "serene", Enter, then Enter again to add.
8. **Long selection.** Select a whole sentence, press the shortcut. "Pick the word" lists its
   words; pick one, Enter: the card keeps the sentence.
9. **Already added.** Select "candid" again, Ctrl+Shift+L, Enter: "This word is already in your
   notebook."
10. **Script routing.** With an English and a Persian notebook, select a Persian word: the card
    says "Added to <Persian notebook>".
11. **Esc and focus.** Open the card, press Esc. The card closes and the app you were in has the
    keyboard again.
12. **Shortcut change.** Settings → Shortcut → Change, press Ctrl+Alt+K. The old shortcut stops
    working, the new one works. Try a shortcut another app holds: Settings says it is taken.
13. **Start on login.** Turn it off and on in Settings. Sign out of Windows and back in: Lexpad
    is in the tray (when on).
14. **Dark mode.** Switch Windows to dark mode; the card, the tray panel and Settings follow.
15. **Sign out.** Settings → Sign out → Sign out. The device disappears from Signed-in devices in
    the web app, and the credential disappears from Credential Manager.
16. **Tray icon.** The tray shows the green Lexpad mark, sharp (not blurred) at 100%, 125%,
    150% and 200% display scaling (Settings → System → Display → Scale; sign out and in if the
    icon does not refresh).
17. **Tray panel.** Left-click the tray icon: a panel opens just above the icon, inside the
    screen, with rounded corners (Windows 11), your e-mail, an "Add a word…" box with the
    keyboard in it, "Select text anywhere and press Ctrl+Shift+L", the last words added from this
    computer, the notebook picker, and Open Lexpad / Settings… / Quit. Type "serene" and press
    Enter: the panel closes and the card opens for "serene". Click a recent word: its page opens
    in the browser. Click the tray icon again, or anywhere else, or press Esc: the panel closes.
    Tab reaches every control. With the taskbar on the left or the top, the panel opens beside or
    below the icon. With two monitors, it opens on the one whose tray was clicked. Signed out, it
    shows "Connect to Lexpad" instead.
18. **Right-click menu.** Right-click the tray icon: Open Lexpad, Settings…, Quit Lexpad.
19. **Uninstall.** Settings → Apps → Lexpad → Uninstall. The tray icon is gone, and the login
    item with it.

Report anything that differs, with a screenshot, in the to-do list.
