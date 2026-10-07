// The browser half of the end-to-end test of Lexpad's window (see
// scripts/e2e-main.sh and src-tauri/src/e2e_main.rs). It plays the learner in
// a real Chromium against the LOCAL stack:
//
//   1. opens the window's page (the built dist/index.html, the web app) with
//      window.__LEXPAD_CONFIG as the app injects it and Tauri's invoke pointed
//      at the Rust test's bridge, which answers with the app's own core code;
//   2. presses "Continue in your browser", signs in on the local web app's
//      /connect-desktop with the LOCAL demo account and presses Allow;
//   3. sees Today, starts a practice session, answers cards, then answers
//      more with the network cut (the page's IndexedDB carries on) and sees
//      them reach the server when it is back;
//   4. opens a word the way the panel does (the core's "main:open");
//   5. signs out from the app's own Settings, through the core.
//
// Screenshots go to docs/e2e/main-*.png. No token is ever in the page.
//
//   node scripts/e2e-main.mjs <work dir> <demo account json> <playwright module path> <api origin>
import { createServer } from 'node:http';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { extname, join, resolve } from 'node:path';

const [work, accountFile, playwrightPath, apiOrigin] = process.argv.slice(2);
if (!/^http:\/\/(localhost|127\.0\.0\.1):\d+$/.test(apiOrigin)) throw new Error('refusing: not a local API');
const { chromium } = await import(playwrightPath);
const shots = resolve('docs/e2e');
const account = JSON.parse(readFileSync(accountFile, 'utf8'));
const log = [];
const note = (line) => {
  log.push(line);
  console.log(line);
};

async function waitForFile(name, seconds = 180) {
  const path = join(work, name);
  for (let i = 0; i < seconds * 4; i += 1) {
    if (existsSync(path)) return readFileSync(path, 'utf8').trim();
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`timed out waiting for ${name}`);
}

// The built window, served as it is.
const types = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.css': 'text/css',
  '.woff2': 'font/woff2',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
  '.json': 'application/json',
};
const root = resolve('dist');
const server = createServer((req, res) => {
  const path = join(root, new URL(req.url, 'http://x').pathname);
  if (!path.startsWith(root) || !existsSync(path)) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { 'Content-Type': types[extname(path)] ?? 'application/octet-stream' });
  res.end(readFileSync(path));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const pages = `http://127.0.0.1:${server.address().port}`;
const bridge = `http://127.0.0.1:${await waitForFile('bridge-port.txt')}`;

// Whatever happens, tell the Rust half the browser is done, so a failure
// here ends the test at once instead of at its time limit.
async function finish() {
  await fetch(`${bridge}/invoke`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ cmd: 'e2e_done', args: {} }),
  }).catch(() => undefined);
}
process.on('uncaughtException', async (e) => {
  console.error(e);
  writeFileSync(join(work, 'driver-failed.txt'), String(e));
  await finish();
  process.exit(1);
});
process.on('unhandledRejection', async (e) => {
  console.error(e);
  writeFileSync(join(work, 'driver-failed.txt'), String(e));
  await finish();
  process.exit(1);
});

const browser = await chromium.launch();
const context = await browser.newContext({ viewport: { width: 1040, height: 760 }, colorScheme: 'light' });
const main = await context.newPage();
main.on('pageerror', (e) => note(`page error: ${e.message}`));
main.on('console', (m) => {
  if (m.type() === 'error' || m.type() === 'warning') note(`console ${m.type()}: ${m.text().slice(0, 300)}`);
});

await main.addInitScript(
  ({ bridgeUrl, api }) => {
    window.__LEXPAD_CONFIG = Object.freeze({
      platform: 'desktop',
      apiUrl: api,
      appVersion: '0.2.0-e2e',
      deviceId: 'e2e-main-window',
    });
    const callbacks = new Map();
    let next = 1;
    window.__e2e = { listeners: {}, calls: 0 };
    const call = async (cmd, args) => {
      const reply = await fetch(`${bridgeUrl}/invoke`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ cmd, args }),
      }).then((r) => r.json());
      if (!reply.ok) throw reply.error;
      return reply.value;
    };
    // What Tauri's event plugin keeps beside its IPC; unlisten reaches for it.
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    // A first-time visitor would see the introduction first; the test goes
    // straight to signing in, as the web app's own end-to-end tests do.
    try {
      localStorage.setItem('wb.intro', '1');
    } catch {
      /* nothing to skip */
    }
    window.__TAURI_INTERNALS__ = {
      transformCallback(cb) {
        const id = next++;
        callbacks.set(id, cb);
        return id;
      },
      unregisterCallback(id) {
        callbacks.delete(id);
      },
      convertFileSrc: (s) => s,
      async invoke(cmd, args = {}) {
        if (cmd === 'plugin:event|listen') {
          window.__e2e.listeners[args.event] = callbacks.get(args.handler);
          return 1;
        }
        if (cmd === 'plugin:event|unlisten') return null;
        if (cmd === 'api_fetch') {
          window.__e2e.calls += 1;
          // The bridge answers the whole body; the channel is fed here, the
          // way the core feeds it piece by piece.
          let reply;
          try {
            reply = await call(cmd, { request: args.request });
          } catch {
            throw 'offline';
          }
          const events = args.events;
          setTimeout(() => {
            if (reply.body !== '') events.onmessage({ kind: 'chunk', data: reply.body });
            events.onmessage({ kind: 'end' });
          }, 0);
          return { status: reply.status, headers: reply.headers };
        }
        return call(cmd, args);
      },
    };
  },
  { bridgeUrl: bridge, api: apiOrigin },
);

