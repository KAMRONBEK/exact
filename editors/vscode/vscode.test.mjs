// The extension's problem matcher reads what the Contract compiler prints, and
// the repository's .vscode/tasks.json names scripts that exist. VS Code runs
// the matcher's regexp as a JavaScript RegExp, as this does.
import { spawn, spawnSync } from 'node:child_process';
import { test } from 'bun:test';
import assert from 'node:assert/strict';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { discoverServer, LspConnection } from './src/lsp-client.js';

const ROOT = resolve(import.meta.dir, '../..');
const matcher = JSON.parse(readFileSync(resolve(import.meta.dir, 'package.json'), 'utf8')).contributes.problemMatchers
  .find(m => m.name === 'exact-contract');

test('the problem matcher reads each diagnostic the compiler prints, and only those', () => {
  const dir = mkdtempSync(resolve(tmpdir(), 'exact-vscode-'));
  try {
    writeFileSync(resolve(dir, 'bad.contract'), 'shape A\n  x: string\n\nshape A\n  y: string\n\ncomponent Main\n  view\n    div\n');
    // Relative to where it runs, as a task in an app's folder runs it.
    const built = spawnSync('bun', [resolve(ROOT, 'scripts/exact.mjs'), 'contract', 'build', 'bad.contract'], { cwd: dir, encoding: 'utf8' });
    assert.equal(built.status, 1, built.stderr);
    const { regexp, file, line, column, code, message } = matcher.pattern, pattern = new RegExp(regexp);
    const found = built.stderr.split('\n').map(l => pattern.exec(l)).filter(Boolean)
      .map(m => ({ file: m[file], line: m[line], column: m[column], code: m[code], message: m[message] }));
    assert.deepEqual(found.map(({ message: _, ...rest }) => rest), [
      { file: 'bad.contract', line: '4', column: '1', code: 'type-duplicate-shape' },
      { file: 'bad.contract', line: '9', column: '5', code: 'lower-unknown-tag' },
    ]);
    assert.match(found[1].message, /^unknown tag `div`/);
    // A related note (CompileError's Display: indented, `file:line:col: note`) is not a diagnostic of its own.
    assert.equal(pattern.exec('  bad.contract:1:1: first declared here'), null);
    // A Windows path keeps its drive letter.
    assert.equal(pattern.exec('C:\\app\\app.contract:2:3 [syntax-x] y')?.[file], 'C:\\app\\app.contract');
    // LLP 1116 D6: a warning is read as one, after the refusals; a refusal leaves the matcher's own severity.
    const warned = pattern.exec('app.contract:12:7 warning [lower-single-tab] this tab bar has 1 tab');
    assert.deepEqual([warned?.[matcher.pattern.severity], warned?.[code], warned?.[message]], ['warning', 'lower-single-tab', 'this tab bar has 1 tab']);
    assert.equal(pattern.exec('bad.contract:4:1 [type-duplicate-shape] x')?.[matcher.pattern.severity], undefined);
  } finally { rmSync(dir, { recursive: true, force: true }); }
}, 600_000); // A cold checkout builds the compiler first.

test('every script the repository tasks run exists, and each matcher is one VS Code can resolve', () => {
  const { tasks } = JSON.parse(readFileSync(resolve(ROOT, '.vscode/tasks.json'), 'utf8'));
  const known = new Set(['$exact-contract', '$rustc']);
  for (const t of tasks) {
    for (const [, script] of (t.command ?? '').matchAll(/\bbun (\S+\.mjs)/g)) assert.ok(existsSync(resolve(ROOT, script)), `${t.label}: ${script}`);
    for (const m of [t.problemMatcher ?? []].flat()) assert.ok(known.has(m), `${t.label}: ${m}`);
    for (const d of t.dependsOn ?? []) assert.ok(tasks.some(other => other.label === d), `${t.label}: no task ${d}`);
  }
});

test('a view repeats with `for`, which is a control keyword and indents; the retired `each` is neither', () => {
  const grammar = readFileSync(resolve(import.meta.dir, 'syntaxes/exact-contract.tmLanguage.json'), 'utf8');
  const keywords = [...grammar.matchAll(/"match": ("[^\n]*"),\n\s*"name": "keyword\.control\.exact-contract"/g)]
    .map(([, source]) => new RegExp(JSON.parse(source)));
  const { indentationRules } = JSON.parse(readFileSync(resolve(import.meta.dir, 'language-configuration.json'), 'utf8'));
  const indents = new RegExp(indentationRules.increaseIndentPattern);
  const line = (word) => `      ${word} item in items key=item.id`;
  assert.ok(keywords.some(k => k.exec(line('for'))?.[0] === 'for'));
  assert.ok(!keywords.some(k => k.exec(line('each'))?.[0] === 'each'));
  assert.ok(indents.test(line('for')));
  assert.ok(!indents.test(line('each')));
});

