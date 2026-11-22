// Exact Contract VS Code Extension — wires `contract lsp --stdio`
// to VS Code's language providers, diagnostics, commands, file watchers, and status bar.
'use strict';

let vscode;
const { discoverServer, LspConnection } = require('./lsp-client');

const SELECTOR = { language: 'exact-contract', scheme: 'file' };

const TOKEN_TYPES = [
  'namespace', 'type', 'class', 'enum', 'interface', 'struct', 'typeParameter',
  'parameter', 'variable', 'property', 'enumMember', 'event', 'function',
  'method', 'macro', 'keyword', 'modifier', 'comment', 'string', 'number',
  'regexp', 'operator', 'decorator',
];
const TOKEN_MODIFIERS = [
  'declaration', 'definition', 'readonly', 'static', 'deprecated',
  'abstract', 'async', 'modification', 'documentation', 'defaultLibrary',
];

function toVscodeRange(r) {
  return new vscode.Range(
    new vscode.Position(r.start.line, r.start.character),
    new vscode.Position(r.end.line, r.end.character),
  );
}

function toVscodeLocation(loc) {
  return new vscode.Location(vscode.Uri.parse(loc.uri), toVscodeRange(loc.range));
}

function toLspPosition(pos) {
  return { line: pos.line, character: pos.character };
}

function toLspRange(range) {
  return { start: toLspPosition(range.start), end: toLspPosition(range.end) };
}

function toVscodeSeverity(sev) {
  switch (sev) {
    case 1: return vscode.DiagnosticSeverity.Error;
    case 2: return vscode.DiagnosticSeverity.Warning;
    case 3: return vscode.DiagnosticSeverity.Information;
    case 4: return vscode.DiagnosticSeverity.Hint;
    default: return vscode.DiagnosticSeverity.Error;
  }
}

function toVscodeSymbolKind(k) {
  return Math.max(0, (k || 13) - 1);
}

function toVscodeCompletionKind(k) {
  return Math.max(0, (k || 1) - 1);
}

function toVscodeDocumentSymbols(items) {
  return (items || []).map((s) => {
    const sym = new vscode.DocumentSymbol(
      s.name,
      s.detail || '',
      toVscodeSymbolKind(s.kind),
      toVscodeRange(s.range),
      toVscodeRange(s.selectionRange || s.range),
    );
    if (Array.isArray(s.children) && s.children.length) {
      sym.children = toVscodeDocumentSymbols(s.children);
    }
    return sym;
  });
}

function toVscodeWorkspaceEdit(edit) {
  const wsEdit = new vscode.WorkspaceEdit();
  if (!edit || !edit.changes) return wsEdit;
  for (const [uri, edits] of Object.entries(edit.changes)) {
    const parsed = vscode.Uri.parse(uri);
    for (const e of edits) {
      wsEdit.replace(parsed, toVscodeRange(e.range), e.newText);
    }
  }
  return wsEdit;
}

function toVscodeMarkdown(doc) {
  if (!doc) return undefined;
  if (typeof doc === 'string') return new vscode.MarkdownString(doc);
  if (doc.value) return new vscode.MarkdownString(doc.value);
  return undefined;
}

function toVscodeSelectionRange(sr) {
  if (!sr?.range) return undefined;
  return new vscode.SelectionRange(
    toVscodeRange(sr.range),
    sr.parent ? toVscodeSelectionRange(sr.parent) : undefined,
  );
}

function toVscodeCallHierarchyItem(raw, store) {
  const item = new vscode.CallHierarchyItem(
    toVscodeSymbolKind(raw.kind),
    raw.name,
    raw.detail || '',
    vscode.Uri.parse(raw.uri),
    toVscodeRange(raw.range),
    toVscodeRange(raw.selectionRange || raw.range),
  );
  store.set(item, raw);
  return item;
}

class ExactContractExtension {
  constructor(context) {
    this.context = context;
    this.output = vscode.window.createOutputChannel('Exact Contract LSP');
    this.diagnostics = vscode.languages.createDiagnosticCollection('exact-contract');
    this.statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    this.statusBar.command = 'exactContract.restartServer';
    this.client = null;
    this.ready = null;
    this.stopping = false;
    this.counts = new Map();
    this.hierarchyRaw = new WeakMap();
    context.subscriptions.push(this.output, this.diagnostics, this.statusBar);
  }

