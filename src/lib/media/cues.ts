/**
 * Which subtitle lines are on screen, for up to three tracks at once, each
 * with its own offset. Pure, so it is tested plainly; the real player's
 * overlay and transcript use the same functions.
 *
 * Times: the `<video>` element's seconds minus the playback's `timeOffset`
 * is the file's time; cues are in the file's milliseconds; a track's offset
 * (ms, positive shows the lines later) is added to every cue.
 */
import type { Cue } from './api.js';

/** The most tracks shown at once (owner's phase 1). */
export const MAX_TRACKS = 3;
/** One press of the offset keys, in milliseconds. */
export const OFFSET_STEP_MS = 100;

/** The file's milliseconds at the element's `currentTime`. */
export function fileMs(currentTime: number, timeOffset: number): number {
  return Math.round((currentTime - timeOffset) * 1000);
}

/** Where to start looking for cues on screen at `t` (cues sorted by start). */
function firstEndingAfter(cues: readonly Cue[], t: number): number {
  // Sorted by start, not by end (cues overlap): find the last cue starting
  // by `t`, then step back over every cue recent enough to still be on.
  let lo = 0;
  let hi = cues.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if ((cues[mid] as Cue).start <= t) lo = mid + 1;
    else hi = mid;
  }
  let i = lo;
  while (i > 0 && (cues[i - 1] as Cue).start > t - LONGEST_CUE_MS) i--;
  return i;
}
/** How far back an overlapping cue may start and still be on screen. */
const LONGEST_CUE_MS = 120_000;

/** The cues on screen at the file's time `t` (ms) with the track's offset. */
export function activeCues(cues: readonly Cue[], t: number, offsetMs: number): Cue[] {
  const at = t - offsetMs;
  const out: Cue[] = [];
  for (let i = firstEndingAfter(cues, at); i < cues.length; i++) {
    const c = cues[i] as Cue;
    if (c.start > at) break;
    if (c.end > at) out.push(c);
  }
  return out;
}

/** The cue closest before `t` (for "this line was just spoken"). */
export function lastCueBefore(cues: readonly Cue[], t: number, offsetMs: number): Cue | undefined {
  const at = t - offsetMs;
  let best: Cue | undefined;
  for (const c of cues) {
    if (c.start > at) break;
    best = c;
  }
  return best;
}

/**
 * "This line was just spoken": the offset that makes `cue` start now. The
 * learner presses it when they hear the line the overlay shows (or the next
 * one); the whole track moves by the difference.
 */
export function offsetToStartNow(cue: Cue, t: number): number {
  return Math.round(t - cue.start);
}

/** Whether a line is right to left (its first strong character). */
export function isRtl(text: string): boolean {
  for (const ch of text) {
    if (/[֐-ࣿיִ-﷿ﹰ-﻿]/u.test(ch)) return true;
    if (/\p{L}/u.test(ch)) return false;
  }
  return false;
}
