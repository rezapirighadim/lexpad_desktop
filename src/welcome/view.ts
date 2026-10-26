/**
 * The Start window (spec uiux/specs/2026-10-26-desktop-first-run.md, round 2
 * direction A with round 3's film frame): one real picture of the Player — a
 * word clicked in a subtitle, its card, Add to Lexpad — and the two things to
 * do now: Open a video… (no account) and Continue with Google / Apple (the
 * notebook and practice). No question, no steps, no Skip: what the learner
 * does tells Lexpad what they came for (the core sets `focus`).
 *
 * Drawn with the DOM and `textContent`; nothing from outside is parsed.
 * Every string is in `COPY` (§5.2), so the page has one table to translate.
 */

export interface StartState {
  os: 'macos' | 'windows' | 'linux';
  signedIn: boolean;
}

/** What "Open a video…" ended in: playing, cancelled, or nothing playable. */
export type OpenResult = 'opened' | 'cancelled' | 'subOnly' | 'notVideo';

export interface StartBackend {
  state(): Promise<StartState>;
  openVideo(): Promise<OpenResult>;
  connect(provider?: 'google' | 'apple'): Promise<void>;
  cancelConnect(): Promise<void>;
  /** Signing in finished: Lexpad's window opens and this one goes. */
  signedIn(): Promise<void>;
  close(): Promise<void>;
  /** Files dragged over the window (the drop itself goes to the core). */
  onDrag(handler: (kind: 'enter' | 'leave' | 'drop') => void): void;
  /** The core refused a drop: subtitles alone, or not a video. */
  onRefused(handler: (why: 'subOnly' | 'notVideo') => void): void;
}

/** The copy (§5.2). */
export const COPY = {
  h1a: 'Watch. Click a word.',
  h1b: 'Keep it.',
  lead: 'Play your own videos with subtitles. Words you keep wait in your notebook, ready to practise.',
  open: 'Open a video…',
  dropHint: 'or drop a video file on this window',
  sep: 'Your notebook and practice',
  google: 'Continue with Google',
  apple: 'Continue with Apple',
  fine: 'Opens your browser. Lexpad never sees your password.',
  other: 'Other ways to sign in',
  waiting: 'Waiting for your browser…',
  waitingNote: 'Finish in the browser tab that just opened.',
  cancel: 'Cancel',
  offline: 'You seem to be offline. Videos still play; try signing in again in a moment.',
  timeout: 'The browser did not answer in time. Try again.',
  failed: 'Could not sign in. Try again.',
  cancelled: 'Nothing was signed in.',
  dropTitle: 'Drop to play',
  dropNote: 'Subtitle files with the same name are added too.',
  dropSubOnly: 'Drop the video too',
  dropSubOnlyNote: 'A subtitle needs its video.',
  dropNotVideo: "This isn't a video",
  dropNotVideoNote: 'Lexpad plays video files.',
} as const;

/**
 * The film in the picture (round 3, owner's pick 26 Oct 2026): Tears of Steel
 * at 0:26, "Look Celia, we have to follow our passions;", the word "passions".
 * The loop and its still are recorded from the real Player by lexpad_front's
 * `e2e/player/start-loop.spec.ts` (its config names the film, time and word);
 * when the scene changes, change these lines with it. The licence (CC BY 3.0)
 * asks for the credit, so it is always on screen and readable.
 */
export const FILM = {
  credit: 'Film: Tears of Steel (CC) Blender Foundation | mango.blender.org',
  alt:
    'The Lexpad player: two people talk on a bridge in Amsterdam. The word passions, clicked in the ' +
    'English subtitle, opens its card with its meaning and Add to Lexpad.',
} as const;

/** The loop plays this many times, then rests on the card (§11). */
export const LOOP_PLAYS = 2;
/** How long a refused drop's words stay (§5.4). */
export const REFUSED_MS = 2500;

export interface Media {
  loop: string;
  still: string;
}

