// Busy identities and rendering are shared by settlement and the launch ledger.
import { beforeAll, afterAll, expect, test } from 'bun:test';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { extname, resolve } from 'node:path';
import { webCompiler } from './module.mjs';
import { engineTests } from '../../scripts/coverage.mjs';
import { suiteBrowser } from '../../scripts/agent-launch.mjs';

const root = resolve(import.meta.dir, '../..');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.wasm': 'application/wasm' };
const APP = `component App
  state spin = true
  action stop
    spin = false
  view
    column
      button "stop" testId="stop" press=stop
      if spin
        progress aria-label="Working" testId="spin"
      dialog id="sheet"
        progress aria-label="Loading"
      view id="popup" popover="manual"
        progress testId="pop-spin"
      box display="none" testId="hidden"
        progress testId="hidden-spin"
`;

async function built() {
  const dir = mkdtempSync(resolve(root, 'target/busy-settle-test-'));
  writeFileSync(resolve(dir, 'app.json'), JSON.stringify({ name: 'Busy Settle Test', app: { id: 'test.busysettle', name: 'Busy Settle Test' }, host: { web: {} }, launch: ['probe'] }));
  writeFileSync(resolve(dir, 'app.contract'), APP);
  mkdirSync(resolve(dir, 'modules/probe/web'), { recursive: true });
  writeFileSync(resolve(dir, 'modules/probe/web/service.js'), 'export const start = () => ({ event() {}, background() {} });');
  const build = Bun.spawn([process.execPath, 'host/web-js/build.mjs', 'busy-settle-test', '--out', resolve(dir, 'dist')], {
    cwd: root, env: { ...process.env, EXACT_APP_DIR: dir }, stdout: 'pipe', stderr: 'pipe',
  });
  const [status, stdout, stderr] = await Promise.all([build.exited, new Response(build.stdout).text(), new Response(build.stderr).text()]);
  expect({ status, error: status ? stdout + stderr : '' }).toEqual({ status: 0, error: '' });
  return dir;
}

let dir, server;
const engines = await engineTests(['chromium']);
beforeAll(async () => {
  if (webCompiler().cmd === 'cargo') throw new Error('build the compiler first: cargo build -p exact-web-js');
  dir = await built();
  server = Bun.serve({ port: 0, hostname: '127.0.0.1', async fetch(req) {
    const path = new URL(req.url).pathname;
    const file = Bun.file(resolve(dir, 'dist', '.' + (path.endsWith('/') ? path + 'index.html' : path)));
    return new Response(file, { headers: { 'content-type': TYPES[extname(file.name)] ?? 'application/octet-stream' } });
  } });
}, 300_000);
afterAll(() => { server?.stop(true); if (dir) rmSync(dir, { recursive: true, force: true }); });

async function pageFor() {
  const browser = await suiteBrowser();
  const page = await browser.newPage();
  await page.goto(`${server.url.href}?agent=1`);
  await page.waitForFunction(() => typeof globalThis.exact?.agentSettled === 'function');
  return page;
}
const ask = (page, req) => page.evaluate(req => globalThis.exact.agentSettled(req), req);

engines.chromium('busy elements use test ids or the view ids tree and layout accept', async () => {
  const page = await pageFor();
  try {
    await page.evaluate(() => document.querySelector('dialog').showModal());
    const { nodes } = await ask(page, { op: 'tree' });
    const loading = nodes.find(n => n.props.accessibilityLabel === 'Loading');
    expect(loading.props.accessibilityBusy).toBe(true);
    expect(await ask(page, { op: 'clock', settle: true })).toMatchObject({ settled: false, reason: 'busy', busy: ['spin', String(loading.id)] });
  } finally { await page.close(); }
}, 60_000);

engines.chromium('closed dialogs, popovers and display:none subtrees hold neither settle nor launch', async () => {
  const page = await pageFor();
  try {
    expect(await ask(page, { op: 'clock', settle: true })).toMatchObject({ settled: false, reason: 'busy', busy: ['spin'] });
    await page.click('[data-testid="stop"]');
    expect(await ask(page, { op: 'clock', settle: true })).toMatchObject({ settled: true });
    await page.waitForFunction(() => ['settled', 'declared'].includes(globalThis.exact.observe?.().outcome));
    await page.evaluate(() => document.querySelector('[popover]').showPopover());
    expect(await ask(page, { op: 'clock', settle: true })).toMatchObject({ settled: false, reason: 'busy', busy: ['pop-spin'] });
    await page.evaluate(() => document.querySelector('[popover]').hidePopover());
    expect(await ask(page, { op: 'clock', settle: true })).toMatchObject({ settled: true });
    await page.evaluate(() => document.querySelector('[data-testid="hidden"]').style.display = 'block');
    expect(await ask(page, { op: 'clock', settle: true })).toMatchObject({ settled: false, reason: 'busy', busy: ['hidden-spin'] });
    await page.evaluate(() => Object.assign(document.querySelector('[data-testid="hidden"]').style, { position: 'absolute', top: '-2000px', visibility: 'hidden', opacity: '0' }));
    expect(await ask(page, { op: 'clock', settle: true })).toMatchObject({ settled: false, reason: 'busy', busy: ['hidden-spin'] });
  } finally { await page.close(); }
}, 60_000);

for (const kind of ['dialog', 'popover']) engines.chromium(`native ${kind} dismissal releases startup without another commit`, async () => {
  const page = await pageFor();
  try {
    await page.evaluate(kind => {
      const el = document.querySelector(kind === 'dialog' ? 'dialog' : '[popover]');
      if (kind === 'dialog') el.showModal();
      else { el.setAttribute('popover', 'auto'); el.showPopover(); }
      // Stop the page's spinner while the presentation still holds startup.
      document.querySelector('[data-testid="stop"]').click();
    }, kind);
    await page.waitForFunction(() => globalThis.exact.observe?.().marks.present !== undefined);
    expect(await page.evaluate(() => globalThis.exact.observe().outcome)).toBeNull();
    await page.keyboard.press('Escape');
    await page.waitForFunction(() => ['settled', 'declared'].includes(globalThis.exact.observe().outcome), null, { timeout: 3000 });
  } finally { await page.close(); }
}, 60_000);
