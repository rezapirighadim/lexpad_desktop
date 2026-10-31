// Copied from lexpad_front apps/web/src/features/player/model/title.ts; keep them identical.
/**
 * What a video is called, read from its file name: "The.Terrace.S01E03.
 * Ephemeral.1080p.mkv" is "The Terrace", season 1, episode 3, "Ephemeral".
 * Local and plain: the name is all there is (no online lookup, phase 1).
 */

export interface VideoTitle {
  /** The show or film: "The Terrace". */
  title: string;
  season?: number;
  episode?: number;
  /** The episode's own name when the file has one: "Ephemeral". */
  episodeTitle?: string;
}

/** Tags release files carry that are not part of a title. */
const NOISE =
  /^(?:\d{3,4}p|[0-9]{3,4}x[0-9]{3,4}|x26[45]|h\.?26[45]|hevc|avc|av1|vp9|xvid|divx|aac(?:2\.0)?|ac3|eac3|dts(?:-hd)?|truehd|atmos|ddp?5\.1|5\.1|7\.1|2\.0|10bit|8bit|hdr|hdr10|dv|sdr|bluray|blu-ray|bdrip|brrip|web-?dl|webrip|web|hdtv|dvdrip|dvd|remux|proper|repack|extended|unrated|imax|internal|multi|subbed|dubbed|nf|amzn|dsnp|hmax|atvp)$/i;

const EXT = /\.[a-z0-9]{2,4}$/i;

function words(text: string): string[] {
  return text
    .replace(/[._]+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .split(' ')
    .filter((w) => w !== '');
}

/** Drops release tags, bracketed groups and a trailing year from a run of words. */
function clean(parts: string[]): string {
  const kept: string[] = [];
  for (const w of parts) {
    if (NOISE.test(w)) break;
    kept.push(w);
  }
  // A year right after the title ends it ("Tears of Steel 2012").
  const out = kept.join(' ').replace(/\s*[([]?(?:19|20)\d{2}[)\]]?$/, '');
  return out.replace(/\s*-\s*$/, '').trim();
}

export function parseTitle(fileName: string): VideoTitle {
  const base = fileName
    .replace(EXT, '')
    .replace(/\[[^\]]*\]|\{[^}]*\}/g, ' ')
    .trim();
  const patterns: RegExp[] = [
    /^(.*?)[\s._-]+s(\d{1,2})[\s._-]*e(\d{1,3})(?:[\s._-]*e\d{1,3})?(.*)$/i,
    /^(.*?)[\s._-]+(\d{1,2})x(\d{1,3})(.*)$/i,
    /^(.*?)[\s._-]+season[\s._-]*(\d{1,2})[\s._-]+episode[\s._-]*(\d{1,3})(.*)$/i,
  ];
  for (const p of patterns) {
    const m = p.exec(base);
    if (m !== null) {
      const title = clean(words(m[1] ?? ''));
      const rest = clean(words((m[4] ?? '').replace(/^[\s._-]+/, '')));
      return {
        title: title === '' ? clean(words(base)) : title,
        season: Number(m[2]),
        episode: Number(m[3]),
        ...(rest !== '' ? { episodeTitle: rest } : {}),
      };
    }
  }
  const ep = /^(.*?)[\s._-]+(?:episode|ep)[\s._-]*(\d{1,3})(.*)$/i.exec(base);
  if (ep !== null) {
    const rest = clean(words((ep[3] ?? '').replace(/^[\s._-]+/, '')));
    return {
      title: clean(words(ep[1] ?? '')),
      episode: Number(ep[2]),
      ...(rest !== '' ? { episodeTitle: rest } : {}),
    };
  }
  const title = clean(words(base));
  return { title: title === '' ? fileName : title };
}

/** "S1 · E3": the episode chip's code, or undefined for a film. */
export function episodeCode(t: VideoTitle): string | undefined {
  if (t.episode === undefined) return undefined;
  return t.season === undefined ? `E${t.episode}` : `S${t.season} · E${t.episode}`;
}

/** "S1 E3", the compact form in "Seen in The Terrace · S1 E3 · 26:58". */
export function episodeShort(t: VideoTitle): string | undefined {
  if (t.episode === undefined) return undefined;
  return t.season === undefined ? `E${t.episode}` : `S${t.season} E${t.episode}`;
}
