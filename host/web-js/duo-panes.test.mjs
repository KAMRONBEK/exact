import { test, expect } from 'bun:test';
import { mkdtempSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import process from 'node:process';
import { suiteBrowser } from '../../scripts/agent-launch.mjs';

test('Duo panes retain state and avoid book and laptop hinges in both reading directions', async () => {
  const root = resolve(import.meta.dirname, '../..'), dir = mkdtempSync(resolve(root, 'target/duo-panes-test-'));
  let suite;
  const build = Bun.spawn([process.execPath, 'host/web-js/build.mjs', 'duo-lab', '--out', dir, '--render', 'none'], {
    cwd: root, env: process.env, stdout: 'pipe', stderr: 'pipe',
  });
  let server;
  try {
    const [status, stdout, stderr] = await Promise.all([build.exited, new Response(build.stdout).text(), new Response(build.stderr).text()]);
    expect({ status, error: status ? stdout + stderr : '' }).toEqual({ status: 0, error: '' });
    server = Bun.serve({ hostname: '127.0.0.1', port: 0, fetch(req) {
      const path = new URL(req.url).pathname;
      return new Response(Bun.file(resolve(dir, path === '/' ? 'index.html' : path.slice(1))));
    } });
    suite = await suiteBrowser();
    const page = await suite.newPage({ viewport: { width: 1280, height: 800 } });
    const errors = [];
    page.on('pageerror', e => errors.push(String(e)));
    await page.goto(server.url.href);
    await page.getByTestId('tab-panes').click();
    expect(await page.getByTestId('panes-split').count()).toBe(0);
    expect(await page.getByTestId('panes-policy').textContent()).toContain('require the native iOS app');
    await page.getByTestId('panes-workspace-draft').fill('Keep this draft');
    await page.getByTestId('panes-activity-increment').click();
    await page.evaluate(() => { globalThis.originalPanes = ['workspace', 'activity'].map(name => document.querySelector(`[data-testid="panes-${name}-pane"]`)); });
    const cdp = await page.context().newCDPSession(page);
    for (const direction of ['ltr', 'rtl']) {
      await page.evaluate(direction => { document.documentElement.dir = direction; }, direction);
      for (const [width, height, orientation, offset] of [[1280, 800, 'vertical', 620], [1280, 800, 'vertical', 400], [1280, 800, 'vertical', 840], [800, 1000, 'horizontal', 480]]) {
        await cdp.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false,
          displayFeature: { orientation, offset, maskLength: 40 } });
        const paneWidth = Math.min(offset, width - offset - 40);
        await page.waitForFunction(([axis, expected, width]) => {
          const pane = document.querySelector('[data-testid="panes-activity-pane"]');
          const rect = pane.getBoundingClientRect();
          return getComputedStyle(pane).position === 'absolute' && Math.abs(rect[axis] - expected) < 1 && (axis !== 'left' || Math.abs(rect.width - width) < 1);
        }, [orientation === 'vertical' ? 'left' : 'top', orientation === 'vertical' ? (direction === 'rtl' ? 0 : width - paneWidth) : offset + 40, paneWidth]);
        const bounds = await page.evaluate(() => globalThis.originalPanes.map((pane, i) => {
          const r = pane.getBoundingClientRect();
          return { retained: pane === document.querySelector(`[data-testid="panes-${i ? 'activity' : 'workspace'}-pane"]`), left: r.left, right: r.right, top: r.top, bottom: r.bottom };
        }));
        expect(bounds.every(b => b.retained)).toBe(true);
        const start = orientation === 'vertical' ? 'left' : 'top', end = orientation === 'vertical' ? 'right' : 'bottom';
        const first = orientation === 'vertical' && direction === 'rtl' ? 1 : 0;
        expect(bounds[first][end]).toBeLessThanOrEqual(offset + 1);
        expect(bounds[1 - first][start]).toBeGreaterThanOrEqual(offset + 39);
        expect(await page.getByTestId('panes-workspace-echo').textContent()).toBe('Keep this draft');
        expect(await page.getByTestId('panes-activity-count').textContent()).toBe('1');
      }
    }
    // The Arrange screen without UIKit's arrangement: each pane on its own segment, the primary on the leading one, and
    // the header in the primary, so a pane on the lower segment starts there
    await page.getByTestId('tab-arrangement').click();
    const arranged = () => page.evaluate(() => ['primary', 'secondary', 'title'].map(name => {
      const r = document.querySelector(`[data-testid="arrangement-1-${name}"]`)?.getBoundingClientRect();
      return r && { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
    }));
    for (const direction of ['ltr', 'rtl']) {
      await page.evaluate(direction => { document.documentElement.dir = direction; }, direction);
      for (const [width, height, orientation, offset] of [[1280, 800, 'vertical', 620], [800, 1000, 'horizontal', 480]]) {
        await cdp.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false,
          displayFeature: { orientation, offset, maskLength: 40 } });
        const leadingIsPrimary = orientation === 'horizontal' || direction === 'ltr';
        const [start, end] = orientation === 'vertical' ? ['left', 'right'] : ['top', 'bottom'];
        // Wait for the placement, then assert what it measured, so a failure prints the edges
        await page.waitForFunction(([leadingIsPrimary, start, end, offset]) => {
          const [primary, secondary] = ['primary', 'secondary'].map(name => document.querySelector(`[data-testid="arrangement-1-${name}"]`)?.getBoundingClientRect());
          const [first, second] = leadingIsPrimary ? [primary, secondary] : [secondary, primary];
          return first && second && first[end] <= offset + 1 && second[start] >= offset + 39;
        }, [leadingIsPrimary, start, end, offset]).catch(() => {});
        const [primary, secondary, title] = await arranged();
        const [first, second] = leadingIsPrimary ? [primary, secondary] : [secondary, primary];
        expect(first[end]).toBeLessThanOrEqual(offset + 1);
        expect(second[start]).toBeGreaterThanOrEqual(offset + 39);
        expect(Math.min(primary.width, primary.height, secondary.width, secondary.height)).toBeGreaterThan(200);
        expect([title.left - primary.left, primary.right - title.right, title.top - primary.top].every(d => d >= 0)).toBe(true);
      }
    }
    // A pushed Arrange screen on a fold has no bar, so its header draws Back
    await page.evaluate(() => { document.documentElement.dir = 'ltr'; });
    await page.getByTestId('arrangement-1-next').click();
    await page.getByTestId('arrangement-2-back').click();
    await expect(page.getByTestId('arrangement-1-title').textContent()).resolves.toBe('Arrangement');
    await page.getByTestId('tab-panes').click();
    await cdp.send('Emulation.setSafeAreaInsetsOverride', { insets: { top: 64, left: 40, right: 24, bottom: 32 } });
    const title = await page.getByTestId('panes-workspace-pane').getByRole('heading', { level: 1 }).boundingBox();
    const activity = await page.getByTestId('panes-activity-pane').getByRole('heading', { name: 'Activity', exact: true }).boundingBox();
    expect(title.y).toBeGreaterThanOrEqual(80);
    expect(title.x).toBeGreaterThanOrEqual(56);
    expect(activity.x).toBeGreaterThanOrEqual(56);
    expect(activity.x + activity.width).toBeLessThanOrEqual(760);
    await cdp.send('Emulation.setSafeAreaInsetsOverride', { insets: { top: 0, left: 0, right: 0, bottom: 0 } });
    await cdp.send('Emulation.setDeviceMetricsOverride', { width: 800, height: 1000, deviceScaleFactor: 1, mobile: false });
    await page.getByTestId('panes-next').click();
    await page.getByTestId('pushed-next').click();
    expect(await page.getByTestId('panes-workspace-echo').textContent()).toBe('Keep this draft');
    await page.getByTestId('tab-fold').click();
    expect(await page.getByTestId('fact-h').textContent()).toBe('1');
    expect(errors).toEqual([]);
  } finally { await suite?.close(); server?.stop(true); rmSync(dir, { recursive: true, force: true }); }
}, 180000);
