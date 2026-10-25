/**
 * The first launch (spec uiux/specs/2026-10-25-video-player.md §24.2, mock-ups
 * 30-welcome-choice and 31-welcome-sign-in): "What would you like to do
 * first?" with three big cards — one click moves on, keys 1 2 3, arrows and
 * Enter, Escape is Both — then a soft sign-in for that path. Watch and Both
 * may skip it; Learn offers the player instead. No trial, no Pro and no
 * numbers on either step.
 *
 * Drawn with the DOM and `textContent`; nothing from outside is parsed.
 * The pictures on the cards are drawn illustrations (a film frame, two
 * cards), not anyone's data; the word in them is a named constant, as in
 * the web app's introduction (`features/intro/story/sample.ts`).
 */

export type Focus = 'watch' | 'learn' | 'both';

export interface WelcomeState {
  os: 'macos' | 'windows' | 'linux';
  signedIn: boolean;
  focus: Focus | null;
}

export interface WelcomeBackend {
  state(): Promise<WelcomeState>;
  choose(focus: Focus): Promise<void>;
  connect(provider?: 'google' | 'apple'): Promise<void>;
  cancelConnect(): Promise<void>;
  finish(target: 'player' | 'learn'): Promise<void>;
}

/** The word the cards' pictures show: an illustration, saved by nobody. */
export const SAMPLE_WORD = 'ephemeral';

interface Card {
  focus: Focus;
  key: string;
  title: string;
  text: string;
}

const CARDS: readonly Card[] = [
  {
    focus: 'watch',
    key: '1',
    title: 'Watch videos with subtitles',
    text: 'Play your own video files. Click a word to keep it.',
  },
  {
    focus: 'learn',
    key: '2',
    title: 'Learn and practise my words',
    text: 'Your notebook and a few minutes of practice a day.',
  },
  {
    focus: 'both',
    key: '3',
    title: 'Both',
    text: 'Watch, keep the words you meet, and practise them later.',
  },
];

const SIGN_IN: Record<Focus, { title: string; lead: string }> = {
  watch: {
    title: 'Sign in to keep the words you click',
    lead: 'Videos play without an account. Signed in, every word you click goes into your notebook, and onto your phone too.',
  },
  both: {
    title: 'Sign in to keep your words',
    lead: 'Watch, save words, practise them here and on your phone.',
  },
  learn: {
    title: 'Sign in to start your notebook',
    lead: 'Your words, your practice, on this computer and your phone.',
  },
};

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

/** A small line icon (24×24, stroke 1.75), from fixed path data. */
function icon(paths: string[], size = 18, fill = false): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', String(size));
  svg.setAttribute('height', String(size));
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('fill', fill ? 'currentColor' : 'none');
  svg.setAttribute('stroke', fill ? 'none' : 'currentColor');
  svg.setAttribute('stroke-width', '1.75');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  for (const d of paths) {
    const p = document.createElementNS(SVG_NS, 'path');
    p.setAttribute('d', d);
    svg.append(p);
  }
  return svg;
}

const ARROW = ['M5 12h14M13 6l6 6-6 6'];
const BACK = ['M15 6l-6 6 6 6'];
const CHECK = ['M5 12.5l4.5 4.5L19 7.5'];
const WATCH = [
  'M6 5h12a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V8a3 3 0 0 1 3-3z',
  'M10.5 9.5v5l4.2-2.5z',
];
const MONITOR = [
  'M5 4.5h14a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z',
  'M9 20h6M12 16.5V20',
];
const PHONE = ['M9 3h6a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z', 'M11 17.5h2'];
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
const APPLE =
  'M16.37 12.6c-.02-2.2 1.8-3.26 1.88-3.31-1.03-1.5-2.62-1.7-3.18-1.73-1.35-.14-2.64.8-3.33.8-.69 0-1.74-.78-2.87-.76-1.47.02-2.83.86-3.59 2.18-1.53 2.66-.39 6.6 1.1 8.76.73 1.06 1.6 2.24 2.73 2.2 1.1-.04 1.51-.71 2.84-.71 1.32 0 1.7.71 2.86.69 1.18-.02 1.93-1.07 2.65-2.13.84-1.22 1.18-2.41 1.2-2.47-.03-.01-2.3-.88-2.32-3.52zM14.18 6.13c.6-.74 1.01-1.75.9-2.77-.87.04-1.93.58-2.55 1.31-.56.65-1.05 1.69-.92 2.69.97.08 1.97-.49 2.57-1.23z';

