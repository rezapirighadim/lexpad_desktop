/**
 * macOS Accessibility, as the card and the Settings window explain it.
 *
 * From 0.3.3 every release is signed with the same local certificate, so
 * macOS keeps the grant across updates. A copy updated from 0.3.2 or older
 * (ad-hoc signed, a new identity each build) still needs Lexpad removed from
 * the list and added again once: the switch can look on while macOS no longer
 * applies it to the new build. The same happens once more on the first
 * release signed with the Developer ID (team 6Z95MR26LZ), whose designated
 * requirement differs from the local certificate's; PERMISSION_STALE is what
 * those users see, because the core remembers the version that had the grant.
 */
/**
 * How often a window asks again whether Accessibility is allowed while it
 * explains the permission, so an explanation goes away once it is allowed,
 * without a restart. Reading it (`AXIsProcessTrusted`) costs nothing.
 */
export const PERMISSION_POLL_MS = 2000;
/** What the card says when macOS has not allowed Lexpad to read the selection. */
export const PERMISSION_MISSING =
  'To add the word you select in any app, allow Lexpad under Privacy & Security, Accessibility. If Lexpad is already on, remove it with − and add it again, then reopen Lexpad. Lexpad reads only the selected text, and only when you press the shortcut.';
/** What it says when an update lost the permission the app had. */
export const PERMISSION_STALE =
  'Lexpad was updated, and macOS no longer applies the permission it had. In System Settings, Privacy & Security, Accessibility: if Lexpad is already on, remove it with − and add it again, then reopen Lexpad.';
