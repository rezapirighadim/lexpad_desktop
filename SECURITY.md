# Security

## Reporting a vulnerability

Please report it privately, not in a public issue:

1. Go to this repository's **Security** tab.
2. Click **Report a vulnerability**.
3. Say what you found, how to reproduce it, and what an attacker could do with it.

Only the maintainer sees the report. Please give us a fair chance to fix the problem before you
tell anyone else about it.

## What is in scope

- This desktop app for macOS and Windows: the Rust core, the small pages, sign-in, the session in
  the Keychain or Credential Manager, reading the selection, and the window's bundled web app.
- The Lexpad API (`api.lexpad.app`) and the web app (`app.lexpad.app`). Their code is not public,
  but report problems with them here in the same way.

Out of scope: problems that need a computer that is already taken over, social engineering, denial
of service by flooding, and reports from automated scanners with no working proof.

## What to expect

- We read every report and reply as soon as we can, usually within a few days.
- We tell you whether we can reproduce it, and when it is fixed.
- If you want, we thank you by name in the changelog.

There is no bug bounty: we cannot pay for reports.
