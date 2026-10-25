import { describe, expect, it } from 'vitest';
import type { Cue } from './api.js';
import { activeCues, fileMs, isRtl, lastCueBefore, offsetToStartNow } from './cues.js';

const cue = (start: number, end: number, text = `${start}`): Cue => ({ start, end, text, spans: [{ text }] });
const cues = [
  cue(1000, 3000),
  cue(2500, 4000),
  cue(5000, 6000),
  cue(5000, 40000, 'long sign'),
  cue(41000, 42000),
];

describe('active cues', () => {
  it('finds overlapping lines', () => {
    expect(activeCues(cues, 2600, 0).map((c) => c.start)).toEqual([1000, 2500]);
    expect(activeCues(cues, 4500, 0)).toEqual([]);
    expect(activeCues(cues, 30_000, 0).map((c) => c.text)).toEqual(['long sign']);
  });

  it('applies the track offset (positive shows lines later)', () => {
    expect(activeCues(cues, 1500, 1000)).toEqual([]);
    expect(activeCues(cues, 2100, 1000).map((c) => c.start)).toEqual([1000]);
    expect(activeCues(cues, 600, -500).map((c) => c.start)).toEqual([1000]);
  });

  it('converts the element time to the file time', () => {
    expect(fileMs(13.25, 1)).toBe(12250);
    expect(fileMs(13.25, 0)).toBe(13250);
  });

  it('"this line was just spoken" moves the track to start it now', () => {
    const last = lastCueBefore(cues, 5200, 0);
    expect(last?.start).toBe(5000);
    expect(offsetToStartNow(cue(5000, 6000), 5800)).toBe(800);
  });

  it('knows right-to-left lines', () => {
    expect(isRtl('سلام دنیا')).toBe(true);
    expect(isRtl('« سلام »')).toBe(true);
    expect(isRtl('Hello سلام')).toBe(false);
    expect(isRtl('123')).toBe(false);
  });
});
