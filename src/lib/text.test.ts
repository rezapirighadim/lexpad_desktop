import { describe, expect, it } from 'vitest';
import { asHeadword, contextHint, pageAddress, sentenceAround } from './text.js';

describe('asHeadword', () => {
  it('keeps a word or a short phrase and strips the punctuation around it', () => {
    expect(asHeadword('  serendipity, ')).toBe('serendipity');
    expect(asHeadword('“give up”')).toBe('give up');
    expect(asHeadword('(بلندپرواز)')).toBe('بلندپرواز');
    expect(asHeadword('take it for granted')).toBe('take it for granted');
  });

  it('refuses what is not a word: empty, numbers, long passages, many words', () => {
    expect(asHeadword('   ')).toBeUndefined();
    expect(asHeadword('2026')).toBeUndefined();
    expect(asHeadword('one two three four five')).toBeUndefined();
    expect(asHeadword('a'.repeat(61))).toBeUndefined();
  });
});

describe('sentenceAround', () => {
  const block =
    'She was ambitious from an early age. Her teachers said she would run a company one day, and she did! Everyone agreed.';

  it('returns the sentence the word sits in', () => {
    expect(sentenceAround(block, 'ambitious')).toBe('She was ambitious from an early age.');
    expect(sentenceAround(block, 'company')).toBe(
      'Her teachers said she would run a company one day, and she did!',
    );
  });

  it('understands Persian sentence marks and collapses whitespace', () => {
    expect(sentenceAround('او   بلندپرواز است؟ بله. ', 'بلندپرواز')).toBe('او بلندپرواز است؟');
  });

  it('clips a very long sentence around the word', () => {
    const long = `${'word '.repeat(80)}focus ${'word '.repeat(80)}`.trim();
    const out = sentenceAround(long, 'focus', 100);
    expect(out.length).toBeLessThanOrEqual(102);
    expect(out).toContain('focus');
    expect(out.startsWith('…')).toBe(true);
  });

  it('falls back to the start of the block when the word is not found', () => {
    expect(sentenceAround('Some text here.', 'missing')).toBe('Some text here.');
  });
});

describe('contextHint', () => {
  it('wraps the sentence for the model within the API limit', () => {
    const hint = contextHint('She was ambitious from an early age.');
    expect(hint).toBe('Seen in: “She was ambitious from an early age.”');
    const long = contextHint('x'.repeat(500));
    expect(long).toBeDefined();
    expect(long?.length ?? 0).toBeLessThanOrEqual(200);
    expect(contextHint('   ')).toBeUndefined();
  });
});

describe('pageAddress', () => {
  it('keeps the origin and the path only', () => {
    expect(pageAddress('https://example.com/p?token=abc#x')).toBe('https://example.com/p');
    expect(pageAddress('https://example.com/a/b/#top')).toBe('https://example.com/a/b/');
    expect(pageAddress('http://example.com:8080/read?id=1')).toBe('http://example.com:8080/read');
    expect(pageAddress('https://example.com')).toBe('https://example.com/');
  });

  it('drops credentials written into the address', () => {
    expect(pageAddress('https://user:secret@example.com/p')).toBe('https://example.com/p');
  });

  it('gives nothing for what is not a web page', () => {
    expect(pageAddress('')).toBe('');
    expect(pageAddress('not a url')).toBe('');
    expect(pageAddress('file:///Users/me/notes.html')).toBe('');
    expect(pageAddress('javascript:alert(1)')).toBe('');
  });
});