test('`.contract` is registered with the TypeScript 7 extension once content mappers are on, and nothing else is woken', async () => {
  const manifest = JSON.parse(readFileSync(resolve(import.meta.dir, 'package.json'), 'utf8'));
  assert.ok(existsSync(resolve(import.meta.dir, manifest.main)) && manifest.files.includes('extension.js'));
  assert.deepEqual(manifest.activationEvents, ['onLanguage:exact-contract']);
  const { activate } = await import('./extension.js');
  const fake = enabled => {
    const calls = [], activated = [], listeners = [], context = { subscriptions: [] };
    const typescript = { activate: async () => { activated.push(1); return { registerContentMappers: (...args) => { calls.push(args); return { dispose() {} }; } }; } };
    const vscode = {
      extensions: { getExtension: id => id === 'TypeScriptTeam.native-preview' ? typescript : undefined },
      workspace: {
        getConfiguration: () => ({ get: key => key === 'js/ts.contentMappers.enabled' && enabled() }),
        onDidChangeConfiguration: listener => { listeners.push(listener); return { dispose() {} }; },
      },
    };
    const change = () => Promise.all(listeners.map(l => l({ affectsConfiguration: key => key === 'js/ts.contentMappers.enabled' })));
    return { vscode, context, calls, activated, change };
  };
  // Off: the TypeScript extension is not even activated.
  let on = false;
  const off = fake(() => on);
  assert.equal(await activate(off.context, off.vscode), false);
  assert.deepEqual([off.activated.length, off.calls.length], [0, 0]);
  // Turned on later: registered then, and once however often the setting changes.
  on = true;
  await off.change(); await off.change();
  assert.deepEqual(off.calls, [['exact.exact-contract', [{ extensions: ['.contract'] }]]]);
  // On from the start, with no TypeScript 7 extension, or one before content mappers: nothing.
  const none = fake(() => true);
  none.vscode.extensions.getExtension = () => undefined;
  assert.equal(await activate(none.context, none.vscode), false);
  const old = fake(() => true);
  old.vscode.extensions.getExtension = () => ({ activate: async () => ({}) });
  assert.equal(await activate(old.context, old.vscode), false);
});

test('a contract is formatted by the compiler the workspace names, as the editor holds it, and not at all until one is named', async () => {
  const bin = process.env.EXACT_CONTRACT ?? resolve(ROOT, 'target/debug/contract');
  if (!existsSync(bin)) { console.log('skipped: no contract binary'); return; }
  const manifest = JSON.parse(readFileSync(resolve(import.meta.dir, 'package.json'), 'utf8'));
  assert.equal(manifest.contributes.configuration.properties['exactContract.compiler'].type, 'string');
  const { activate } = await import('./extension.js');
  const fake = compiler => {
    const providers = [], warnings = [], listeners = [], context = { subscriptions: [] };
    const quiet = () => ({ dispose() {} }); // this test opens no document
    const vscode = {
      extensions: { getExtension: () => undefined },
      workspace: {
        getConfiguration: () => ({ get: key => key === 'exactContract.compiler' ? compiler() : undefined }),
        onDidChangeConfiguration: listener => { listeners.push(listener); return { dispose() {} }; },
        onDidOpenTextDocument: quiet, onDidChangeTextDocument: quiet, onDidCloseTextDocument: quiet, textDocuments: [],
      },
      languages: {
        registerDocumentFormattingEditProvider: (language, provider) => { providers.push([language, provider]); return { dispose() { providers.length = 0; } }; },
        createDiagnosticCollection: () => ({ dispose() {} }),
      },
      window: { showWarningMessage: message => warnings.push(message) },
      Range: function (start, end) { return { start, end }; },
      TextEdit: { replace: (range, text) => ({ range, text }) },
    };
    const change = () => Promise.all(listeners.map(l => l({ affectsConfiguration: key => key === 'exactContract.compiler' })));
    /** The edits a Format Document of `text` gets, the file on disk being another text or none. */
    const format = text => providers[0][1].provideDocumentFormattingEdits({ fileName: '/nowhere/app.contract', getText: () => text, positionAt: offset => offset });
    return { vscode, context, providers, warnings, change, format };
  };
  // No compiler named: no formatter is registered, so nothing is run.
  let named;
  const editor = fake(() => named);
  await activate(editor.context, editor.vscode);
  assert.equal(editor.providers.length, 0);
  // Named later: registered for contracts, once.
  named = bin;
  await editor.change(); await editor.change();
  assert.deepEqual(editor.providers.map(([language]) => language), ['exact-contract']);
  // The whole text is replaced by the compiler's formatting of it; text already so has no edit.
  const unformatted = 'component A\n  view\n    text   "a"   font-size=12\n', formatted = 'component A\n  view\n    text "a" font-size=12\n';
  assert.deepEqual(await editor.format(unformatted), [{ range: { start: 0, end: unformatted.length }, text: formatted }]);
  assert.deepEqual(await editor.format(formatted), []);
  // Text the compiler refuses is left as it is: the refusal is the compiler's to show, where it shows its others.
  assert.deepEqual(await editor.format('component A\n  view\n    text "a'), []);
  assert.deepEqual(editor.warnings, []);
  // A compiler that is not there is said once, by the setting that names it.
  named = '/nowhere/contract';
  await editor.change();
  assert.deepEqual([await editor.format(unformatted), await editor.format(unformatted)], [[], []]);
  assert.equal(editor.warnings.length, 1);
  assert.match(editor.warnings[0], /exactContract\.compiler.*\/nowhere\/contract/);
  // Unset again: the formatter goes.
  named = undefined;
  await editor.change();
  assert.equal(editor.providers.length, 0);
});

