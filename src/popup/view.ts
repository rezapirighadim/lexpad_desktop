/**
 * The popup: the extension's meaning card (lexpad_extension
 * src/content/index.ts), in a window of its own, plus the three things a
 * desktop needs that a web page does not: a notebook picker, a box to type a
 * word when nothing is selected, and a way to pick the word out of a longer
 * selection.
 *
 * Everything is drawn with the DOM and `textContent`; no HTML from the API or
 * the selection is ever parsed. Enter adds (or looks up), Escape closes.
 */
import type { Backend } from '../lib/backend.js';
import { failureOf } from '../lib/backend.js';
import { compose, pick, plan, wordsOf } from '../lib/compose.js';
import { notebookFor } from '../lib/script.js';
import { asHeadword, contextHint, sentenceAround } from '../lib/text.js';
import type { Card, Notebook, Source, State } from '../lib/types.js';

/** Right-to-left scripts, for the direction and the face. */
const RTL = new RegExp('[֐-ࣿיִ-﷿ﹰ-﻿]');
/** How long "Added" stays before the popup puts itself away. */
const DONE_MS = 1600;
const MOVED_MS = 2400;

const MARK =
  '<svg width="14" height="14" viewBox="0 0 32 32" fill="none" stroke="#fff" stroke-width="2.6" stroke-linecap="round"><rect x="10" y="8" width="12" height="16" rx="2"/><path d="M14 8v16M17 12h2M17 15h2"/></svg>';
const TICK =
  '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5L20 7"/></svg>';

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

function bidi<T extends HTMLElement>(node: T, text: string): T {
  node.textContent = text;
  node.dir = RTL.test(text) ? 'rtl' : 'ltr';
  return node;
}

function button(className: string, text: string, onClick: () => void): HTMLButtonElement {
  const b = el('button', className, text);
  b.type = 'button';
  b.addEventListener('click', onClick);
  return b;
}

export class Popup {
  private state: State | undefined;
  /** What Enter does right now. */
  private enter: (() => void) | undefined;
  private closeTimer: number | undefined;
  /** Bumped on every render, so an answer for an older card is dropped. */
  private generation = 0;

