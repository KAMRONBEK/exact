import { test } from 'bun:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { settingValues } from '../js/bake/src/__exact_env.mjs';
import { driveBuildEnv } from './agent-build.mjs';
import { resolveApp } from './app.mjs';

const launch = name => ({ name, launch: true });

function withApp(env, run) {
  const dir = realpathSync(mkdtempSync(resolve(tmpdir(), 'exact-agent-build-env-')));
  try {
    writeFileSync(resolve(dir, 'app.json'), JSON.stringify({ name: 'launch-probe', app: { id: 'com.exact.launch-probe', name: 'Launch Probe' }, env }));
    writeFileSync(resolve(dir, 'app.contract'), 'component App\n  view\n    text "hello"\n');
    run(dir);
  } finally { rmSync(dir, { recursive: true, force: true }); }
}

test('a drive build keeps file and build settings and leaves shell launch settings to the launch (#1336)', () => {
  withApp([launch('API_TOKEN'), launch('SESSION'), 'API_BASE_URL'], dir => {
    writeFileSync(resolve(dir, '.env'), 'SESSION=from-dotenv\n');
    const app = resolveApp(dir);
    const shell = { PATH: '/usr/bin', API_TOKEN: 'from-shell', SESSION: 'from-dotenv', API_BASE_URL: 'https://api.test', EXACT_BUN_LOADED: 'SESSION' };
    const env = driveBuildEnv(app, shell);
    assert.deepEqual(env, { PATH: '/usr/bin', SESSION: 'from-dotenv', API_BASE_URL: 'https://api.test', EXACT_BUN_LOADED: 'SESSION' });
    assert.deepEqual(settingValues(dir, ['API_TOKEN', 'SESSION'], env), { API_TOKEN: null, SESSION: { value: 'from-dotenv', source: '.env' } });
    assert.equal(driveBuildEnv(app, shell, { PATH: '/usr/bin' }).API_TOKEN, 'from-shell');
    assert.equal(shell.API_TOKEN, 'from-shell');
  });
});

test('Windows build filtering matches manifest, launch and Bun-loaded names regardless of case', () => {
  const app = { manifest: { env: [launch('API_TOKEN'), launch('SESSION')] } };
  const env = { Api_Token: 'from-shell', Session: 'from-dotenv', exact_bun_loaded: 'sEsSiOn' };
  const launched = { api_token: '', SESSION: 'from-dotenv' };
  assert.deepEqual(driveBuildEnv(app, env, launched, { windows: true }), { Session: 'from-dotenv', exact_bun_loaded: 'sEsSiOn' });
  assert.deepEqual(driveBuildEnv(app, env, launched), env);
});

// Keep the module mock in a separate Bun process so the other build tests use real children.
function checkSpawns(names, script) {
  withApp(names.map(launch), dir => {
    const result = spawnSync(process.execPath, ['--no-env-file', '-e', `
      import assert from 'node:assert/strict';
      import { mock } from 'bun:test';
      import * as processes from 'node:child_process';
      import { EventEmitter } from 'node:events';
      import { PassThrough } from 'node:stream';
      import { resolve } from 'node:path';
      const dir = ${JSON.stringify(dir)}, calls = [];
      delete process.env.EXACT_MAC_BIN;
      delete process.env.EXACT_WINDOWS_BIN;
      delete process.env.SIMCTL_CHILD_API_TOKEN;
      let onSpawn = () => {};
      mock.module('node:child_process', () => ({ ...processes, spawn(command, args, options) {
        calls.push({ command, args, ...options });
        onSpawn(command, args);
        const child = Object.assign(new EventEmitter(), {
          pid: process.pid, stdout: new PassThrough(), stderr: new PassThrough(), unref() {},
        });
        queueMicrotask(() => { child.emit('exit', 0); child.emit('close', 0); });
        return child;
      } }));
      const { buildFirst, makeOverride, prebuild, windowsBuildFirst } = await import('./scripts/agent-build.mjs');
      const { resolveApp } = await import('./scripts/app.mjs');
      const app = resolveApp(dir), quiet = { say() {} };
      ${script}
    `], {
      cwd: resolve(import.meta.dir, '..'), encoding: 'utf8', timeout: 20_000,
      env: { ...process.env, CARGO_TARGET_DIR: resolve(dir, 'target'), EXACT_APP_DIR: dir, EXACT_BAKE_OUTPUT: resolve(dir, 'baked'), EXACT_BUN_LOADED: '',
        EXACT_UPDATE_TRUST: 'development', EXACT_VARIANT: '', EXACT_WINDOWS_OUTPUT: resolve(dir, 'dist-windows') },
    });
    assert.equal(result.status, 0, result.stdout + result.stderr + (result.error ?? ''));
  });
}

