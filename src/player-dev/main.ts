/**
 * The media engine's development page (cargo feature `player-dev`, never
 * shipped): open or drop videos and subtitles, play them through the
 * engine, show up to three subtitle tracks with their own offsets, switch
 * the audio track and its delay, go through a series' episodes, resume.
 * With `LEXPAD_PLAYER_BENCH=<folder>` it plays every video there and
 * reports start-up and seek times (scripts/player-bench.sh).
 *
 * Deliberately plain: the real player's design comes from the designer.
 */
import { invoke } from '@tauri-apps/api/core';
import {
  media,
  type Dropped,
  type Episodes,
  type FileRef,
  type Playback,
  type Subtitles,
  type Track,
} from '../lib/media/api.js';
import {
  activeCues,
  fileMs,
  isRtl,
  lastCueBefore,
  MAX_TRACKS,
  OFFSET_STEP_MS,
  offsetToStartNow,
} from '../lib/media/cues.js';
import { StreamPlayer } from '../lib/media/stream.js';

function el<T extends HTMLElement>(id: string): T {
  const e = document.getElementById(id);
  if (e === null) throw new Error(`#${id} missing`);
  return e as T;
}

// Errors and progress reach the benchmark's log (stdout), since a release
// build has no inspector.
const report = (r: Record<string, unknown>): void => void invoke('media_dev_report', { report: r });
window.addEventListener('error', (e) => report({ phase: 'error', message: String(e.message) }));
window.addEventListener('unhandledrejection', (e) => report({ phase: 'error', message: String(e.reason) }));
report({ phase: 'loaded', mse: 'MediaSource' in window, managed: 'ManagedMediaSource' in window });

const video = el<HTMLVideoElement>('video');
const overlay = el<HTMLDivElement>('overlay');
const status = el<HTMLSpanElement>('status');
const logBox = el<HTMLPreElement>('log');
const info = el<HTMLPreElement>('info');
const audioSel = el<HTMLSelectElement>('audio');
const delay = el<HTMLInputElement>('delay');
const slotsBox = el<HTMLDivElement>('slots');
const episodesList = el<HTMLOListElement>('episodes');

let firstFrameWaiter: ((ms: number) => void) | undefined;
let seekWaiter: ((ms: number, fresh: boolean) => void) | undefined;

const player = new StreamPlayer(video, {
  onFirstFrame: (ms) => {
    log(`first frame ${ms.toFixed(0)} ms`);
    firstFrameWaiter?.(ms);
    firstFrameWaiter = undefined;
  },
  onSeeked: (ms, fresh) => {
    log(`seek ${fresh ? '(new run)' : '(buffered)'} ${ms.toFixed(0)} ms`);
    seekWaiter?.(ms, fresh);
    seekWaiter = undefined;
  },
  onError: (m) => {
    status.textContent = `Error: ${m}`;
    log(`error: ${m}`);
  },
});

function log(line: string): void {
  logBox.textContent = `${new Date().toISOString().slice(11, 19)} ${line}\n${logBox.textContent ?? ''}`.slice(
    0,
    8000,
  );
}

interface Slot {
  track: Track | undefined;
  offset: number;
  select: HTMLSelectElement;
  label: HTMLSpanElement;
}
const slots: Slot[] = [];
let current: Playback | undefined;
let subtitles: Subtitles | undefined;
let droppedSubs: FileRef[] = [];
let episodes: Episodes | undefined;

function option(value: string, text: string): HTMLOptionElement {
  const o = document.createElement('option');
  o.value = value;
  o.textContent = text;
  return o;
}

function buildSlots(): void {
  slotsBox.replaceChildren();
  slots.length = 0;
  for (let i = 0; i < MAX_TRACKS; i++) {
    const row = document.createElement('div');
    row.className = 'slot';
    const select = document.createElement('select');
    const label = document.createElement('span');
    const minus = document.createElement('button');
    minus.textContent = `−${OFFSET_STEP_MS}`;
    const plus = document.createElement('button');
    plus.textContent = `+${OFFSET_STEP_MS}`;
    const spoken = document.createElement('button');
    spoken.textContent = 'Line just spoken';
    const slot: Slot = { track: undefined, offset: 0, select, label };
    const setOffset = (ms: number): void => {
      slot.offset = ms;
      label.textContent = describe(slot);
      if (current !== undefined && slot.track !== undefined) {
        void media.saveSubtitleOffset(current.file.id, slot.track.key, ms);
      }
    };
    minus.onclick = () => setOffset(slot.offset - OFFSET_STEP_MS);
    plus.onclick = () => setOffset(slot.offset + OFFSET_STEP_MS);
    spoken.onclick = () => {
      if (slot.track === undefined || current === undefined) return;
      const t = fileMs(video.currentTime, current.timeOffset);
      const cue = lastCueBefore(slot.track.cues, t, slot.offset);
      if (cue !== undefined) setOffset(offsetToStartNow(cue, t));
    };
    select.onchange = () => void loadSlot(slot);
    row.append(`Track ${i + 1}`, select, minus, plus, spoken, label);
    slotsBox.append(row);
    slots.push(slot);
  }
}

