import { describe, expect, it } from 'vitest';
import { signInWords } from './signIn.js';

describe('signInWords', () => {
  it('speaks of the browser where the sign-in happens there', () => {
    const w = signInWords(undefined);
    expect(w.how).toContain('browser');
    expect(w.waiting).toContain('browser');
  });

  it('never speaks of a browser in the copy that signs in inside the app', () => {
    for (const text of Object.values(signInWords('app'))) {
      expect(text.toLowerCase()).not.toContain('browser');
    }
  });
});