const SVG_NS = 'http://www.w3.org/2000/svg';

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className = '',
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function svg(paths: [string | null, string][], size: number, stroke = false): SVGSVGElement {
  const s = document.createElementNS(SVG_NS, 'svg');
  s.setAttribute('viewBox', '0 0 24 24');
  s.setAttribute('width', String(size));
  s.setAttribute('height', String(size));
  s.setAttribute('aria-hidden', 'true');
  if (stroke) {
    s.setAttribute('fill', 'none');
    s.setAttribute('stroke', 'currentColor');
    s.setAttribute('stroke-width', '1.9');
    s.setAttribute('stroke-linecap', 'round');
    s.setAttribute('stroke-linejoin', 'round');
  }
  for (const [fill, d] of paths) {
    const p = document.createElementNS(SVG_NS, 'path');
    if (fill !== null) p.setAttribute('fill', fill);
    p.setAttribute('d', d);
    s.append(p);
  }
  return s;
}

const FOLDER: [null, string][] = [
  [null, 'M3.5 7.5a2 2 0 0 1 2-2h3.6l2 2.2h7.4a2 2 0 0 1 2 2v7.8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z'],
];
const FILM_ICON: [null, string][] = [
  [null, 'M6 4h12a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z'],
  [null, 'M8 4v16M16 4v16M4 9h4M4 15h4M16 9h4M16 15h4'],
];
/** Google's "G" and Apple's mark: the real brand marks (owner rule "real brand icons"). */
const GOOGLE: [string, string][] = [
  [
    '#4285F4',
    'M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z',
  ],
  [
    '#34A853',
    'M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z',
  ],
  [
    '#FBBC05',
    'M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z',
  ],
  [
    '#EA4335',
    'M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z',
  ],
];
const APPLE: [string, string][] = [
  [
    'currentColor',
    'M16.37 12.6c-.02-2.2 1.8-3.26 1.88-3.31-1.03-1.5-2.62-1.7-3.18-1.73-1.35-.14-2.64.8-3.33.8-.69 0-1.74-.78-2.87-.76-1.47.02-2.83.86-3.59 2.18-1.53 2.66-.39 6.6 1.1 8.76.73 1.06 1.6 2.24 2.73 2.2 1.1-.04 1.51-.71 2.84-.71 1.32 0 1.7.71 2.86.69 1.18-.02 1.93-1.07 2.65-2.13.84-1.22 1.18-2.41 1.2-2.47-.03-.01-2.3-.88-2.32-3.52zM14.18 6.13c.6-.74 1.01-1.75.9-2.77-.87.04-1.93.58-2.55 1.31-.56.65-1.05 1.69-.92 2.69.97.08 1.97-.49 2.57-1.23z',
  ],
];

type Provider = 'google' | 'apple';
type Line = { kind: 'fine' } | { kind: 'waiting' } | { kind: 'bad'; text: string };

/** The words a failed sign-in shows, by the core's reason. */
export function failureText(reason: string, online: boolean): string {
  if (reason === 'cancelled') return COPY.cancelled;
  if (reason === 'offline' || !online) return COPY.offline;
  if (reason === 'timeout') return COPY.timeout;
  return COPY.failed;
}

export class StartView {
  private state: StartState = { os: 'macos', signedIn: false };
  private waiting: Provider | 'other' | null = null;
  private line: Line = { kind: 'fine' };
  private plays = 0;
  private refusedTimer = 0;
  private readonly reduced: boolean;
  private els!: {
    primary: HTMLButtonElement;
    google: HTMLButtonElement;
    apple: HTMLButtonElement;
    foot: HTMLElement;
    overlay: HTMLElement;
    overlayTitle: HTMLElement;
    overlayNote: HTMLElement;
  };

  constructor(
    private readonly root: HTMLElement,
    private readonly backend: StartBackend,
    private readonly media: Media,
    private readonly env: { online: () => boolean; reducedMotion: () => boolean } = {
      online: () => navigator.onLine,
      reducedMotion: () => window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    },
  ) {
    this.reduced = env.reducedMotion();
    document.addEventListener('keydown', (e) => {
      this.key(e);
    });
  }

  async start(): Promise<void> {
    try {
      this.state = await this.backend.state();
    } catch {
      // Drawn as signed out; the buttons still work.
    }
    document.documentElement.dataset.platform = this.state.os;
    this.render();
    this.backend.onDrag((kind) => {
      this.drag(kind);
    });
    this.backend.onRefused((why) => {
      this.refused(why);
    });
    // The first focus is the main action, with no ring until a key is pressed (§8).
    this.els.primary.focus({ preventScroll: true });
  }