function describe(slot: Slot): string {
  const t = slot.track;
  if (t === undefined) return '';
  const lang = t.language.best;
  return `${t.cues.length} lines · ${t.format} · ${t.encoding.encoding} (${t.encoding.how})${t.persianLettersFixed ? ' · ی/ک fixed' : ''} · ${lang ? `${lang.name} ${Math.round(lang.confidence * 100)}% (${lang.source})` : 'language unknown'} · offset ${slot.offset} ms`;
}

function fillSlotChoices(): void {
  for (const slot of slots) {
    const keep = slot.select.value;
    slot.select.replaceChildren(option('', '— none —'));
    for (const e of subtitles?.embedded ?? []) {
      const o = option(
        `embedded:${e.stream}`,
        `Embedded #${e.stream} ${e.codec} ${e.language ?? '?'} ${e.title ?? ''}`,
      );
      o.disabled = !e.supported;
      if (!e.supported) o.textContent += ' (pictures: not supported)';
      slot.select.append(o);
    }
    for (const x of subtitles?.external ?? []) {
      slot.select.append(
        option(`file:${x.file.id}`, `${x.file.name} (fit ${x.fit}${x.hint.lang ? `, ${x.hint.lang}` : ''})`),
      );
    }
    for (const f of droppedSubs) slot.select.append(option(`file:${f.id}`, `${f.name} (dropped)`));
    slot.select.value = keep;
  }
}

async function loadSlot(slot: Slot): Promise<void> {
  const v = slot.select.value;
  slot.track = undefined;
  slot.label.textContent = '';
  if (v === '' || current === undefined) return;
  try {
    const started = performance.now();
    slot.track = v.startsWith('embedded:')
      ? await media.loadSubtitle(current.file.id, Number(v.slice(9)))
      : await media.loadSubtitle(v.slice(5));
    slot.offset = current.saved.subtitleOffsets?.[slot.track.key] ?? 0;
    slot.label.textContent = describe(slot);
    log(`subtitle ${slot.track.key} loaded in ${(performance.now() - started).toFixed(0)} ms`);
  } catch (e) {
    slot.label.textContent = `Error: ${String(e)}`;
  }
}

function render(): void {
  overlay.replaceChildren();
  if (current !== undefined) {
    const t = fileMs(video.currentTime, current.timeOffset);
    slots.forEach((slot, i) => {
      if (slot.track === undefined) return;
      for (const cue of activeCues(slot.track.cues, t, slot.offset)) {
        const line = document.createElement('div');
        line.className = `line t${i}`;
        line.dir = isRtl(cue.text) ? 'rtl' : 'ltr';
        for (const s of cue.spans) {
          const span = document.createElement('span');
          span.textContent = s.text;
          if (s.bold) span.style.fontWeight = '700';
          if (s.italic) span.style.fontStyle = 'italic';
          if (s.underline) span.style.textDecoration = 'underline';
          if (s.color) span.style.color = s.color;
          line.append(span);
        }
        if (cue.align !== undefined && cue.align >= 7) {
          line.style.top = '6%';
          line.style.bottom = 'auto';
        }
        overlay.append(line);
      }
    });
  }
  requestAnimationFrame(render);
}

function fillAudio(p: Playback): void {
  audioSel.replaceChildren();
  for (const a of p.info.audio) {
    const o = option(
      String(a.index),
      `#${a.index} ${a.codec} ${a.channels}ch ${a.language ?? a.languageTag ?? ''} ${a.title ?? ''}`,
    );
    audioSel.append(o);
  }
  audioSel.value = String(p.plan.audio?.index ?? '');
  delay.value = String(p.plan.audio?.delayMs ?? 0);
}

