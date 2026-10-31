import { describe, expect, it } from 'vitest';
import type { RecentEntry } from '../lib/media/api.js';
import { videoName, watchRow } from './watch.js';

const entry = (over: Partial<RecentEntry> = {}): RecentEntry => ({
  key: 'k',
  fileId: 'f',
  name: 'Tears.of.Steel.2012.1080p.mkv',
  position: 60,
  duration: 734,
  watched: false,
  updated: 1,
  missing: false,
  ...over,
});

describe('the panel Player row', () => {
  it('is "Watch a video…" with nothing watched yet', () => {
    expect(watchRow([])).toEqual({ kind: 'pick' });
  });

  it('continues the newest video, in whole minutes left', () => {
    expect(watchRow([entry(), entry({ fileId: 'older' })])).toEqual({
      kind: 'continue',
      fileId: 'f',
      title: 'Tears of Steel',
      minutesLeft: 11,
    });
  });

  it('drops the minutes under one minute left, or when the length is not known', () => {
    expect(watchRow([entry({ position: 700 })])).toEqual({
      kind: 'continue',
      fileId: 'f',
      title: 'Tears of Steel',
    });
    expect(watchRow([entry({ duration: 0 })])).toEqual({
      kind: 'continue',
      fileId: 'f',
      title: 'Tears of Steel',
    });
  });

  it('never continues a moved file or a finished one', () => {
    const { fileId: _gone, ...moved } = entry({ missing: true });
    expect(watchRow([moved])).toEqual({ kind: 'pick' });
    expect(watchRow([entry({ watched: true })])).toEqual({ kind: 'pick' });
  });

  it('names an episode as the Player does', () => {
    expect(videoName(entry({ name: 'The.Terrace.S01E03.Ephemeral.1080p.mkv' }))).toBe(
      'The Terrace · S1 · E3',
    );
    expect(videoName(entry({ name: 'show.mkv', season: 2, episode: 4 }))).toBe('show · S2 · E4');
  });
});