  config() {
    const cfg = vscode.workspace.getConfiguration('exactContract');
    return {
      serverPath: cfg.get('server.path', '') || cfg.get('compiler', ''),
      serverArgs: cfg.get('server.args', []),
      inlayHints: cfg.get('inlayHints.enabled', true),
      codeLens: cfg.get('codeLens.enabled', true),
      trace: cfg.get('trace.server', 'off'),
    };
  }

  updateStatus(stateText) {
    let errors = 0;
    let warnings = 0;
    for (const c of this.counts.values()) {
      errors += c.errors;
      warnings += c.warnings;
    }
    if (stateText) {
      this.statusBar.text = `$(symbol-misc) Contract: ${stateText}`;
    } else if (errors > 0) {
      this.statusBar.text = `$(error) Contract (${errors}E ${warnings}W)`;
    } else if (warnings > 0) {
      this.statusBar.text = `$(warning) Contract (${warnings}W)`;
    } else {
      this.statusBar.text = '$(check) Contract';
    }
    this.statusBar.tooltip = 'Exact Contract Language Server (click to restart)';
    this.statusBar.show();
  }

  async startServer() {
    await this.stopServer();
    this.stopping = false;
    const folders = (vscode.workspace.workspaceFolders || []).map((f) => f.uri.fsPath);
    const cfg = this.config();
    const { command, args } = discoverServer(folders, cfg);
    const targetDesc = `${command} ${args.join(' ')}`;
    this.output.appendLine(`Starting Exact Contract LSP: ${targetDesc}`);
    this.updateStatus('starting…');

    const conn = new LspConnection(command, args, {
      cwd: folders[0],
      onNotification: (method, params) => this._onNotification(method, params),
      onStderr: (text) => this.output.append(text),
      onExit: (code, err) => {
        if (!this.stopping) {
          this.output.appendLine(`Contract LSP exited (${code})${err ? `: ${err.message}` : ''}`);
          this.updateStatus('stopped');
        }
      },
    });
    this.client = conn;

    const workspaceFolders = (vscode.workspace.workspaceFolders || []).map((f) => ({
      uri: f.uri.toString(),
      name: f.name,
    }));
    this.ready = conn.initialize({
      rootUri: workspaceFolders[0]?.uri ?? null,
      workspaceFolders,
    }).then((res) => {
      this.output.appendLine(`Connected to ${res?.serverInfo?.name || 'contract-lsp'} ${res?.serverInfo?.version || ''}`);
      this.updateStatus();
      for (const doc of vscode.workspace.textDocuments) {
        if (doc.languageId === 'exact-contract' && doc.uri.scheme === 'file') {
          this._sendDidOpen(doc);
        }
      }
      return res;
    }).catch((err) => {
      this.output.appendLine(`Failed to start Contract LSP (${targetDesc}): ${err.message}`);
      this.updateStatus('error');
      vscode.window.showErrorMessage(
        `Exact Contract LSP failed (${err.message}). Target: ${targetDesc}`,
        'Show Output',
        'Configure Server Path',
      ).then((choice) => {
        if (choice === 'Show Output') this.output.show(true);
        if (choice === 'Configure Server Path') {
          vscode.commands.executeCommand('workbench.action.openSettings', 'exactContract.server.path');
        }
      });
      throw err;
    });
    return this.ready;
  }

  async stopServer() {
    this.stopping = true;
    const client = this.client;
    this.client = null;
    this.ready = null;
    if (client) await client.stop();
  }

  async request(method, params) {
    if (!this.ready) return null;
    try {
      await this.ready;
      if (!this.client) return null;
      return await this.client.request(method, params);
    } catch (err) {
      if (this.config().trace !== 'off') {
        this.output.appendLine(`[${method}] ${err.message}`);
      }
      return null;
    }
  }

  _sendDidOpen(doc) {
    if (!this.client) return;
    this.client.notify('textDocument/didOpen', {
      textDocument: {
        uri: doc.uri.toString(),
        languageId: doc.languageId,
        version: doc.version,
        text: doc.getText(),
      },
    });
  }

