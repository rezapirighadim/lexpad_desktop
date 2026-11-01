import { describe, expect, it } from 'vitest';
import { failureOf, noticeOf } from './backend.js';
import { heldText, limitText, offersPro, untilText } from './limits.js';
import type { LimitNotice } from './types.js';

const now = Date.parse('2026-11-10T16:00:00Z');
function withoutPro(n: LimitNotice): LimitNotice {
  const copy = { ...n };
  delete copy.proPerDay;
  return copy;
}

const base: LimitNotice = {
  kind: 'words',
  surface: 'player',
  perDay: 5,
  left: 0,
  resetsAt: '2026-11-10T21:00:00Z',
  plan: 'free',
  proPerDay: 200,
};

describe('the daily numbers’ messages', () => {
  it('says the Player’s words in the approved words, numbers from the server', () => {
    expect(limitText(base, now)).toBe(
      "Not saved. You've kept today's 5 words from your films on Lexpad Free. More at midnight, in about 5 hours. Pro keeps 200 a day here.",
    );
    expect(limitText({ ...base, perDay: 7, proPerDay: 300 }, now)).toContain("today's 7 words");
    expect(limitText({ ...base, perDay: 7, proPerDay: 300 }, now)).toContain('Pro keeps 300 a day here.');
  });

  it('says Pro’s most for one day, here and in the films', () => {
    expect(limitText(withoutPro({ ...base, plan: 'pro', perDay: 200 }), now)).toBe(
      'Not saved. 200 words today from your films is the most for one day. More at midnight, in about 5 hours.',
    );
    expect(limitText({ ...base, surface: 'desktop', plan: 'pro', perDay: 200 }, now)).toContain(
      'from the desktop app is the most',
    );
  });

  it('names the Player for its AI cards', () => {
    expect(limitText({ ...base, kind: 'cards' }, now, 'resilient')).toBe(
      "“resilient” is saved without a meaning. You've used today's 5 AI cards in Lexpad Player. They come back at midnight, in about 5 hours. Pro gives you 200 a day here.",
    );
  });

  it('counts down in minutes in the last hour', () => {
    expect(untilText('2026-11-10T21:00:00Z', Date.parse('2026-11-10T20:59:00Z'))).toBe('in 1 minute');
    expect(untilText('2026-11-10T21:00:00Z', Date.parse('2026-11-10T20:20:00Z'))).toBe('in 40 minutes');
    expect(untilText('2026-11-10T21:00:00Z', Date.parse('2026-11-10T19:50:00Z'))).toBe('in about 1 hour');
  });

  it('offers Pro only to Free, and only when Pro gives more', () => {
    expect(offersPro(base)).toBe(true);
    expect(offersPro({ ...base, plan: 'pro' })).toBe(false);
    expect(offersPro(withoutPro(base))).toBe(false);
    expect(offersPro({ ...base, proPerDay: 5 })).toBe(false);
  });

  it('reads a refusal from the core, and every older failure as before', () => {
    const cause = JSON.stringify({ failure: 'daily_limit', notice: base });
    expect(failureOf(cause)).toBe('daily_limit');
    expect(noticeOf(cause)).toEqual(base);
    expect(failureOf('duplicate')).toBe('duplicate');
    expect(noticeOf('duplicate')).toBeUndefined();
    expect(failureOf('{not json')).toBe('error');
    expect(noticeOf(JSON.stringify({ failure: 'daily_limit', notice: { kind: 'words' } }))).toBeUndefined();
  });

  it('says plainly that a word kept for after midnight uses one of tomorrow’s', () => {
    expect(heldText(base)).toBe("Saved after midnight; it uses one of tomorrow's 5.");
    expect(heldText({ ...base, plan: 'pro', perDay: 200 })).toBe(
      "Saved after midnight; it uses one of tomorrow's 200.",
    );
    expect(limitText(base, now, '', true)).toBe(
      "Not saved. You've kept today's 5 words from your films on Lexpad Free. More at midnight, in about 5 hours. Pro keeps 200 a day here. Saved after midnight; it uses one of tomorrow's 5.",
    );
    // Not held, or a card (the word is already saved): no such line.
    expect(limitText(base, now)).not.toContain('tomorrow');
    expect(limitText({ ...base, kind: 'cards' }, now, 'x', true)).not.toContain('tomorrow');
  });
});
