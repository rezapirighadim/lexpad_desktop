import { cpSync, existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { defineConfig, type Plugin } from 'vite';

const dist = resolve(import.meta.dirname, 'dist');
const web = resolve(import.meta.dirname, 'web');

/** Every file under `dir`, as paths relative to it. */
function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path).map((f) => join(name, f)) : [name];
  });
}

/**
 * Lexpad's window is the web app built by scripts/build-web.sh into web/. It
 * is copied beside the three small pages after they are built, with its
 * index.html at the root (the window's page). A file of the same name on
 * both sides must be the same file (the shared fonts, named by their
 * content's hash); anything else is an error, never an overwrite.
 */
function webApp(): Plugin {
  return {
    name: 'lexpad:web-app',
    apply: 'build',
    closeBundle() {
      if (!existsSync(join(web, 'index.html'))) {
        throw new Error('web/ has no build of the web app: run scripts/build-web.sh');
      }
      for (const file of files(web)) {
        if (file === 'PROVENANCE.txt') continue;
        const to = join(dist, file);
        if (existsSync(to)) {
          if (readFileSync(to).equals(readFileSync(join(web, file)))) continue;
          throw new Error(`web/${file} would overwrite ${relative(dist, to)}`);
        }
        cpSync(join(web, file), to);
      }
    },
  };
}

// Three pages, one per small window: the add-a-word popup, the menu-bar /
// tray panel and Settings; and Lexpad's window, the web app from web/.
// Everything is bundled; the app loads no remote code (see the CSP in
// tauri.conf.json).
export default defineConfig({
  root: 'src',
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  plugins: [webApp()],
  build: {
    outDir: dist,
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
