import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Backend } from '../lib/backend.js';
import type { Capture, Card, Notebook, State } from '../lib/types.js';
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
    expect(root.querySelector('.word')?.textContent).toBe('Connect Lexpad');
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
