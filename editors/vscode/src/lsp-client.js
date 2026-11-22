// Zero-dependency JSON-RPC 2.0 stdio LSP client for `contract lsp`.
// Shared by the VS Code extension and reusable directly by Node/Bun scripts and coding agents.
'use strict';

const { spawn, spawnSync } = require('node:child_process');
const { existsSync, realpathSync } = require('node:fs');
const { dirname, join, resolve } = require('node:path');

/**
 * Check whether a direct `contract` binary supports the `lsp` subcommand.
 * Avoids picking a stale binary from an older checkout that exits with code 2.
 */
function binarySupportsLsp(binPath) {
  try {
    const res = spawnSync(binPath, ['lsp', '--help'], {
      encoding: 'utf8',
      timeout: 3000,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    return res.status === 0;
  } catch {
    return false;
  }
}

/**
 * Locate the best command + args to launch `contract lsp --stdio`.
 * Order: explicit config -> workspace target binaries supporting `lsp` ->
 * extension's own checkout target binaries (following symlinks) ->
 * workspace `exact.mjs` -> extension checkout `scripts/exact.mjs` -> PATH `contract`.
 */
function discoverServer(workspaceFolders = [], config = {}) {
  const configuredPath = (config.serverPath || '').trim();
  const extraArgs = Array.isArray(config.serverArgs) ? config.serverArgs : [];
  if (configuredPath) {
    return { command: configuredPath, args: ['lsp', '--stdio', ...extraArgs] };
  }

  const binName = process.platform === 'win32' ? 'contract.exe' : 'contract';
  const roots = workspaceFolders.filter(Boolean);
  let realSrcDir = __dirname;
  try {
    realSrcDir = realpathSync(__dirname);
  } catch {
    // Keep __dirname if realpath fails.
  }
  const extRepoRoot = resolve(dirname(realSrcDir), '..', '..');
  const binSearchRoots = [...roots];
  if (!binSearchRoots.includes(extRepoRoot)) binSearchRoots.push(extRepoRoot);

  // 1. Prefer compiled binaries that actually support `lsp`.
  for (const root of binSearchRoots) {
    for (const rel of [
      join('target', 'host-dev', binName),
      join('target', 'release', binName),
      join('target', 'debug', binName),
    ]) {
      const candidate = resolve(root, rel);
      if (existsSync(candidate) && binarySupportsLsp(candidate)) {
        return { command: candidate, args: ['lsp', '--stdio', ...extraArgs] };
      }
    }
  }

  // 2. Fall back to `bun <exact.mjs> contract lsp --stdio` in workspace or extension checkout.
  for (const root of roots) {
    const repoCli = resolve(root, 'scripts', 'exact.mjs');
    if (existsSync(repoCli)) {
      return { command: 'bun', args: [repoCli, 'contract', 'lsp', '--stdio', ...extraArgs] };
    }
    const appCli = resolve(root, 'exact.mjs');
    if (existsSync(appCli)) {
      return { command: 'bun', args: [appCli, 'contract', 'lsp', '--stdio', ...extraArgs] };
    }
  }
  const extCli = resolve(extRepoRoot, 'scripts', 'exact.mjs');
  if (existsSync(extCli)) {
    return { command: 'bun', args: [extCli, 'contract', 'lsp', '--stdio', ...extraArgs] };
  }

  return { command: 'contract', args: ['lsp', '--stdio', ...extraArgs] };
}

/**
 * Lightweight LSP 3.17 JSON-RPC 2.0 client over stdio (`Content-Length` framing).
 */
class LspConnection {
  constructor(command, args, options = {}) {
    this.command = command;
    this.args = args || [];
    this.cwd = options.cwd;
    this.env = options.env || process.env;
    this.onNotification = options.onNotification || (() => {});
    this.onStderr = options.onStderr || (() => {});
    this.onExit = options.onExit || (() => {});
    this.proc = null;
    this.nextId = 1;
    this.pending = new Map();
    this.buffer = Buffer.alloc(0);
    this.capabilities = null;
  }

  start() {
    if (this.proc) return;
    this.proc = spawn(this.command, this.args, {
      cwd: this.cwd,
      env: this.env,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    this.proc.stdout.on('data', (chunk) => this._onData(chunk));
    this.proc.stderr.on('data', (chunk) => this.onStderr(chunk.toString('utf8')));
    this.proc.on('error', (err) => {
      for (const { reject } of this.pending.values()) reject(err);
      this.pending.clear();
      this.onExit(-1, err);
    });
    this.proc.on('exit', (code) => {
      const err = new Error(`contract lsp exited (${code})`);
      for (const { reject } of this.pending.values()) reject(err);
      this.pending.clear();
      this.proc = null;
      this.onExit(code ?? 0, null);
    });
  }

  async initialize(params = {}) {
    this.start();
    const res = await this.request('initialize', {
      processId: process.pid,
      clientInfo: { name: 'exact-contract-client', version: '0.5.0' },
      capabilities: {
        textDocument: {
          synchronization: { didSave: true },
          hover: { contentFormat: ['markdown', 'plaintext'] },
          completion: {
            completionItem: {
              snippetSupport: true,
              documentationFormat: ['markdown', 'plaintext'],
            },
          },
          publishDiagnostics: { relatedInformation: true },
          semanticTokens: { requests: { full: true } },
          codeAction: {
            codeActionLiteralSupport: {
              codeActionKind: { valueSet: ['quickfix', 'source.organizeImports', 'source.fixAll'] },
            },
          },
          colorProvider: {},
          documentLink: {},
          selectionRange: {},
          callHierarchy: {},
        },
        workspace: {
          workspaceFolders: true,
          fileOperations: { willRename: true },
        },
      },
      ...params,
    });
    this.capabilities = res?.capabilities ?? {};
    this.notify('initialized', {});
    return res;
  }

  request(method, params) {
    if (!this.proc || !this.proc.stdin.writable) {
      return Promise.reject(new Error('contract lsp is not running'));
    }
    const id = this.nextId++;
    const payload = { jsonrpc: '2.0', id, method, params };
    return new Promise((resolvePromise, rejectPromise) => {
      this.pending.set(id, { resolve: resolvePromise, reject: rejectPromise });
      this._send(payload);
    });
  }

  notify(method, params) {
    if (!this.proc || !this.proc.stdin.writable) return;
    this._send({ jsonrpc: '2.0', method, params });
  }

  async stop() {
    if (!this.proc) return;
    const proc = this.proc;
    try {
      await Promise.race([
        this.request('shutdown', null),
        new Promise((r) => setTimeout(r, 1500)),
      ]);
      this.notify('exit', null);
    } catch {
      // Ignore shutdown errors when stopping.
    }
    setTimeout(() => {
      try {
        if (!proc.killed) proc.kill();
      } catch {
        // Already exited.
      }
    }, 300);
    this.proc = null;
  }

  _send(obj) {
    const body = Buffer.from(JSON.stringify(obj), 'utf8');
    const header = Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, 'ascii');
    this.proc.stdin.write(Buffer.concat([header, body]));
  }

  _onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    while (true) {
      const sep = this.buffer.indexOf('\r\n\r\n');
      if (sep < 0) return;
      const headerText = this.buffer.slice(0, sep).toString('ascii');
      const match = /Content-Length:\s*(\d+)/i.exec(headerText);
      if (!match) {
        this.buffer = this.buffer.slice(sep + 4);
        continue;
      }
      const len = Number(match[1]);
      const start = sep + 4;
      if (this.buffer.length < start + len) return;
      const raw = this.buffer.slice(start, start + len).toString('utf8');
      this.buffer = this.buffer.slice(start + len);
      let msg;
      try {
        msg = JSON.parse(raw);
      } catch {
        continue;
      }
      if (msg.id !== undefined && this.pending.has(msg.id)) {
        const { resolve: res, reject: rej } = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        if (msg.error) {
          const err = new Error(msg.error.message || 'LSP error');
          err.code = msg.error.code;
          rej(err);
        } else {
          res(msg.result);
        }
      } else if (msg.method) {
        this.onNotification(msg.method, msg.params);
      }
    }
  }
}

module.exports = { discoverServer, binarySupportsLsp, LspConnection };
