/**
 * The messages a daily number says (owner-approved wording, 2 Nov 2026,
 * reports/specs/2026-11-01-surface-limits.md §11). Every number comes from
 * the server's refusal (`LimitNotice`); none is typed here. Lexpad Player's
 * own page (lexpad_front features/player) says its words; this file is the
 * desktop app's card, and `limitText` also serves the Player's queue answer.
 */
import type { LimitNotice } from './types.js';

/** "in about 5 hours" / "in 40 minutes" until the learner's midnight. */
export function untilText(resetsAt: string, now: number): string {
  const at = Date.parse(resetsAt);
  if (Number.isNaN(at) || at <= now) return 'now';
  const minutes = Math.max(1, Math.round((at - now) / 60_000));
  if (minutes < 60) return `in ${minutes} minute${minutes === 1 ? '' : 's'}`;
  const hours = Math.round(minutes / 60);
  return `in about ${hours} hour${hours === 1 ? '' : 's'}`;
}

/** The message for a refusal; `headword` is the word, for a card refusal. */
export function limitText(n: LimitNotice, now: number, headword = ''): string {
  const when = untilText(n.resetsAt, now);
  const player = n.surface === 'player';
  if (n.plan === 'pro') {
    const here = player ? 'your films' : 'the desktop app';
    return `Not saved. ${n.perDay} words today from ${here} is the most for one day. More at midnight, ${when}.`;
  }
  const pro = n.proPerDay;
  if (n.kind === 'cards') {
    const place = player ? 'Lexpad Player' : 'the desktop app';
    const tail = pro ? ` Pro gives you ${pro} a day here.` : '';
    return `“${headword}” is saved without a meaning. You've used today's ${n.perDay} AI cards in ${place}. They come back at midnight, ${when}.${tail}`;
  }
  if (player) {
    const tail = pro ? ` Pro keeps ${pro} a day here.` : '';
    return `Not saved. You've kept today's ${n.perDay} words from your films on Lexpad Free. More at midnight, ${when}.${tail}`;
  }
  const tail = pro ? ` Pro gives you ${pro} a day here.` : '';
  return `Not saved. You've added today's ${n.perDay} words from the desktop app on Lexpad Free. You can add more at midnight, ${when}, or in the Lexpad app.${tail}`;
}

/** Whether the message offers "See Pro": only to a Free account that Pro gives more. */
export function offersPro(n: LimitNotice): boolean {
  return n.plan === 'free' && typeof n.proPerDay === 'number' && n.proPerDay > n.perDay;
}
