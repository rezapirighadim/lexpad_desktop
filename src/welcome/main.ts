/** The Start window (src-tauri/src/welcome.rs). */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { StartView, type OpenResult, type StartBackend, type StartState } from './view.js';

const root = document.getElementById('root');
if (root === null) throw new Error('welcome.html has no #root');

const win = getCurrentWebviewWindow();

const backend: StartBackend = {
  state: () => invoke<StartState>('welcome_state'),
  openVideo: () => invoke<OpenResult>('start_open_video'),
  connect: async (provider) => {
    await invoke('connect', provider === undefined ? {} : { provider });
  },
  cancelConnect: () => invoke<undefined>('cancel_connect'),
  signedIn: () => invoke<undefined>('start_signed_in'),
  close: () => win.close(),
  onDrag: (handler) => {
    void win.onDragDropEvent((e) => {
      const kind = e.payload.type;
      if (kind === 'enter') handler('enter');
      else if (kind === 'leave') handler('leave');
      else if (kind === 'drop') handler('drop');
    });
  },
  onRefused: (handler) => {
    void listen<string>('start:refused', (e) => {
      if (e.payload === 'subOnly' || e.payload === 'notVideo') handler(e.payload);
    });
  },
};

const media = {
  loop: new URL('../assets/welcome/start-loop.mp4', import.meta.url).href,
  still: new URL('../assets/welcome/start-card.webp', import.meta.url).href,
};

void new StartView(root, backend, media).start();
