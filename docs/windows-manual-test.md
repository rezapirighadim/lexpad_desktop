# Windows manual test (about 20 minutes)

Use the installer from the latest `build` run on GitHub (Actions → build → artifact
`lexpad-desktop-Windows`). It is unsigned, so SmartScreen warns: More info → Run anyway.

1. **Install.** Run `Lexpad_<version>_x64-setup.exe`. It installs for this user only (no admin
   prompt). Lexpad's icon appears in the system tray (it may sit under the ^ arrow). The Start
   window opens on first run (the Player's loop, Open a video…, Continue with Google / Apple),
   and nothing is added to Settings → Apps → Startup. The Start menu has "Lexpad" and
   "Lexpad Player"; "Lexpad Player" opens straight into the Player.
2. **Single instance.** Start Lexpad again from the Start menu. No second tray icon appears; the
   Start window comes forward (signed out) or the window last in front (signed in).
3. **Sign in.** In Settings, Sign in…. The browser opens app.lexpad.app/connect-desktop.
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
18. **Card placement and drag.** Move the pointer to the bottom right corner, just above the
    taskbar, select a word there and press Ctrl+Shift+L: the card opens above and to the left of
    the pointer, wholly above the taskbar, and stays there when the meanings load; Add and
    Cancel are visible and clickable. Repeat with the taskbar on the left and on top, at 125 %
    and 150 % scaling, and on a second monitor (also one placed left of or above the main
    one). Drag the card by its header to another place: it moves; × still closes it. The next
    shortcut opens it near the pointer again. On a short screen (1366 × 768 at 125 %) a card
    with three meanings is cut to the screen and scrolls, with Add and Cancel still showing.
19. **Lexpad's window (0.2).** Click Open Lexpad in the panel: a window opens with the sign-in
    screen ("Continue in your browser") or, signed in, Today. Sign in through the browser: the
    window opens your notebook. Start a session, answer a few cards, turn Wi-Fi off, answer more,
    turn it back on: nothing is lost (Progress shows the reviews). Click a recent word in the
    panel: the window opens on that word. Settings → This computer: change the shortcut, the
    notebook, notifications and the start-up switches. Close the window and open it again: same
    size and place; move it to a second monitor, unplug that monitor, open it again: it comes
    back on the remaining screen. Set a reminder time a few minutes ahead (Settings →
    Notifications), close the window: a toast arrives at that time; click it: the window opens.
20. **Right-click menu.** Right-click the tray icon: Open Lexpad, Open a video…, Lexpad on your
    phone… (signed in only; "Lexpad everywhere…" once the account used a phone app), Settings…,
    Quit Lexpad. Lexpad on your phone… opens Settings → Lexpad everywhere in Lexpad's window. Sign
    out: the item is gone from the menu; sign in again: it is back. The panel shows the same row
    above Open Lexpad, and Tab reaches it after the recent words.
21. **Uninstall.** Settings → Apps → Lexpad → Uninstall. The tray icon is gone, and the login
    item with it.

22. **The Microsoft Store copy (0.3.2).** Install Lexpad from the Store (or, before it is live,
    the artifact `lexpad-desktop-msix`, which installs only after signing; CI's
    `scripts/msix-smoke.ps1` shows how). Go through 3, 5, 6, 13, 17 and 19 again with it. Start
    on login is the package's startup task here: turning the switch off and on shows Lexpad as
    Disabled / Enabled under Settings → Apps → Startup and in Task Manager → Startup apps; once
    turned off there, the switch in Lexpad cannot turn it back on (Windows' rule), and the app's
    Settings shows it off. A reminder's toast carries the name Lexpad and opens the window.
    Uninstalling from Start removes the package, its startup task and its data; the session in
    Credential Manager stays until Sign out (as with the installer).

23. **The video player's engine (branch feat/video-player).** Build with
    `LEXPAD_PLAYER_DEV=1 pnpm tauri build --features player-dev --no-bundle` after
    `scripts/ffmpeg/fetch-prebuilt.sh win64`, copy `scripts/media-samples.sh`'s files over (made
    on the Mac), and run the exe: the development page opens. Check, against the Mac:
    - The engine line names an H.264 encoder (`nvenc`, `qsv`, `amf` or `mediaFoundation`;
      `openH264` means no hardware encoder worked) and `MSE: yes`. No console window flashes
      while a file opens, seeks or loads a subtitle (CREATE_NO_WINDOW).
    - Every sample plays in the same mode as on the Mac, with sound; HEVC 4K and HDR10 play
      smoothly (tone-mapped: the colours look like the SDR samples, not washed out).
    - Seeking far ahead answers in about the Mac's time (`scripts/player-bench.sh` numbers in
      the branch's report); the subtitles stay in step after seeks.
    - Dropping a video with its .srt files at once, and one at a time, works; a Persian
      Windows-1256 .srt shows Persian, right to left.
    - Installed from the NSIS setup: Explorer's "Open with" lists Lexpad for .mkv/.mp4/.srt,
      double-clicking still opens the old default app; Settings → Apps → Default apps → Lexpad
      exists and lists the types; choosing Lexpad for .mkv there makes a double-click open the
      player, and a second file opens in the same Lexpad. Uninstalling removes the entries.
    - The MSIX copy: "Open with" lists Lexpad too, and ffmpeg.exe and ffprobe.exe are in the
      package (the player works).

24. **First run and the Player's window (0.3.19).**
    - Fresh profile, double-click a video with "Open with Lexpad": only the Player opens and
      plays; no Start window, no Settings window.
    - The Start window: Ctrl+O opens the file dialog; dropping a .srt alone says "Drop the video
      too"; Esc does nothing; Ctrl+W closes it. Closing the last window once shows the toast
      "Lexpad is still running"; never again after that.
    - The Player: drag on the picture moves the window, a click still pauses, a double-click is
      full screen; no blue line at the window's foot (accent colour on title bars and borders
      switched on in Settings → Personalisation → Colours); right-click shows the system menu
      with the same items as on the Mac; two subtitles sit where they do on the Mac at 100, 125
      and 150% scaling.

Report anything that differs, with a screenshot, in the to-do list.