function googleMark(): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', '20');
  svg.setAttribute('height', '20');
  svg.setAttribute('aria-hidden', 'true');
  for (const [fill, d] of GOOGLE) {
    const p = document.createElementNS(SVG_NS, 'path');
    p.setAttribute('fill', fill);
    p.setAttribute('d', d);
    svg.append(p);
  }
  return svg;
}

function appleMark(): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('width', '20');
  svg.setAttribute('height', '20');
  svg.setAttribute('aria-hidden', 'true');
  const p = document.createElementNS(SVG_NS, 'path');
  p.setAttribute('fill', 'currentColor');
  p.setAttribute('d', APPLE);
  svg.append(p);
  return svg;
}

function stepDots(step: 1 | 2): HTMLElement {
  const wrap = el('div', 'stepno');
  const dots = el('span', 'dots');
  dots.setAttribute('aria-hidden', 'true');
  for (const n of [1, 2]) dots.append(el('i', n === step ? 'on' : ''));
  wrap.append(dots, document.createTextNode(`${String(step)} of 2`));
  return wrap;
}

/** The card's picture: a drawn film frame, two drawn cards, or both. */
function art(focus: Focus): HTMLElement {
  const box = el('div', `art art-${focus}`);
  box.setAttribute('aria-hidden', 'true');
  if (focus === 'learn') {
    box.append(miniCard('back'), miniCard('front'));
    const grades = el('div', 'grades');
    grades.append(el('span', 'again', 'Again'), el('span', 'knew', 'Knew it'));
    box.append(grades);
    return box;
  }
  const line = el('div', focus === 'watch' ? 'cue' : 'cue cue-left');
  if (focus === 'watch') line.append(document.createTextNode('it was '));
  line.append(el('mark', '', SAMPLE_WORD));
  if (focus === 'watch') line.append(document.createTextNode('.'));
  box.append(line);
  if (focus === 'watch') {
    const bar = el('div', 'bar');
    bar.append(el('i'));
    box.append(bar);
  } else {
    const card = miniCard('float');
    box.append(card);
    const later = el('div', 'later');
    later.append(icon(CHECK, 12), document.createTextNode('Practise later'));
    box.append(later);
  }
  return box;
}

function miniCard(kind: 'back' | 'front' | 'float'): HTMLElement {
  const card = el('div', `mini mini-${kind}`);
  card.append(el('div', 'mini-word', kind === 'back' ? '·····' : SAMPLE_WORD));
  card.append(el('div', 'mini-line'));
  return card;
}

export class WelcomeView {
  private state: WelcomeState = { os: 'macos', signedIn: false, focus: null };
  private focus: Focus | null = null;
  private waiting = false;
  private note = '';
  private readonly onKey = (e: KeyboardEvent): void => {
    this.key(e);
  };

  constructor(
    private readonly root: HTMLElement,
    private readonly backend: WelcomeBackend,
  ) {
    document.addEventListener('keydown', this.onKey);
  }

  async start(): Promise<void> {
    try {
      this.state = await this.backend.state();
    } catch {
      // Drawn as signed out on this computer's own guess.
    }
    document.documentElement.dataset.platform = this.state.os;
    this.renderChoice();
  }

