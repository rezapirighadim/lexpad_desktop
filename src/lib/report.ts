/**
 * Report on the card's AI-written meaning. App stores require a way to
 * report what a generative model wrote; the card is too small for the
 * report form, so its Report button hands the card's text to the core,
 * which opens the web app's `/report-ai` page with it in the fragment
 * (Lexpad's window, or the browser). The same payload as the browser
 * extension's card (lexpad_extension `src/lib/report.ts`), read by the web
 * app (lexpad_front `features/report/report.ts`).
 */
import type { Card } from './types.js';

/** The longest text handed over; the core refuses a longer payload. */
const MAX_SNAPSHOT = 1500;

/** The card's text, as shown: the word, then up to three senses with their first example. */
export function cardText(headword: string, card: Card): string {
  const head = [
    card.correction && card.correction !== headword ? card.correction : headword,
    card.pos,
    card.level,
  ]
    .filter((part): part is string => typeof part === 'string' && part !== '')
    .join(' · ');
  const lines = [head];
  for (const [index, sense] of (card.meanings ?? []).slice(0, 3).entries()) {
    lines.push(`${String(index + 1)}. ${sense.meaning}`);
    if (sense.gloss) lines.push(sense.gloss);
    const example = sense.examples?.[0];
    if (example)
      lines.push(
        example.translation ? `“${example.sentence}” — ${example.translation}` : `“${example.sentence}”`,
      );
  }
  const text = lines.join('\n');
  const chars = [...text];
  return chars.length <= MAX_SNAPSHOT ? text : chars.slice(0, MAX_SNAPSHOT).join('');
}

/** The payload: base64url of the UTF-8 JSON the web page decodes. */
export function reportFragment(headword: string, card: Card): string {
  const payload = JSON.stringify({ surface: 'card', snapshot: cardText(headword, card), headword });
  let binary = '';
  for (const byte of new TextEncoder().encode(payload)) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}
