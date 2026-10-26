import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  COPY,
  FILM,
  REFUSED_MS,
  StartView,
  failureText,
  type OpenResult,
  type StartBackend,
  type StartState,
} from './view.js';

type Fake = StartBackend & {
  openVideo: ReturnType<typeof vi.fn>;
  connect: ReturnType<typeof vi.fn>;
  cancelConnect: ReturnType<typeof vi.fn>;
  signedIn: ReturnType<typeof vi.fn>;
  close: ReturnType<typeof vi.fn>;
  drag: (kind: 'enter' | 'leave' | 'drop') => void;
  refuse: (why: 'subOnly' | 'notVideo') => void;
};

function backend(state: Partial<StartState> = {}, over: Partial<StartBackend> = {}): Fake {
  const full: StartState = { os: 'macos', signedIn: false, ...state };
  let dragHandler: (kind: 'enter' | 'leave' | 'drop') => void = () => undefined;
  let refusedHandler: (why: 'subOnly' | 'notVideo') => void = () => undefined;
  return {
    state: vi.fn(async () => full),
    openVideo: vi.fn(async (): Promise<OpenResult> => 'opened'),
    connect: vi.fn(async () => undefined),
    cancelConnect: vi.fn(async () => undefined),
    signedIn: vi.fn(async () => undefined),
    close: vi.fn(async () => undefined),
    onDrag: (h) => {
      dragHandler = h;
    },
    onRefused: (h) => {
      refusedHandler = h;
    },
    drag: (k) => {
      dragHandler(k);
    },
    refuse: (w) => {
      refusedHandler(w);
    },
    ...over,
  } as Fake;
}

const media = { loop: 'loop.mp4', still: 'still.webp' };
const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0));
let root: HTMLElement;

beforeEach(() => {
  document.body.innerHTML = '<main id="root"></main>';
  document.documentElement.removeAttribute('data-keyboard');
  root = document.getElementById('root')!;
});

async function draw(b: Fake, env: { online?: boolean; reduced?: boolean } = {}): Promise<StartView> {
  const view = new StartView(root, b, media, {
    online: () => env.online ?? true,
    reducedMotion: () => env.reduced ?? false,
  });
  await view.start();
  return view;
}

function buttons(): HTMLButtonElement[] {
  return [...root.querySelectorAll('button')];
}

function key(k: string, mods: Partial<KeyboardEventInit> = {}): void {
  document.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, ...mods }));
}

