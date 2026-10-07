/**
 * The popup window's entry. The core shows the window and says "popup:open";
 * the card reads the state and draws itself, and the window follows its height.
 */
import { listen } from '@tauri-apps/api/event';
import { tauriBackend } from '../lib/backend.js';
import { Popup } from './view.js';

const root = document.getElementById('root');
if (root === null) throw new Error('popup.html has no #root');

const popup = new Popup(root, tauriBackend);

// The window is exactly as tall as the card, whatever the card is showing.
let last = 0;
new ResizeObserver(() => {
  const height = Math.ceil(root.getBoundingClientRect().height);
  if (height !== last) {
    last = height;
    void tauriBackend.fit(height);
  }
}).observe(root);

void listen('popup:open', () => void popup.open());
void listen('session:changed', () => void popup.open());
