/**
 * The words around "Sign in…" in the small windows. The site's dmg and the
 * Windows builds sign in in the system browser (RFC 8252); the Mac App Store
 * copy signs in inside the app, in Apple's sheet or a sign-in sheet over the
 * window (App Review Guideline 4; the core says `signInVia: 'app'`), so it
 * never speaks of a browser tab.
 */
export interface SignInWords {
  /** After "Sign in to …": how it is done. */
  how: string;
  waiting: string;
  finish: string;
  timeout: string;
}

export function signInWords(via: 'app' | undefined): SignInWords {
  if (via === 'app') {
    return {
      how: 'Lexpad never sees your password.',
      waiting: 'Waiting for you to sign in…',
      finish: 'Finish signing in in the window that just opened.',
      timeout: 'Signing in did not finish in time. Try again.',
    };
  }
  return {
    how: 'It opens your browser; Lexpad never sees your password.',
    waiting: 'Waiting for your browser…',
    finish: 'Finish in the browser tab that just opened.',
    timeout: 'The browser did not answer in time. Try again.',
  };
}
