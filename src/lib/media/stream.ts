/**
 * Plays a `Playback` in a `<video>`: `direct` by its address (byte ranges,
 * the element seeks itself); the streamed modes through Media Source
 * Extensions, pulling the core's fragmented MP4 run by run.
 *
 * A run is pulled while the element has less than `AHEAD_S` buffered past
 * the playhead. A seek to a time the element has (or will have soon from
 * the run in flight) just seeks; any other seek asks the core for a new run
 * there (`media.seek`) and pulls that instead. Every run is on one timeline
 * (the core's `-copyts` plus a fixed offset), so what was buffered from
 * earlier runs stays usable. When the SourceBuffer is full, what is well
 * behind the playhead is let go.
 *
 * Seek latency is measured here: from the seek to the first frame shown
 * after it (`requestVideoFrameCallback` where the webview has it).
 */
import { media, type Playback, type Run } from './api.js';

/** Seconds buffered ahead of the playhead before pulling stops. */
const AHEAD_S = 45;
/** Seconds kept behind the playhead when the buffer must make room. */
const BEHIND_S = 20;
/** A seek this far past what is buffered, inside the run in flight, waits for it instead. */
const WAIT_WINDOW_S = 8;
/**
 * A hole this small just ahead of the playhead is jumped. A run's first
 * video frame comes a little after its start time (B-frames delay it), and
 * WebKit, unlike Chromium, waits at such a hole for ever.
 */
const MAX_GAP_S = 1;

type MS = MediaSource;

function mediaSourceClass(): typeof MediaSource | undefined {
  const w = window as unknown as {
    ManagedMediaSource?: typeof MediaSource;
    MediaSource?: typeof MediaSource;
  };
  return w.MediaSource ?? w.ManagedMediaSource;
}

export interface StreamEvents {
  /** Milliseconds from `load` to the first frame. */
  onFirstFrame?: (ms: number) => void;
  /** Milliseconds from a seek to the first frame after it; `fresh` when it needed a new run. */
  onSeeked?: (ms: number, fresh: boolean) => void;
  onError?: (message: string) => void;
}

export class StreamPlayer {
  private playback: Playback | undefined;
  private ms: MS | undefined;
  private sb: SourceBuffer | undefined;
  private run: Run | undefined;
  private runToken = 0;
  private abort: AbortController | undefined;
  private objectUrl: string | undefined;
  private runEnded = false;
  private seekStarted = 0;
  private seekFresh = false;
  /** The next `seeking` is the load placing the playhead. */
  private placing = false;
  private readonly onSeeking = (): void => void this.handleSeeking();

  constructor(
    private readonly video: HTMLVideoElement,
    private readonly events: StreamEvents = {},
  ) {
    video.addEventListener('seeking', this.onSeeking);
    video.addEventListener('timeupdate', () => this.kick());
    video.addEventListener('waiting', () => this.jumpGap());
  }

  /** Moves the playhead over a small hole to the next buffered range. */
  private jumpGap(): boolean {
    const t = this.video.currentTime;
    if (this.buffered(t) !== undefined && this.video.readyState >= HTMLMediaElement.HAVE_FUTURE_DATA)
      return false;
    const b = this.video.buffered;
    for (let i = 0; i < b.length; i++) {
      const start = b.start(i);
      if (start > t && start - t <= MAX_GAP_S) {
        this.placing = true;
        this.video.currentTime = start + 0.001;
        return true;
      }
    }
    return false;
  }

  get current(): Playback | undefined {
    return this.playback;
  }

  /** The file's seconds at the playhead. */
  fileTime(): number {
    return this.video.currentTime - (this.playback?.timeOffset ?? 0);
  }