test('the compiler\'s warnings on an open contract are problems of warning severity, once the workspace names a compiler', async () => {
  const bin = process.env.EXACT_CONTRACT ?? resolve(ROOT, 'target/debug/contract');
  if (!existsSync(bin)) { console.log('skipped: no contract binary'); return; }
  const { activate } = await import('./extension.js');
  const dir = mkdtempSync(resolve(tmpdir(), 'exact-vscode-warnings-'));
  // A sized button with no text and no label builds, with `lower-button-unlabelled`; the row beside it has props, so alone it is no app.
  const blank = '      button press=bump width=20 height=20\n', labelled = '      button "Go" press=bump\n';
  const app = body => `use Row from "./lib/row.contract"\ncomponent App\n  state n = 0\n  action bump\n    n = n + 1\n  view\n    main\n${body}      Row(label="é")\n`;
  const row = body => `component Row\n  props\n    label: string\n  state n = 0\n  action bump\n    n = n + 1\n  view\n    box\n      text "é" + label\n${body}`;
  mkdirSync(resolve(dir, 'lib'));
  writeFileSync(resolve(dir, 'app.contract'), app(labelled));
  writeFileSync(resolve(dir, 'lib/row.contract'), row(labelled));
  const listeners = { open: [], change: [], close: [], configuration: [] }, shown = new Map(), context = { subscriptions: [] };
  let named;
  const on = kind => listener => { listeners[kind].push(listener); return { dispose() {} }; };
  const vscode = {
    extensions: { getExtension: () => undefined },
    workspace: {
      getConfiguration: () => ({ get: key => key === 'exactContract.compiler' ? named : undefined }),
      onDidChangeConfiguration: on('configuration'), onDidOpenTextDocument: on('open'), onDidChangeTextDocument: on('change'), onDidCloseTextDocument: on('close'),
      textDocuments: [],
    },
    languages: {
      registerDocumentFormattingEditProvider: () => ({ dispose() {} }),
      createDiagnosticCollection: () => ({ set: (uri, problems) => shown.set(String(uri), problems), delete: uri => shown.delete(String(uri)), dispose: () => shown.clear() }),
    },
    window: { showWarningMessage() {} },
    Range: function (startLine, start, endLine, end) { return [startLine, start, endLine, end]; },
    Diagnostic: function (range, message, severity) { return { range, message, severity }; },
    DiagnosticSeverity: { Warning: 'warning' },
  };
  const document = (name, text, scheme = 'file') => ({ languageId: 'exact-contract', uri: { scheme, toString: () => name }, fileName: resolve(dir, name), getText: () => text, lineAt: line => ({ text: text.split('\n')[line] }) });
  /** What the editor shows for a document once the compiler has answered: each problem's range, code and severity. */
  const problems = async (kind, doc) => { await Promise.all(listeners[kind].map(l => l(kind === 'change' ? { document: doc } : doc))); return (shown.get(String(doc.uri)) ?? []).map(p => [p.range, p.code, p.severity, p.source]); };
  try {
    // No compiler named: nothing listens for a document, so nothing is run.
    await activate(context, vscode);
    assert.deepEqual(listeners.open, []);
    named = bin;
    await Promise.all(listeners.configuration.map(l => l({ affectsConfiguration: key => key === 'exactContract.compiler' })));
    // The editor's text, saved or not: the saved app has a label, this one does not.
    assert.deepEqual(await problems('open', document('app.contract', app(blank))), [[[7, 6, 7, 12], 'lower-button-unlabelled', 'warning', 'contract']]);
    assert.deepEqual(await problems('change', document('app.contract', app(labelled))), []);
    // A file its app uses is built with that app, and has its own warnings only. Its place counts UTF-16 units, the compiler's UTF-8 bytes.
    const used = `      text "é" + label\n      button press=bump width=20 height=20\n`;
    assert.deepEqual(await problems('open', document('lib/row.contract', row(used))), [[[10, 6, 10, 12], 'lower-button-unlabelled', 'warning', 'contract']]);
    // A diff's copy of a contract is not checked: its problems would stand beside the file's.
    assert.deepEqual(await problems('open', document('old.contract', app(blank), 'git')), []);
    // A refusal is no warning: the compiler's errors are the content mapper's to show.
    assert.deepEqual(await problems('change', document('app.contract', app('      nope\n'))), []);
    // Lines deleted while the compiler ran: the answer for the older text is dropped, and the newer text is still checked.
    const live = { ...document('app.contract', ''), text: app(blank), getText() { const text = this.text; this.typed?.(); return text; }, lineAt(line) { const text = this.text.split('\n')[line]; if (text === undefined) throw new Error('Illegal value for `line`'); return { text }; } };
    const edited = new Promise(done => { live.typed = () => { live.typed = undefined; live.text = 'component App\n'; done(Promise.all(listeners.change.map(l => l({ document: live })))); }; });
    await Promise.all([...listeners.open.map(l => l(live)), edited]);
    assert.deepEqual(shown.get('app.contract'), []);
    live.text = app(blank);
    assert.deepEqual(await problems('change', live), [[[7, 6, 7, 12], 'lower-button-unlabelled', 'warning', 'contract']]);
    // A check that fails does not stop the ones after it.
    const set = shown.set.bind(shown);
    shown.set = () => { shown.set = set; throw new Error('once'); };
    await assert.rejects(problems('change', live));
    assert.deepEqual(await problems('change', live), [[[7, 6, 7, 12], 'lower-button-unlabelled', 'warning', 'contract']]);
    // Closed: its problems go.
    const closing = document('lib/row.contract', row(used));
    await Promise.all(listeners.close.map(l => l(closing)));
    assert.equal(shown.has('lib/row.contract'), false);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('Enter indents after every opener whose body is a block, and not after a line that is whole', () => {
  const { indentationRules } = JSON.parse(readFileSync(resolve(import.meta.dir, 'language-configuration.json'), 'utf8'));
  const indents = new RegExp(indentationRules.increaseIndentPattern);
  for (const opener of ['theme', 'params', 'parts', 'members Composer', 'perf', 'sequence intro when open key=step', 'sequence spin mount',
    'test "boots"', 'test "boots" on web', 'test "boots" on web, macos', '  test "boots" on ios // the phone', 'component Row'])
    assert.ok(indents.test(opener), opener);
  for (const whole of ['theme.accent', 'params x', 'members "./more.contract"', 'test "boots" on', 'expect perf layout <= 3', 'text "parts"'])
    assert.ok(!indents.test(whole), whole);
});

test('forma grammar, snippets, and stale-binary guard in discoverServer are valid', () => {
  const formaGrammar = JSON.parse(readFileSync(resolve(import.meta.dir, 'syntaxes/exact-forma.tmLanguage.json'), 'utf8'));
  const formaCfg = JSON.parse(readFileSync(resolve(import.meta.dir, 'forma-language-configuration.json'), 'utf8'));
  const snippets = JSON.parse(readFileSync(resolve(import.meta.dir, 'snippets/exact-contract.code-snippets'), 'utf8'));
  assert.equal(formaGrammar.scopeName, 'source.exact-forma');
  assert.equal(formaCfg.comments.lineComment, '//');
  assert.ok(snippets.Component && snippets['For Loop'] && snippets.Resource);
  const dir = mkdtempSync(resolve(tmpdir(), 'exact-stale-bin-'));
  try {
    const staleBin = resolve(dir, 'target', 'debug', 'contract');
    mkdirSync(resolve(dir, 'target', 'debug'), { recursive: true });
    writeFileSync(staleBin, '#!/bin/sh\nexit 2\n');
    chmodSync(staleBin, 0o755);
    const found = discoverServer([dir]);
    assert.notEqual(found.command, staleBin);
    assert.ok(found.args.includes('lsp'));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('LspConnection drives `contract lsp --stdio` and `contract mcp --stdio` end-to-end', async () => {
  const dir = mkdtempSync(resolve(tmpdir(), 'exact-lsp-stdio-'));
  const appPath = resolve(dir, 'app.contract');
  const src = [
    'shape Task',
    '  id: string',
    '  title: string',
    '',
    'component App',
    '  state count = 0',
    '  action inc',
    '    count = count + 1',
    '  view',
    '    column',
    '      text `${count}`',
    '      button "Add" press=inc',
    '',
  ].join('\n');
  writeFileSync(appPath, src);

  const { command, args } = discoverServer([ROOT]);
  const lsp = new LspConnection(command, args, { cwd: ROOT });
  try {
    const init = await lsp.initialize({ rootUri: `file://${dir}` });
    assert.equal(init.serverInfo.name, 'exact-contract-lsp');
    const uri = `file://${appPath}`;
    lsp.notify('textDocument/didOpen', {
      textDocument: { uri, languageId: 'exact-contract', version: 1, text: src },
    });
    const hover = await lsp.request('textDocument/hover', {
      textDocument: { uri },
      position: { line: 5, character: 9 },
    });
    assert.match(hover.contents.value, /state count: number/);
    const outline = await lsp.request('exact/outline', { uri });
    assert.equal(outline.parsed, true);
    assert.deepEqual(outline.components[0].actions[0].writes, ['count']);

    const baseArgs = args.slice(0, args.indexOf('lsp'));
    const mcpProc = spawn(command, [...baseArgs, 'mcp', '--stdio'], { cwd: ROOT, stdio: ['pipe', 'pipe', 'pipe'] });
    const mcpReplies = [];
    let buf = '';
    const mcpDone = new Promise((done) => {
      mcpProc.stdout.on('data', (chunk) => {
        buf += chunk.toString('utf8');
        while (buf.includes('\n')) {
          const idx = buf.indexOf('\n');
          const line = buf.slice(0, idx).trim();
          buf = buf.slice(idx + 1);
          if (line) mcpReplies.push(JSON.parse(line));
          if (mcpReplies.length >= 2) done();
        }
      });
    });
    mcpProc.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: {} }) + '\n');
    mcpProc.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name: 'contract_vocab', arguments: { query: 'button', limit: 3 } } }) + '\n');
    mcpProc.stdin.end();
    await mcpDone;
    assert.equal(mcpReplies[0].result.serverInfo.name, 'exact-contract-mcp');
    assert.equal(mcpReplies[1].result.isError, false);
    mcpProc.kill();
  } finally {
    await lsp.stop();
    rmSync(dir, { recursive: true, force: true });
  }
}, 600_000);

