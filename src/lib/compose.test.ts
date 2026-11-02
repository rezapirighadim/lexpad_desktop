import { describe, expect, it } from 'vitest';
import { compose, pick, plan, seenIn, wordsOf } from './compose.js';
import type { Capture } from './types.js';

const capture = (over: Partial<Capture>): Capture => ({
  text: null,
  context: null,
  app: 'TextEdit',
  permission: 'granted',
  via: 'accessibility',
  ...over,
});

describe('compose', () => {
  it('keeps the sentence as the first example and the app as the private note', () => {
    const word = compose(
      'candid',
      { sentence: 'She was candid about it.', app: 'TextEdit' },
      { headword: 'candid', meanings: [{ meaning: 'honest', examples: [{ sentence: 'A candid talk.' }] }] },
    );
    expect(word.source).toBe('ai');
    expect(word.memo).toBe('Seen in TextEdit');
    expect(word.meanings?.[0]?.examples?.map((e) => e.sentence)).toEqual([
      'She was candid about it.',
      'A candid talk.',
    ]);
  });

  it('invents nothing: no card, no sentence means a bare word', () => {
    const word = compose('candid', { sentence: '', app: null }, undefined);
    expect(word).toEqual({ headword: 'candid', category: 'general', source: 'manual' });
  });

  it('keeps a sentence even without a meaning', () => {
    const word = compose('candid', { sentence: 'Be candid.', app: 'Notes' }, undefined);
    expect(word.meanings).toEqual([{ meaning: '—', examples: [{ sentence: 'Be candid.' }] }]);
    expect(word.memo).toBe('Seen in Notes');
  });

  it('never writes more than an app name into the note', () => {
    expect(seenIn('Microsoft‮Word\n\t')).toBe('MicrosoftWord');
    expect(seenIn('  Google   Chrome ')).toBe('Google Chrome');
    expect(seenIn(null)).toBe('');
    expect(seenIn('x'.repeat(200))).toHaveLength(80);
  });
});

describe('plan', () => {
  it('looks a word up, with the sentence only when the platform gave the text around it', () => {
    expect(plan(capture({ text: 'candid', context: 'He was candid. Then he left.' }))).toEqual({
      kind: 'word',
      headword: 'candid',
      sentence: 'He was candid.',
    });
    expect(plan(capture({ text: 'candid', via: 'clipboard' }))).toEqual({
      kind: 'word',
      headword: 'candid',
      sentence: '',
    });
  });

  it('offers a longer selection to pick from, and the box when there is nothing', () => {
    expect(plan(capture({ text: 'one two three four five six seven eight' }))).toEqual({
      kind: 'sentence',
      text: 'one two three four five six seven eight',
    });
    expect(plan(capture({ text: null }))).toEqual({ kind: 'type' });
    expect(plan(capture({ text: '12345' }))).toEqual({ kind: 'type' });
    expect(plan(null)).toEqual({ kind: 'type' });
  });

  it('takes a phrase of five to seven words as one word', () => {
    for (const phrase of [
      'once in a blue moon',
      'the ball is in your court',
      'every cloud has a silver lining',
    ]) {
      expect(plan(capture({ text: phrase }))).toEqual({ kind: 'word', headword: phrase, sentence: '' });
    }
  });

  it('picks up to seven words in a row', () => {
    const words = wordsOf(' Well, that is the last  straw for me. ');
    expect(words).toEqual(['Well,', 'that', 'is', 'the', 'last', 'straw', 'for', 'me.']);
    expect(pick(words, 7, 7)).toBe('me');
    expect(pick(words, 3, 1)).toBe('that is the');
    expect(pick(words, 0, 4)).toBe('Well, that is the last');
    expect(pick(words, 1, 7)).toBe('that is the last straw for me');
    expect(pick(words, 0, 7)).toBeUndefined();
  });
});

describe('the note the web app reads back', () => {
  it('starts with the same words as lexpad_front lib/seenIn.ts', async () => {
    const { SEEN_IN } = await import('./compose.js');
    expect(SEEN_IN).toBe('Seen in ');
  });
});
