import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Backend } from '../lib/backend.js';
import type { Capture, Card, Notebook, State } from '../lib/types.js';
import { PERMISSION_POLL_MS } from '../lib/accessibility.js';
import { Popup } from './view.js';

const en: Notebook = { id: 'n-en', title: 'English', targetLang: 'en', meaningLang: 'fa', isDefault: true };
const fa: Notebook = { id: 'n-fa', title: 'Persian', targetLang: 'fa', meaningLang: 'en', isDefault: false };
const card: Card = {
  headword: 'candid',
  pos: 'adjective',
  level: 'B2',
  meanings: [{ meaning: 'truthful and straightforward', examples: [{ sentence: 'A candid answer.' }] }],
};

function capture(over: Partial<Capture> = {}): Capture {
  return { text: null, context: null, app: 'TextEdit', permission: 'granted', via: 'accessibility', ...over };
}

function fakeBackend(state: Partial<State>, over: Partial<Backend> = {}): Backend & { calls: string[] } {
  const calls: string[] = [];
  const full: State = {
    connected: true,
    user: { id: 'u', email: 'lena@example.test', displayName: 'Lena' },
    notebooks: [en],
    notebookId: 'n-en',
    capture: null,
    shortcut: 'CommandOrControl+Shift+L',
    permission: 'granted',
    version: '0.1.0',
    ...state,
  };
  return {
    calls,
    state: vi.fn(async () => full),
    lookup: vi.fn(async () => card),
    addWord: vi.fn(async () => 'w1'),
    setNotebook: vi.fn(async () => undefined),
    connect: vi.fn(async () => full.user!),
    cancelConnect: vi.fn(async () => undefined),
    hide: vi.fn(async () => {
      calls.push('hide');
    }),
    fit: vi.fn(async () => undefined),
    accessibility: vi.fn(async () => 'granted' as const),
    openAccessibilitySettings: vi.fn(async () => undefined),
    openSettings: vi.fn(async () => undefined),
    openReport: vi.fn(async () => undefined),
    videoHint: vi.fn(async () => null),
    openPlayerPick: vi.fn(async () => undefined),
    ...over,
  };
}

function press(key: string): void {
  document.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
}

const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

let root: HTMLElement;
beforeEach(() => {
  document.body.innerHTML = '<main id="root"></main>';
  root = document.getElementById('root')!;
});
afterEach(() => {
  vi.useRealTimers();
});