test('full VS Code host activation avoids duplicate formatters/diagnostics when exactContract.compiler is set', async () => {
  const { command } = discoverServer([ROOT]);
  if (!existsSync(command)) { console.log('skipped: no contract binary'); return; }
  const { activate, deactivate } = await import('./extension.js');
  const formatters = [];
  const diagCollections = [];
  const commands = new Map();
  let hoverProvider = null;
  class Position { constructor(line, character) { this.line = line; this.character = character; } }
  class Range {
    constructor(a, b, c, d) {
      if (typeof a === 'number') { this.start = new Position(a, b); this.end = new Position(c, d); }
      else { this.start = a; this.end = b; }
    }
  }
  class Location { constructor(uri, range) { this.uri = uri; this.range = range; } }
  class Uri {
    constructor(fsPath) { this.fsPath = fsPath; this.scheme = 'file'; this._href = `file://${fsPath}`; }
    toString() { return this._href; }
    static parse(s) { return new Uri(s.replace(/^file:\/\//, '')); }
  }
  class Diagnostic { constructor(range, message, severity) { this.range = range; this.message = message; this.severity = severity; } }
  class MarkdownString { constructor(value) { this.value = value; } }
  class Hover { constructor(contents, range) { this.contents = contents; this.range = range; } }
  class SemanticTokensLegend { constructor(t, m) { this.tokenTypes = t; this.tokenModifiers = m; } }
  const dir = mkdtempSync(resolve(tmpdir(), 'exact-vscode-host-'));
  const appPath = resolve(dir, 'app.contract');
  const src = 'component App\n  state count = 0\n  view\n    text `${count}`\n';
  writeFileSync(appPath, src);
  const doc = { uri: new Uri(appPath), fileName: appPath, languageId: 'exact-contract', version: 1, getText: () => src };
  const noopDisp = () => ({ dispose() {} });
  const vscode = {
    Position, Range, Location, Uri, Diagnostic, MarkdownString, Hover, SemanticTokensLegend,
    DiagnosticSeverity: { Error: 0, Warning: 1, Information: 2, Hint: 3 },
    StatusBarAlignment: { Left: 1, Right: 2 },
    CodeActionKind: { QuickFix: 'quickfix', SourceOrganizeImports: 'source.organizeImports', SourceFixAll: 'source.fixAll' },
    extensions: { getExtension: () => undefined },
    window: {
      createOutputChannel: () => ({ append() {}, appendLine() {}, show() {}, dispose() {} }),
      createStatusBarItem: () => ({ text: '', tooltip: '', command: '', show() {}, dispose() {} }),
      showWarningMessage: () => Promise.resolve(),
      showErrorMessage: () => Promise.resolve(),
    },
    workspace: {
      workspaceFolders: [{ uri: new Uri(ROOT), name: 'exact' }],
      textDocuments: [doc],
      getConfiguration: (section) => ({
        get: (key, def) => {
          const full = section ? `${section}.${key}` : key;
          if (full === 'exactContract.compiler' || key === 'compiler') return command;
          return def;
        },
      }),
      onDidOpenTextDocument: noopDisp,
      onDidChangeTextDocument: noopDisp,
      onDidSaveTextDocument: noopDisp,
      onDidCloseTextDocument: noopDisp,
      onWillRenameFiles: noopDisp,
      onDidChangeConfiguration: noopDisp,
      createFileSystemWatcher: () => ({ onDidCreate: noopDisp, onDidChange: noopDisp, onDidDelete: noopDisp, dispose() {} }),
    },
    languages: {
      createDiagnosticCollection: (name) => {
        const col = { name, set() {}, delete() {}, dispose() {} };
        diagCollections.push(col);
        return col;
      },
      registerHoverProvider: (_sel, p) => { hoverProvider = p; return noopDisp(); },
      registerDocumentFormattingEditProvider: (_sel, p) => { formatters.push(p); return noopDisp(); },
      registerDefinitionProvider: noopDisp,
      registerTypeDefinitionProvider: noopDisp,
      registerReferenceProvider: noopDisp,
      registerDocumentHighlightProvider: noopDisp,
      registerRenameProvider: noopDisp,
      registerDocumentSymbolProvider: noopDisp,
      registerWorkspaceSymbolProvider: noopDisp,
      registerDocumentLinkProvider: noopDisp,
      registerColorProvider: noopDisp,
      registerSelectionRangeProvider: noopDisp,
      registerCallHierarchyProvider: noopDisp,
      registerCompletionItemProvider: noopDisp,
      registerSignatureHelpProvider: noopDisp,
      registerDocumentSemanticTokensProvider: noopDisp,
      registerInlayHintsProvider: noopDisp,
      registerCodeActionsProvider: noopDisp,
      registerFoldingRangeProvider: noopDisp,
      registerCodeLensProvider: noopDisp,
    },
    commands: { registerCommand: (id, fn) => { commands.set(id, fn); return noopDisp(); } },
  };
  const context = { subscriptions: [] };
  try {
    await activate(context, vscode);
    assert.equal(formatters.length, 1, 'must register exactly one formatter');
    assert.equal(diagCollections.length, 1, 'must register exactly one diagnostic collection');
    assert.equal(commands.size, 7);
    const hover = await hoverProvider.provideHover(doc, new Position(1, 9));
    assert.match(hover.contents.value, /state count: number/);
  } finally {
    await deactivate();
    for (const s of context.subscriptions) s?.dispose?.();
    rmSync(dir, { recursive: true, force: true });
  }
});
