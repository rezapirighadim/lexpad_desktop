import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { PanelBackend } from '../lib/backend.js';
import type { Notebook, RecentWord, State } from '../lib/types.js';
import { Panel } from './view.js';

const NOW = Date.UTC(2026, 9, 7, 12, 0, 0);
const en: Notebook = { id: 'n-en', title: 'English', targetLang: 'en', meaningLang: 'fa', isDefault: true };
const de: Notebook = { id: 'n-de', title: 'German', targetLang: 'de', meaningLang: 'en', isDefault: false };
const recent: RecentWord[] = [
  { id: 'w2', headword: 'candid', notebookId: 'n-en', userId: 'u', addedAt: NOW - 2 * 60_000 },
  { id: 'w1', headword: 'Fernweh', notebookId: 'n-de', userId: 'u', addedAt: NOW - 3 * 3_600_000 },
];

function fake(state: Partial<State> = {}, over: Partial<PanelBackend> = {}): PanelBackend {
  const full: State = {
    connected: true,
    user: { id: 'u', email: 'lena@example.test', displayName: 'Lena' },
    notebooks: [en, de],
    notebookId: 'n-en',
    capture: null,
    shortcut: 'CommandOrControl+Shift+L',
    permission: 'granted',
    version: '0.1.0',
    ...state,
  };
  return {
    state: vi.fn(async () => full),
    recent: vi.fn(async () => recent),
    setNotebook: vi.fn(async () => undefined),
    connect: vi.fn(async () => full.user!),
    cancelConnect: vi.fn(async () => undefined),
    addTyped: vi.fn(async () => undefined),
    hidePanel: vi.fn(async () => undefined),
    fitPanel: vi.fn(async () => undefined),
    openWeb: vi.fn(async () => undefined),
    openSettings: vi.fn(async () => undefined),
    openAccessibilitySettings: vi.fn(async () => undefined),
    quit: vi.fn(async () => undefined),
    ...over,
  };
}

let root: HTMLElement;
beforeEach(() => {
  document.body.innerHTML = '<main id="root"></main>';
  root = document.getElementById('root')!;
});

async function draw(backend: PanelBackend, mac = true): Promise<Panel> {
  const panel = new Panel(root, backend, () => NOW, mac);
  await panel.open();
  return panel;
}

describe('the panel, signed in', () => {
  it('shows the account, focuses the add box and says the shortcut as the platform writes it', async () => {
    await draw(fake());
    expect(root.querySelector('.account')?.textContent).toBe('lena@example.test');
    expect(document.activeElement).toBe(root.querySelector('.add input'));
    expect(root.querySelector('.hint kbd')?.textContent).toBe('⌘⇧L');
  });

  it('says the Windows shortcut on Windows, and the one the learner chose', async () => {
    await draw(fake({ shortcut: 'Alt+Shift+K' }), false);
    expect(root.querySelector('.hint kbd')?.textContent).toBe('Alt+Shift+K');
  });

  it('opens the card for a typed word and clears the box', async () => {
    const backend = fake();
    await draw(backend);
    const input = root.querySelector<HTMLInputElement>('.add input')!;
    input.value = '  serendipity ';
    root.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
    expect(backend.addTyped).toHaveBeenCalledWith('serendipity');
    expect(input.value).toBe('');
  });

  it('does nothing for an empty box', async () => {
    const backend = fake();
    await draw(backend);
    root.querySelector('form')!.dispatchEvent(new Event('submit', { cancelable: true }));
    expect(backend.addTyped).not.toHaveBeenCalled();
  });

  it('lists the words added from this computer, newest first, each opening in the web app', async () => {
    const backend = fake();
    await draw(backend);
    const rows = [...root.querySelectorAll<HTMLButtonElement>('.recent .row')];
    expect(rows.map((r) => r.querySelector('.hw')?.textContent)).toEqual(['candid', 'Fernweh']);
    expect(rows.map((r) => r.querySelector('.when')?.textContent)).toEqual(['2 min ago', '3 h ago']);
    expect(rows[1]!.getAttribute('aria-label')).toContain('to German');
    rows[0]!.click();
    expect(backend.openWeb).toHaveBeenCalledWith('w2');
  });

  it('says where the list comes from when it is empty, without inventing words', async () => {
    await draw(fake({}, { recent: vi.fn(async () => []) }));
    expect(root.querySelectorAll('.recent .row')).toHaveLength(0);
    expect(root.querySelector('.recent .empty')).not.toBeNull();
  });

  it('changes the default notebook', async () => {
    const backend = fake();
    await draw(backend);
    const select = root.querySelector<HTMLSelectElement>('#notebook')!;
    expect(select.value).toBe('n-en');
    select.value = 'n-de';
    select.dispatchEvent(new Event('change'));
    expect(backend.setNotebook).toHaveBeenCalledWith('n-de');
  });

  it('shows one notebook without a choice to make', async () => {
    await draw(fake({ notebooks: [en] }));
    expect(root.querySelector<HTMLSelectElement>('#notebook')!.disabled).toBe(true);
  });

  it('offers the Accessibility pane when macOS has not allowed it', async () => {
    const backend = fake({ permission: 'missing' });
    await draw(backend);
    root.querySelector<HTMLButtonElement>('.warn .link')!.click();
    expect(backend.openAccessibilitySettings).toHaveBeenCalled();
  });

  it('keeps what was typed when a setting changes while it is open', async () => {
    const panel = await draw(fake());
    const input = root.querySelector<HTMLInputElement>('.add input')!;
    input.value = 'half-typed';
    await panel.open(false);
    expect(root.querySelector<HTMLInputElement>('.add input')!.value).toBe('half-typed');
  });
});

