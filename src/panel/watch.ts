/**
 * The panel's Player row (spec uiux/2026-10-31-player-next A2): "Watch a
 * video…" with ⌘O / Ctrl+O, or, after a video, "Continue {title} · {n} min
 * left" for the newest video in Continue watching that is still where it
 * was and not finished. The title is what the Player itself shows
 * (`videoTitle.ts`, the Watch page's Continue watching card).
 */
import type { RecentEntry } from '../lib/media/api.js';
import { episodeCode, parseTitle } from '../lib/videoTitle.js';

export type WatchRow =
  | { kind: 'pick' }
  | {
      kind: 'continue';
      fileId: string;
      title: string;
      /** Whole minutes left; absent under one minute. */
      minutesLeft?: number;
    };

/** The video's name as the Player shows it: "The Terrace · S1 · E3". */
export function videoName(entry: RecentEntry): string {
  const title = parseTitle(entry.name);
  const code = episodeCode({
    ...title,
    ...(entry.season !== undefined ? { season: entry.season } : {}),
    ...(entry.episode !== undefined ? { episode: entry.episode } : {}),
  });
  return code === undefined ? title.title : `${title.title} · ${code}`;
}

export function watchRow(recent: readonly RecentEntry[]): WatchRow {
  const last = recent[0];
  if (last === undefined || last.missing || last.watched || last.fileId === undefined) {
    return { kind: 'pick' };
  }
  const left = Math.floor(Math.max(0, last.duration - last.position) / 60);
  return {
    kind: 'continue',
    fileId: last.fileId,
    title: videoName(last),
    ...(left >= 1 ? { minutesLeft: left } : {}),
  };
}
