// Copied from lexpad_extension src/lib/script.ts; keep the two the same (see CLAUDE.md).
/**
 * Which notebook a selected word belongs in.
 *
 * A mirror of `packages/core/src/script.ts` in the learner app, kept small
 * and kept here because this extension is its own repository with its own
 * bundle: reaching for the app's package would mean publishing it, and
 * asking the server per selection would mean a round trip before the bubble
 * could even be drawn.
 *
 * Only the writing system is decided here. Which of two Latin languages a
 * word is in is not knowable, and this does not pretend otherwise: it
 * answers "this word cannot be in that notebook", which is the only thing
 * worth interrupting somebody for.
 */

export type Script =
  | 'latin'
  | 'arabic'
  | 'cyrillic'
  | 'greek'
  | 'hebrew'
  | 'armenian'
  | 'georgian'
  | 'devanagari'
  | 'bengali'
  | 'gurmukhi'
  | 'gujarati'
  | 'tamil'
  | 'telugu'
  | 'kannada'
  | 'malayalam'
  | 'sinhala'
  | 'thai'
  | 'lao'
  | 'khmer'
  | 'myanmar'
  | 'ethiopic'
  | 'han'
  | 'kana'
  | 'hangul'
  | 'unknown';

const SCRIPTS: Record<string, Script> = {
  ru: 'cyrillic',
  uk: 'cyrillic',
  be: 'cyrillic',
  bg: 'cyrillic',
  sr: 'cyrillic',
  mk: 'cyrillic',
  kk: 'cyrillic',
  ky: 'cyrillic',
  tg: 'cyrillic',
  ar: 'arabic',
  fa: 'arabic',
  ur: 'arabic',
  ps: 'arabic',
  ku: 'arabic',
  he: 'hebrew',
  el: 'greek',
  hy: 'armenian',
  ka: 'georgian',
  hi: 'devanagari',
  mr: 'devanagari',
  ne: 'devanagari',
  bn: 'bengali',
  pa: 'gurmukhi',
  gu: 'gujarati',
  ta: 'tamil',
  te: 'telugu',
  kn: 'kannada',
  ml: 'malayalam',
  si: 'sinhala',
  th: 'thai',
  lo: 'lao',
  km: 'khmer',
  my: 'myanmar',
  am: 'ethiopic',
  zh: 'han',
  ja: 'kana',
  ko: 'hangul',
};

/**
 * The script a language is written in. Everything not listed is Latin,
 * which is the safe default: a language this build has not heard of yet
 * falls through to the script that never raises a false alarm.
 */
export function scriptOfLanguage(language: string): Script {
  return SCRIPTS[language.toLowerCase().split(/[-_]/)[0] ?? ''] ?? 'latin';
}

// Ordered so the narrower tests come first: Japanese is full of Han
// characters and would otherwise read as Chinese.
const RANGES: readonly (readonly [Script, RegExp])[] = [
  ['hangul', /[ᄀ-ᇿ㄰-㆏가-힯]/u],
  ['kana', /[぀-ゟ゠-ヿ]/u],
  ['han', /[㐀-䶿一-鿿豈-﫿]/u],
  ['arabic', /[؀-ۿݐ-ݿﭐ-﷿ﹰ-﻿]/u],
  ['hebrew', /[֐-׿]/u],
  ['cyrillic', /[Ѐ-ԯ]/u],
  ['greek', /[Ͱ-Ͽἀ-῿]/u],
  ['armenian', /[԰-֏]/u],
  ['georgian', /[Ⴀ-ჿᲐ-Ჿ]/u],
  ['devanagari', /[ऀ-ॿ]/u],
  ['bengali', /[ঀ-৿]/u],
  ['gurmukhi', /[਀-੿]/u],
  ['gujarati', /[઀-૿]/u],
  ['tamil', /[஀-௿]/u],
  ['telugu', /[ఀ-౿]/u],
  ['kannada', /[ಀ-೿]/u],
  ['malayalam', /[ഀ-ൿ]/u],
  ['sinhala', /[඀-෿]/u],
  ['thai', /[฀-๿]/u],
  ['lao', /[຀-໿]/u],
  ['khmer', /[ក-៿]/u],
  ['myanmar', /[က-႟]/u],
  ['ethiopic', /[ሀ-፿]/u],
  ['latin', /\p{Script=Latin}/u],
];

/** The writing system a selection is in. */
export function scriptOf(text: string): Script {
  for (const [script, pattern] of RANGES) {
    if (pattern.test(text)) return script;
  }
  return 'unknown';
}

/**
 * The notebook a selected word should go to.
 *
 * The learner's own choice wins unless the word could not possibly be in
 * its language — an English notebook cannot hold a Persian word, and
 * filing it there silently is how somebody loses it. When exactly one other
 * notebook takes that script, that is the answer; when none does, there is
 * no answer and the caller says so.
 */
export function notebookFor<T extends { id: string; targetLang: string }>(
  text: string,
  chosen: T | undefined,
  all: readonly T[],
): { notebook: T; moved: boolean } | { notebook: undefined; moved: false } {
  const script = scriptOf(text);
  if (script === 'unknown' || chosen === undefined) {
    return chosen === undefined ? { notebook: undefined, moved: false } : { notebook: chosen, moved: false };
  }
  if (scriptOfLanguage(chosen.targetLang) === script) return { notebook: chosen, moved: false };

  const fitting = all.filter((notebook) => scriptOfLanguage(notebook.targetLang) === script);
  const only = fitting.length === 1 ? fitting[0] : undefined;
  if (only !== undefined) return { notebook: only, moved: true };
  // Several could take it, or none could. Neither is this module's to
  // decide: the popup's picker and the app both do it better.
  return fitting.length === 0 ? { notebook: undefined, moved: false } : { notebook: chosen, moved: false };
}