// 1-2. Sign in through the browser.
await main.goto(`${pages}/index.html`);
await main.getByRole('button', { name: 'Continue in your browser' }).waitFor({ timeout: 30000 });
await main.screenshot({ path: `${shots}/main-01-sign-in.png` });
await main.getByRole('button', { name: 'Continue in your browser' }).click();
const url = new URL(await waitForFile('connect-url.txt'));
if (url.origin !== 'http://localhost:4173') throw new Error('refusing: not the local web app');
const web = await context.newPage();
await web.goto(`${url.origin}/robots.txt`);
await web.evaluate(() => localStorage.setItem('wb.intro', '1'));
await web.goto(url.href);
await web.getByLabel('Email').fill(account.email);
await web.getByLabel('Password').fill(account.password);
await web.getByRole('button', { name: 'Sign in' }).click();
await web
  .getByRole('button', { name: 'Allow' })
  .waitFor({ timeout: 30000 })
  .catch(async (cause) => {
    await web.screenshot({ path: join(work, 'failed-connect.png') });
    note(`connect page: ${(await web.textContent('body'))?.slice(0, 400)}`);
    throw cause;
  });
const back = web.waitForURL(/^http:\/\/127\.0\.0\.1:\d+\/callback\?/, { timeout: 30000 });
await web.getByRole('button', { name: 'Allow' }).click();
await back;
await web.close();
note('connect: done');

// 3. Today, then practice.
const start = main.getByRole('button', { name: /^(Start session|Practise anyway)$/ });
await start.waitFor({ timeout: 90000 });
await main.waitForTimeout(1500);
await main.screenshot({ path: `${shots}/main-02-today.png` });
note('today: shown');
await start.click();
await main.getByRole('button', { name: 'Close' }).waitFor({ timeout: 30000 });
await main.waitForTimeout(600);
await main.screenshot({ path: `${shots}/main-03-practice-card.png` });

/** The session's counter, "3/25". */
const counter = () =>
  main
    .getByText(/^\d+\/\d+$/)
    .first()
    .textContent()
    .catch(() => '');

/**
 * Answers the card on screen with the keys the session takes (space flips,
 * right arrow grades a flashcard; 1-4 picks a choice, Enter goes on; a typed
 * answer is typed), until the counter moves. True when it did.
 */
async function answerOne() {
  const before = await counter();
  const attempts = [
    async () => {
      const show = main.getByRole('button', { name: 'Show answer' });
      if (!(await show.isVisible().catch(() => false))) return;
      await show.click();
      await main.getByRole('button', { name: 'Knew it' }).click();
    },
    async () => {
      await main.keyboard.press('1');
      await main.waitForTimeout(400);
      await main.keyboard.press('Enter');
    },
    async () => {
      const input = main.locator('main input[type="text"], main input:not([type])').first();
      if (!(await input.isVisible().catch(() => false))) return;
      await input.fill('x');
      await main.keyboard.press('Enter');
      await main.waitForTimeout(400);
      await main.keyboard.press('Enter');
    },
    async () => {
      await main.keyboard.press('Space');
      await main.waitForTimeout(300);
      await main.keyboard.press('ArrowRight');
    },
  ];
  for (const attempt of attempts) {
    await attempt();
    await main.waitForTimeout(700);
    if ((await counter()) !== before) return true;
  }
  return false;
}

/** Items the server has answered by refusing them (see docs/e2e/README.md). */
const refused = () =>
  main.evaluate(
    () =>
      new Promise((resolve) => {
        const open = indexedDB.open('lexpad');
        open.onsuccess = () => {
          const all = open.result.transaction('outbox', 'readonly').objectStore('outbox').getAll();
          all.onsuccess = () => resolve(all.result.filter((i) => i.parked === true).length);
        };
      }),
  );

