import { describe, expect, it } from 'vitest';
import { cardText, reportFragment } from './report.js';

function decode(fragment: string): unknown {
  const base64 = fragment.replace(/-/g, '+').replace(/_/g, '/');
  const bytes = Uint8Array.from(atob(base64 + '='.repeat((4 - (base64.length % 4)) % 4)), (c) =>
    c.charCodeAt(0),
  );
  return JSON.parse(new TextDecoder().decode(bytes));
}

describe('report payload', () => {
  const card = {
    headword: 'run',
    pos: 'verb',
    meanings: [
      {
        meaning: 'دویدن',
        gloss: 'move fast',
        examples: [{ sentence: 'She runs.', translation: 'او می‌دود.' }],
      },
      { meaning: 'اداره کردن' },
    ],
  };

  it('carries the text the card showed, Persian intact', () => {
    expect(cardText('run', card)).toBe(
      'run · verb\n1. دویدن\nmove fast\n“She runs.” — او می‌دود.\n2. اداره کردن',
    );
  });

  it('is base64url only, which is all the core accepts, and short enough for it', () => {
    const fragment = reportFragment('run', card);
    expect(/^[A-Za-z0-9_-]+$/.test(fragment)).toBe(true);
    expect(decode(fragment)).toEqual({ surface: 'card', snapshot: cardText('run', card), headword: 'run' });
    const long = reportFragment('x', { headword: 'x', meanings: [{ meaning: '字'.repeat(5000) }] });
    // 1500 three-byte characters, base64: well inside the core's 8192.
    expect(long.length).toBeLessThan(8192);
  });
});