describe('the panel, signed out', () => {
  it('offers to connect instead of the add box', async () => {
    const backend = fake({ connected: false, user: null, notebooks: [] });
    await draw(backend);
    expect(root.querySelector('.add')).toBeNull();
    expect(root.querySelector('.account')?.textContent).toBe('Not signed in');
    root.querySelector<HTMLButtonElement>('.connect .btn')!.click();
    expect(backend.connect).toHaveBeenCalled();
  });

  it('says why when connecting fails', async () => {
    const backend = fake(
      { connected: false, user: null, notebooks: [] },
      { connect: vi.fn(async () => Promise.reject('timeout')) },
    );
    await draw(backend);
    root.querySelector<HTMLButtonElement>('.connect .btn')!.click();
    await new Promise((r) => setTimeout(r, 0));
    expect(root.querySelector('.connect .note')?.textContent).toBe(
      'The browser did not answer in time. Try again.',
    );
  });
});

describe('the panel, always', () => {
  it('says so when the account cannot be read, and still offers the way out', async () => {
    await draw(fake({}, { state: vi.fn(async () => Promise.reject('error')) }));
    expect(root.querySelector('.note.bad')).not.toBeNull();
    expect(root.querySelectorAll('.foot .act')).toHaveLength(3);
  });

  it('opens Lexpad, Settings, and quits', async () => {
    const backend = fake();
    await draw(backend);
    const [open, settings, quit] = [...root.querySelectorAll<HTMLButtonElement>('.foot .act')];
    open!.click();
    settings!.click();
    quit!.click();
    expect(backend.openWeb).toHaveBeenCalledWith(null);
    expect(backend.openSettings).toHaveBeenCalled();
    expect(backend.quit).toHaveBeenCalled();
  });

  it('closes on Escape', async () => {
    const backend = fake();
    await draw(backend);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(backend.hidePanel).toHaveBeenCalled();
  });
});

describe('the one-time hint after the last window closed', () => {
  it('says where Lexpad went, Got it puts it away, and it goes by itself after a while', async () => {
    vi.useFakeTimers();
    try {
      const backend = fake();
      const panel = new Panel(root, backend, () => NOW, true);
      panel.hint(172);
      expect(document.documentElement.dataset.hint).toBe('');
      expect(document.documentElement.style.getPropertyValue('--caret-x')).toBe('172px');
      expect(root.querySelector('.still-title')?.textContent).toBe('Lexpad is still here');
      expect(root.querySelector('.still-body')?.textContent).toBe(
        'Click this icon to open Lexpad or a video again.',
      );
      const ok = root.querySelector<HTMLButtonElement>('.still-ok');
      expect(document.activeElement).not.toBe(ok);
      ok?.click();
      expect(backend.hidePanel).toHaveBeenCalledTimes(1);
      vi.advanceTimersByTime(8000);
      expect(backend.hidePanel).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it('opening the panel as itself ends the hint, and its timer then does nothing', async () => {
    vi.useFakeTimers();
    try {
      const backend = fake();
      const panel = new Panel(root, backend, () => NOW, true);
      panel.hint();
      await panel.open();
      expect(root.querySelector('.still')).toBeNull();
      // The caret goes with the hint.
      expect(document.documentElement.dataset.hint).toBeUndefined();
      vi.advanceTimersByTime(8000);
      expect(backend.hidePanel).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});
