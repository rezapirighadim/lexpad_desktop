/**
 * The menu-bar / tray panel: who is signed in, a box to add a word (the card
 * opens for it, exactly as for a selection), the shortcut, the words added
 * from this computer, the default notebook, and Open Lexpad / Settings /
 * Quit. Signed out, it offers to connect instead.
 *
 * Drawn with the DOM and `textContent`; nothing from the API is parsed as
 * HTML. Esc closes it; every control is reachable with Tab.
 */
import type { PanelBackend } from '../lib/backend.js';
import { failureOf } from '../lib/backend.js';
import { isMac, prettyShortcut } from '../lib/shortcut.js';
import { ago } from '../lib/time.js';
import type { Notebook, RecentWord, State } from '../lib/types.js';

/** Right-to-left scripts, for the direction and the face. */
const RTL = new RegExp('[֐-ࣿיִ-﷿ﹰ-﻿]');

const MARK =
  '<svg viewBox="0 0 32 32" width="28" height="28" aria-hidden="true"><rect width="32" height="32" rx="8" fill="#2E7B4E"/><rect x="10" y="8" width="12" height="16" rx="2" fill="none" stroke="#fff" stroke-width="2"/><path d="M14 8v16M17 12h2M17 15h2" stroke="#fff" stroke-width="2" stroke-linecap="round"/></svg>';
const RETURN =
  '<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M13 3v5a2 2 0 0 1-2 2H3"/><path d="M6 7l-3 3 3 3"/></svg>';

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

function button(className: string, text: string, onClick: () => void): HTMLButtonElement {
  const b = el('button', className, text);
  b.type = 'button';
  b.addEventListener('click', onClick);
  return b;
}

export class Panel {
  private state: State | undefined;
  /** Bumped on every render, so an answer for an older one is dropped. */
  private generation = 0;