function showInfo(p: Playback): void {
  const v = p.info.video[0];
  info.textContent = [
    p.file.name,
    `mode: ${p.mode}`,
    ...p.plan.reasons.map((r) => `  · ${r}`),
    `mime: ${p.mime}`,
    `container: ${p.info.container}`,
    v
      ? `video: ${v.codec} ${v.profile ?? ''} ${v.width}x${v.height} ${v.bitDepth}-bit ${v.hdr ?? 'SDR'} ${v.fps?.toFixed(2) ?? '?'} fps`
      : 'no video',
    `out: ${p.plan.video.width}x${p.plan.video.height}${p.plan.video.encoder ? ` with ${p.plan.video.encoder}` : ''}${p.plan.video.tonemap ? ' tone-mapped' : ''}`,
    `duration: ${p.duration?.toFixed(1) ?? '?'} s · chapters: ${p.info.chapters.map((c) => c.title ?? '').join(', ') || 'none'}`,
    `resume at ${p.resumeAt.toFixed(1)} s`,
  ].join('\n');
}

async function openVideo(file: FileRef, opts: { at?: number } = {}): Promise<void> {
  status.textContent = `Opening ${file.name}…`;
  try {
    const p = await media.open(file.id, opts);
    current = p;
    showInfo(p);
    fillAudio(p);
    await player.load(p);
    void video.play().catch(() => undefined);
    status.textContent = `${file.name} · ${p.mode}`;
    subtitles = await media.subtitles(file.id);
    fillSlotChoices();
    // The best matched subtitle file goes to track 1.
    const best = subtitles.external[0];
    const first = slots[0];
    if (first !== undefined && best !== undefined && best.fit > 0) {
      first.select.value = `file:${best.file.id}`;
      await loadSlot(first);
    }
    episodes = await media.episodes(file.id);
    showEpisodes();
  } catch (e) {
    status.textContent = `Error: ${String(e)}`;
  }
}

function showEpisodes(): void {
  episodesList.replaceChildren();
  for (const [i, e] of (episodes?.items ?? []).entries()) {
    const li = document.createElement('li');
    li.textContent = `${e.file.name}${e.watched ? ' ✓' : e.position > 0 ? ` (${Math.round(e.position)} s)` : ''}`;
    if (i === episodes?.current) li.className = 'current';
    li.onclick = () => void openVideo(e.file);
    episodesList.append(li);
  }
}

async function onFiles(d: Dropped): Promise<void> {
  droppedSubs = [...droppedSubs, ...d.subtitles];
  if (d.unsupported.length > 0) log(`not supported: ${d.unsupported.join(', ')}`);
  const v = d.videos[0];
  if (v !== undefined) await openVideo(v);
  else fillSlotChoices();
}

audioSel.onchange = () => void changeAudio();
delay.onchange = () => void changeAudio();
async function changeAudio(): Promise<void> {
  if (current === undefined) return;
  const p = await media.setAudio(
    current.session,
    Number(audioSel.value),
    Number(delay.value) || 0,
    video.currentTime,
  );
  current = p;
  showInfo(p);
  await player.load(p);
  void video.play().catch(() => undefined);
}

el<HTMLButtonElement>('open').onclick = async () => onFiles(await media.pick());
el<HTMLButtonElement>('next').onclick = () => episodes?.next && void openVideo(episodes.next);
el<HTMLButtonElement>('prev').onclick = () => episodes?.previous && void openVideo(episodes.previous);
// Series: the next episode plays by itself.
video.addEventListener('ended', () => {
  if (current !== undefined)
    void media.saveResume(current.file.id, current.duration ?? 0, current.duration ?? 0);
  if (episodes?.next !== undefined) void openVideo(episodes.next);
});
setInterval(() => {
  if (current !== undefined && !video.paused) {
    void media.saveResume(current.file.id, player.fileTime(), current.duration ?? 0);
  }
}, 5000);
video.addEventListener('pause', () => {
  if (current !== undefined) void media.saveResume(current.file.id, player.fileTime(), current.duration ?? 0);
});

buildSlots();
requestAnimationFrame(render);
void media.onFiles((d) => void onFiles(d));
void media.engine().then((e) => {
  el('engine').textContent = e.available
    ? `${e.version ?? 'FFmpeg'} · H.264: ${e.caps?.h264 ?? 'none'} · hw decode: ${e.caps?.hwaccel ?? 'none'} · zscale: ${e.caps?.zscale ? 'yes' : 'no'} · MSE: ${'MediaSource' in window ? 'yes' : 'ManagedMediaSource' in window ? 'managed' : 'no'}`
    : 'FFmpeg missing (scripts/ffmpeg/)';
});
void media.takePending().then((d) => d && onFiles(d));
void bench();

