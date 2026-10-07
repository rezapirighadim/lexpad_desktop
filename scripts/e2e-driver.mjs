// The browser half of the local end-to-end test (see scripts/e2e-local.sh and
// src-tauri/src/e2e.rs). It plays the learner in a real Chromium:
//
//   1. opens the connect page the Rust test wrote, signs in with the LOCAL demo
//      account, presses Allow, and follows the redirect to the loopback callback;
//   2. opens the built popup page (dist/popup.html) with Tauri's `invoke` pointed
//      at the Rust test's bridge, which answers with the app's own API client;
//      waits for the meanings, presses Enter and sees "Added";
//   3. shows the word and the device in the web app, then the Settings page;
//   4. tells the Rust test it is done (it then signs out and checks the revoke).
//
// Screenshots go to docs/e2e/. Nothing secret is on any of them: no token is
// ever in a page, and the connect page's address bar is not in a screenshot.
//
//   node scripts/e2e-driver.mjs <work dir> <demo account json> <playwright module path>
import { createServer } from 'node:http';
import { existsSync, readFileSync } from 'node:fs';
import { extname, join, resolve } from 'node:path';

const [work, accountFile, playwrightPath] = process.argv.slice(2);
const { chromium } = await import(playwrightPath);
const shots = resolve('docs/e2e');
const account = JSON.parse(readFileSync(accountFile, 'utf8'));

async function waitForFile(name, seconds = 180) {
  const path = join(work, name);
  for (let i = 0; i < seconds * 4; i += 1) {
    if (existsSync(path)) return readFileSync(path, 'utf8').trim();
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`timed out waiting for ${name}`);
}

// The built pages, served as they are.
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const server = createServer((req, res) => {
  const path = join(resolve('dist'), new URL(req.url, 'http://x').pathname);
  if (!path.startsWith(resolve('dist')) || !existsSync(path)) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { 'Content-Type': types[extname(path)] ?? 'application/octet-stream' });
  res.end(readFileSync(path));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const pages = `http://127.0.0.1:${server.address().port}`;

const browser = await chromium.launch();
const context = await browser.newContext({ viewport: { width: 520, height: 760 }, colorScheme: 'light' });
const web = await context.newPage();

// 1. Connect.
const url = new URL(await waitForFile('connect-url.txt'));
// The web app named by LEXPAD_E2E_APP, and only on this computer: never production.
if (url.hostname !== 'localhost' || url.origin !== new URL(process.env.LEXPAD_E2E_APP ?? '').origin) {
  throw new Error('refusing: not the local web app');
}
await web.goto(`${url.origin}/robots.txt`);
await web.evaluate(() => localStorage.setItem('wb.intro', '1'));
await web.goto(url.href);
await web.getByLabel('Email').fill(account.email);
await web.getByLabel('Password').fill(account.password);
await web.screenshot({ path: `${shots}/01-browser-sign-in.png` });
await web.getByRole('button', { name: 'Sign in' }).click();
await web.getByRole('button', { name: 'Allow' }).waitFor({ timeout: 30000 });
await web.screenshot({ path: `${shots}/02-connect-desktop-allow.png` });
const back = web.waitForURL(/^http:\/\/127\.0\.0\.1:\d+\/callback\?/, { timeout: 30000 });
await web.getByRole('button', { name: 'Allow' }).click();
await back;
await web.getByText('Lexpad is connected').waitFor();
await web.screenshot({ path: `${shots}/03-browser-connected.png` });
console.log('connect: done');

// 2. The popup.
const bridge = `http://127.0.0.1:${await waitForFile('bridge-port.txt')}`;
const popup = await context.newPage();
await popup.setViewportSize({ width: 360, height: 640 });
await popup.addInitScript((bridgeUrl) => {
  const callbacks = new Map();
  window.__e2e = { listeners: {}, added: null };
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
        window.__e2e.listeners[args.event] = callbacks.get(args.handler);
        return 1;
      }
      const reply = await fetch(`${bridgeUrl}/invoke`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ cmd, args: args ?? {} }),
      }).then((r) => r.json());
      if (!reply.ok) throw reply.error;
      if (cmd === 'add_word') window.__e2e.added = reply.value;
      return reply.value;
    },
  };
}, bridge);
await popup.goto(`${pages}/popup.html`);
await popup.waitForFunction(() => window.__e2e.listeners['popup:open'] !== undefined);
await popup.evaluate(() =>
  window.__e2e.listeners['popup:open']({ event: 'popup:open', id: 1, payload: null }),
);
await popup.locator('.meaning').first().waitFor({ timeout: 90000 });
await popup.locator('#root').screenshot({ path: `${shots}/04-popup-card.png` });
await popup.keyboard.press('Enter');
await popup.locator('.done').waitFor({ timeout: 30000 });
await popup.locator('#root').screenshot({ path: `${shots}/05-popup-added.png` });
const wordId = await popup.evaluate(() => window.__e2e.added);
console.log('popup: added');

// The same word again: the card says it is already there.
await popup.evaluate(() =>
  window.__e2e.listeners['popup:open']({ event: 'popup:open', id: 2, payload: null }),
);
await popup.getByRole('button', { name: 'Add to Lexpad' }).waitFor({ timeout: 30000 });
await popup.locator('.meaning').first().waitFor({ timeout: 90000 });
await popup.keyboard.press('Enter');
await popup.getByText('This word is already in your notebook.').waitFor({ timeout: 30000 });
await popup.locator('#root').screenshot({ path: `${shots}/05b-popup-already-added.png` });

// 3. The word and the device, in the web app.
await web.goto(`${url.origin}/words/${wordId}`);
await web.getByText('Seen in TextEdit').first().waitFor({ timeout: 30000 });
await web.waitForTimeout(500);
await web.screenshot({ path: `${shots}/06-web-word-seen-in-textedit.png`, fullPage: true });
await web.goto(`${url.origin}/settings/account`);
await web
  .getByText(/Desktop app/)
  .first()
  .waitFor({ timeout: 30000 });
await web
  .getByText(/Desktop app/)
  .first()
  .scrollIntoViewIfNeeded();
await web.screenshot({ path: `${shots}/07-web-signed-in-devices.png` });

const settings = await context.newPage();
await settings.addInitScript((bridgeUrl) => {
  window.__TAURI_INTERNALS__ = {
    transformCallback: () => 1,
    unregisterCallback() {},
    convertFileSrc: (s) => s,
    async invoke(cmd, args) {
      if (cmd.startsWith('plugin:event|')) return 1;
      const reply = await fetch(`${bridgeUrl}/invoke`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ cmd, args: args ?? {} }),
      }).then((r) => r.json());
      if (!reply.ok) throw reply.error;
      return reply.value;
    },
  };
}, bridge);
await settings.setViewportSize({ width: 520, height: 1180 });
await settings.goto(`${pages}/settings.html`);
await settings.getByText('lena@example.test').first().waitFor({ timeout: 30000 });
await settings.screenshot({ path: `${shots}/08-settings.png`, fullPage: true });

// 4. Done: the Rust test signs out and checks the session is revoked.
await fetch(`${bridge}/invoke`, {
  method: 'POST',
  headers: { 'content-type': 'application/json' },
  body: JSON.stringify({ cmd: 'e2e_done', args: {} }),
});
await waitForFile('signed-out.txt', 60);
await web.goto(`${url.origin}/settings/account`);
await web.waitForTimeout(1500);
await web.screenshot({ path: `${shots}/09-web-devices-after-sign-out.png` });
console.log('done');
await browser.close();
server.close();
