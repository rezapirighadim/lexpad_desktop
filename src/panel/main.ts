/**
 * The panel window's entry. The core shows the window under the menu-bar /
 * tray icon and says "panel:open"; the panel reads the state and draws
 * itself, and the window follows its height.
 */
import { listen } from '@tauri-apps/api/event';
import { tauriBackend } from '../lib/backend.js';
import { isMac } from '../lib/shortcut.js';
import { Panel } from './view.js';

const root = document.getElementById('root');
if (root === null) throw new Error('panel.html has no #root');

// macOS draws the panel's rounded corners in the page (the window is
// transparent); Windows 11 rounds an undecorated window itself.
document.documentElement.dataset.platform = isMac() ? 'mac' : 'windows';

const panel = new Panel(root, tauriBackend);

let last = 0;
new ResizeObserver(() => {
  const height = Math.ceil(root.getBoundingClientRect().height);
  if (height !== last) {
    last = height;
    void tauriBackend.fitPanel(height);
  }
}).observe(root);

const redraw = (): void => void panel.open(false);
void listen('panel:open', () => void panel.open(true));
void listen('session:changed', redraw);
void listen('settings:changed', redraw);
void listen('recent:changed', redraw);