  /* ------------------------------------------------------------ keys */

  private key(e: KeyboardEvent): void {
    // Rings show only once the keyboard is in use (§8).
    document.documentElement.dataset.keyboard = '';
    const mod = this.state.os === 'macos' ? e.metaKey : e.ctrlKey;
    const k = e.key.toLowerCase();
    if (mod && !e.shiftKey && !e.altKey && k === 'o') {
      e.preventDefault();
      void this.openVideo();
    } else if (mod && !e.shiftKey && !e.altKey && k === 'w') {
      e.preventDefault();
      void this.backend.close();
    }
    // Escape does nothing: a first window must not vanish on a stray key.
  }

  /* ------------------------------------------------------------ drawing */

  private render(): void {
    this.root.replaceChildren();
    this.root.className = 'start';
    const bar = el('div', 'titlebar');
    bar.dataset.tauriDragRegion = '';
    this.root.append(bar, this.picture(), this.main(), this.overlay());
    this.drawFoot();
  }

  private picture(): HTMLElement {
    const pane = el('div', 'pane');
    pane.setAttribute('role', 'img');
    pane.setAttribute('aria-label', FILM.alt);
    const shot = el('div', 'shot');
    const still = el('img', 'still');
    still.src = this.media.still;
    still.alt = '';
    shot.append(still);
    if (!this.reduced) {
      const video = el('video', 'loop');
      video.muted = true;
      video.playsInline = true;
      video.autoplay = true;
      video.preload = 'auto';
      video.setAttribute('aria-hidden', 'true');
      video.src = this.media.loop;
      // Twice, then it rests on the card (round 2 §11).
      video.addEventListener('ended', () => {
        this.plays += 1;
        if (this.plays < LOOP_PLAYS) {
          video.currentTime = 0;
          void video.play().catch(() => undefined);
        } else {
          shot.classList.add('rested');
        }
      });
      video.addEventListener('error', () => {
        shot.classList.add('rested');
      });
      shot.append(video);
    } else {
      shot.classList.add('rested');
    }
    // The licence's credit: read aloud too (not hidden), since it is a fact.
    pane.append(shot, el('p', 'credit', FILM.credit));
    return pane;
  }

  private main(): HTMLElement {
    const main = el('section', 'main');
    const logo = el('div', 'logo');
    logo.setAttribute('role', 'img');
    logo.setAttribute('aria-label', 'Lexpad');
    const h1 = el('h1');
    h1.append(document.createTextNode(COPY.h1a), el('br'), document.createTextNode(COPY.h1b));
    const lead = el('p', 'lead', COPY.lead);

    const primary = el('button', 'btn primary');
    primary.type = 'button';
    const mac = this.state.os === 'macos';
    primary.setAttribute('aria-keyshortcuts', mac ? 'Meta+O' : 'Control+O');
    const keys = el('span', 'keys');
    keys.setAttribute('aria-hidden', 'true');
    keys.append(el('kbd', '', mac ? '⌘' : 'Ctrl'), el('kbd', '', 'O'));
    primary.append(svg(FOLDER, 18, true), el('span', 'label', COPY.open), keys);
    primary.addEventListener('click', () => void this.openVideo());

    const sep = el('div', 'sep', COPY.sep);
    const google = this.provider('google');
    const apple = this.provider('apple');
    const foot = el('div', 'foot');
    main.append(logo, h1, lead, primary, el('p', 'hint', COPY.dropHint), sep, google, apple, foot);
    this.els = {
      primary,
      google,
      apple,
      foot,
      overlay: el('div'),
      overlayTitle: el('div'),
      overlayNote: el('div'),
    };
    return main;
  }

  private provider(p: Provider): HTMLButtonElement {
    const b = el('button', `btn prov ${p}`);
    b.type = 'button';
    b.dataset.provider = p;
    b.addEventListener('click', () => void this.connect(p));
    this.drawProvider(b, p);
    return b;
  }

