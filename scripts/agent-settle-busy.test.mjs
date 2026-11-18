import { expect, test } from 'bun:test';
import { readFileSync, statSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { MalformedOperation } from './agent-keys.mjs';

const source = readFileSync(new URL('./agent-session.mjs', import.meta.url), 'utf8');
const start = source.indexOf("    async clock(spec = 'settle'"), end = source.indexOf('\n    /** The window', start);

// Exercise the driver's clock on the same stand-in host as agent-clock-until.test.mjs.
function clockOn(reply) {
  const s = { now: 0, async op() { return { ...reply }; } };
  return new Function('s', 'carrier', 'performance', 'statSync', 'resolve', 'dirname', 'MalformedOperation', 'CLOCK_STEP_MS',
    'worldTells', 'nextCommitTasks', 'settleBound',
    `return ({${source.slice(start, end)}}).clock;`)(s, { host: 'ios' }, performance, statSync, resolve, dirname, MalformedOperation, 250,
    (_s, r) => r, async () => [], () => ({}));
}

test('a busy settle names its elements, implicit progress state and source lookup', async () => {
  const reply = await clockOn({ clock: 0, settled: false, reason: 'busy', busy: ['104', 'spinner'] })('settle');
  expect(reply.diagnostic).toContain('aria-busy (still loading): 104, spinner');
  expect(reply.diagnostic).toContain('by test id, else view id');
  expect(reply.diagnostic).toContain('An indeterminate `progress` (no `value`) is aria-busy');
  expect(reply.diagnostic).toContain('`layout <id>` names its line in the source');
  expect(reply.diagnostic).not.toContain('world:*');
});

test('other unsettled replies keep their diagnostic and settled replies have none', async () => {
  expect((await clockOn({ clock: 0, settled: false, reason: 'world' })('settle')).diagnostic).toContain('did not reach quiescence');
  expect((await clockOn({ clock: 0, settled: true })('settle')).diagnostic).toBeUndefined();
});
