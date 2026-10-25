import { beforeEach, describe, expect, it, vi } from 'vitest';
import { WelcomeView, type WelcomeBackend, type WelcomeState } from './view.js';

function backend(
  state: Partial<WelcomeState> = {},
  over: Partial<WelcomeBackend> = {},
): WelcomeBackend & {
  choose: ReturnType<typeof vi.fn>;
  finish: ReturnType<typeof vi.fn>;
  connect: ReturnType<typeof vi.fn>;
} {
  const full: WelcomeState = { os: 'macos', signedIn: false, focus: null, ...state };
  return {
    state: vi.fn(async () => full),
    choose: vi.fn(async () => undefined),
    connect: vi.fn(async () => undefined),
    cancelConnect: vi.fn(async () => undefined),
    finish: vi.fn(async () => undefined),
    ...over,
  } as never;
}

const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0));
let root: HTMLElement;
beforeEach(() => {
  document.body.innerHTML = '<main id="root"></main>';
  root = document.getElementById('root')!;
});

function key(k: string): void {
  document.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true }));
}

describe('the first launch', () => {
  it('asks what to do first, three cards in a radio group, no Continue button', async () => {
    await new WelcomeView(root, backend()).start();
    expect(root.querySelector('h1')?.textContent).toBe('What would you like to do first?');
    const cards = [...root.querySelectorAll('[role="radio"]')];
    expect(cards.map((c) => c.querySelector('.t')?.textContent)).toEqual([
      'Watch videos with subtitles',
      'Learn and practise my words',
      'Both',
    ]);
    expect(root.querySelector('[role="radiogroup"]')).not.toBeNull();
    expect(root.textContent).not.toMatch(/Continue\b(?! with)/);
    expect(root.textContent).toContain('Not sure? Choose Both.');
  });

  it('moves on with one click and asks for sign-in with Skip for now (Watch)', async () => {
    const b = backend();
    await new WelcomeView(root, b).start();
    (root.querySelector('[data-focus="watch"]') as HTMLButtonElement).click();
    await flush();
    expect(b.choose).toHaveBeenCalledWith('watch');
    expect(root.querySelector('h1')?.textContent).toBe('Sign in to keep the words you click');
    const skip = [...root.querySelectorAll('button')].find((x) => x.textContent === 'Skip for now');
    expect(skip).toBeDefined();
    skip?.click();
    await flush();
    expect(b.finish).toHaveBeenCalledWith('player');
  });

  it('takes 1, 2, 3 from the keyboard, and Escape as Both', async () => {
    const b = backend();
    await new WelcomeView(root, b).start();
    key('2');
    await flush();
    expect(b.choose).toHaveBeenCalledWith('learn');
    expect(root.querySelector('h1')?.textContent).toBe('Sign in to start your notebook');
    // Learn has no Skip: it offers the player instead.
    expect([...root.querySelectorAll('button')].some((x) => x.textContent === 'Skip for now')).toBe(false);
    expect(root.textContent).toContain('Just want to watch? Videos play without an account.');
    key('Escape');
    await flush();
    expect(root.querySelector('h1')?.textContent).toBe('What would you like to do first?');
    key('Escape');
    await flush();
    expect(b.choose).toHaveBeenLastCalledWith('both');
  });

  it('signs in with one tap on a provider and opens the place the choice picked', async () => {
    const b = backend();
    await new WelcomeView(root, b).start();
    key('3');
    await flush();
    const google = [...root.querySelectorAll('button')].find((x) =>
      x.textContent?.includes('Continue with Google'),
    );
    google?.click();
    await flush();
    await flush();
    expect(b.connect).toHaveBeenCalledWith('google');
    expect(b.finish).toHaveBeenCalledWith('learn');
  });

  it('says plainly when sign-in did not happen, and never names a trial or a price', async () => {
    const b = backend({}, { connect: vi.fn(async () => Promise.reject('cancelled')) });
    await new WelcomeView(root, b).start();
    key('1');
    await flush();
    (root.querySelector('.apple') as HTMLButtonElement).click();
    await flush();
    await flush();
    expect(root.querySelector('.note')?.textContent).toBe('Nothing was signed in.');
    expect(root.textContent).not.toMatch(/trial|Pro\b|days|\$|€/);
  });

  it('goes straight on for a computer that is already signed in', async () => {
    const b = backend({ signedIn: true });
    await new WelcomeView(root, b).start();
    key('1');
    await flush();
    expect(b.finish).toHaveBeenCalledWith('player');
  });
});
