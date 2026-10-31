/**
 * The video player's media engine, as the page sees it: typed wrappers for
 * the core's commands (src-tauri/src/media/commands.rs, word_queue.rs).
 * Files are ids the core handed out for what the learner dropped or chose;
 * a page never passes a path. Errors are the core's short words
 * (`MediaError`), which the page turns into its own text.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type MediaError =
  | 'unknown_file'
  | 'ffmpeg_missing'
  | 'unreadable'
  | 'no_video'
  | 'no_encoder'
  | 'unsupported_subtitle'
  | 'unreadable_subtitle'
  | 'empty_subtitle'
  | 'too_big'
  | 'file_gone'
  | 'no_session'
  | 'timeout'
  | 'cancelled';

export type FileKind = 'video' | 'subtitle' | 'unsupportedSubtitle' | 'other';

export interface FileRef {
  id: string;
  name: string;
  kind: FileKind;
  size: number;
}

/** A drop or an open: play the first video (it replaces the current one), ask about each subtitle. */
export interface Dropped {
  videos: FileRef[];
  subtitles: FileRef[];
  unsupported: string[];
}

export type Encoder = 'videoToolbox' | 'nvenc' | 'qsv' | 'amf' | 'mediaFoundation' | 'openH264';

export interface Engine {
  available: boolean;
  version?: string;
  caps?: { h264: Encoder | null; hwaccel: string | null; zscale: boolean };
  licence: string;
  baseUrl: string;
  /** The engine makes the sound louder itself (`boost`), for volume above 100% on macOS. */
  audioBoost?: boolean;
}

export type Hdr = 'pq' | 'hlg' | 'dolbyVisionOnly';

export interface VideoStream {
  index: number;
  codec: string;
  profile: string | null;
  level: number | null;
  pixFmt: string | null;
  width: number;
  height: number;
  sar: [number, number];
  fps: number | null;
  bitDepth: number;
  interlaced: boolean;
  hdr?: Hdr;
  default: boolean;
}

export interface AudioStream {
  index: number;
  codec: string;
  profile: string | null;
  channels: number;
  layout: string | null;
  sampleRate: number | null;
  /** ISO 639-1, when the container's tag is a language the core knows. */
  language: string | null;
  languageTag: string | null;
  title: string | null;
  default: boolean;
}

export interface SubtitleStream {
  index: number;
  codec: string;
  kind: 'text' | 'image' | 'unknown';
  extractAs: SubtitleFormat | null;
  language: string | null;
  languageTag: string | null;
  title: string | null;
  default: boolean;
  /** Signs only. A full track the file flags forced (its only one in that language, hundreds of lines) comes as false. */
  forced: boolean;
  hearingImpaired: boolean;
  /** How many lines the container says the track holds (Matroska's statistics), when it says. */
  events?: number;
}

/** `media:subtitle-progress`: an embedded track being read out of a big file or one on a slow drive. */
export interface SubtitleProgress {
  fileId: string;
  stream: number;
  /** 0..1 of the film's length. */
  fraction: number;
}

export interface Chapter {
  /** The file's seconds (add `timeOffset` for the element's). */
  start: number;
  end: number;
  title: string | null;
}

export interface MediaInfo {
  container: string;
  brand?: string;
  duration: number | null;
  startTime: number;
  size: number;
  bitRate: number | null;
  video: VideoStream[];
  audio: AudioStream[];
  subtitles: SubtitleStream[];
  chapters: Chapter[];
}

export type Mode = 'direct' | 'remux' | 'audioTranscode' | 'videoTranscode';

export interface Plan {
  mode: Mode;
  video: {
    index: number;
    copy: boolean;
    encoder?: Encoder;
    width: number;
    height: number;
    tonemap: boolean;
    deinterlace: boolean;
  };
  audio?: { index: number; copy: boolean; delayMs: number; downmix: boolean; gain?: number };
  mime: string;
  timeOffset: number;
  reasons: string[];
}

export type Unplayable = { reason: 'noVideo' } | { reason: 'noEncoder' };

export interface Probe {
  info: MediaInfo;
  plan?: Plan;
  unplayable?: Unplayable;
}

/** Where a video was left, and the learner's choices for it. */
export interface Saved {
  position: number;
  duration: number;
  watched: boolean;
  updated: number;
  audio?: number;
  audioDelayMs?: number;
  subtitleOffsets?: Record<string, number>;
}