  constructor(
    private readonly root: HTMLElement,
    private readonly backend: Backend,
  ) {
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        void this.close();
      } else if (event.key === 'Enter' && !event.isComposing && this.enter !== undefined) {
        const target = event.target as HTMLElement | null;
        if (target?.tagName === 'SELECT' || target?.tagName === 'BUTTON') return;
        event.preventDefault();
        this.enter();
      }
    });
  }

  /** Reads the state again and draws what the capture calls for. */
  async open(): Promise<void> {
    window.clearTimeout(this.closeTimer);
    let state: State;
    try {
      state = await this.backend.state();
    } catch {
      this.frame('Lexpad', '');
      this.root.append(el('div', 'note bad', 'Lexpad could not start the card. Try again in a moment.'));
      return;
    }
    this.state = state;
    if (!state.connected) {
      this.showSignedOut();
      return;
    }
    const next = plan(state.capture);
    if (next.kind === 'word')
      this.showCard(next.headword, { sentence: next.sentence, app: state.capture?.app ?? null });
    else if (next.kind === 'sentence') this.showSentence(next.text);
    else this.showType();
  }

  async close(): Promise<void> {
    window.clearTimeout(this.closeTimer);
    this.generation += 1;
    this.enter = undefined;
    await this.backend.hide();
  }

  /** Clears the card and draws its header: the mark, a title, a line under it, the close button. */
  private frame(title: string, meta: string): { word: HTMLElement; meta: HTMLElement } {
    this.generation += 1;
    this.enter = undefined;
    this.root.replaceChildren();
    const head = el('div', 'head');
    const mark = el('div', 'mark');
    mark.innerHTML = MARK;
    const titles = el('div', 'titles');
    const word = bidi(el('div', 'word'), title);
    const metaEl = el('div', 'meta', meta);
    titles.append(word, metaEl);
    const close = button('close', '×', () => void this.close());
    close.setAttribute('aria-label', 'Close');
    head.append(mark, titles, close);
    this.root.append(head);
    return { word, meta: metaEl };
  }

  /* ------------------------------------------------------- signed out */

  private showSignedOut(): void {
    this.frame('Connect Lexpad', '');
    this.root.append(
      el(
        'div',
        'note',
        'Connect this app to your Lexpad account to add words. You allow it in your browser; the app never sees your password.',
      ),
    );
    const note = el('div', 'note');
    const actions = el('div', 'actions');
    const connect = button('btn primary', 'Connect to Lexpad', () => void go());
    actions.append(connect);
    this.root.append(note, actions);
    const go = async (): Promise<void> => {
      connect.disabled = true;
      connect.textContent = 'Waiting for the browser…';
      note.className = 'note';
      note.textContent = 'Allow Lexpad in the browser tab that just opened.';
      const cancel = button('btn', 'Cancel', () => void this.backend.cancelConnect());
      actions.append(cancel);
      try {
        await this.backend.connect();
        await this.open();
      } catch (cause) {
        const reason = failureOf(cause);
        connect.disabled = false;
        connect.textContent = 'Connect to Lexpad';
        cancel.remove();
        note.className = reason === 'cancelled' ? 'note' : 'note bad';
        note.textContent =
          reason === 'cancelled'
            ? 'Nothing was connected.'
            : reason === 'offline'
              ? 'You seem to be offline. Try again in a moment.'
              : reason === 'timeout'
                ? 'The browser did not answer in time. Try again.'
                : 'Could not connect. Try again.';
      }
    };
    this.enter = () => {
      if (!connect.disabled) void go();
    };
  }

  /* ---------------------------------------------------- type a word */

  private showType(prefill = ''): void {
    this.frame('Add a word', 'Type it, then press Enter');
    const input = el('input', 'typed');
    input.type = 'text';
    input.value = prefill;
    input.placeholder = 'A word or a short phrase';
    input.setAttribute('aria-label', 'Word to add');
    input.autocomplete = 'off';
    input.spellcheck = false;
    input.addEventListener('input', () => {
      input.dir = RTL.test(input.value) ? 'rtl' : 'ltr';
      note.textContent = '';
    });
    const note = el('div', 'note bad');
    this.root.append(input, note);
    if (this.state?.permission === 'missing') this.root.append(this.permissionBox());
    this.enter = () => {
      const headword = asHeadword(input.value);
      if (headword === undefined) {
        note.textContent = 'Type a word or a phrase of up to four words.';
        return;
      }
      this.showCard(headword, { sentence: '', app: null });
    };
    window.setTimeout(() => input.focus(), 0);
  }

  /** macOS without Accessibility: why, and the button to the right Settings pane. */
  private permissionBox(): HTMLElement {
    const box = el('div', 'permission');
    box.append(
      el(
        'p',
        '',
        'To add the word you select in any app, allow Lexpad under Privacy & Security, Accessibility. Lexpad reads only the selected text, and only when you press the shortcut.',
      ),
    );
    box.append(button('btn', 'Open System Settings', () => void this.backend.openAccessibilitySettings()));
    return box;
  }

  /* ------------------------------------------- pick from a sentence */

  private showSentence(text: string): void {
    this.frame('Pick the word', 'That is more than a word. Choose up to four in a row.');
    const words = wordsOf(text);
    let from = -1;
    let to = -1;
    const tokens = el('div', 'tokens');
    const buttons: HTMLButtonElement[] = [];
    const note = el('div', 'note');
    const actions = el('div', 'actions');
    const look = button('btn primary', 'Look it up', () => this.enter?.());
    look.disabled = true;
    const paint = (): void => {
      buttons.forEach((b, i) => b.setAttribute('aria-pressed', String(from >= 0 && i >= from && i <= to)));
      look.disabled = from < 0;
    };
    words.forEach((word, i) => {
      const b = button('token', word, () => {
        if (from < 0 || i < from - 3 || i > to + 3 || (i >= from && i <= to && from === to)) {
          from = i;
          to = i;
        } else if (i < from) from = i;
        else to = i;
        if (to - from >= 4) from = to = i;
        paint();
      });
      b.dir = RTL.test(word) ? 'rtl' : 'ltr';
      b.setAttribute('aria-pressed', 'false');
      buttons.push(b);
      tokens.append(b);
    });
    actions.append(
      look,
      button('btn', 'Type instead', () => this.showType()),
    );
    this.root.append(tokens, note, actions);
    this.enter = () => {
      const headword = from < 0 ? undefined : pick(words, from, to);
      if (headword === undefined) {
        note.className = 'note bad';
        note.textContent = 'Choose a word, or up to four in a row.';
        return;
      }
      // The selection is the sentence the word was met in.
      this.showCard(headword, {
        sentence: sentenceAround(text, headword),
        app: this.state?.capture?.app ?? null,
      });
    };
  }

  /* ------------------------------------------------------- the card */

  private notebooks(): Notebook[] {
    return this.state?.notebooks ?? [];
  }

  private chosen(): Notebook | undefined {
    const id = this.state?.notebookId;
    return this.notebooks().find((n) => n.id === id) ?? this.notebooks()[0];
  }

  showCard(headword: string, source: Source): void {
    const { meta } = this.frame(headword, 'Looking up the meaning…');
    const generation = this.generation;
    const root = this.root;
    root.setAttribute('aria-label', `Add ${headword} to Lexpad`);

    // Which notebook: the picker's, unless the word cannot be in its language.
    const picker = el('div', 'notebook');
    const select = el('select');
    select.setAttribute('aria-label', 'Notebook');
    for (const n of this.notebooks()) {
      const option = el('option', '', n.title);
      option.value = n.id;
      select.append(option);
    }
    select.value = this.chosen()?.id ?? '';
    picker.append(el('span', '', 'Notebook'), select);

    const body = el('div', 'body');
    for (const w of ['w60', 'w90', 'w40']) body.append(el('div', `skel ${w}`));
    const from = el('div', 'from');
    if (source.sentence !== '') {
      from.append(el('b', '', source.app ? `Seen in ${source.app}` : 'Seen in'));
      from.append(bidi(el('div'), source.sentence));
    }
    const note = el('div', 'note');
    const actions = el('div', 'actions');
    const add = button('btn primary', 'Add to Lexpad', () => void doAdd());
    const cancel = button('btn', 'Cancel', () => void this.close());
    actions.append(add, cancel);

    root.append(body);
    if (source.sentence !== '') root.append(from);
    if (this.notebooks().length > 1) root.append(picker);
    root.append(note, actions);

    let fetched: Card | undefined;
    let adding = false;

    const target = (): { notebook: Notebook | undefined; moved: boolean } => {
      const chosen = this.notebooks().find((n) => n.id === select.value) ?? this.chosen();
      return notebookFor(headword, chosen, this.notebooks());
    };

    const nowhere = (): void => {
      meta.textContent = '';
      body.replaceChildren(
        el('div', 'note', 'You have no notebook for this language. Make one in Lexpad and try again.'),
      );
      actions.replaceChildren(button('btn', 'Close', () => void this.close()));
      this.enter = undefined;
    };

    const render = (c: Card): void => {
      fetched = c;
      const bits = [c.pos, c.level, c.pronunciation].filter(
        (v): v is string => typeof v === 'string' && v !== '',
      );
      meta.textContent = bits.join(' · ');
      if (c.correction && c.correction !== headword)
        meta.textContent = `${c.correction}${bits.length ? ' · ' : ''}${meta.textContent}`;
      body.replaceChildren();
      const senses = (c.meanings ?? []).slice(0, 3);
      if (senses.length === 0)
        body.append(el('div', 'note', 'No meaning came back, but the word can still be added.'));
      for (const sense of senses) {
        const wrap = el('div', 'sense');
        wrap.append(bidi(el('div', 'meaning'), sense.meaning));
        if (sense.gloss) wrap.append(bidi(el('div', 'gloss'), sense.gloss));
        const example = sense.examples?.[0];
        if (example) {
          wrap.append(bidi(el('div', 'example'), `“${example.sentence}”`));
          if (example.translation) wrap.append(bidi(el('div', 'example-tr'), example.translation));
        }
        body.append(wrap);
      }
    };

    const lookUp = async (): Promise<void> => {
      const { notebook } = target();
      if (notebook === undefined) {
        if (this.notebooks().length === 0) {
          meta.textContent = '';
          body.replaceChildren(el('div', 'note', 'Create a notebook in Lexpad first.'));
          return;
        }
        nowhere();
        return;
      }
      try {
        const card = await this.backend.lookup(notebook.id, headword, contextHint(source.sentence));
        if (generation === this.generation) render(card);
      } catch (cause) {
        if (generation !== this.generation) return;
        const reason = failureOf(cause);
        if (reason === 'signed_out') {
          this.showSignedOut();
          return;
        }
        meta.textContent = '';
        body.replaceChildren(
          el(
            'div',
            'note',
            reason === 'offline'
              ? 'You seem to be offline. The word can still be added and filled in later.'
              : 'The meaning is not available right now. Add the word and fill it in later in Lexpad.',
          ),
        );
      }
    };

    const doAdd = async (): Promise<void> => {
      if (adding) return;
      const { notebook, moved } = target();
      if (notebook === undefined) {
        nowhere();
        return;
      }
      adding = true;
      add.disabled = true;
      add.textContent = 'Adding…';
      note.textContent = '';
      note.className = 'note';
      try {
        await this.backend.addWord(notebook.id, compose(headword, source, fetched));
        if (generation !== this.generation) return;
        actions.replaceChildren();
        picker.remove();
        const done = el('div', 'done');
        done.innerHTML = TICK;
        // A word that could not be in the chosen notebook goes to the one it
        // could be in, and the card says so.
        done.append(document.createTextNode(moved ? `Added to ${notebook.title}` : 'Added to your notebook'));
        root.append(done);
        this.enter = undefined;
        this.closeTimer = window.setTimeout(() => void this.close(), moved ? MOVED_MS : DONE_MS);
      } catch (cause) {
        if (generation !== this.generation) return;
        adding = false;
        add.disabled = false;
        add.textContent = 'Add to Lexpad';
        const reason = failureOf(cause);
        if (reason === 'signed_out') {
          this.showSignedOut();
          return;
        }
        note.className = 'note bad';
        if (reason === 'duplicate') {
          note.textContent = 'This word is already in your notebook.';
          add.textContent = 'Already added';
          add.disabled = true;
        } else if (reason === 'no_notebook') note.textContent = 'Create a notebook in Lexpad first.';
        else if (reason === 'offline') note.textContent = 'You seem to be offline. Try again in a moment.';
        else note.textContent = 'Could not add the word. Try again.';
      }
    };

    select.addEventListener('change', () => {
      void this.backend.setNotebook(select.value);
      if (this.state) this.state.notebookId = select.value;
    });
    this.enter = () => {
      if (!add.disabled) void doAdd();
    };
    void lookUp();
  }
}
