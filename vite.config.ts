import { resolve } from 'node:path';
import { defineConfig } from 'vite';

// Three pages, one per window: the add-a-word popup, the menu-bar / tray
// panel and Settings. Everything is
// bundled; the app loads no remote code (see the CSP in tauri.conf.json).
export default defineConfig({
  root: 'src',
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    outDir: resolve(import.meta.dirname, 'dist'),
    emptyOutDir: true,
    target: ['safari15', 'chrome110'],
    sourcemap: false,
    rollupOptions: {
      input: {
        popup: resolve(import.meta.dirname, 'src/popup.html'),
        panel: resolve(import.meta.dirname, 'src/panel.html'),
        settings: resolve(import.meta.dirname, 'src/settings.html'),
      },
    },
  },
});
