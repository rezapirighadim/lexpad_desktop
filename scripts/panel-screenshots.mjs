// Screenshots of the menu-bar / tray panel for docs/screenshots/, in light and
// dark, signed in and signed out. The shell this was made in may not record
// the screen, so the built page (dist/panel.html) is drawn in Chromium with
// Tauri's `invoke` answered by the sample below. The sample lives only in
// this script: it is never bundled and never reaches the app.
//
//   pnpm build && node scripts/panel-screenshots.mjs <playwright module path>
//   e.g. node scripts/panel-screenshots.mjs ../front/node_modules/.pnpm/playwright@1.62.1/node_modules/playwright/index.mjs
import { createServer } from 'node:http';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { extname, join, resolve } from 'node:path';

const [playwrightPath] = process.argv.slice(2);
if (!playwrightPath) throw new Error('usage: node scripts/panel-screenshots.mjs <playwright module path>');
const { chromium } = await import(resolve(playwrightPath));
const shots = resolve('docs/screenshots');
mkdirSync(shots, { recursive: true });

const dist = resolve('dist');
const types = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.css': 'text/css',
  '.woff2': 'font/woff2',
};
const server = createServer((req, res) => {
  const path = join(dist, new URL(req.url, 'http://x').pathname);
  if (!path.startsWith(dist) || !existsSync(path)) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { 'Content-Type': types[extname(path)] ?? 'application/octet-stream' });
  res.end(readFileSync(path));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const pages = `http://127.0.0.1:${server.address().port}`;

// Screenshot sample (see the note at the top).
const now = Date.now();
const signedIn = {
  state: {
    connected: true,
    user: { id: 'u1', email: 'lena@example.com', displayName: 'Lena' },
    notebooks: [
      { id: 'N1', title: 'English', targetLang: 'en', meaningLang: 'fa', isDefault: true },
      { id: 'N2', title: 'German', targetLang: 'de', meaningLang: 'en', isDefault: false },
    ],
    notebookId: 'N1',
    capture: null,
    shortcut: 'CommandOrControl+Shift+L',
    permission: 'granted',
    version: '0.1.0',
  },
  recent: [
    { id: 'W5', headword: 'serendipity', notebookId: 'N1', userId: 'u1', addedAt: now - 2 * 60_000 },
    { id: 'W4', headword: 'candid', notebookId: 'N1', userId: 'u1', addedAt: now - 25 * 60_000 },
    { id: 'W3', headword: 'Fernweh', notebookId: 'N2', userId: 'u1', addedAt: now - 3 * 3_600_000 },
    { id: 'W2', headword: 'take for granted', notebookId: 'N1', userId: 'u1', addedAt: now - 26 * 3_600_000 },
    { id: 'W1', headword: 'meticulous', notebookId: 'N1', userId: 'u1', addedAt: now - 4 * 86_400_000 },
  ],
};
const signedOut = {
  state: { ...signedIn.state, connected: false, user: null, notebooks: [], notebookId: null },
  recent: [],
};

const browser = await chromium.launch();
async function shoot(name, sample, colorScheme, platform = 'MacIntel') {
  const context = await browser.newContext({
    viewport: { width: 340, height: 600 },
    deviceScaleFactor: 2,
    colorScheme,
  });
  const page = await context.newPage();
  await page.addInitScript(
    ({ sample, platform }) => {
      Object.defineProperty(navigator, 'platform', { get: () => platform });
      const callbacks = new Map();
      window.__shots = { listeners: {} };
      window.__TAURI_INTERNALS__ = {
        transformCallback(cb) {
          const id = callbacks.size + 1;
          callbacks.set(id, cb);
          return id;
        },
        unregisterCallback(id) {
          callbacks.delete(id);
        },
        convertFileSrc: (s) => s,
        async invoke(cmd, args) {
          if (cmd === 'plugin:event|listen') {
            window.__shots.listeners[args.event] = callbacks.get(args.handler);
            return 1;
          }
          if (cmd === 'state') return sample.state;
          if (cmd === 'recent') return sample.recent;
          return null;
        },
      };
    },
    { sample, platform },
  );
  await page.goto(`${pages}/panel.html`);
  await page.waitForFunction(() => window.__shots.listeners['panel:open'] !== undefined);
  await page.evaluate(() =>
    window.__shots.listeners['panel:open']({ event: 'panel:open', id: 1, payload: null }),
  );
  await page.locator('.foot').waitFor();
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(300); // the opening animation
  await page.locator('#root').screenshot({ path: `${shots}/${name}.png`, omitBackground: true });
  await context.close();
  console.log(`${name}.png`);
}

await shoot('panel-light', signedIn, 'light');
await shoot('panel-dark', signedIn, 'dark');
await shoot('panel-signed-out-light', signedOut, 'light');
await shoot('panel-signed-out-dark', signedOut, 'dark');
await shoot('panel-windows-light', signedIn, 'light', 'Win32');
await browser.close();
server.close();
