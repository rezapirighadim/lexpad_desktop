import { describe, expect, it } from 'vitest';
import { notebookFor, scriptOf, scriptOfLanguage } from './script.js';

const english = { id: 'nb1', targetLang: 'en' };
const persian = { id: 'nb2', targetLang: 'fa' };
const french = { id: 'nb3', targetLang: 'fr' };

describe('the writing system of a selection', () => {
  it('names the ones that matter', () => {
    expect(scriptOf('ambitious')).toBe('latin');
    expect(scriptOf('جاه‌طلب')).toBe('arabic');
    expect(scriptOf('野心的な')).toBe('kana');
    expect(scriptOf('123')).toBe('unknown');
  });

  it('treats a language it has not heard of as Latin, so it never cries wolf', () => {
    expect(scriptOfLanguage('en')).toBe('latin');
    expect(scriptOfLanguage('fa-IR')).toBe('arabic');
    expect(scriptOfLanguage('xx')).toBe('latin');
  });
});

describe('where a selected word goes', () => {
  it('leaves the learner’s own choice alone when the word could be in it', () => {
    expect(notebookFor('ambitious', english, [english, persian])).toEqual({
      notebook: english,
      moved: false,
    });
  });

  it('moves a word its notebook could not possibly hold', () => {
    // An English notebook cannot hold a Persian word, and filing it there
    // silently is how somebody loses it.
    expect(notebookFor('جاه‌طلب', english, [english, persian])).toEqual({ notebook: persian, moved: true });
  });

  it('answers nothing when no notebook takes that script', () => {
    expect(notebookFor('野心的な', english, [english, french])).toEqual({
      notebook: undefined,
      moved: false,
    });
  });

  it('does not choose between two that could both take it', () => {
    const arabic = { id: 'nb4', targetLang: 'ar' };
    // The popup's picker and the app both do that better than a guess.
    expect(notebookFor('كتاب', english, [english, persian, arabic])).toEqual({
      notebook: english,
      moved: false,
    });
  });

  it('says nothing about digits and punctuation', () => {
    expect(notebookFor('42', english, [english, persian])).toEqual({ notebook: english, moved: false });
  });
});
