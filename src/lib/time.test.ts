import { describe, expect, it } from 'vitest';
import { ago } from './time.js';

const now = Date.UTC(2026, 9, 7, 12, 0, 0);

describe('ago', () => {
  it('reads as a person would say it', () => {
    expect(ago(now - 5_000, now)).toBe('just now');
    expect(ago(now - 5 * 60_000, now)).toBe('5 min ago');
    expect(ago(now - 3 * 3_600_000, now)).toBe('3 h ago');
    expect(ago(now - 30 * 3_600_000, now)).toBe('yesterday');
    expect(ago(now - 4 * 86_400_000, now)).toBe('4 d ago');
  });

  it('never says a time in the future', () => {
    expect(ago(now + 60_000, now)).toBe('just now');
  });
});
