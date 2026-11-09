# Mac App Store: what the owner does

The plan is [mac-app-store-plan.md](mac-app-store-plan.md). The work is done up to the steps
below. Each step says who does it. Nothing here asks you to type a password into anything
except Apple's own pages.

## 1. The record decision (approved 1 Nov 2026)

Approved by the owner on 1 Nov 2026, with the iPhone's review account
`apple-reviewer@lexpad.app` for the Mac too. The Mac app goes on the **same App Store record as the iPhone app** ("Lexpad: AI Vocabulary
Notebook"), with the same bundle id `app.lexpad.ios` and the same two subscriptions. One
subscription then works on iPhone and Mac, and nobody can pay Apple twice for Pro. The
server needs no change for Mac receipts.

Done.

## 2. Signing certificates and the profile (no clicks for you)

Lexpad needs two new Apple certificates and one profile to sign the App Store copy:
"Apple Distribution", "3rd Party Mac Developer Installer", and a "Mac App Store" profile for
`app.lexpad.ios`.

Who: the main session, after your yes. It runs

```
scripts/mas-signing-setup.sh            # looks only
scripts/mas-signing-setup.sh --apply    # makes them
```

The script uses the App Store Connect API key already on this Mac. The private keys are made
on this Mac and stay in the Lexpad signing keychain. You do not need to click anything.

If you would rather do it yourself in Xcode: Xcode › Settings › Accounts › your team ›
Manage Certificates… › + › Apple Distribution, then + › Mac Installer Distribution. Then tell
the main session; it makes the profile with the same script.

## 3. Build and upload (no clicks for you)

Who: the main session.

```
LEXPAD_MAS_UPLOAD=1 scripts/release-mas.sh
```

It builds, signs, checks, makes the package, has Apple validate it, and uploads it. The build
shows up in App Store Connect after about 15 to 30 minutes.

## 4. In App Store Connect (in Chrome, signed in as you)

Who: the main session prepares; you check and press the last button.

The main session (or you) opens <https://appstoreconnect.apple.com/apps/6822224295>:

1. If there is no macOS platform yet: in the left column, next to the platform name, click
   **+** (Add Platform) › **macOS**. Version: the desktop version (for example `0.3.27`).
   (After your yes in step 1 the main session can do this, and fill the
   texts and the review details, with `scripts/mas-asc-listing.py --apply`. It submits nothing.)
2. Click **macOS App › 0.3.27 (Prepare for Submission)**.
3. **Screenshots** (Mac): drag in 3 to 10 PNGs of 2880 × 1800 (see section 6).
4. **Promotional Text, Description, Keywords, Support URL, Marketing URL**: paste from
   section 5 (the main session may already have filled them through the API; check them).
5. **Build**: click **+ Add Build**, pick the uploaded build, **Done**.
6. **In-App Purchases and Subscriptions**: click **+**, tick `pro_monthly` and `pro_yearly`.
7. **App Review Information**: Sign-in required: on. User name: `apple-reviewer@lexpad.app`.
   **Password: you type it here yourself.** Contact: Reza, hello@lexpad.app. Notes: paste
   from section 5.
8. **Version Release**: "Manually release this version" (you decide when it goes live).
9. **Save** (top right).
10. Left column › **App Privacy**: check the answers still hold for the Mac (they are shared
    with the iPhone; the Mac app collects nothing more). No change expected.
11. Left column › **Pricing and Availability**: shared with the iPhone; nothing to do.
12. Tell the main session it is ready. **Only when you say so**, press **Add for Review**,
    then **Submit to App Review**.

You never accept a new agreement or pay anything for this: the Paid Apps agreement is already
in place for the iPhone subscriptions. If App Store Connect shows a banner asking you to accept
an updated agreement, that is yours to read and accept (Business › Agreements).

## 5. The listing text

Plain English, no other platforms named (Guideline 2.3.10), numbers as lexpad.app shows them
on 1 Nov 2026 (`landing/scripts/features.py`, `site/llms.txt`).

**Promotional text** (170 characters max; App Store Connect refuses the ⌘ and ⇧ symbols in the
listing, so the key is written out)

> Select a word in any app and press Command-Shift-L. Lexpad writes the card and brings the
> word back before you forget it. Watch your own films and click any word.

**Description**

```
Lexpad is a vocabulary notebook for the words you meet. Save a word from what you read or watch on your Mac, and Lexpad writes the card and brings the word back just before you would forget it.

ADD A WORD FROM ANY APP
• Select a word in Safari, Mail, Pages, Notes and most other apps, then press Command-Shift-L. Or right-click it and choose Services › Add to Lexpad.
• A small card shows the meaning while the word goes into your notebook.
• Or type a word in the menu bar panel.

WATCH WITH LEXPAD PLAYER
• Play your own video files (MKV, MP4, AVI, MOV, WebM and more) with up to three subtitles at once, each placed, sized and styled on its own.
• Click a word in a subtitle to keep it, with its line.
• Right to left subtitles work, and old Persian and Arabic subtitle files are fixed for you.
• Watching needs no account.

YOU BRING THE WORDS
• AI writes each card: meanings, examples with translations, pronunciation and level. You can change anything it writes.
• 81 languages, in any pair.

PRACTISE A LITTLE EACH DAY
• Today shows what is due: due words first, then new ones.
• Flashcards, recall, multiple choice, spelling and cloze, plus games and drills with your own words.
• Five Leitner boxes: know a word and it waits longer; miss it and it comes back sooner. Or try the Smart schedule, which learns how fast you forget.
• Practice works offline and syncs when you are back online.

FREE AND PRO
Lexpad is free to use, with 10 AI cards a day (300 a month). Lexpad Pro adds:
• 150 AI cards a day (2,000 a month).
• Lex, the AI tutor that knows your newest words: 50 messages in any 24 hours (600 a month).
• Words worth keeping: paste a text and AI picks out the expressions worth learning (10 texts a day).
• Say it and Use it in a sentence, voice input, and as many notebooks as you like.
New accounts get Pro free for 7 days. Nothing is charged when it ends.

Lexpad Pro is an auto-renewing subscription, monthly or yearly. The price is shown in the app before you buy. Payment is charged to your Apple Account when you confirm the purchase. The subscription renews automatically unless it is cancelled at least 24 hours before the end of the current period, and your account is charged for the renewal within 24 hours before the period ends. You can manage or cancel it in the App Store: click your name, then Account Settings, then Subscriptions.

YOUR DATA
No ads and no trackers. Lexpad asks before anything you type is sent to its AI providers, Anthropic (Claude) and OpenAI. Lexpad reads only the text you select, and only when you press the shortcut. Export your notebook any time, and delete your account from Settings.

Terms of Use (EULA): https://lexpad.app/terms
Apple Standard EULA: https://www.apple.com/legal/internet-services/itunes/dev/stdeula/
Privacy Policy: https://lexpad.app/privacy
Support: hello@lexpad.app
```

**Keywords** (100 characters max; 92 used)

```
vocabulary,flashcards,spaced repetition,leitner,subtitles,video player,language learning,srs
```

**Support URL** `https://lexpad.app/support` · **Marketing URL** `https://lexpad.app`

**Notes for App Review**

```
Lexpad is a personal vocabulary notebook with spaced repetition. Learners add the words they meet themselves; the app never generates or pre-fills vocabulary. AI only writes a short meaning and example for a word the learner added, and only after the learner allows it. Every AI card and every Lex answer has a "Report" link.

SIGNING IN
Click "Sign in…" (or "Continue with Google / Apple" on the first window). Lexpad opens the Mac's default browser on lexpad.app, where you sign in with the demo account above (or with Apple), and the browser hands the sign-in back to the app: it opens lexpad://auth/callback, and macOS may ask "Open Lexpad?" (choose Open). The app never sees a password. This is the standard sign-in for native apps (RFC 8252, private-use URI scheme redirect with PKCE). The app does not listen on any port and has no network server entitlement.

ADDING A WORD FROM ANOTHER APP
Select a word in Safari or TextEdit and press Command-Shift-L, or right-click it and choose Services > Add to Lexpad. The app is sandboxed and does not use Accessibility; macOS hands it only the selected text through the Services menu.

LEXPAD PLAYER
Window > Player, or File > Open Video… (Command-O). Choose a video file; a subtitle file with the same name next to it (Film.mkv and Film.srt) opens with it. Click any word in a subtitle to save it. The bundled FFmpeg (LGPL) runs inside the app's sandbox and only reads the files you open.

IN-APP PURCHASES
The same auto-renewable subscriptions as the iPhone app (Universal Purchase), group "Lexpad Pro": pro_monthly (4.99 USD a month) and pro_yearly (29.99 USD a year; new subscribers may get the introductory first-year price 20.99 USD). To reach the paywall: Settings > Plan > Get Lexpad Pro. The sheet shows the price, the period and auto-renewal, with Restore purchases, Terms and Privacy. Restore purchases is also on Settings > Plan. Pro is sold in the Mac app only through Apple's in-app purchase.
New accounts get Pro free for 7 days with no payment details; nothing is charged when it ends.

The demo account is on the Free plan, so Lexpad Pro can be bought.
```

## 6. Screenshots (needs your screen for about 10 minutes, or the Mac locked)

Apple wants 2880 × 1800 (or 1440 × 900) PNGs of the real Mac app. They must come from this
build, with real words (Guideline 2.3.3), and follow `reports/lessons.md` (Store creatives and
Store screenshots). The main session takes them when you say the screen is free:

1. Lexpad's window on Today, with the reviewer account's real words.
2. The card after ⌘⇧L on a word in Safari.
3. Lexpad Player with a film and two subtitles, a word clicked.
4. A practice card.
5. Settings › Plan with Apple's prices (StoreKit test prices are not allowed in screenshots).

## 7. What is left before Submit

- [x] Your yes to the record decision (step 1), 1 Nov 2026.
- [ ] Certificates and profile (step 2), then the signed upload (step 3).
- [ ] Merge order: back PR first (deploy with `scripts/deploy.sh`), then front PR, then the
      desktop PR with `web/` rebuilt from front's merged main.
- [ ] The full sandboxed end-to-end run on this Mac with the screen free:
      `scripts/mas-smoke.sh` (sign in, ⌘⇧L through Services, Player with a film and its
      subtitle). It opens windows and the browser, so it waits for you.
- [ ] A sandbox purchase with a Sandbox Apple Account (App Store Connect › Users and Access ›
      Sandbox): you sign in to it when the purchase sheet asks; you type that password.
- [ ] Screenshots (step 6) and the listing (step 4).
- [x] Guideline 2.3.10: the Mac App Store build hides Android, Google Play and the Microsoft
      Store, and keeps the iPhone app, the web app, the extension and Windows (front PR #91).
- [ ] The iPhone review: the subscriptions are still "Waiting for Review" with iPhone 1.0.81;
      the Mac version can be submitted while they are, but it is cleaner to wait for that review.
- [ ] Press Submit only after you confirm.