export interface Run {
  /** Pull with `?from=<offset>`. */
  url?: string;
  /** Where the run starts, in the element's seconds. */
  start: number;
}

export interface Playback {
  session: string;
  file: FileRef;
  mode: Mode;
  /** `direct`: the `<video src>`. */
  src?: string;
  /** Streamed modes: the first run. */
  run?: Run;
  mime: string;
  /** Element seconds = file seconds + timeOffset. */
  timeOffset: number;
  duration: number | null;
  plan: Plan;
  info: MediaInfo;
  /** The file's seconds where playback starts. */
  resumeAt: number;
  saved: Saved;
}

export type SubtitleFormat = 'srt' | 'vtt' | 'ass';

export interface Span {
  text: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
  color?: string;
}

export interface Cue {
  /** Milliseconds of the file's timeline. */
  start: number;
  end: number;
  text: string;
  spans: Span[];
  /** Keypad position (1-9); absent is bottom centre. */
  align?: number;
  /** An exact place, fractions of the frame. */
  position?: [number, number];
  style?: string;
}

export interface Style {
  name: string;
  font: string;
  /** Fraction of the frame's height. */
  size: number;
  color: string;
  outlineColor: string;
  bold: boolean;
  italic: boolean;
  align: number;
}

export interface LanguageGuess {
  code: string;
  name: string;
  confidence: number;
  source: 'text' | 'fileName' | 'track';
}

export interface Track {
  key: string;
  format: SubtitleFormat;
  cues: Cue[];
  styles: Style[];
  encoding: { encoding: string; how: 'bom' | 'utf8' | 'utf16' | 'detected'; replaced: boolean };
  language: { best?: LanguageGuess; text?: LanguageGuess; fileName?: string; track?: string };
  persianLettersFixed: boolean;
  /** Punctuation typed at the start of right-to-left lines was put back at their end. */
  punctuationMoved: boolean;
}

export interface EmbeddedSubtitle {
  key: string;
  stream: number;
  codec: string;
  supported: boolean;
  language?: string;
  title?: string;
  default: boolean;
  forced: boolean;
  hearingImpaired: boolean;
}

export interface ExternalSubtitle {
  key: string;
  file: FileRef;
  /** 2 named after the video, 1 the same episode, 0 only beside it. */
  fit: number;
  hint: { lang?: string; forced: boolean; sdh: boolean };
}

export interface Subtitles {
  embedded: EmbeddedSubtitle[];
  external: ExternalSubtitle[];
}

export interface Episode {
  file: FileRef;
  season?: number;
  episode?: number;
  position: number;
  duration: number;
  watched: boolean;
}

export interface Episodes {
  items: Episode[];
  current: number;
  next?: FileRef;
  previous?: FileRef;
}

export interface DefaultPlayerStatus {
  types: { ext: string; subtitle: boolean; isDefault: boolean; current?: string; caution: boolean }[];
  /** `direct`: `request` sets it (macOS). `systemSettings`: it opens the system's page (Windows). */
  method: 'direct' | 'systemSettings';
}

/** "Continue watching": a file watched lately on this computer. */
export interface RecentEntry {
  key: string;
  /** Granted again when the file is still where it was. */
  fileId?: string;
  name: string;
  position: number;
  duration: number;
  watched: boolean;
  /** Seconds since 1970. */
  updated: number;
  missing: boolean;
  season?: number;
  episode?: number;
  /** A JPEG of the frame at `position`. */
  thumb?: string;
}

export interface MediaPrefs {
  rememberRecent: boolean;
  previousHandlers: Record<string, string>;
}

/** While a run converts: the element's seconds ready so far. */
export interface Progress {
  session: string;
  run: number;
  readyUntil: number;
  etaSeconds?: number;
}

/** Frames are made every this many seconds of the file (the core's thumbs::STEP_S). */
export const FRAME_STEP_S = 10;

/** The seek bar's preview frame of a file at `seconds` (the file's time). */
export function frameUrl(baseUrl: string, fileId: string, seconds: number): string {
  const s = Math.max(0, Math.floor(seconds / FRAME_STEP_S) * FRAME_STEP_S);
  return `${baseUrl}/t/${fileId}/${s * 1000}.jpg`;
}

/**
 * Where in the player a word was picked. Passed as `from` to `words_save`,
 * and to `lookup` / `add_word` when the player calls them; the core refuses
 * any other value. Without it `lookup` / `add_word` are the popup's.
 */
