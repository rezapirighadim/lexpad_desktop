/** The first launch's window (src-tauri/src/welcome.rs). */
import { invoke } from '@tauri-apps/api/core';
import { WelcomeView, type Focus, type WelcomeBackend, type WelcomeState } from './view.js';

const root = document.getElementById('root');
if (root === null) throw new Error('welcome.html has no #root');

const backend: WelcomeBackend = {
  state: () => invoke<WelcomeState>('welcome_state'),
  choose: (focus: Focus) => invoke<undefined>('welcome_choose', { focus }),
  connect: async (provider) => {
    await invoke('connect', provider === undefined ? {} : { provider });
  },
  cancelConnect: () => invoke<undefined>('cancel_connect'),
  finish: (target) => invoke<undefined>('welcome_finish', { target }),
};

void new WelcomeView(root, backend).start();