  private drawProvider(b: HTMLButtonElement, p: Provider): void {
    const busy = this.waiting === p;
    b.replaceChildren(
      busy ? el('span', 'spinner') : svg(p === 'google' ? GOOGLE : APPLE, 18),
      el('span', 'label', busy ? COPY.waiting : p === 'google' ? COPY.google : COPY.apple),
    );
    b.disabled = this.waiting !== null && !busy;
    b.setAttribute('aria-busy', busy ? 'true' : 'false');
  }

  /** The line under the buttons: the fine print, the waiting note with Cancel, or a failure (§5.3). */
  private drawFoot(): void {
    const foot = this.els.foot;
    foot.replaceChildren();
    if (this.line.kind === 'waiting') {
      const p = el('p', 'fine', COPY.waitingNote);
      p.setAttribute('role', 'status');
      const cancel = el('button', 'link', COPY.cancel);
      cancel.type = 'button';
      cancel.addEventListener('click', () => void this.backend.cancelConnect());
      foot.append(p, cancel);
      cancel.focus({ preventScroll: true });
      return;
    }
    if (this.line.kind === 'bad') {
      const p = el('p', 'fine bad', this.line.text);
      p.setAttribute('role', 'status');
      foot.append(p);
    } else {
      const p = el('p', 'fine', COPY.fine);
      p.setAttribute('role', 'status');
      foot.append(p);
    }
    const other = el('button', 'link', COPY.other);
    other.type = 'button';
    other.addEventListener('click', () => void this.connect(undefined));
    foot.append(other);
  }

  private overlay(): HTMLElement {
    const box = el('div', 'drop');
    box.setAttribute('aria-hidden', 'true');
    const tile = el('div', 'tile');
    tile.append(svg(FILM_ICON, 28, true));
    const title = el('div', 'drop-title', COPY.dropTitle);
    const note = el('div', 'drop-note', COPY.dropNote);
    box.append(tile, title, note);
    // Drawn after main(), which made `els`.
    this.els.overlay = box;
    this.els.overlayTitle = title;
    this.els.overlayNote = note;
    return box;
  }

  /* ------------------------------------------------------------ actions */

  private async openVideo(): Promise<void> {
    let result: OpenResult;
    try {
      result = await this.backend.openVideo();
    } catch {
      return;
    }
    if (result === 'subOnly' || result === 'notVideo') this.refused(result);
    // "opened": the core closed this window; "cancelled": stay (§4).
  }

  private async connect(provider: Provider | undefined): Promise<void> {
    if (this.waiting !== null) return;
    if (!this.env.online()) {
      this.line = { kind: 'bad', text: COPY.offline };
      this.drawFoot();
      return;
    }
    this.waiting = provider ?? 'other';
    this.line = { kind: 'waiting' };
    this.redrawProviders();
    this.drawFoot();
    try {
      await this.backend.connect(provider);
      await this.backend.signedIn();
    } catch (cause) {
      const reason = typeof cause === 'string' ? cause : '';
      this.waiting = null;
      this.line = { kind: 'bad', text: failureText(reason, this.env.online()) };
      this.redrawProviders();
      this.drawFoot();
    }
  }

  private redrawProviders(): void {
    this.drawProvider(this.els.google, 'google');
    this.drawProvider(this.els.apple, 'apple');
  }

  /* ------------------------------------------------------------ dropping */

  private drag(kind: 'enter' | 'leave' | 'drop'): void {
    if (kind === 'enter') {
      window.clearTimeout(this.refusedTimer);
      this.els.overlayTitle.textContent = COPY.dropTitle;
      this.els.overlayNote.textContent = COPY.dropNote;
      this.els.overlay.classList.remove('refused');
      this.els.overlay.classList.add('on');
    } else if (!this.els.overlay.classList.contains('refused')) {
      this.els.overlay.classList.remove('on');
    }
  }

  private refused(why: 'subOnly' | 'notVideo'): void {
    const o = this.els.overlay;
    this.els.overlayTitle.textContent = why === 'subOnly' ? COPY.dropSubOnly : COPY.dropNotVideo;
    this.els.overlayNote.textContent = why === 'subOnly' ? COPY.dropSubOnlyNote : COPY.dropNotVideoNote;
    o.classList.add('on', 'refused');
    window.clearTimeout(this.refusedTimer);
    this.refusedTimer = window.setTimeout(() => {
      o.classList.remove('on', 'refused');
    }, REFUSED_MS);
  }
}
