import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { SettingsBackend } from '../lib/backend.js';
import type { State } from '../lib/types.js';
import { SettingsView } from './view.js';

function backend(state: Partial<State>, over: Partial<SettingsBackend> = {}): SettingsBackend {
  const full: State = {
    connected: true,
    user: { id: 'u', email: 'lena@example.test', displayName: 'Lena' },
    notebooks: [{ id: 'n1', title: 'English', targetLang: 'en', meaningLang: 'fa', isDefault: true }],
    notebookId: 'n1',
    capture: null,
    shortcut: 'CommandOrControl+Shift+L',
    permission: 'missing',
    version: '0.1.0',
    ...state,
  };
  return {
    state: vi.fn(async () => full),
    setNotebook: vi.fn(async () => undefined),
    connect: vi.fn(async () => full.user!),
    cancelConnect: vi.fn(async () => undefined),
    disconnect: vi.fn(async () => undefined),
    accessibility: vi.fn(async () => full.permission),
    openAccessibilitySettings: vi.fn(async () => undefined),
    getSettings: vi.fn(async () => ({
      shortcut: full.shortcut,
      startOnLogin: true,
      developmentBuild: false,
    })),
    setShortcut: vi.fn(async (s: string) => s),
    setStartOnLogin: vi.fn(async (on: boolean) => on),
    appInfo: vi.fn(async () => ({
      version: '0.1.0',
      apiOrigin: 'https://api.lexpad.app',
      appOrigin: 'https://app.lexpad.app',
      autostartEnabled: true,
    })),
    ...over,
  };
}

const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0));
let root: HTMLElement;
beforeEach(() => {
  document.body.innerHTML = '<main id="root"></main>';
  root = document.getElementById('root')!;
});

describe('Settings', () => {
  it('shows the account, the shortcut and why Accessibility is asked for', async () => {
    await new SettingsView(root, backend({}), true).render();
    expect(root.textContent).toContain('lena@example.test');
    expect(root.querySelector('.keys')?.textContent).toBe('⌘⇧L');
    expect(root.textContent).toContain('Accessibility is not allowed yet.');
    expect(root.textContent).toContain('Never a window title');
  });

  it('asks before signing out', async () => {
    const b = backend({});
    await new SettingsView(root, b, true).render();
    [...root.querySelectorAll('button')].find((x) => x.textContent === 'Sign out')!.click();
    expect(b.disconnect).not.toHaveBeenCalled();
    expect(root.textContent).toContain('Sign out of Lexpad on this computer?');
    root.querySelector<HTMLButtonElement>('.btn.danger')!.click();
    await flush();
    expect(b.disconnect).toHaveBeenCalled();
  });

  it('records a new shortcut and says when it is taken', async () => {
    const b = backend({}, { setShortcut: vi.fn(async () => Promise.reject('taken')) });
    await new SettingsView(root, b, true).render();
    [...root.querySelectorAll('button')].find((x) => x.textContent === 'Change')!.click();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { code: 'KeyK', key: 'k', metaKey: true, altKey: true }),
    );
    await flush();
    expect(b.setShortcut).toHaveBeenCalledWith('CommandOrControl+Alt+K');
    expect(root.textContent).toContain('Another app or the system uses that shortcut.');
    expect(root.querySelector('.keys')?.textContent).toBe('⌘⇧L');
  });

  it('offers to connect when signed out', async () => {
    const b = backend({ connected: false, user: null, notebooks: [] });
    await new SettingsView(root, b, false).render();
    expect(root.textContent).toContain('it never sees your password');
    expect(root.querySelector('.keys')?.textContent).toBe('Ctrl+Shift+L');
  });
});