  async load(p: Playback): Promise<void> {
    // A new audio track or delay keeps the session: only its stream is replaced.
    this.stop(this.playback?.session === p.session);
    this.playback = p;
    const started = performance.now();
    this.firstFrame(() => this.events.onFirstFrame?.(performance.now() - started));
    const startAt = p.resumeAt + p.timeOffset;
    if (p.mode === 'direct') {
      this.video.src = p.src ?? '';
      this.placing = startAt > 0;
      this.video.currentTime = startAt;
      return;
    }
    const MSClass = mediaSourceClass();
    if (MSClass === undefined) {
      this.events.onError?.('This webview has no Media Source Extensions');
      return;
    }
    if (!MSClass.isTypeSupported(p.mime)) {
      this.events.onError?.(`The webview cannot play ${p.mime}`);
      return;
    }
    const ms = new MSClass();
    this.ms = ms;
    // ManagedMediaSource (Safari 17+) plays only without remote playback.
    this.video.disableRemotePlayback = true;
    this.objectUrl = URL.createObjectURL(ms);
    this.video.src = this.objectUrl;
    await new Promise<void>((resolve) => ms.addEventListener('sourceopen', () => resolve(), { once: true }));
    this.sb = ms.addSourceBuffer(p.mime);
    if (p.duration !== null) ms.duration = p.duration + p.timeOffset;
    if (p.run !== undefined) this.startPulling(p.run);
    this.placing = true;
    this.video.currentTime = startAt;
  }

  /** Ends playback and lets the core stop its FFmpeg (unless the session goes on). */
  stop(keepSession = false): void {
    this.abort?.abort();
    this.abort = undefined;
    this.runToken++;
    this.run = undefined;
    if (this.playback !== undefined && !keepSession) void media.close(this.playback.session);
    this.playback = undefined;
    this.sb = undefined;
    this.ms = undefined;
    this.video.removeAttribute('src');
    this.video.load();
    if (this.objectUrl !== undefined) URL.revokeObjectURL(this.objectUrl);
    this.objectUrl = undefined;
  }

  /** Bumped by every load (`load`) and every seek (`seek`), so a frame callback left from an earlier one is ignored. */
  private readonly frameTokens = { load: 0, seek: 0 };

  private firstFrame(done: () => void, kind: 'load' | 'seek' = 'load'): void {
    const token = ++this.frameTokens[kind];
    const once = (): void => {
      if (token === this.frameTokens[kind]) done();
    };
    const v = this.video as HTMLVideoElement & { requestVideoFrameCallback?: (cb: () => void) => number };
    if (typeof v.requestVideoFrameCallback === 'function') v.requestVideoFrameCallback(once);
    else this.video.addEventListener('playing', once, { once: true });
  }

  private buffered(t: number): { start: number; end: number } | undefined {
    const b = this.video.buffered;
    for (let i = 0; i < b.length; i++) {
      // A small gap at a fragment's edge counts as inside.
      if (b.start(i) <= t && t < b.end(i)) return { start: b.start(i), end: b.end(i) };
    }
    return undefined;
  }

  private ahead(): number {
    const r = this.buffered(this.video.currentTime);
    return r === undefined ? 0 : r.end - this.video.currentTime;
  }

  private async handleSeeking(): Promise<void> {
    const p = this.playback;
    if (p === undefined) return;
    const t = this.video.currentTime;
    this.seekStarted = performance.now();
    this.seekFresh = false;
    // Placing the playhead at the start of a load is not a seek.
    if (this.placing) {
      this.placing = false;
      if (p.mode === 'direct' || this.buffered(t) !== undefined || this.run !== undefined) return;
    } else {
      this.firstFrame(
        () => this.events.onSeeked?.(performance.now() - this.seekStarted, this.seekFresh),
        'seek',
      );
    }
    if (p.mode === 'direct' || this.buffered(t) !== undefined) return;
    // Just past the buffer, inside the run in flight: it gets there soon.
    const end = this.bufferedEndOfRun();
    if (!this.runEnded && end !== undefined && t >= end && t - end < WAIT_WINDOW_S) return;
    this.seekFresh = true;
    try {
      const run = await media.seek(p.session, t);
      if (this.playback !== p) return;
      this.startPulling(run);
    } catch (e) {
      this.events.onError?.(String(e));
    }
  }

