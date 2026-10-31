import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { PanelBackend } from '../lib/backend.js';
import type { RecentEntry } from '../lib/media/api.js';
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
    phoneUsed: vi.fn(async () => false),
    openEverywhere: vi.fn(async () => undefined),
    recentVideos: vi.fn(async () => []),
    watch: vi.fn(async () => undefined),
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

describe('the Lexpad everywhere row', () => {
  it('offers the phone while no phone app was used, and opens Lexpad everywhere', async () => {
    const backend = fake();
    await draw(backend);
    const row = root.querySelector<HTMLButtonElement>('#everywhere')!;
    expect(row.textContent).toBe('Lexpad on your phone…');
    expect(row.type).toBe('button');
    row.click();
    expect(backend.openEverywhere).toHaveBeenCalledTimes(1);
    expect(backend.openWeb).not.toHaveBeenCalled();
  });

  it('says Lexpad everywhere once a phone app was used', async () => {
    await draw(fake({}, { phoneUsed: vi.fn(async () => true) }));
    expect(root.querySelector('#everywhere')?.textContent).toBe('Lexpad everywhere…');
  });

  it('keeps the offer when the answer cannot be read', async () => {
    await draw(fake({}, { phoneUsed: vi.fn(async () => Promise.reject('offline')) }));
    expect(root.querySelector('#everywhere')?.textContent).toBe('Lexpad on your phone…');
  });

  it('sits right above the footer, after the recent words, the notebook and the Player row', async () => {
    await draw(fake());
    const row = root.querySelector('#everywhere')!;
    expect(row.nextElementSibling?.classList.contains('foot')).toBe(true);
    expect(row.previousElementSibling?.id).toBe('watch');
    expect(row.previousElementSibling?.previousElementSibling?.classList.contains('notebook')).toBe(true);
    // Tab order is the document order: recent words, the rows, Open Lexpad, Settings…, Quit.
    const order = [...root.querySelectorAll<HTMLElement>('button, input, select')]
      .filter((b) => !(b as HTMLButtonElement).disabled)
      .map((b) => b.id || b.textContent?.trim() || b.tagName);
    expect(order.slice(-6)).toEqual(['notebook', 'watch', 'everywhere', 'Open Lexpad', 'Settings…', 'Quit']);
    expect(order.indexOf('everywhere')).toBeGreaterThan(order.findIndex((t) => t.startsWith('candid')));
  });

  it('asks once per opening, beside the state', async () => {
    const backend = fake();
    const panel = await draw(backend);
    await panel.open(false);
    expect(backend.phoneUsed).toHaveBeenCalledTimes(2);
  });
});

describe('the panel, signed out', () => {
  it('offers to connect instead of the add box', async () => {
    const backend = fake({ connected: false, user: null, notebooks: [] });
    await draw(backend);
    expect(root.querySelector('.add')).toBeNull();
    expect(root.querySelector('.account')?.textContent).toBe('Not signed in');
    expect(root.querySelector('#everywhere')).toBeNull();
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
    expect(root.querySelector('#everywhere')).toBeNull();
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

const film: RecentEntry = {
  key: 'k1',
  fileId: 'f1',
  name: 'Tears.of.Steel.2012.1080p.mkv',
  position: 254,
  duration: 734,
  watched: false,
  updated: 1,
  missing: false,
};

const { fileId: _gone, ...movedFile } = film;
const moved: RecentEntry = { ...movedFile, missing: true };

describe('the Player row', () => {
  it('says "Watch a video…" with ⌘O on a Mac, above the phone row, and opens the dialog', async () => {
    const backend = fake();
    await draw(backend);
    const rows = [...root.querySelectorAll('.every')].map((r) => r.id);
    expect(rows).toEqual(['watch', 'everywhere']);
    const row = root.querySelector<HTMLButtonElement>('#watch')!;
    expect(row.querySelector('.every-label')?.textContent).toBe('Watch a video…');
    expect(row.querySelector('kbd')?.textContent).toBe('⌘O');
    row.click();
    expect(backend.watch).toHaveBeenCalledWith(null);
  });

  it('says Ctrl+O on Windows', async () => {
    await draw(fake(), false);
    expect(root.querySelector('#watch kbd')?.textContent).toBe('Ctrl+O');
  });

  it('continues the last video with the minutes left, and plays that file', async () => {
    const backend = fake({}, { recentVideos: vi.fn(async () => [film]) });
    await draw(backend);
    const row = root.querySelector<HTMLButtonElement>('#watch')!;
    expect(row.querySelector('.watch-name')?.textContent).toBe('Continue Tears of Steel');
    expect(row.querySelector('.watch-title')?.textContent).toBe('Tears of Steel');
    // The minutes sit outside the two-line clamp, so they always show (review R4).
    expect(row.querySelector('.watch-name .watch-left')).toBeNull();
    expect(row.querySelector('.every-label > .watch-left')?.textContent).toBe('· 8 min left');
    expect(row.getAttribute('aria-label')).toBe('Continue Tears of Steel, 8 minutes left');
    expect(row.title).toBe('Continue Tears of Steel, 8 minutes left');
    expect(row.querySelector('kbd')).toBeNull();
    expect(row.querySelector('.every-chev svg')).not.toBeNull();
    row.click();
    expect(backend.watch).toHaveBeenCalledWith('f1');
  });

  it('offers the dialog when the last video moved or failed to load', async () => {
    await draw(fake({}, { recentVideos: vi.fn(async () => [moved]) }));
    expect(root.querySelector('#watch .every-label')?.textContent).toBe('Watch a video…');
    root.replaceChildren();
    await draw(fake({}, { recentVideos: vi.fn(async () => Promise.reject(new Error('no engine'))) }));
    expect(root.querySelector('#watch .every-label')?.textContent).toBe('Watch a video…');
  });

  it('is there signed out too, where there is no phone row', async () => {
    await draw(fake({ connected: false, user: null, notebooks: [] }));
    expect(root.querySelector('#watch .every-label')?.textContent).toBe('Watch a video…');
    expect(root.querySelector('#everywhere')).toBeNull();
    // Right above the footer.
    expect(root.querySelector('#watch')?.nextElementSibling?.className).toBe('foot');
  });

  it('comes after the notebook and before the phone row when tabbing', async () => {
    await draw(fake());
    const order = [...root.querySelectorAll<HTMLElement>('input, select, button')].map(
      (e) => e.id || e.className,
    );
    expect(order.indexOf('notebook')).toBeLessThan(order.indexOf('watch'));
    expect(order.indexOf('watch') + 1).toBe(order.indexOf('everywhere'));
  });
});