  private key(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault();
      if (this.focus === null) void this.pick('both');
      else this.back();
      return;
    }
    if (this.focus !== null) return;
    const n = ['1', '2', '3'].indexOf(e.key);
    if (n >= 0 && !e.metaKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      void this.pick(CARDS[n]?.focus ?? 'both');
      return;
    }
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft' || e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      const cards = [...this.root.querySelectorAll<HTMLButtonElement>('.choice')];
      const at = cards.indexOf(document.activeElement as HTMLButtonElement);
      const step = e.key === 'ArrowRight' || e.key === 'ArrowDown' ? 1 : -1;
      const next = cards[(at + step + cards.length) % cards.length];
      if (next !== undefined) {
        e.preventDefault();
        next.focus();
      }
    }
  }

  private async pick(focus: Focus): Promise<void> {
    this.focus = focus;
    try {
      await this.backend.choose(focus);
    } catch {
      // The choice still shapes this session; settings.json missed it.
    }
    // Already signed in (a reinstall that kept its session): straight on.
    if (this.state.signedIn) {
      await this.finishFor(focus);
      return;
    }
    this.note = '';
    this.renderSignIn();
  }

  private back(): void {
    if (this.waiting) return;
    this.focus = null;
    this.renderChoice();
  }

  private async finishFor(focus: Focus): Promise<void> {
    await this.backend.finish(focus === 'watch' ? 'player' : 'learn');
  }

  private renderChoice(): void {
    this.root.replaceChildren();
    this.root.className = 'wel step1';
    this.root.append(stepDots(1));
    const head = el('div', 'head');
    const logo = el('div', 'logo');
    logo.setAttribute('role', 'img');
    logo.setAttribute('aria-label', 'Lexpad');
    head.append(logo, el('h1', '', 'What would you like to do first?'));
    head.append(
      el('p', 'lead', 'You can do both any time. This only picks where Lexpad opens and which tips you see.'),
    );
    this.root.append(head);
    const group = el('div', 'choices');
    group.setAttribute('role', 'radiogroup');
    group.setAttribute('aria-label', 'What would you like to do first?');
    CARDS.forEach((c, i) => {
      const b = el('button', 'choice');
      b.type = 'button';
      b.setAttribute('role', 'radio');
      b.setAttribute('aria-checked', 'false');
      b.setAttribute('aria-keyshortcuts', c.key);
      b.dataset.focus = c.focus;
      b.append(art(c.focus), el('span', 't', c.title), el('span', 'd', c.text));
      const go = el('span', 'go');
      go.append(icon(ARROW, 16));
      const kbd = el('kbd', '', c.key);
      b.append(go, kbd);
      b.addEventListener('click', () => void this.pick(c.focus));
      group.append(b);
      if (i === 2) window.setTimeout(() => b.focus({ preventScroll: true }), 0);
    });
    this.root.append(group);
    const foot = el('p', 'foot');
    foot.append(
      document.createTextNode('Not sure? Choose '),
      el('b', '', 'Both'),
      document.createTextNode('. You can change it later in Settings.'),
    );
    this.root.append(foot);
  }

  private renderSignIn(): void {
    const focus = this.focus ?? 'both';
    const copy = SIGN_IN[focus];
    this.root.replaceChildren();
    this.root.className = 'wel step2';
    const form = el('section', 'form');
    const back = el('button', 'back');
    back.type = 'button';
    back.setAttribute('aria-label', 'Back');
    back.append(icon(BACK, 20));
    back.addEventListener('click', () => {
      this.back();
    });
    const chosen = el('div', 'chosen');
    chosen.append(
      icon(WATCH, 15),
      document.createTextNode(CARDS.find((c) => c.focus === focus)?.title ?? ''),
    );
    const change = el('button', 'change', 'Change');
    change.type = 'button';
    change.addEventListener('click', () => {
      this.back();
    });
    chosen.append(change);
    form.append(back, chosen, el('h1', '', copy.title), el('p', 'lead', copy.lead));
    const buttons = el('div', 'buttons');
    const google = el('button', 'btn google');
    google.type = 'button';
    google.append(
      googleMark(),
      document.createTextNode(this.waiting ? 'Waiting for your browser…' : 'Continue with Google'),
    );
    google.disabled = this.waiting;
    google.addEventListener('click', () => void this.connect('google'));
    const apple = el('button', 'btn apple');
    apple.type = 'button';
    apple.append(appleMark(), document.createTextNode('Continue with Apple'));
    apple.disabled = this.waiting;
    apple.addEventListener('click', () => void this.connect('apple'));
    buttons.append(google, apple);
    if (this.waiting) {
      const cancel = el('button', 'btn text', 'Cancel');
      cancel.type = 'button';
      cancel.addEventListener('click', () => void this.backend.cancelConnect());
      buttons.append(cancel);
    } else if (focus === 'learn') {
      const just = el('p', 'just');
      const open = el('button', 'link', 'Open the player');
      open.type = 'button';
      open.addEventListener('click', () => void this.backend.finish('player'));
      just.append(document.createTextNode('Just want to watch? Videos play without an account. '), open);
      buttons.append(just);
    } else {
      const skip = el('button', 'btn text skip', 'Skip for now');
      skip.type = 'button';
      skip.addEventListener('click', () => void this.backend.finish('player'));
      buttons.append(skip);
    }
    form.append(buttons);
    if (this.note !== '') {
      const note = el('p', 'note', this.note);
      note.setAttribute('role', 'status');
      form.append(note);
    }
    const fine = el('p', 'fine');
    fine.append(
      document.createTextNode(
        this.waiting
          ? 'Finish in the browser tab that just opened.'
          : 'Lexpad opens your browser to sign in and never sees your password.',
      ),
      el('br'),
    );
    const other = el('button', 'link', 'Other ways to sign in');
    other.type = 'button';
    other.addEventListener('click', () => void this.connect(undefined));
    if (!this.waiting) fine.append(other);
    form.append(fine);
    this.root.append(form, this.sidePanel());
    window.setTimeout(() => {
      if (!this.waiting) google.focus({ preventScroll: true });
    }, 0);
  }

  /** The dark panel: the word as it lands in the notebook, on both devices. */
  private sidePanel(): HTMLElement {
    const side = el('aside', 'side');
    side.setAttribute('aria-hidden', 'true');
    side.append(stepDots(2));
    const card = el('div', 'added');
    const top = el('div', 'added-top');
    const mark = el('span', 'mark');
    top.append(mark, el('span', 'added-word', SAMPLE_WORD));
    const ok = el('span', 'added-ok');
    ok.append(icon(CHECK, 14), document.createTextNode('Added'));
    top.append(ok);
    card.append(top, el('div', 'added-mean', 'lasting a very short time'));
    const from = el('div', 'added-from');
    from.append(
      el('b', '', 'Seen in The Terrace · S1 E3 · 26:58'),
      document.createTextNode(`And honestly, it was ${SAMPLE_WORD}.`),
    );
    card.append(from);
    side.append(card);
    const both = el('div', 'devices');
    both.append(
      icon(MONITOR, 18),
      el('span', '', 'This computer'),
      el('span', 'dash'),
      icon(PHONE, 18),
      el('span', '', 'Your phone'),
    );
    side.append(both, el('div', 'devices-note', 'The word and its sentence, in both places.'));
    return side;
  }

  private async connect(provider: 'google' | 'apple' | undefined): Promise<void> {
    if (this.waiting) return;
    this.waiting = true;
    this.note = '';
    this.renderSignIn();
    try {
      await this.backend.connect(provider);
      this.waiting = false;
      await this.finishFor(this.focus ?? 'both');
    } catch (cause) {
      this.waiting = false;
      const word = typeof cause === 'string' ? cause : '';
      this.note =
        word === 'cancelled'
          ? 'Nothing was signed in.'
          : word === 'offline'
            ? 'You seem to be offline. Try again in a moment.'
            : word === 'timeout'
              ? 'The browser did not answer in time. Try again.'
              : 'Could not sign in. Try again.';
      this.renderSignIn();
    }
  }
}
