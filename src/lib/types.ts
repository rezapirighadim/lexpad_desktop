/**
 * The shapes that cross from the core (src-tauri) to the windows. The core
 * holds the session and calls the API; the windows render and ask through
 * the commands in `backend.ts`. No token ever reaches this side.
 */

/** A card as the API writes it (a subset of WordCreate the card shows). Same as the extension's. */
export interface Card {
  headword: string;
  pos?: string;
  level?: string;
  pronunciation?: string;
  correction?: string;
  category?: string;
  meanings?: { meaning: string; gloss?: string; examples?: { sentence: string; translation?: string }[] }[];
  collocations?: string[];
  [key: string]: unknown;
}

export interface Notebook {
  id: string;
  title: string;
  targetLang: string;
  meaningLang: string;
  isDefault: boolean;
}

export interface User {
  id: string;
  email: string;
  displayName: string;
}

export type Permission = 'granted' | 'missing' | 'not_needed';

/** What the shortcut (or Services, or the tray) found in the app in front. */
export interface Capture {
  /** The selected text, trimmed; absent when nothing was selected or readable. */
  text: string | null;
  /** Text around the selection, when the platform's accessibility interface gives it. */
  context: string | null;
  /** The app's display name ("TextEdit", "Microsoft Word"); never a window title. */
  app: string | null;
  permission: Permission;
  via: 'accessibility' | 'clipboard' | 'service' | 'typed' | 'none';
}

export interface State {
  connected: boolean;
  user: User | null;
  notebooks: Notebook[];
  /**
   * Whether `notebooks` is the API's answer of this read, not the copy kept
   * from an earlier one (offline, a server error, a cold start). Only a known
   * empty list means the account has no notebook yet.
   */
  notebooksKnown: boolean;
  notebookId: string | null;
  capture: Capture | null;
  shortcut: string;
  permission: Permission;
  /** Accessibility was allowed in an earlier version, and macOS dropped it with the update. */
  permissionStale?: boolean;
  version: string;
  /** `services` in the Mac App Store build: the shortcut is the Services menu's key. */
  shortcutVia?: 'services';
  /**
   * Whether that service is live yet (the core's probe, services_live.rs):
   * `pending` while macOS reads the new app's services on a first launch,
   * `otherCopy` while the copy from lexpad.app runs and takes the key.
   */
  services?: 'pending' | 'live' | 'otherCopy' | 'notFound';
}

/** Where a word was met: the sentence (or none) and the app. */
export interface Source {
  sentence: string;
  app: string | null;
}

export type Failure =
  | 'signed_out'
  | 'offline'
  | 'ai_unavailable'
  | 'duplicate'
  | 'no_notebook'
  /** The word is in a language none of this account's notebooks holds. */
  | 'no_notebook_for_language'
  | 'cancelled'
  | 'timeout'
  | 'taken'
  /** The surface's daily number of saved words is used up; nothing was saved. */
  | 'daily_limit'
  /** The surface's daily AI cards are used up; the word can be saved without a meaning. */
  | 'cards_used_up'
  | 'error';

/**
 * What a daily number that refused says (the core's `LimitNotice`): the
 * server's own numbers, never typed here.
 */
export interface LimitNotice {
  kind: 'words' | 'cards';
  surface: 'desktop' | 'player';
  perDay: number;
  left: number;
  /** The learner's next midnight (RFC 3339). */
  resetsAt: string;
  plan: 'free' | 'pro';
  /** Pro's number on this surface, for a Free account. */
  proPerDay?: number;
}

export interface Settings {
  shortcut: string;
  startOnLogin: boolean;
  /** Open Lexpad's window when the app starts. */
  openOnLaunch: boolean;
  /** "Open Lexpad" goes to the browser instead of the app's window. */
  openInBrowser: boolean;
  developmentBuild: boolean;
}

export interface AppInfo {
  version: string;
  apiOrigin: string;
  appOrigin: string;
  autostartEnabled: boolean;
}

/** A word added from this computer, for the panel's list (newest first). */
export interface RecentWord {
  id: string;
  headword: string;
  notebookId: string;
  userId: string;
  /** Milliseconds since the Unix epoch. */
  addedAt: number;
}