test('macOS drive rebuilds leave launch settings out of the Apple build child', () => {
  checkSpawns(['API_TOKEN'], `
    process.env.API_TOKEN = 'mac-launch';
    assert.equal(await buildFirst('macos', app, quiet), 0);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].args[0], resolve('host/apple/build.mjs'));
    assert.equal(calls[0].env.API_TOKEN, undefined);
    assert.equal(process.env.API_TOKEN, 'mac-launch');
  `);
});

test('iOS drive rebuilds filter settings the simulator launch receives', () => {
  checkSpawns(['API_TOKEN', 'EXACT_STORE'], `
    process.env.API_TOKEN = 'build-only';
    process.env.EXACT_STORE = 'real';
    assert.equal(await buildFirst('ios', app, quiet), 0);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].args[0], resolve('host/apple/build.mjs'));
    assert.ok(calls[0].args.includes('--ios'));
    assert.equal(calls[0].env.API_TOKEN, 'build-only');
    assert.equal(calls[0].env.EXACT_STORE, undefined);
  `);
});

test('Windows drive rebuilds leave a mixed-case launch setting out of the Windows build child', () => {
  checkSpawns(['API_TOKEN'], `
    delete process.env.API_TOKEN;
    process.env.Api_Token = 'windows-launch';
    assert.equal(await windowsBuildFirst(app, quiet), 0);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].args[0], resolve('host/windows/build.mjs'));
    assert.ok(!Object.keys(calls[0].env).some(name => name.toUpperCase() === 'API_TOKEN'), 'the build child received API_TOKEN');
  `);
});

test('Apple prebuilds filter launch settings for macOS and the iOS simulator', () => {
  checkSpawns(['API_TOKEN', 'EXACT_STORE'], `
    process.env.API_TOKEN = 'from-shell';
    process.env.EXACT_STORE = 'real';
    for (const { flags, token, store } of [
      { flags: [], token: undefined, store: undefined },
      { flags: ['--ios'], token: 'from-shell', store: undefined },
      { flags: ['--device'], token: 'from-shell', store: 'real' },
      { flags: ['--tvos'], token: 'from-shell', store: 'real' },
    ]) {
      assert.equal(await prebuild(app, [...flags, '--prebuild'], quiet), 0);
      const { args, env } = calls.at(-1);
      assert.ok(args.includes(resolve('host/apple/build.mjs')));
      assert.deepEqual([env.API_TOKEN, env.EXACT_STORE], [token, store], flags.join(' ') || 'macOS');
    }
    assert.equal(calls.length, 4);
  `);
});

test('a plan-only bake keeps Cargo settings when compiling the bake tool', () => {
  checkSpawns(['CARGO_TARGET_DIR'], `
    const { mkdirSync, writeFileSync } = await import('node:fs');
    const { createHash } = await import('node:crypto');
    const { isDeepStrictEqual } = await import('node:util');
    const { bakeOutput } = await import('./scripts/app.mjs');
    const { bakeTool, linkedBakeDir } = await import('./scripts/linked-plan.mjs');
    const target = 'aarch64-apple-darwin', platform = 'macos';
    const linked = linkedBakeDir({ EXACT_BAKE_OUTPUT: bakeOutput(app) }, platform, target);
    const plan = 'built plan', sha256 = createHash('sha256').update(plan).digest('hex');
    const receipt = resolve(dir, 'receipt.json');
    mkdirSync(linked, { recursive: true });
    writeFileSync(resolve(linked, 'app.plan'), plan);
    writeFileSync(resolve(linked, 'app.module.json'), '{}');
    writeFileSync(receipt, JSON.stringify({ target, build: { compat: { inputs: { platform }, embedded: { plan: { sha256 } } } } }));
    onSpawn = (command, args) => {
      if (command !== bakeTool.built) return;
      mkdirSync(args[2], { recursive: true });
      writeFileSync(resolve(args[2], 'app.module.json'), '{}');
    };
    assert.deepEqual(await makeOverride('macos', app, { receipt, changed: ['app.contract'] }, { log: resolve(dir, 'plan.log') }), { made: true, output: '' });
    assert.deepEqual(calls.map(call => call.command), [bakeTool.command, bakeTool.built]);
    assert.equal(calls[0].env.CARGO_TARGET_DIR, resolve(dir, 'target'));
    assert.ok(isDeepStrictEqual(calls[0].env, bakeTool.options().env), 'Cargo received a different environment');
    assert.equal(calls[1].env.CARGO_TARGET_DIR, undefined);
  `);
});