describe('Popup', () => {
  it('is dragged by its header, and the buttons stay direct children of the card so they stick to its bottom', async () => {
    const backend = fakeBackend({ capture: capture({ text: 'candid' }) });
    const popup = new Popup(root, backend);
    await popup.open();
    await flush();

    const head = root.querySelector<HTMLElement>('.head');
    expect(head?.dataset.tauriDragRegion).toBe('deep');
    // The close button is a button, which the drag region leaves clickable.
    expect(head?.querySelector('button.close')).not.toBeNull();
    expect(root.querySelector(':scope > .actions')).not.toBeNull();
  });

  it('shows the card for the selected word and adds it with the sentence and where it was seen', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const backend = fakeBackend({
      capture: capture({ text: 'candid', context: 'Nobody else was so candid. The rest stayed quiet.' }),
    });
    const popup = new Popup(root, backend);
    await popup.open();
    await flush();

    expect(root.querySelector('.word')?.textContent).toBe('candid');
    expect(root.querySelector('.from')?.textContent).toContain('Seen in TextEdit');
    expect(root.querySelector('.from')?.textContent).toContain('Nobody else was so candid.');
    expect(backend.lookup).toHaveBeenCalledWith('n-en', 'candid', 'Seen in: “Nobody else was so candid.”');
    expect(root.querySelector('.meta')?.textContent).toBe('adjective · B2');
    expect(root.querySelector('.meaning')?.textContent).toBe('truthful and straightforward');

    press('Enter');
    await flush();
    const [notebookId, word] = vi.mocked(backend.addWord).mock.calls[0]!;
    expect(notebookId).toBe('n-en');
    expect(word).toMatchObject({ headword: 'candid', source: 'ai', memo: 'Seen in TextEdit' });
    expect((word as Card).meanings?.[0]?.examples?.[0]?.sentence).toBe('Nobody else was so candid.');
    expect(root.querySelector('.done')?.textContent).toBe('Added to your notebook');

    await vi.advanceTimersByTimeAsync(1700);
    expect(backend.calls).toContain('hide');
  });

  it('offers Report on the AI-written meaning, handing its text to the core', async () => {
    const backend = fakeBackend({ capture: capture({ text: 'candid' }) });
    await new Popup(root, backend).open();
    await flush();
    const report = root.querySelector<HTMLButtonElement>('button.report');
    expect(report?.getAttribute('aria-label')).toBe('Report this AI-written meaning');
    expect(report?.textContent).toBe('Report');
    report?.click();
    const fragment = vi.mocked(backend.openReport).mock.calls[0]![0];
    expect(/^[A-Za-z0-9_-]+$/.test(fragment)).toBe(true);
    const base64 = fragment.replace(/-/g, '+').replace(/_/g, '/');
    const bytes = Uint8Array.from(atob(base64 + '='.repeat((4 - (base64.length % 4)) % 4)), (c) =>
      c.charCodeAt(0),
    );
    const payload = JSON.parse(new TextDecoder().decode(bytes)) as {
      surface: string;
      snapshot: string;
      headword: string;
    };
    expect(payload.surface).toBe('card');
    expect(payload.headword).toBe('candid');
    expect(payload.snapshot).toContain('truthful and straightforward');
    // Enter on Report reports; it never adds the word by mistake.
    expect(backend.addWord).not.toHaveBeenCalled();
  });

  it('offers no Report when nothing AI-written came back', async () => {
    const backend = fakeBackend(
      { capture: capture({ text: 'candid' }) },
      { lookup: vi.fn(async () => Promise.reject('ai_unavailable')) },
    );
    await new Popup(root, backend).open();
    await flush();
    expect(root.querySelector('button.report')).toBeNull();
  });

  it('says a word is already in the notebook', async () => {
    const backend = fakeBackend(
      { capture: capture({ text: 'candid' }) },
      { addWord: vi.fn(async () => Promise.reject('duplicate')) },
    );
    await new Popup(root, backend).open();
    await flush();
    press('Enter');
    await flush();
    expect(root.querySelector('.note.bad')?.textContent).toBe('This word is already in your notebook.');
    expect(root.querySelector<HTMLButtonElement>('.btn.primary')?.textContent).toBe('Already added');
    expect(root.querySelector<HTMLButtonElement>('.btn.primary')?.disabled).toBe(true);
  });

  it('never invents a meaning when the lookup fails, and still adds the word', async () => {
    const backend = fakeBackend(
      { capture: capture({ text: 'candid' }) },
      { lookup: vi.fn(async () => Promise.reject('ai_unavailable')) },
    );
    await new Popup(root, backend).open();
    await flush();
    expect(root.querySelector('.body')?.textContent).toContain('The meaning is not available right now.');
    press('Enter');
    await flush();
    const word = vi.mocked(backend.addWord).mock.calls[0]![1];
    expect(word).toEqual({
      headword: 'candid',
      category: 'general',
      source: 'manual',
      memo: 'Seen in TextEdit',
    });
  });

  it('files a Persian word in the Persian notebook and says so', async () => {
    const backend = fakeBackend({ notebooks: [en, fa], capture: capture({ text: 'کتاب' }) });
    await new Popup(root, backend).open();
    await flush();
    expect(backend.lookup).toHaveBeenCalledWith('n-fa', 'کتاب', undefined);
    expect(root.querySelector('.word')?.getAttribute('dir')).toBe('rtl');
    press('Enter');
    await flush();
    expect(vi.mocked(backend.addWord).mock.calls[0]![0]).toBe('n-fa');
    expect(root.querySelector('.done')?.textContent).toBe('Added to Persian');
  });

  it('says plainly when an update lost the permission, and how to get it back', async () => {
    const backend = fakeBackend({
      capture: capture({ permission: 'missing' }),
      permission: 'missing',
      permissionStale: true,
    });
    await new Popup(root, backend).open();
    expect(root.querySelector('.permission')?.textContent).toContain(
      'remove it with − and add it again, then reopen Lexpad',
    );
  });

  it('takes the permission box away once Accessibility is allowed, on focus or by itself', async () => {
    let now: 'missing' | 'granted' = 'missing';
    const backend = fakeBackend(
      { capture: capture({ permission: 'missing' }), permission: 'missing' },
      { accessibility: vi.fn(async () => now) },
    );
    await new Popup(root, backend).open();
    expect(root.querySelector('.permission')?.textContent).toContain(
      'If Lexpad is already on, remove it with − and add it again, then reopen Lexpad',
    );
    window.dispatchEvent(new Event('focus'));
    await flush();
    expect(root.querySelector('.permission')).not.toBeNull();
    expect(backend.accessibility).toHaveBeenCalledWith(false);

    now = 'granted';
    window.dispatchEvent(new Event('focus'));
    await flush();
    expect(root.querySelector('.permission')).toBeNull();
    expect(root.querySelector('input.typed')).not.toBeNull();

    // Without the focus: the next tick finds it.
    now = 'missing';
    vi.useFakeTimers();
    try {
      await new Popup(root, backend).open();
      expect(root.querySelector('.permission')).not.toBeNull();
      now = 'granted';
      await vi.advanceTimersByTimeAsync(PERMISSION_POLL_MS);
      expect(root.querySelector('.permission')).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('offers the type-a-word box, and explains the permission it lacks', async () => {
    const backend = fakeBackend({ capture: capture({ permission: 'missing' }), permission: 'missing' });
    await new Popup(root, backend).open();
    const input = root.querySelector<HTMLInputElement>('input.typed')!;
    expect(root.querySelector('.permission')?.textContent).toContain('Accessibility');
    root.querySelector<HTMLButtonElement>('.permission .btn')!.click();
    expect(backend.openAccessibilitySettings).toHaveBeenCalled();

    input.value = 'one two three four five';
    press('Enter');
    expect(root.querySelector('.note.bad')?.textContent).toContain('up to four words');
    input.value = '  candid ';
    press('Enter');
    await flush();
    expect(root.querySelector('.word')?.textContent).toBe('candid');
    // Typed words were met nowhere: no sentence, no note.
    expect(root.querySelector('.from')).toBeNull();
    press('Enter');
    await flush();
    expect(vi.mocked(backend.addWord).mock.calls[0]![1]).not.toHaveProperty('memo');
  });

  it('lets a word be picked out of a longer selection, which becomes its sentence', async () => {
    const backend = fakeBackend({
      capture: capture({ text: 'The committee reached a tentative agreement today.' }),
    });
    await new Popup(root, backend).open();
    const tokens = [...root.querySelectorAll<HTMLButtonElement>('.token')];
    expect(tokens.map((t) => t.textContent)).toEqual([
      'The',
      'committee',
      'reached',
      'a',
      'tentative',
      'agreement',
      'today.',
    ]);
    tokens[4]!.click();
    expect(tokens[4]!.getAttribute('aria-pressed')).toBe('true');
    press('Enter');
    await flush();
    expect(root.querySelector('.word')?.textContent).toBe('tentative');
    expect(root.querySelector('.from')?.textContent).toContain(
      'The committee reached a tentative agreement today.',
    );
  });

  it('asks to connect when signed out, through the browser', async () => {
    let connected = false;
    const backend = fakeBackend({});
    backend.state = vi.fn(async () => ({
      connected,
      user: null,
      notebooks: connected ? [en] : [],
      notebookId: null,
      capture: null,
      shortcut: 'x',
      permission: 'granted' as const,
      version: '0.1.0',
    }));
    backend.connect = vi.fn(async () => {
      connected = true;
      return { id: 'u', email: 'e', displayName: 'E' };
    });
    await new Popup(root, backend).open();
    expect(root.querySelector('.word')?.textContent).toBe('Sign in to Lexpad');
    expect(root.textContent).toContain('never sees your password');
    root.querySelector<HTMLButtonElement>('.btn.primary')!.click();
    await flush();
    await flush();
    expect(backend.connect).toHaveBeenCalled();
    expect(root.querySelector('.word')?.textContent).toBe('Add a word');
  });

  it('closes on Escape', async () => {
    const backend = fakeBackend({ capture: capture({ text: 'candid' }) });
    await new Popup(root, backend).open();
    press('Escape');
    expect(backend.calls).toContain('hide');
  });
});

describe('the hint when a video app is in front', () => {
  const vlc = (): ReturnType<typeof capture> => capture({ app: 'VLC media player', via: 'none' });

  it('asks the core about the app in front and, when due, says the Player can open the film', async () => {
    const backend = fakeBackend({ capture: vlc() }, { videoHint: vi.fn(async () => 'VLC') });
    await new Popup(root, backend).open();
    expect(backend.videoHint).toHaveBeenCalledWith('VLC media player');
    const hint = root.querySelector('.video-hint');
    expect(hint?.querySelector('b')?.textContent).toBe('Watching in VLC?');
    expect(hint?.querySelector('p')?.textContent).toBe(
      'Watching in VLC? Open the film in Lexpad Player and click any word.',
    );
    // Under the box, above anything else.
    expect(root.querySelector('input.typed')?.nextElementSibling?.nextElementSibling).toBe(hint);
    hint!.querySelector<HTMLButtonElement>('button')!.click();
    expect(backend.openPlayerPick).toHaveBeenCalled();
  });

  it('shows on the card too, above its buttons', async () => {
    const backend = fakeBackend(
      { capture: capture({ app: 'IINA', text: 'candid' }) },
      { videoHint: vi.fn(async () => 'IINA') },
    );
    await new Popup(root, backend).open();
    await flush();
    const hint = root.querySelector('.video-hint');
    expect(hint?.querySelector('b')?.textContent).toBe('Watching in IINA?');
    expect(hint?.nextElementSibling?.className).toBe('note');
  });

  it('shows nothing when the core says it is not due (another app, shown lately, or three in all)', async () => {
    const backend = fakeBackend({ capture: vlc() });
    await new Popup(root, backend).open();
    expect(root.querySelector('.video-hint')).toBeNull();
  });

  it('never asks signed out, or with no app in front', async () => {
    const out = fakeBackend({ connected: false, user: null, capture: vlc() });
    await new Popup(root, out).open();
    expect(out.videoHint).not.toHaveBeenCalled();
    const none = fakeBackend({ capture: capture({ app: null }) });
    await new Popup(root, none).open();
    expect(none.videoHint).not.toHaveBeenCalled();
  });

  it('opens the card even when the core cannot answer', async () => {
    const backend = fakeBackend(
      { capture: vlc() },
      { videoHint: vi.fn(async () => Promise.reject(new Error('x'))) },
    );
    await new Popup(root, backend).open();
    expect(root.querySelector('input.typed')).not.toBeNull();
    expect(root.querySelector('.video-hint')).toBeNull();
  });
});