const outbox = () =>
  main.evaluate(
    () =>
      new Promise((resolve) => {
        const open = indexedDB.open('lexpad');
        open.onsuccess = () => {
          // What is still waiting to be sent. A parked item has had the
          // server's answer (it refused it) and is not waiting any more.
          const all = open.result.transaction('outbox', 'readonly').objectStore('outbox').getAll();
          all.onsuccess = () => resolve(all.result.filter((i) => i.parked !== true).length);
        };
        open.onerror = () => resolve(-1);
      }),
  );

let online = 0;
for (let i = 0; i < 2; i += 1) if (await answerOne()) online += 1;
await main.waitForTimeout(1500);
note(`practice: answered ${online} online (${await counter()}), outbox ${await outbox()}`);
if (online === 0) throw new Error('no card could be answered');

// Offline: the core cannot be reached; the session carries on from IndexedDB.
await context.setOffline(true);
const before = await main.evaluate(() => window.__e2e.calls);
let offline = 0;
for (let i = 0; i < 2; i += 1) if (await answerOne()) offline += 1;
await main.waitForTimeout(800);
const queued = await outbox();
await main.screenshot({ path: `${shots}/main-04-practice-offline.png` });
note(`offline: answered ${offline} more (${await counter()}), outbox ${queued}`);
if (queued < 1 && offline > 0) throw new Error('offline answers were not queued');
await context.setOffline(false);
let left = queued;
for (let i = 0; i < 60 && left > 0; i += 1) {
  await main.waitForTimeout(1000);
  left = await outbox();
}
note(
  `online again: outbox ${left} after sync, ${await main.evaluate(() => window.__e2e.calls)} calls (was ${before})`,
);
if (left !== 0) {
  const items = await main.evaluate(
    () =>
      new Promise((resolve) => {
        const open = indexedDB.open('lexpad');
        open.onsuccess = () => {
          const all = open.result.transaction('outbox', 'readonly').objectStore('outbox').getAll();
          all.onsuccess = () =>
            resolve(all.result.map((i) => ({ entity: i.entity, attempts: i.attempts, error: i.lastError })));
        };
      }),
  );
  note(`outbox: ${JSON.stringify(items).slice(0, 800)}`);
  throw new Error('the queued answers did not reach the server');
}
await main.keyboard.press('Escape');
await main.waitForTimeout(500);

// 4. A word opened from the panel: the core says "main:open".
const wordId = await main.evaluate(
  () =>
    new Promise((resolve) => {
      const open = indexedDB.open('lexpad');
      open.onsuccess = () => {
        const tx = open.result.transaction('words', 'readonly');
        const cursor = tx.objectStore('words').openCursor();
        cursor.onsuccess = () => resolve(cursor.result ? cursor.result.value.id : null);
      };
    }),
);
if (wordId) {
  await main.evaluate((id) => {
    window.__e2e.listeners['main:open']?.({ event: 'main:open', id: 9, payload: `/words/${id}` });
  }, wordId);
  await main.waitForTimeout(1500);
  note(`open word: ${await main.evaluate(() => location.hash)}`);
  await main.screenshot({ path: `${shots}/main-05-word-from-panel.png` });
}

// 5. This computer: the desktop section of the app's Settings.
await main.evaluate(() => {
  location.hash = '#/settings/desktop';
});
await main.getByRole('heading', { name: 'This computer' }).waitFor({ timeout: 30000 });
await main.waitForTimeout(800);
await main.screenshot({ path: `${shots}/main-06-settings-this-computer.png`, fullPage: true });
note('settings: This computer shown');

// 6. Sign out from the app's own Settings, through the core.
await main.evaluate(() => {
  location.hash = '#/settings/account';
});
// The account's own row (its name carries the e-mail), not a device's "Sign out".
const signOutRow = main.getByRole('button', {
  name: new RegExp(`^Sign out\\s*${account.email.replace('.', '\\.')}`),
});
await signOutRow.waitFor({ timeout: 30000 });
await main.screenshot({ path: `${shots}/main-07-settings-account.png` });
await signOutRow.click();
await main.getByRole('dialog').getByRole('button', { name: 'Sign out' }).click();
await waitForFile('signed-out.txt', 30).catch(async (cause) => {
  await main.screenshot({ path: join(work, 'failed-sign-out.png') });
  note(
    `sign out: ${(
      await main
        .getByRole('status')
        .allTextContents()
        .catch(() => [])
    ).join(' | ')}`,
  );
  throw cause;
});
await main.getByRole('button', { name: 'Continue in your browser' }).waitFor({ timeout: 30000 });
note('sign out: back to "Continue in your browser"');

writeFileSync(join(work, 'main-log.txt'), log.join('\n') + '\n');
await fetch(`${bridge}/invoke`, {
  method: 'POST',
  headers: { 'content-type': 'application/json' },
  body: JSON.stringify({ cmd: 'e2e_done', args: {} }),
});
await browser.close();
server.close();
note('done');
