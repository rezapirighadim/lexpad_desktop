/**
 * The Settings window: the account, the shortcut, reading selections,
 * start on login, the notebook words go to, what leaves this computer, and
 * the version. Copy is short and says nothing about counts or limits.
 */
import type { SettingsBackend } from '../lib/backend.js';
import { failureOf } from '../lib/backend.js';
import { acceleratorOf, isMac, prettyShortcut } from '../lib/shortcut.js';
import type { AppInfo, Settings, State } from '../lib/types.js';

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

function section(title: string): HTMLElement {
  const s = el('section', 'section');
  s.append(el('h2', '', title));
  return s;
}

export class SettingsView {
  private recording: ((event: KeyboardEvent) => void) | undefined;

  constructor(
    private readonly root: HTMLElement,
    private readonly backend: SettingsBackend,
    private readonly mac: boolean = isMac(),
  ) {}

  async render(): Promise<void> {
    let state: State;
    let settings: Settings;
    let info: AppInfo;
    try {
      [state, settings, info] = await Promise.all([
        this.backend.state(),
        this.backend.getSettings(),
        this.backend.appInfo(),
      ]);
    } catch {
      this.root.replaceChildren(
        el('p', 'note bad', 'Settings could not be read. Close this window and open it again.'),
      );
      return;
    }
    const header = el('header', 'top');
    header.append(el('h1', '', 'Lexpad'));
    header.append(
      el(
        'p',
        'lead',
        `Select a word in any app and press ${prettyShortcut(settings.shortcut, this.mac)}. Its meaning shows while it goes into your notebook.`,
      ),
    );
    this.root.replaceChildren(
      header,
      this.account(state),
      this.shortcut(settings),
      this.reading(state),
      this.notebook(state),
      this.startup(settings, info),
      this.privacy(),
      this.about(info),
    );
  }

  /* ------------------------------------------------------- account */

  private account(state: State): HTMLElement {
    const s = section('Account');
    const row = el('div', 'row');
    const note = el('p', 'note');
    if (state.connected && state.user) {
      const who = el('div', 'grow');
      who.append(
        el('div', 'strong', state.user.displayName || state.user.email),
        el('div', 'dim', state.user.email),
      );
      const out = button('btn', 'Sign out', () => {
        // Signing out asks first.
        row.replaceChildren(
          el('div', 'grow', 'Sign out of Lexpad on this computer?'),
          button('btn danger', 'Sign out', () => void signOut()),
          button('btn', 'Cancel', () => void this.render()),
        );
      });
      row.append(who, out);
      s.append(
        row,
        el('p', 'dim small', 'Listed under Settings, Signed-in devices in Lexpad as “Desktop app”.'),
      );
      const signOut = async (): Promise<void> => {
        await this.backend.disconnect();
        await this.render();
      };
    } else {
      row.append(
        el('div', 'grow', 'Not connected. You allow this app in your browser; it never sees your password.'),
      );
      const connect = button('btn primary', 'Connect to Lexpad', () => void go());
      row.append(connect);
      s.append(row, note);
      const go = async (): Promise<void> => {
        connect.disabled = true;
        connect.textContent = 'Waiting for the browser…';
        note.className = 'note';
        note.textContent = 'Allow Lexpad in the browser tab that just opened.';
        const cancel = button('btn', 'Cancel', () => void this.backend.cancelConnect());
        row.append(cancel);
        try {
          await this.backend.connect();
          await this.render();
        } catch (cause) {
          const reason = failureOf(cause);
          connect.disabled = false;
          connect.textContent = 'Connect to Lexpad';
          cancel.remove();
          note.className = reason === 'cancelled' ? 'note' : 'note bad';
          note.textContent =
            reason === 'cancelled'
              ? 'Nothing was connected.'
              : reason === 'timeout'
                ? 'The browser did not answer in time. Try again.'
                : 'Could not connect. Try again.';
        }
      };
    }
    return s;
  }

  /* ------------------------------------------------------ shortcut */

  private shortcut(settings: Settings): HTMLElement {
    const s = section('Shortcut');
    const row = el('div', 'row');
    const keys = el('kbd', 'keys', prettyShortcut(settings.shortcut, this.mac));
    const note = el('p', 'note');
    const change = button('btn', 'Change', () => {
      keys.textContent = 'Press the new shortcut…';
      change.disabled = true;
      note.className = 'note';
      note.textContent =
        'Use at least one of ' +
        (this.mac ? '⌘, ⌃ or ⌥' : 'Ctrl or Alt') +
        ', then a letter or a number. Esc keeps the old one.';
      this.recording = (event: KeyboardEvent) => {
        event.preventDefault();
        if (event.key === 'Escape') {
          stop();
          keys.textContent = prettyShortcut(settings.shortcut, this.mac);
          note.textContent = '';
          return;
        }
        const accelerator = acceleratorOf(event, this.mac);
        if (accelerator === undefined) return;
        stop();
        void save(accelerator);
      };
      document.addEventListener('keydown', this.recording);
    });
    const stop = (): void => {
      if (this.recording) document.removeEventListener('keydown', this.recording);
      this.recording = undefined;
      change.disabled = false;
    };
    const save = async (accelerator: string): Promise<void> => {
      try {
        const saved = await this.backend.setShortcut(accelerator);
        settings.shortcut = saved;
        keys.textContent = prettyShortcut(saved, this.mac);
        note.className = 'note';
        note.textContent = 'Saved.';
      } catch (cause) {
        keys.textContent = prettyShortcut(settings.shortcut, this.mac);
        note.className = 'note bad';
        note.textContent =
          failureOf(cause) === 'taken'
            ? 'Another app or the system uses that shortcut. Try another.'
            : 'Could not change the shortcut. Try again.';
      }
    };
    row.append(el('div', 'grow', 'Add the selected word'), keys, change);
    s.append(row, note);
    return s;
  }