// The benchmark.
function waitFirstFrame(timeoutMs: number): Promise<number> {
  return new Promise((resolve) => {
    firstFrameWaiter = resolve;
    setTimeout(() => resolve(-1), timeoutMs);
  });
}
function waitSeek(timeoutMs: number): Promise<number> {
  return new Promise((resolve) => {
    seekWaiter = (ms) => resolve(ms);
    setTimeout(() => resolve(-1), timeoutMs);
  });
}
const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

async function bench(): Promise<void> {
  const d = await invoke<Dropped | null>('media_dev_bench_files');
  if (d === null) return;
  video.muted = true;
  const soakMs = Number(new URLSearchParams(location.search).get('soak') ?? '40000');
  for (const file of d.videos) {
    try {
      report({ phase: 'start', file: file.name });
      const started = performance.now();
      const firstFrame = waitFirstFrame(30_000);
      const p = await media.open(file.id, { at: 0 });
      current = p;
      showInfo(p);
      await player.load(p);
      void video.play().catch(() => undefined);
      const first = await firstFrame;
      const total = first < 0 ? -1 : performance.now() - started;
      await sleep(1500);
      const duration = p.duration ?? 30;
      const seeks: { to: number; ms: number }[] = [];
      for (const share of [0.7, 0.2, 0.9, 0.45]) {
        const to = duration * share + p.timeOffset;
        const w = waitSeek(30_000);
        video.currentTime = to;
        const ms = Math.round(await w);
        seeks.push({ to: Math.round(to), ms });
        if (ms < 0) {
          const b = video.buffered;
          const ranges = Array.from(
            { length: b.length },
            (_, k) => `${b.start(k).toFixed(1)}-${b.end(k).toFixed(1)}`,
          );
          report({
            phase: 'seek-timeout',
            file: file.name,
            to,
            at: video.currentTime,
            ranges,
            readyState: video.readyState,
            paused: video.paused,
            error: video.error?.message ?? null,
          });
        }
        await sleep(1200);
      }
      const back = waitSeek(10_000);
      video.currentTime = Math.max(p.timeOffset, video.currentTime - 1);
      const buffered = Math.round(await back);
      // Another audio track, where there is one: time to the first frame after the switch.
      let audioSwitchMs: number | null = null;
      const other = p.info.audio.find((a) => a.index !== p.plan.audio?.index);
      if (other !== undefined && current !== undefined) {
        const t0 = performance.now();
        const ff = waitFirstFrame(30_000);
        const switched = await media.setAudio(current.session, other.index, 0, video.currentTime);
        current = switched;
        await player.load(switched);
        void video.play().catch(() => undefined);
        const got = await ff;
        audioSwitchMs = got < 0 ? -1 : Math.round(performance.now() - t0);
        await sleep(1500);
        const back = await media.setAudio(switched.session, p.plan.audio?.index, 0, video.currentTime);
        current = back;
        await player.load(back);
        void video.play().catch(() => undefined);
        await sleep(1500);
      }
      // Plays a while for the CPU and memory samples (scripts/player-bench.sh):
      // from near the start, as long as the file allows.
      const soak = Math.min(soakMs, Math.max(5000, (duration - 3) * 1000));
      video.currentTime = p.timeOffset + 1;
      void invoke('media_dev_report', { report: { file: file.name, phase: 'soak', ms: soak, mode: p.mode } });
      await sleep(soak);
      const quality = (
        video as HTMLVideoElement & { getVideoPlaybackQuality?: () => VideoPlaybackQuality }
      ).getVideoPlaybackQuality?.();
      await invoke('media_dev_report', {
        report: {
          file: file.name,
          mode: p.mode,
          encoder: p.plan.video.encoder ?? null,
          out: `${p.plan.video.width}x${p.plan.video.height}`,
          firstFrameMs: Math.round(first),
          openToFirstFrameMs: Math.round(total),
          seeks,
          bufferedSeekMs: buffered,
          audioSwitchMs,
          droppedFrames: quality?.droppedVideoFrames ?? null,
          totalFrames: quality?.totalVideoFrames ?? null,
          error: status.textContent?.startsWith('Error') ? status.textContent : null,
        },
      });
      player.stop();
      status.textContent = '';
    } catch (e) {
      report({ file: file.name, phase: 'failed', error: String(e) });
      player.stop();
    }
  }
  await invoke('media_dev_report', { report: { done: true } });
}
