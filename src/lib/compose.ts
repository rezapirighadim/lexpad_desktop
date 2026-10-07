/**
 * The word as it is saved, and what the popup does with a capture. Pure, so
 * it is tested plainly.
 *
 * `compose` mirrors the extension's (`src/background.ts` in
 * `lexpad_extension`): the model's card when there is one, the sentence the
 * word was met in as its first example, and where it was met as the
 * learner's private note. On a desktop the note names the app, "Seen in
 * Microsoft Word", and nothing else: never a window title, which carries
 * document names, e-mail subjects and the names of people in a chat.
 */
import { asHeadword, MAX_WORDS, sentenceAround } from './text.js';
import type { Capture, Card, Source } from './types.js';

/** The most a private note may hold, as the API allows. */
const MAX_MEMO = 500;

export type WordCreate = Card & { source: string; memo?: string };

export function compose(headword: string, source: Source, card: Card | undefined): WordCreate {
  const base: Card = card ? { ...card } : { headword, category: 'general' };
  base.headword = headword;
  const meanings = (base.meanings ?? []).map((m) => ({ ...m, examples: [...(m.examples ?? [])] }));
  if (source.sentence !== '') {
    const first = meanings[0] ?? { meaning: '', examples: [] };
    const already = (first.examples ?? []).some((e) => e.sentence.trim() === source.sentence.trim());
    if (!already) first.examples = [{ sentence: source.sentence }, ...(first.examples ?? [])].slice(0, 5);
    if (meanings.length === 0) meanings.push(first);
    else meanings[0] = first;
  }
  // A meaning is required by the API only when present; an empty one is dropped.
  const kept = meanings.filter((m) => m.meaning.trim() !== '' || (m.examples?.length ?? 0) > 0);
  const out: WordCreate = { ...base, source: card ? 'ai' : 'manual' };
  delete out.meanings;
  if (kept.length > 0) {
    out.meanings = kept.map((m) => (m.meaning.trim() === '' ? { ...m, meaning: '—' } : m));
  }
  const app = seenIn(source.app);
  if (app !== '') out.memo = `Seen in ${app}`.slice(0, MAX_MEMO);
  return out;
}

/** An app's name as it may appear in a note: one line, no control characters, short. */
export function seenIn(app: string | null): string {
  if (app === null) return '';
  return app
    .replace(/[\p{Cc}\p{Cf}]/gu, '')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 80);
}

/** What the popup shows for a capture. */
export type Plan =
  /** A word or short phrase: look it up, with the sentence it was in if the platform gave one. */
  | { kind: 'word'; headword: string; sentence: string }
  /** More than a phrase: the text, to pick the word from; it becomes the example sentence. */
  | { kind: 'sentence'; text: string }
  /** Nothing to go on: the type-a-word box. */
  | { kind: 'type' };

export function plan(capture: Capture | null): Plan {
  const text = capture?.text?.trim() ?? '';
  if (text === '') return { kind: 'type' };
  const headword = asHeadword(text);
  if (headword !== undefined) {
    // Only text the selection API itself gave is a sentence the word was met in.
    const context = capture?.context ?? '';
    return { kind: 'word', headword, sentence: context === '' ? '' : sentenceAround(context, headword) };
  }
  // Letters but too long to be one entry: offer it as a sentence to pick from.
  if (/\p{L}/u.test(text)) return { kind: 'sentence', text };
  return { kind: 'type' };
}

/** The words of a text, as the picker shows them, with what to strip for the headword. */
export function wordsOf(text: string): string[] {
  return text
    .replace(/\s+/g, ' ')
    .trim()
    .split(' ')
    .filter((w) => w !== '');
}

/**
 * The headword for words `from`..`to` (inclusive) of a text, as many as the
 * extension allows in one entry, or undefined when that is not one.
 */
export function pick(words: readonly string[], from: number, to: number): string | undefined {
  const a = Math.max(0, Math.min(from, to));
  const b = Math.min(words.length - 1, Math.max(from, to));
  if (b - a + 1 > MAX_WORDS) return undefined;
  return asHeadword(words.slice(a, b + 1).join(' '));
}