describe('the Start window', () => {
  it('shows what Lexpad does and the two things to do, no question and no Skip', async () => {
    await draw(backend());
    expect(root.querySelector('h1')?.textContent).toBe('Watch. Click a word.Keep it.');
    expect(root.querySelector('h1 br')).not.toBeNull();
    expect(root.querySelector('.lead')?.textContent).toBe(COPY.lead);
    expect(buttons().map((b) => b.textContent)).toEqual([
      'Open a video…⌘O',
      'Continue with Google',
      'Continue with Apple',
      'Other ways to sign in',
    ]);
    expect(root.textContent).toContain(COPY.dropHint);
    expect(root.textContent).toContain(COPY.sep);
    expect(root.textContent).toContain(COPY.fine);
    expect(root.textContent).not.toMatch(/What would you like|Skip|Both|1 of 2/);
  });

  it('shows the real Player, with the film credit readable and the picture described', async () => {
    await draw(backend());
    const pane = root.querySelector('.pane');
    expect(pane?.getAttribute('role')).toBe('img');
    expect(pane?.getAttribute('aria-label')).toBe(FILM.alt);
    expect(root.querySelector('.credit')?.textContent).toBe(FILM.credit);
    expect(root.querySelector('.credit')?.getAttribute('aria-hidden')).toBeNull();
    expect(FILM.credit).toContain('Tears of Steel (CC) Blender Foundation | mango.blender.org');
    expect(root.querySelector<HTMLImageElement>('.still')?.getAttribute('src')).toBe('still.webp');
    expect(root.querySelector<HTMLVideoElement>('video.loop')?.getAttribute('src')).toBe('loop.mp4');
  });

  it('plays the loop twice, then rests on the card', async () => {
    await draw(backend());
    const video = root.querySelector<HTMLVideoElement>('video.loop')!;
    video.play = vi.fn(async () => undefined);
    const shot = root.querySelector('.shot')!;
    video.dispatchEvent(new Event('ended'));
    expect(video.play).toHaveBeenCalledTimes(1);
    expect(shot.classList.contains('rested')).toBe(false);
    video.dispatchEvent(new Event('ended'));
    expect(video.play).toHaveBeenCalledTimes(1);
    expect(shot.classList.contains('rested')).toBe(true);
  });

  it('with reduced motion shows the still card and no video at all', async () => {
    await draw(backend(), { reduced: true });
    expect(root.querySelector('video')).toBeNull();
    expect(root.querySelector('.shot')?.classList.contains('rested')).toBe(true);
    expect(root.querySelector('.still')).not.toBeNull();
  });

  it('focuses Open a video… first, with no ring until a key is pressed', async () => {
    await draw(backend());
    const primary = root.querySelector('.primary');
    expect(document.activeElement).toBe(primary);
    expect(primary?.getAttribute('aria-keyshortcuts')).toBe('Meta+O');
    expect(primary?.querySelector('.keys')?.getAttribute('aria-hidden')).toBe('true');
    expect(document.documentElement.dataset.keyboard).toBeUndefined();
    key('Tab');
    expect(document.documentElement.dataset.keyboard).toBe('');
  });

  it('says Ctrl O on Windows and answers Ctrl+O there', async () => {
    const b = backend({ os: 'windows' });
    await draw(b);
    expect(root.querySelector('.primary .keys')?.textContent).toBe('CtrlO');
    expect(root.querySelector('.primary')?.getAttribute('aria-keyshortcuts')).toBe('Control+O');
    key('o', { metaKey: true });
    expect(b.openVideo).not.toHaveBeenCalled();
    key('o', { ctrlKey: true });
    expect(b.openVideo).toHaveBeenCalledTimes(1);
  });

  it('opens a video with the button or ⌘O, and stays when the dialog is cancelled', async () => {
    const b = backend({}, { openVideo: vi.fn(async (): Promise<OpenResult> => 'cancelled') });
    await draw(b);
    (root.querySelector('.primary') as HTMLButtonElement).click();
    key('o', { metaKey: true });
    await flush();
    expect(b.openVideo).toHaveBeenCalledTimes(2);
    expect(root.querySelector('.drop')?.classList.contains('on')).toBe(false);
  });

  it('Escape does nothing; ⌘W closes', async () => {
    const b = backend();
    await draw(b);
    key('Escape');
    expect(b.close).not.toHaveBeenCalled();
    expect(root.querySelector('h1')).not.toBeNull();
    key('w', { metaKey: true });
    expect(b.close).toHaveBeenCalledTimes(1);
  });

  it('waits for the browser on the pressed button, keeps Open a video… on, and offers Cancel', async () => {
    let finish: () => void = () => undefined;
    const b = backend(
      {},
      {
        connect: vi.fn(
          () =>
            new Promise<void>((r) => {
              finish = r;
            }),
        ),
      },
    );
    await draw(b);
    (root.querySelector('[data-provider="google"]') as HTMLButtonElement).click();
    await flush();
    expect(b.connect).toHaveBeenCalledWith('google');
    const google = root.querySelector<HTMLButtonElement>('[data-provider="google"]')!;
    const apple = root.querySelector<HTMLButtonElement>('[data-provider="apple"]')!;
    expect(google.textContent).toBe(COPY.waiting);
    expect(google.querySelector('.spinner')).not.toBeNull();
    expect(apple.disabled).toBe(true);
    expect(root.querySelector<HTMLButtonElement>('.primary')?.disabled).toBe(false);
    expect(root.querySelector('.foot .fine')?.textContent).toBe(COPY.waitingNote);
    const cancel = buttons().find((x) => x.textContent === COPY.cancel)!;
    expect(document.activeElement).toBe(cancel);
    cancel.click();
    expect(b.cancelConnect).toHaveBeenCalled();
    finish();
    await flush();
    expect(b.signedIn).toHaveBeenCalledTimes(1);
  });

  it('says why a sign-in did not finish, in one status line, and lets the learner try again', async () => {
    const b = backend({}, { connect: vi.fn(async () => Promise.reject('timeout')) });
    await draw(b);
    (root.querySelector('[data-provider="apple"]') as HTMLButtonElement).click();
    await flush();
    const line = root.querySelector('.foot .fine.bad');
    expect(line?.textContent).toBe(COPY.timeout);
    expect(line?.getAttribute('role')).toBe('status');
    expect(root.querySelector<HTMLButtonElement>('[data-provider="apple"]')?.disabled).toBe(false);
    expect(root.querySelector('[data-provider="apple"]')?.textContent).toBe(COPY.apple);
    expect(b.signedIn).not.toHaveBeenCalled();
  });

  it('offline, says so without opening the browser, and videos still open', async () => {
    const b = backend();
    await draw(b, { online: false });
    (root.querySelector('[data-provider="google"]') as HTMLButtonElement).click();
    await flush();
    expect(b.connect).not.toHaveBeenCalled();
    expect(root.querySelector('.foot .fine.bad')?.textContent).toBe(COPY.offline);
    expect(root.querySelector<HTMLButtonElement>('.primary')?.disabled).toBe(false);
  });

  it('names each failure plainly', () => {
    expect(failureText('cancelled', true)).toBe(COPY.cancelled);
    expect(failureText('offline', true)).toBe(COPY.offline);
    expect(failureText('anything', false)).toBe(COPY.offline);
    expect(failureText('timeout', true)).toBe(COPY.timeout);
    expect(failureText('denied', true)).toBe(COPY.failed);
  });

  it('Other ways to sign in connects without a provider', async () => {
    const b = backend();
    await draw(b);
    buttons()
      .find((x) => x.textContent === COPY.other)
      ?.click();
    await flush();
    expect(b.connect).toHaveBeenCalledWith(undefined);
  });

  it('shows Drop to play over the window while files are dragged over it', async () => {
    const b = backend();
    await draw(b);
    const drop = root.querySelector('.drop')!;
    b.drag('enter');
    expect(drop.classList.contains('on')).toBe(true);
    expect(drop.textContent).toContain(COPY.dropTitle);
    expect(drop.textContent).toContain(COPY.dropNote);
    b.drag('leave');
    expect(drop.classList.contains('on')).toBe(false);
  });

  it('says why a drop was refused, for a moment', async () => {
    vi.useFakeTimers();
    try {
      const b = backend();
      await draw(b);
      const drop = root.querySelector('.drop')!;
      b.drag('enter');
      b.drag('drop');
      b.refuse('subOnly');
      expect(drop.classList.contains('on')).toBe(true);
      expect(drop.querySelector('.drop-title')?.textContent).toBe('Drop the video too');
      expect(drop.querySelector('.drop-note')?.textContent).toBe('A subtitle needs its video.');
      vi.advanceTimersByTime(REFUSED_MS);
      expect(drop.classList.contains('on')).toBe(false);
      b.refuse('notVideo');
      expect(drop.querySelector('.drop-title')?.textContent).toBe("This isn't a video");
      expect(drop.querySelector('.drop-note')?.textContent).toBe('Lexpad plays video files.');
    } finally {
      vi.useRealTimers();
    }
  });

  it('a picked file that is not a video is refused the same way', async () => {
    const b = backend({}, { openVideo: vi.fn(async (): Promise<OpenResult> => 'notVideo') });
    await draw(b);
    (root.querySelector('.primary') as HTMLButtonElement).click();
    await flush();
    expect(root.querySelector('.drop')?.textContent).toContain(COPY.dropNotVideo);
  });
});
