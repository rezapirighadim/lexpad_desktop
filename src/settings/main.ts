/** The Settings window's entry. It draws itself again whenever the session or a setting changes. */
import { listen } from '@tauri-apps/api/event';
import { tauriBackend } from '../lib/backend.js';
import { SettingsView } from './view.js';

const root = document.getElementById('root');
if (root === null) throw new Error('settings.html has no #root');

const view = new SettingsView(root, tauriBackend);
void view.render();
void listen('session:changed', () => void view.render());
void listen('settings:changed', () => void view.render());
// macOS: coming back from System Settings shows whether Accessibility is now allowed.
window.addEventListener('focus', () => void view.render());