  _onNotification(method, params) {
    if (method !== 'textDocument/publishDiagnostics' || !params?.uri) return;
    const uri = vscode.Uri.parse(params.uri);
    let errors = 0;
    let warnings = 0;
    const items = (params.diagnostics || []).map((d) => {
      const sev = toVscodeSeverity(d.severity);
      if (sev === vscode.DiagnosticSeverity.Error) errors += 1;
      if (sev === vscode.DiagnosticSeverity.Warning) warnings += 1;
      const diag = new vscode.Diagnostic(toVscodeRange(d.range), d.message, sev);
      diag.code = d.code;
      diag.source = d.source || 'exact-contract';
      if (Array.isArray(d.relatedInformation)) {
        diag.relatedInformation = d.relatedInformation.map(
          (r) => new vscode.DiagnosticRelatedInformation(toVscodeLocation(r.location), r.message),
        );
      }
      return diag;
    });
    this.diagnostics.set(uri, items);
    this.counts.set(params.uri, { errors, warnings });
    this.updateStatus();
  }

  registerProviders() {
    const subs = this.context.subscriptions;
    const legend = new vscode.SemanticTokensLegend(TOKEN_TYPES, TOKEN_MODIFIERS);

    // Document lifecycle sync.
    subs.push(
      vscode.workspace.onDidOpenTextDocument((doc) => {
        if (doc.languageId === 'exact-contract' && doc.uri.scheme === 'file') this._sendDidOpen(doc);
      }),
      vscode.workspace.onDidChangeTextDocument((e) => {
        if (e.document.languageId !== 'exact-contract' || e.document.uri.scheme !== 'file' || !this.client) return;
        this.client.notify('textDocument/didChange', {
          textDocument: { uri: e.document.uri.toString(), version: e.document.version },
          contentChanges: e.contentChanges.map((c) => ({
            range: toLspRange(c.range),
            text: c.text,
          })),
        });
      }),
      vscode.workspace.onDidSaveTextDocument((doc) => {
        if (doc.languageId !== 'exact-contract' || doc.uri.scheme !== 'file' || !this.client) return;
        this.client.notify('textDocument/didSave', {
          textDocument: { uri: doc.uri.toString() },
          text: doc.getText(),
        });
      }),
      vscode.workspace.onDidCloseTextDocument((doc) => {
        if (doc.languageId !== 'exact-contract' || doc.uri.scheme !== 'file' || !this.client) return;
        this.client.notify('textDocument/didClose', { textDocument: { uri: doc.uri.toString() } });
      }),
      vscode.workspace.onDidChangeConfiguration((e) => {
        if (
          e.affectsConfiguration('exactContract.server.path')
          || e.affectsConfiguration('exactContract.server.args')
          || e.affectsConfiguration('exactContract.compiler')
        ) {
          this.startServer().catch(() => {});
        }
      }),
      vscode.workspace.onWillRenameFiles((e) => {
        const contractFiles = e.files.filter(
          (f) => f.oldUri.fsPath.endsWith('.contract') || f.newUri.fsPath.endsWith('.contract'),
        );
        if (!contractFiles.length) return;
        e.waitUntil(
          this.request('workspace/willRenameFiles', {
            files: contractFiles.map((f) => ({
              oldUri: f.oldUri.toString(),
              newUri: f.newUri.toString(),
            })),
          }).then((edit) => (edit ? toVscodeWorkspaceEdit(edit) : undefined)),
        );
      }),
    );

    const watcher = vscode.workspace.createFileSystemWatcher('**/{*.contract,*.forma,app.json,strings/*.json}');
    const onWatch = (uri, type) => {
      if (!this.client) return;
      this.client.notify('workspace/didChangeWatchedFiles', {
        changes: [{ uri: uri.toString(), type }],
      });
    };
    subs.push(
      watcher,
      watcher.onDidCreate((u) => onWatch(u, 1)),
      watcher.onDidChange((u) => onWatch(u, 2)),
      watcher.onDidDelete((u) => onWatch(u, 3)),
    );

    // Language feature providers.
    subs.push(
      vscode.languages.registerHoverProvider(SELECTOR, {
        provideHover: async (doc, pos) => {
          const res = await this.request('textDocument/hover', {
            textDocument: { uri: doc.uri.toString() },
            position: toLspPosition(pos),
          });
          if (!res?.contents) return null;
          const md = toVscodeMarkdown(res.contents);
          return new vscode.Hover(md, res.range ? toVscodeRange(res.range) : undefined);
        },
      }),
      vscode.languages.registerDefinitionProvider(SELECTOR, {
        provideDefinition: async (doc, pos) => {
          const res = await this.request('textDocument/definition', {
            textDocument: { uri: doc.uri.toString() },
            position: toLspPosition(pos),
          });
          if (!res) return null;
          return Array.isArray(res) ? res.map(toVscodeLocation) : toVscodeLocation(res);
        },
      }),
      vscode.languages.registerTypeDefinitionProvider(SELECTOR, {
        provideTypeDefinition: async (doc, pos) => {
          const res = await this.request('textDocument/typeDefinition', {
            textDocument: { uri: doc.uri.toString() },
            position: toLspPosition(pos),
          });
          if (!res) return null;
          return Array.isArray(res) ? res.map(toVscodeLocation) : toVscodeLocation(res);
        },
      }),
      vscode.languages.registerReferenceProvider(SELECTOR, {
        provideReferences: async (doc, pos, ctx) => {
          const res = await this.request('textDocument/references', {
            textDocument: { uri: doc.uri.toString() },
            position: toLspPosition(pos),
            context: { includeDeclaration: ctx.includeDeclaration },
          });
          return Array.isArray(res) ? res.map(toVscodeLocation) : [];
        },
      }),
      vscode.languages.registerDocumentHighlightProvider(SELECTOR, {
        provideDocumentHighlights: async (doc, pos) => {
          const res = await this.request('textDocument/documentHighlight', {
            textDocument: { uri: doc.uri.toString() },
            position: toLspPosition(pos),
          });
          return (res || []).map(
            (h) => new vscode.DocumentHighlight(toVscodeRange(h.range), (h.kind || 1) - 1),
          );
        },
      }),
      vscode.languages.registerRenameProvider(SELECTOR, {
        prepareRename: async (doc, pos) => {
          const res = await this.request('textDocument/prepareRename', {
            textDocument: { uri: doc.uri.toString() },
            position: toLspPosition(pos),
          });
          if (!res?.range) throw new Error('Symbol cannot be renamed');
          return { range: toVscodeRange(res.range), placeholder: res.placeholder };
        },
        provideRenameEdits: async (doc, pos, newName) => {
          const res = await this.request('textDocument/rename', {
            textDocument: { uri: doc.uri.toString() },
            position: toLspPosition(pos),
            newName,
          });
          return res ? toVscodeWorkspaceEdit(res) : null;
        },
      }),
      vscode.languages.registerDocumentSymbolProvider(SELECTOR, {
        provideDocumentSymbols: async (doc) => {
          const res = await this.request('textDocument/documentSymbol', {
            textDocument: { uri: doc.uri.toString() },
          });
          return toVscodeDocumentSymbols(res);
        },
      }),
      vscode.languages.registerWorkspaceSymbolProvider({
        provideWorkspaceSymbols: async (query) => {
          const res = await this.request('workspace/symbol', { query });
          return (res || []).map(
            (s) => new vscode.SymbolInformation(
              s.name,
              toVscodeSymbolKind(s.kind),
              s.containerName || '',
              toVscodeLocation(s.location),
            ),
          );
        },
      }),
      vscode.languages.registerDocumentLinkProvider(SELECTOR, {
        provideDocumentLinks: async (doc) => {
          const res = await this.request('textDocument/documentLink', {
            textDocument: { uri: doc.uri.toString() },
          });
          return (res || []).map((l) => {
            const link = new vscode.DocumentLink(
              toVscodeRange(l.range),
              l.target ? vscode.Uri.parse(l.target) : undefined,
            );
            if (l.tooltip) link.tooltip = l.tooltip;
            return link;
          });
        },
      }),
      vscode.languages.registerColorProvider(SELECTOR, {
        provideDocumentColors: async (doc) => {
          const res = await this.request('textDocument/documentColor', {
            textDocument: { uri: doc.uri.toString() },
          });
          return (res || []).map(
            (c) => new vscode.ColorInformation(
              toVscodeRange(c.range),
              new vscode.Color(c.color.red, c.color.green, c.color.blue, c.color.alpha),
            ),
          );
        },
        provideColorPresentations: async (color, ctx) => {
          const res = await this.request('textDocument/colorPresentation', {
            textDocument: { uri: ctx.document.uri.toString() },
            color: { red: color.red, green: color.green, blue: color.blue, alpha: color.alpha },
            range: toLspRange(ctx.range),
          });
          return (res || []).map((p) => {
            const pres = new vscode.ColorPresentation(p.label);
            if (p.textEdit) {
              pres.textEdit = vscode.TextEdit.replace(toVscodeRange(p.textEdit.range), p.textEdit.newText);
            }
            return pres;
          });
        },
      }),
      vscode.languages.registerSelectionRangeProvider(SELECTOR, {
        provideSelectionRanges: async (doc, positions) => {
          const res = await this.request('textDocument/selectionRange', {
            textDocument: { uri: doc.uri.toString() },
            positions: positions.map(toLspPosition),
          });
          return (res || []).map(toVscodeSelectionRange).filter(Boolean);
        },
      }),
      vscode.languages.registerCallHierarchyProvider(SELECTOR, {
        prepareCallHierarchy: async (doc, pos) => {
          const res = await this.request('textDocument/prepareCallHierarchy', {
            textDocument: { uri: doc.uri.toString() },
            position: toLspPosition(pos),
          });
          return (res || []).map((raw) => toVscodeCallHierarchyItem(raw, this.hierarchyRaw));
        },
        provideCallHierarchyIncomingCalls: async (item) => {
          const raw = this.hierarchyRaw.get(item);
          if (!raw) return [];
          const res = await this.request('callHierarchy/incomingCalls', { item: raw });
          return (res || []).map(
            (c) => new vscode.CallHierarchyIncomingCall(
              toVscodeCallHierarchyItem(c.from, this.hierarchyRaw),
              (c.fromRanges || []).map(toVscodeRange),
            ),
          );
        },
        provideCallHierarchyOutgoingCalls: async (item) => {
          const raw = this.hierarchyRaw.get(item);
          if (!raw) return [];
          const res = await this.request('callHierarchy/outgoingCalls', { item: raw });
          return (res || []).map(
            (c) => new vscode.CallHierarchyOutgoingCall(
              toVscodeCallHierarchyItem(c.to, this.hierarchyRaw),
              (c.fromRanges || []).map(toVscodeRange),
            ),
          );
        },
      }),
      vscode.languages.registerCompletionItemProvider(
        SELECTOR,
        {
          provideCompletionItems: async (doc, pos) => {
            const res = await this.request('textDocument/completion', {
              textDocument: { uri: doc.uri.toString() },
              position: toLspPosition(pos),
            });
            const list = Array.isArray(res) ? res : res?.items || [];
            return list.map((i) => {
              const item = new vscode.CompletionItem(i.label, toVscodeCompletionKind(i.kind));
              if (i.detail) item.detail = i.detail;
              if (i.documentation) item.documentation = toVscodeMarkdown(i.documentation);
              if (i.sortText) item.sortText = i.sortText;
              if (i.insertText) {
                item.insertText = i.insertTextFormat === 2
                  ? new vscode.SnippetString(i.insertText)
                  : i.insertText;
              }
              if (Array.isArray(i.additionalTextEdits)) {
                item.additionalTextEdits = i.additionalTextEdits.map(
                  (e) => vscode.TextEdit.replace(toVscodeRange(e.range), e.newText),
                );
              }
              return item;
            });
          },
        },
        '.', '=', '"', ':', '/', '(',
      ),
      vscode.languages.registerSignatureHelpProvider(
        SELECTOR,
        {
          provideSignatureHelp: async (doc, pos) => {
            const res = await this.request('textDocument/signatureHelp', {
              textDocument: { uri: doc.uri.toString() },
              position: toLspPosition(pos),
            });
            if (!res?.signatures?.length) return null;
            const help = new vscode.SignatureHelp();
            help.activeSignature = res.activeSignature || 0;
            help.activeParameter = res.activeParameter || 0;
            help.signatures = res.signatures.map((s) => {
              const sig = new vscode.SignatureInformation(s.label, toVscodeMarkdown(s.documentation));
              sig.parameters = (s.parameters || []).map((p) => new vscode.ParameterInformation(p.label));
              return sig;
            });
            return help;
          },
        },
        '(', ',', '=',
      ),
      vscode.languages.registerDocumentSemanticTokensProvider(
        SELECTOR,
        {
          provideDocumentSemanticTokens: async (doc) => {
            const res = await this.request('textDocument/semanticTokens/full', {
              textDocument: { uri: doc.uri.toString() },
            });
            if (!res?.data) return null;
            return new vscode.SemanticTokens(new Uint32Array(res.data));
          },
        },
        legend,
      ),
      vscode.languages.registerInlayHintsProvider(SELECTOR, {
        provideInlayHints: async (doc, range) => {
          if (!this.config().inlayHints) return [];
          const res = await this.request('textDocument/inlayHint', {
            textDocument: { uri: doc.uri.toString() },
            range: toLspRange(range),
          });
          return (res || []).map((h) => {
            const hint = new vscode.InlayHint(
              new vscode.Position(h.position.line, h.position.character),
              h.label,
              h.kind === 1 ? vscode.InlayHintKind.Type : vscode.InlayHintKind.Parameter,
            );
            hint.paddingLeft = Boolean(h.paddingLeft);
            hint.paddingRight = Boolean(h.paddingRight);
            return hint;
          });
        },
      }),
      vscode.languages.registerCodeActionsProvider(
        SELECTOR,
        {
          provideCodeActions: async (doc, range, ctx) => {
            const res = await this.request('textDocument/codeAction', {
              textDocument: { uri: doc.uri.toString() },
              range: toLspRange(range),
              context: {
                diagnostics: ctx.diagnostics.map((d) => ({
                  range: toLspRange(d.range),
                  code: typeof d.code === 'object' ? d.code.value : d.code,
                  message: d.message,
                })),
              },
            });
            return (res || []).map((a) => {
              const kind = a.kind === 'source.organizeImports'
                ? vscode.CodeActionKind.SourceOrganizeImports
                : a.kind === 'source.fixAll'
                  ? vscode.CodeActionKind.SourceFixAll
                  : vscode.CodeActionKind.QuickFix;
              const act = new vscode.CodeAction(a.title, kind);
              act.isPreferred = Boolean(a.isPreferred);
              if (a.edit) act.edit = toVscodeWorkspaceEdit(a.edit);
              return act;
            });
          },
        },
        {
          providedCodeActionKinds: [
            vscode.CodeActionKind.QuickFix,
            vscode.CodeActionKind.SourceOrganizeImports,
            vscode.CodeActionKind.SourceFixAll,
          ],
        },
      ),
      vscode.languages.registerDocumentFormattingEditProvider(SELECTOR, {
        provideDocumentFormattingEdits: async (doc) => {
          const res = await this.request('textDocument/formatting', {
            textDocument: { uri: doc.uri.toString() },
            options: { tabSize: 2, insertSpaces: true },
          });
          return (res || []).map((e) => vscode.TextEdit.replace(toVscodeRange(e.range), e.newText));
        },
      }),
      vscode.languages.registerFoldingRangeProvider(SELECTOR, {
        provideFoldingRanges: async (doc) => {
          const res = await this.request('textDocument/foldingRange', {
            textDocument: { uri: doc.uri.toString() },
          });
          return (res || []).map(
            (f) => new vscode.FoldingRange(
              f.startLine,
              f.endLine,
              f.kind === 'comment' ? vscode.FoldingRangeKind.Comment : undefined,
            ),
          );
        },
      }),
      vscode.languages.registerCodeLensProvider(SELECTOR, {
        provideCodeLenses: async (doc) => {
          if (!this.config().codeLens) return [];
          const res = await this.request('textDocument/codeLens', {
            textDocument: { uri: doc.uri.toString() },
          });
          return (res || []).map(
            (l) => new vscode.CodeLens(toVscodeRange(l.range), l.command),
          );
        },
      }),
    );

    // Commands.
    subs.push(
      vscode.commands.registerCommand('exactContract.restartServer', async () => {
        try {
          await this.startServer();
        } catch {
          // Error is already shown via toast and Output channel.
        }
      }),
      vscode.commands.registerCommand('exactContract.fixUses', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor || editor.document.languageId !== 'exact-contract') return;
        const actions = await this.request('textDocument/codeAction', {
          textDocument: { uri: editor.document.uri.toString() },
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
          context: { diagnostics: [] },
        });
        const org = (actions || []).find((a) => a.kind === 'source.organizeImports');
        if (org?.edit) {
          await vscode.workspace.applyEdit(toVscodeWorkspaceEdit(org.edit));
        } else {
          vscode.window.showInformationMessage('All Contract `use` imports are up to date.');
        }
      }),
      vscode.commands.registerCommand('exactContract.applyQuickFixes', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor || editor.document.languageId !== 'exact-contract') return;
        const res = await this.request('exact/fix', {
          uri: editor.document.uri.toString(),
          apply: false,
          format: true,
        });
        if (!res?.ok || !res.changed || typeof res.text !== 'string') {
          vscode.window.showInformationMessage('No deterministic Contract quick fixes to apply.');
          return;
        }
        const doc = editor.document;
        const fullRange = new vscode.Range(
          doc.positionAt(0),
          doc.positionAt(doc.getText().length),
        );
        const edit = new vscode.WorkspaceEdit();
        edit.replace(doc.uri, fullRange, res.text);
        await vscode.workspace.applyEdit(edit);
        vscode.window.showInformationMessage(`Applied ${(res.fixes || []).length} Contract fix(es).`);
      }),
      vscode.commands.registerCommand('exactContract.lookupVocab', async () => {
        const query = await vscode.window.showInputBox({
          prompt: 'Look up or search a Contract tag, attribute, event, host command, function, or concept',
          placeHolder: 'e.g. button, background-color, navigate, filter, list',
        });
        if (!query) return;
        const trimmed = query.trim();
        let res = await this.request('exact/vocab', { name: trimmed });
        if (!res?.known) {
          res = await this.request('exact/vocab', { query: trimmed, limit: 15 });
        }
        const doc = await vscode.workspace.openTextDocument({
          language: 'json',
          content: JSON.stringify(res, null, 2),
        });
        await vscode.window.showTextDocument(doc, { preview: true });
      }),
      vscode.commands.registerCommand('exactContract.showOutline', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor || editor.document.languageId !== 'exact-contract') return;
        const res = await this.request('exact/outline', { uri: editor.document.uri.toString() });
        const doc = await vscode.workspace.openTextDocument({
          language: 'json',
          content: JSON.stringify(res, null, 2),
        });
        await vscode.window.showTextDocument(doc, { preview: true });
      }),
      vscode.commands.registerCommand('exactContract.checkSeam', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor || editor.document.languageId !== 'exact-contract') return;
        const res = await this.request('exact/seam', { uri: editor.document.uri.toString() });
        const doc = await vscode.workspace.openTextDocument({
          language: 'json',
          content: JSON.stringify(res, null, 2),
        });
        await vscode.window.showTextDocument(doc, { preview: true });
      }),
      vscode.commands.registerCommand('exactContract.showImpact', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor || editor.document.languageId !== 'exact-contract') return;
        const wordRange = editor.document.getWordRangeAtPosition(editor.selection.active);
        const initial = wordRange ? editor.document.getText(wordRange) : '';
        const symbol = await vscode.window.showInputBox({
          prompt: 'Symbol name to analyze blast radius for',
          value: initial,
          placeHolder: 'e.g. Station, App, selectStation',
        });
        if (!symbol) return;
        const res = await this.request('exact/impact', {
          uri: editor.document.uri.toString(),
          symbol: symbol.trim(),
        });
        const doc = await vscode.workspace.openTextDocument({
          language: 'json',
          content: JSON.stringify(res, null, 2),
        });
        await vscode.window.showTextDocument(doc, { preview: true });
      }),
    );
  }
}

let ext = null;

async function activate(context, vscodeApi = require('vscode')) {
  vscode = vscodeApi;
  ext = new ExactContractExtension(context);
  ext.registerProviders();
  try {
    await ext.startServer();
  } catch {
    // Error is logged to Output channel and status bar.
  }
}

async function deactivate() {
  if (ext) {
    await ext.stopServer();
    ext = null;
  }
}

module.exports = { activate, deactivate };
