// Copied from lexpad_extension src/lib/text.ts; keep the two the same (see CLAUDE.md).
/**
 * Text rules with no DOM in them, so they can be tested plainly: what counts
 * as a word worth adding, and which sentence it was met in.
 */

/** The longest selection the bubble offers to add. */
export const MAX_WORDS = 4;
const MAX_CHARS = 60;

/** Trims a selection and says whether it looks like a word or short phrase. */
export function asHeadword(selection: string): string | undefined {
  const text = selection
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/^[\s"'“”‘’(\[{«»]+|[\s"'“”‘’)\]},.;:!?«»]+$/g, '');
  if (text === '' || text.length > MAX_CHARS) return undefined;
  if (!/\p{L}/u.test(text)) return undefined;
  if (text.split(' ').length > MAX_WORDS) return undefined;
  return text;
}

/** The sentence in `block` that contains `word`, trimmed to a readable length. */
export function sentenceAround(block: string, word: string, maxChars = 220): string {
  const text = block.replace(/\s+/g, ' ').trim();
  if (text === '') return '';
  const at = text.toLowerCase().indexOf(word.toLowerCase());
  if (at < 0) return clip(text, maxChars);
  // Sentence boundaries in Latin, Persian and CJK punctuation.
  const enders = /[.!?؟。！？]/;
  let start = at;
  while (start > 0 && !enders.test(text[start - 1] ?? '')) start -= 1;
  let end = at + word.length;
  while (end < text.length && !enders.test(text[end] ?? '')) end += 1;
  if (end < text.length) end += 1;
  const sentence = text.slice(start, end).trim();
  return clip(sentence, maxChars, at - start);
}

/** Shortens text to `max` characters, keeping the part around `focus` and marking the cut. */
function clip(text: string, max: number, focus = 0): string {
  if (text.length <= max) return text;
  const half = Math.floor(max / 2);
  const from = Math.max(0, Math.min(focus - half, text.length - max));
  const out = text.slice(from, from + max).trim();
  return `${from > 0 ? '…' : ''}${out}${from + max < text.length ? '…' : ''}`;
}

/** A hint for the model: the sentence the word was met in, within the API's limit. */
export function contextHint(sentence: string, max = 200): string | undefined {
  const lead = 'Seen in: ';
  // Two quote marks, and the clip may add an ellipsis on each side.
  const room = max - lead.length - 4;
  if (sentence.trim() === '' || room <= 20) return undefined;
  return `${lead}“${clip(sentence.trim(), room)}”`;
}

/**
 * The address a word is filed under: the page's origin and path, nothing
 * else. Query strings and fragments often carry what was never meant to be
 * kept (sign-in codes, reset links, signed download addresses, session ids),
 * and this address ends up in the notebook, its exports and any notebook that
 * is shared. User names and passwords in the address go too. Anything that is
 * not an http(s) page gives an empty address rather than a guess.
 */
export function pageAddress(href: string): string {
  let url: URL;
  try {
    url = new URL(href);
  } catch {
    return '';
  }
  if (url.protocol !== 'https:' && url.protocol !== 'http:') return '';
  return `${url.origin}${url.pathname}`;
}
