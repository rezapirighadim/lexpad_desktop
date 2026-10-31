// Copied from lexpad_front apps/web/src/features/player/model/title.test.ts; keep them identical.
import { describe, expect, it } from 'vitest';
import { episodeCode, episodeShort, parseTitle } from './videoTitle.js';

describe('titles from file names', () => {
  it('reads a series episode with its own name', () => {
    const t = parseTitle('The.Terrace.S01E03.Ephemeral.1080p.WEB-DL.x264.mkv');
    expect(t).toEqual({ title: 'The Terrace', season: 1, episode: 3, episodeTitle: 'Ephemeral' });
    expect(episodeCode(t)).toBe('S1 · E3');
    expect(episodeShort(t)).toBe('S1 E3');
  });

  it('reads 1x03 and "Season 1 Episode 3"', () => {
    expect(parseTitle('Night Market 2x01.mp4')).toMatchObject({
      title: 'Night Market',
      season: 2,
      episode: 1,
    });
    expect(parseTitle('Mina_s Kitchen Season 1 Episode 7.mkv')).toMatchObject({ season: 1, episode: 7 });
  });

  it('drops a year and release tags from a film', () => {
    expect(parseTitle('Tears.of.Steel.2012.mp4')).toEqual({ title: 'Tears of Steel' });
    expect(parseTitle('Sintel.2010.720p.mkv')).toEqual({ title: 'Sintel' });
    expect(parseTitle('[Group] Paper Lanterns (2019) [1080p].mkv')).toEqual({ title: 'Paper Lanterns' });
  });

  it('keeps a plain name as it is', () => {
    expect(parseTitle('holiday.mov')).toEqual({ title: 'holiday' });
    expect(episodeCode({ title: 'holiday' })).toBeUndefined();
  });
});