  constructor(
    private readonly root: HTMLElement,
    private readonly backend: PanelBackend,
    private readonly now: () => number = Date.now,
    private readonly mac: boolean = isMac(),
  ) {
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        void this.backend.hidePanel();
      }
    });
  }

  /**
   * Reads the state again and draws the panel. On opening (`focus`), the add
   * box gets the keyboard; a redraw while open (a setting changed) keeps
   * what was typed and where the keyboard was.
   */
  async open(focus = true): Promise<void> {
    const generation = ++this.generation;
    let state: State;
    let recent: RecentWord[] = [];
    try {
      state = await this.backend.state();
      if (state.connected) recent = await this.backend.recent().catch(() => []);
    } catch {
      if (generation !== this.generation) return;
      this.state = undefined;
      this.root.replaceChildren(
        this.header(null),
        el('p', 'note bad', 'Lexpad could not load your account. Try again in a moment.'),
        this.footer(),
      );
      return;
    }
    if (generation !== this.generation) return;
    this.state = state;
    const typed = focus ? '' : (this.root.querySelector<HTMLInputElement>('.add input')?.value ?? '');
    const active = document.activeElement;
    const activeId = active instanceof HTMLElement && this.root.contains(active) ? active.id : '';
    this.root.replaceChildren(
      ...(state.connected ? this.connected(state, recent) : this.signedOut()),
      this.footer(),
    );
    const input = this.root.querySelector<HTMLInputElement>('.add input');
    if (input && typed !== '') {
      input.value = typed;
      input.dispatchEvent(new Event('input'));
    }
    if (focus) input?.focus();
    else if (activeId !== '') document.getElementById(activeId)?.focus();
  }

  /* ---------------------------------------------------------- parts */

  private header(state: State | null): HTMLElement {
    const head = el('header', 'top');
    const mark = el('div', 'logo');
    mark.innerHTML = MARK;
    const who = el('div', 'who');
    who.append(el('div', 'name', 'Lexpad'));
    const user = state?.user;
    const account = el('div', 'account', user ? user.email : state ? 'Not connected' : '');
    if (user?.displayName) account.title = user.displayName;
    who.append(account);
    head.append(mark, who);
    return head;
  }

  private connected(state: State, recent: RecentWord[]): HTMLElement[] {
    const parts: HTMLElement[] = [this.header(state)];

    // Add a word: Enter opens the card for it.
    const form = el('form', 'add');
    form.setAttribute('role', 'search');
    const input = el('input');
    input.id = 'add-word';
    input.type = 'text';
    input.placeholder = 'Add a word…';
    input.setAttribute('aria-label', 'Add a word');
    input.autocomplete = 'off';
    input.spellcheck = false;
    input.maxLength = 200;
    const enter = el('span', 'enter');
    enter.innerHTML = RETURN;
    enter.setAttribute('aria-hidden', 'true');
    input.addEventListener('input', () => {
      input.dir = RTL.test(input.value) ? 'rtl' : 'ltr';
      form.classList.toggle('typed', input.value.trim() !== '');
    });
    form.addEventListener('submit', (event) => {
      event.preventDefault();
      const text = input.value.trim();
      if (text === '') return;
      input.value = '';
      form.classList.remove('typed');
      void this.backend.addTyped(text);
    });
    form.append(input, enter);
    parts.push(form);

    // The shortcut, as the learner set it.
    const hint = el('p', 'hint');
    hint.append('Select text anywhere and press ');
    hint.append(el('kbd', '', prettyShortcut(state.shortcut, this.mac)));
    parts.push(hint);
    if (state.permission === 'missing') {
      const warn = el('p', 'warn');
      warn.append('To read what you select, allow Lexpad under Accessibility. ');
      warn.append(
        button('link', 'Open System Settings', () => void this.backend.openAccessibilitySettings()),
      );
      parts.push(warn);
    }

    parts.push(this.recentList(state, recent));
    const picker = this.notebookPicker(state.notebooks, state.notebookId);
    if (picker) parts.push(picker);
    return parts;
  }

  private recentList(state: State, recent: RecentWord[]): HTMLElement {
    const section = el('section', 'recent');
    section.setAttribute('aria-labelledby', 'recent-title');
    const title = el('h2', '', 'Added from this computer');
    title.id = 'recent-title';
    section.append(title);
    if (recent.length === 0) {
      section.append(el('p', 'empty', 'Words you add here or with the shortcut show up in this list.'));
      return section;
    }
    const list = el('ul');
    const titles = new Map(state.notebooks.map((n) => [n.id, n.title]));
    const now = this.now();
    for (const word of recent) {
      const item = el('li');
      const open = button('row', '', () => void this.backend.openWeb(word.id));
      const head = el('span', 'hw', word.headword);
      head.dir = RTL.test(word.headword) ? 'rtl' : 'ltr';
      const meta = el('span', 'when', ago(word.addedAt, now));
      const notebook = titles.get(word.notebookId);
      open.setAttribute(
        'aria-label',
        `${word.headword}, added ${meta.textContent}${notebook ? ` to ${notebook}` : ''}. Open in Lexpad`,
      );
      if (notebook && state.notebooks.length > 1) open.title = notebook;
      open.append(head, meta);
      item.append(open);
      list.append(item);
    }
    section.append(list);
    return section;
  }

  private notebookPicker(notebooks: Notebook[], chosen: string | null): HTMLElement | undefined {
    if (notebooks.length === 0) return undefined;
    const row = el('div', 'notebook');
    const label = el('label', '', 'Notebook');
    label.htmlFor = 'notebook';
    const select = el('select');
    select.id = 'notebook';
    for (const n of notebooks) {
      const option = el('option', '', n.title);
      option.value = n.id;
      select.append(option);
    }
    select.value = chosen ?? notebooks[0]?.id ?? '';
    select.disabled = notebooks.length < 2;
    select.addEventListener('change', () => {
      void this.backend.setNotebook(select.value);
      if (this.state) this.state.notebookId = select.value;
    });
    row.append(label, select);
    return row;
  }

  private signedOut(): HTMLElement[] {
    const box = el('div', 'connect');
    box.append(
      el(
        'p',
        '',
        'Connect this app to your Lexpad account to add words from any app. You allow it in your browser; the app never sees your password.',
      ),
    );
    const note = el('p', 'note');
    const connect = button('btn primary', 'Connect to Lexpad', () => void go());
    box.append(connect, note);
    const go = async (): Promise<void> => {
      connect.disabled = true;
      connect.textContent = 'Waiting for the browser…';
      note.className = 'note';
      note.textContent = 'Allow Lexpad in the browser tab that just opened.';
      try {
        await this.backend.connect();
        await this.open();
      } catch (cause) {
        const reason = failureOf(cause);
        connect.disabled = false;
        connect.textContent = 'Connect to Lexpad';
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
    return [this.header(this.state ?? null), box];
  }

  private footer(): HTMLElement {
    const foot = el('footer', 'foot');
    foot.append(
      button('act', 'Open Lexpad', () => void this.backend.openWeb(null)),
      button('act', 'Settings…', () => void this.backend.openSettings()),
      button('act quit', 'Quit', () => void this.backend.quit()),
    );
    return foot;
  }
}