  /* ------------------------------------------- reading selections */

  private reading(state: State): HTMLElement {
    const s = section('Reading the selection');
    if (state.permission === 'not_needed') {
      s.append(
        el(
          'p',
          'dim',
          'Lexpad reads the selected text through Windows UI Automation. Where an app does not offer it, Lexpad copies the selection and then puts your clipboard back as it was.',
        ),
      );
      return s;
    }
    const row = el('div', 'row');
    const allowed = state.permission === 'granted';
    row.append(
      el('div', 'grow', allowed ? 'Accessibility is allowed.' : 'Accessibility is not allowed yet.'),
      button('btn' + (allowed ? '' : ' primary'), 'Open System Settings', () => {
        void this.backend.openAccessibilitySettings();
      }),
    );
    s.append(row);
    if (!allowed && state.permissionStale === true)
      s.append(
        el(
          'p',
          'note bad',
          'Lexpad was updated, and macOS no longer applies the permission it had. In System Settings, Privacy & Security, Accessibility, turn Lexpad off and on again (or remove it with − and add it back).',
        ),
      );
    s.append(
      el(
        'p',
        'dim small',
        'After an update, macOS may ask again: then turn Lexpad off and on in Accessibility. macOS asks before an app may read what you select in other apps. Lexpad reads only the selected text (and the sentence around it, to keep as an example), and only when you press the shortcut. If an app does not share its selection, Lexpad copies it and then puts your clipboard back exactly as it was. Without the permission you can still type a word.',
      ),
    );
    return s;
  }

  /* ------------------------------------------------------ notebook */

  private notebook(state: State): HTMLElement {
    const s = section('Notebook');
    if (!state.connected) {
      s.append(el('p', 'dim', 'Connect to choose the notebook new words go to.'));
      return s;
    }
    if (state.notebooks.length === 0) {
      s.append(el('p', 'dim', 'Create a notebook in Lexpad first.'));
      return s;
    }
    const row = el('div', 'row');
    const select = el('select');
    select.setAttribute('aria-label', 'Notebook for new words');
    for (const n of state.notebooks) {
      const o = el('option', '', n.title);
      o.value = n.id;
      select.append(o);
    }
    select.value = state.notebookId ?? '';
    select.addEventListener('change', () => void this.backend.setNotebook(select.value));
    row.append(el('div', 'grow', 'New words go to'), select);
    s.append(
      row,
      el(
        'p',
        'dim small',
        'A word that cannot be in this notebook’s language (a Persian word in an English notebook) goes to the notebook that takes its script, and the card says so.',
      ),
    );
    return s;
  }

  /* ------------------------------------------------- start on login */

  private startup(settings: Settings, info: AppInfo): HTMLElement {
    const s = section('Start');
    const label = el('label', 'row');
    const box = el('input');
    box.type = 'checkbox';
    box.checked = settings.startOnLogin;
    box.addEventListener('change', () => void this.backend.setStartOnLogin(box.checked));
    label.append(el('div', 'grow', 'Open Lexpad when I log in'), box);
    s.append(label);
    s.append(
      this.toggle('Show Lexpad’s window when the app starts', settings.openOnLaunch, (on) =>
        this.backend.setOpenOnLaunch(on),
      ),
      this.toggle('Open Lexpad in my browser instead of this app’s window', settings.openInBrowser, (on) =>
        this.backend.setOpenInBrowser(on),
      ),
    );
    if (settings.developmentBuild) {
      s.append(
        el(
          'p',
          'dim small',
          `This is a development build (${info.apiOrigin}); it never adds itself to your login items.`,
        ),
      );
    }
    s.append(el('p', 'dim small', 'Lexpad follows your system’s light or dark appearance.'));
    return s;
  }

  /** A row with a switch that saves itself, and goes back if saving fails. */
  private toggle(text: string, on: boolean, save: (on: boolean) => Promise<boolean>): HTMLElement {
    const label = el('label', 'row');
    const box = el('input');
    box.type = 'checkbox';
    box.checked = on;
    box.addEventListener('change', () => {
      const wanted = box.checked;
      save(wanted).catch(() => {
        box.checked = !wanted;
      });
    });
    label.append(el('div', 'grow', text), box);
    return label;
  }

  /* -------------------------------------------------------- privacy */

  private privacy(): HTMLElement {
    const s = section('What leaves this computer');
    const list = el('ul', 'dim small');
    for (const line of [
      'When you look a word up: the word, and the sentence it was in as a hint for its meaning.',
      'When you add it: the word, its meaning, that sentence as an example, and a private note naming the app, such as “Seen in Microsoft Word”.',
      'Never a window title, a document name, the rest of the text, or anything you did not select.',
    ]) {
      list.append(el('li', '', line));
    }
    s.append(list);
    return s;
  }

  /* ---------------------------------------------------------- about */

  private about(info: AppInfo): HTMLElement {
    const s = section('About');
    s.append(el('p', 'dim', `Lexpad for ${this.mac ? 'macOS' : 'Windows'}, version ${info.version}`));
    return s;
  }
}
