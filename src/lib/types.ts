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
  notebookId: string | null;
  capture: Capture | null;
  shortcut: string;
  permission: Permission;
  /** Accessibility was allowed in an earlier version, and macOS dropped it with the update. */
  permissionStale?: boolean;
  version: string;
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
  | 'error';

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
