/**
 * The commands the core exposes to the windows (src-tauri/src/commands.rs).
 * A refusal arrives as one of the `Failure` words.
 */
import { invoke } from '@tauri-apps/api/core';
import type { AppInfo, Card, Failure, Permission, RecentWord, Settings, State, User } from './types.js';

const KNOWN: ReadonlySet<string> = new Set([
  'signed_out',
  'offline',
  'ai_unavailable',
  'duplicate',
  'no_notebook',
  'no_notebook_for_language',
  'cancelled',
  'timeout',
  'taken',
  'error',
]);

/** The failure a rejected command carries; anything unexpected is `error`. */
export function failureOf(cause: unknown): Failure {
  return typeof cause === 'string' && KNOWN.has(cause) ? (cause as Failure) : 'error';
}

/** Everything the popup needs from the core. Swapped for a fake in tests. */
export interface Backend {
  state(): Promise<State>;
  lookup(notebookId: string, headword: string, hint: string | undefined): Promise<Card>;
  addWord(notebookId: string, word: Record<string, unknown>): Promise<string>;
  setNotebook(notebookId: string): Promise<void>;
  connect(): Promise<User>;
  cancelConnect(): Promise<void>;
  hide(): Promise<void>;
  fit(height: number): Promise<void>;
  accessibility(request: boolean): Promise<Permission>;
  openAccessibilitySettings(): Promise<void>;
  openSettings(): Promise<void>;
}

export interface SettingsBackend {
  state(): Promise<State>;
  setNotebook(notebookId: string): Promise<void>;
  connect(): Promise<User>;
  cancelConnect(): Promise<void>;
  disconnect(): Promise<void>;
  accessibility(request: boolean): Promise<Permission>;
  openAccessibilitySettings(): Promise<void>;
  getSettings(): Promise<Settings>;
  setShortcut(shortcut: string): Promise<string>;
  setStartOnLogin(on: boolean): Promise<boolean>;
  appInfo(): Promise<AppInfo>;
}

/** What the menu-bar / tray panel needs from the core. */
export interface PanelBackend {
  state(): Promise<State>;
  recent(): Promise<RecentWord[]>;
  setNotebook(notebookId: string): Promise<void>;
  connect(): Promise<User>;
  cancelConnect(): Promise<void>;
  /** Opens the add-a-word card for typed text, where the panel was. */
  addTyped(text: string): Promise<void>;
  hidePanel(): Promise<void>;
  fitPanel(height: number): Promise<void>;
  /** The web app in the browser: its home, or one word's page. */
  openWeb(wordId: string | null): Promise<void>;
  openSettings(): Promise<void>;
  openAccessibilitySettings(): Promise<void>;
  quit(): Promise<void>;
}

export const tauriBackend: Backend & SettingsBackend & PanelBackend = {
  state: () => invoke<State>('state'),
  lookup: (notebookId, headword, hint) =>
    invoke<Card>('lookup', { notebookId, headword, hint: hint ?? null }),
  addWord: (notebookId, word) => invoke<string>('add_word', { notebookId, word }),
  setNotebook: (notebookId) => invoke<void>('set_notebook', { notebookId }),
  connect: () => invoke<User>('connect'),
  cancelConnect: () => invoke<void>('cancel_connect'),
  disconnect: () => invoke<void>('disconnect'),
  hide: () => invoke<void>('hide_popup'),
  fit: (height) => invoke<void>('fit_popup', { height }),
  accessibility: (request) => invoke<Permission>('accessibility', { request }),
  openAccessibilitySettings: () => invoke<void>('open_accessibility_settings'),
  openSettings: () => invoke<void>('open_settings'),
  getSettings: () => invoke<Settings>('get_settings'),
  setShortcut: (shortcut) => invoke<string>('set_shortcut', { shortcut }),
  setStartOnLogin: (on) => invoke<boolean>('set_start_on_login', { on }),
  appInfo: () => invoke<AppInfo>('app_info'),
  recent: () => invoke<RecentWord[]>('recent'),
  addTyped: (text) => invoke<void>('panel_add', { text }),
  hidePanel: () => invoke<void>('hide_panel'),
  fitPanel: (height) => invoke<void>('fit_panel', { height }),
  openWeb: (wordId) => invoke<void>('open_web', { wordId }),
  quit: () => invoke<void>('quit'),
};