  /** The end of the buffered range the current run is filling. */
  private bufferedEndOfRun(): number | undefined {
    const run = this.run;
    if (run === undefined) return undefined;
    const r = this.buffered(Math.max(run.start, this.video.currentTime - 0.01)) ?? this.buffered(run.start);
    return r?.end;
  }

  private startPulling(run: Run): void {
    this.abort?.abort();
    this.abort = new AbortController();
    this.run = run;
    this.runEnded = false;
    const token = ++this.runToken;
    // A new run begins with its own init segment: reset the parser, which
    // may hold half a fragment of the old run.
    try {
      if (this.sb !== undefined && this.ms?.readyState === 'open') this.sb.abort();
    } catch {
      /* not updating: nothing to reset */
    }
    void this.pull(run, token, this.abort.signal);
  }

  private wake: (() => void) | undefined;

  private kick(): void {
    if (this.ahead() < AHEAD_S) {
      this.wake?.();
      this.wake = undefined;
    }
  }

  private async pull(run: Run, token: number, signal: AbortSignal): Promise<void> {
    let next = 0;
    while (token === this.runToken && !signal.aborted) {
      if (this.ahead() >= AHEAD_S) {
        await new Promise<void>((resolve) => {
          this.wake = resolve;
          setTimeout(resolve, 1000);
        });
        continue;
      }
      let res: Response;
      try {
        res = await fetch(`${run.url ?? ''}?from=${next}`, { signal });
      } catch {
        if (signal.aborted) return;
        await new Promise((r) => setTimeout(r, 300));
        continue;
      }
      if (token !== this.runToken) return;
      if (res.status === 410) return;
      if (res.status === 204) {
        next = Number(res.headers.get('X-Lexmedia-Next') ?? next);
        continue;
      }
      if (!res.ok) {
        const message = await res.text();
        // A late answer for a stream already replaced says nothing about this one.
        if (token === this.runToken) this.events.onError?.(message);
        return;
      }
      const bytes = await res.arrayBuffer();
      next = Number(res.headers.get('X-Lexmedia-Next') ?? next + bytes.byteLength);
      if (bytes.byteLength > 0) {
        await this.append(bytes, token);
        // The run's first frames may sit just after the playhead.
        if (this.video.readyState < HTMLMediaElement.HAVE_FUTURE_DATA) this.jumpGap();
      }
      if (res.headers.get('X-Lexmedia-End') === '1') {
        this.runEnded = true;
        const d = this.playback?.duration;
        // The run reached the end of the file: the stream is complete.
        if (d !== undefined && d !== null && this.ms?.readyState === 'open' && !this.sb?.updating) {
          try {
            this.ms.endOfStream();
          } catch {
            /* another run will append more */
          }
        }
        return;
      }
    }
  }

  private async append(bytes: ArrayBuffer, token: number): Promise<void> {
    const sb = this.sb;
    if (sb === undefined) return;
    for (let attempt = 0; attempt < 3; attempt++) {
      if (token !== this.runToken) return;
      await this.idle(sb);
      try {
        sb.appendBuffer(bytes);
        await this.idle(sb);
        return;
      } catch (e) {
        if ((e as DOMException).name !== 'QuotaExceededError') throw e;
        // Full: let go of what is well behind the playhead (and far ahead).
        const t = this.video.currentTime;
        if (t - BEHIND_S > 0) {
          sb.remove(0, t - BEHIND_S);
          await this.idle(sb);
        }
        const duration = this.ms?.duration ?? 0;
        if (Number.isFinite(duration) && duration > t + AHEAD_S * 2) {
          sb.remove(t + AHEAD_S * 2, duration);
          await this.idle(sb);
        }
      }
    }
  }

  private idle(sb: SourceBuffer): Promise<void> {
    if (!sb.updating) return Promise.resolve();
    return new Promise((resolve) => sb.addEventListener('updateend', () => resolve(), { once: true }));
  }
}