export type PickedIn = 'subtitles' | 'transcript';

export interface NewWord {
  headword: string;
  /** ISO 639-1 of the subtitle it came from. */
  lang?: string;
  sentence?: string;
  memo?: string;
  /** Where in the player it was picked, for the activity reports; absent means the subtitles. */
  from?: PickedIn;
}

export type SavedWord =
  { status: 'added'; id: string; notebookId: string } | { status: 'queued'; pending: number; reason: string };

export interface PendingWord {
  id: string;
  headword: string;
  lang?: string;
  sentence?: string;
  memo?: string;
  savedAt: number;
  account?: string;
  waiting?: string;
}

export const media = {
  engine: (): Promise<Engine> => invoke('media_engine'),
  pick: (): Promise<Dropped> => invoke('media_pick'),
  takePending: (): Promise<Dropped | null> => invoke('media_take_pending'),
  probe: (fileId: string): Promise<Probe> => invoke('media_probe', { fileId }),
  /** `at`: the file's seconds; absent resumes where it was left. */
  open: (
    fileId: string,
    opts: { audio?: number; audioDelayMs?: number; at?: number; boost?: boolean } = {},
  ): Promise<Playback> => invoke('media_open', { fileId, ...opts }),
  /** `at`: the element's seconds. */
  seek: (session: string, at: number): Promise<Run> => invoke('media_seek', { session, at }),
  /** A new audio track or delay; `at` in the element's seconds. The mode may change. */
  setAudio: (
    session: string,
    audio: number | undefined,
    audioDelayMs: number,
    at: number,
    boost?: boolean,
  ): Promise<Playback> => invoke('media_set_audio', { session, audio, audioDelayMs, at, boost }),
  close: (session: string): Promise<void> => invoke('media_close', { session }),
  subtitles: (fileId: string): Promise<Subtitles> => invoke('media_subtitles', { fileId }),
  /** An embedded track (`stream` of the video) or a subtitle file (`fileId` alone). */
  loadSubtitle: (fileId: string, stream?: number): Promise<Track> =>
    invoke('media_subtitle_load', stream === undefined ? { fileId } : { fileId, stream }),
  saveSubtitleOffset: (fileId: string, key: string, offsetMs: number): Promise<void> =>
    invoke('media_subtitle_offset', { fileId, key, offsetMs }),
  episodes: (fileId: string): Promise<Episodes> => invoke('media_episodes', { fileId }),
  /** The file's seconds. */
  saveResume: (fileId: string, position: number, duration: number): Promise<Saved> =>
    invoke('media_resume_save', { fileId, position, duration }),
  defaultPlayer: {
    status: (types?: string[]): Promise<DefaultPlayerStatus> =>
      invoke('media_default_player_status', types === undefined ? {} : { types }),
    /** Only after the learner confirmed. */
    request: (types: string[]): Promise<DefaultPlayerStatus> =>
      invoke('media_default_player_request', { types }),
  },
  onFiles: (handler: (d: Dropped) => void): Promise<UnlistenFn> =>
    listen<Dropped>('media:files', (e) => handler(e.payload)),
  /** How far an embedded subtitle track is being read (only while it takes a while). */
  onSubtitleProgress: (handler: (p: SubtitleProgress) => void): Promise<UnlistenFn> =>
    listen<SubtitleProgress>('media:subtitle-progress', (e) => handler(e.payload)),
};

/** Words saved from the player: to the notebook now, or kept on this computer until signed in or online. */
export const words = {
  save: (word: NewWord): Promise<SavedWord> => invoke('words_save', { word }),
  pending: (): Promise<PendingWord[]> => invoke('words_pending'),
  flush: (): Promise<number> => invoke('words_flush'),
  discard: (id: string): Promise<number> => invoke('words_discard', { id }),
  /**
   * Sends one waiting word now, on the learner's tap, whatever its age
   * (e.g. `waiting: 'too_old_to_move'`). Refused ('not_this_account') for a
   * word another account saved; 'duplicate' when it is already there.
   */
  sendOne: (id: string): Promise<SavedWord> => invoke('words_send_one', { id }),
  onPending: (handler: (waiting: number) => void): Promise<UnlistenFn> =>
    listen<number>('words:pending', (e) => handler(e.payload)),
};
