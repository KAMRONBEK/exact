//! Contract Language Server Protocol (LSP 3.17) and MCP server.
//!
//! Serves VS Code, Cursor, Neovim, Helix, Zed, Emacs, and coding agents over
//! stdio JSON-RPC 2.0 (`Content-Length` framing). Also exposes native MCP mode
//! via [`mcp`] (`contract mcp`).

mod agent_tools;
mod agents;
mod completion;
mod completion_ext;
mod features;
mod hierarchy;
/// Native Model Context Protocol (MCP) server over stdio (`contract mcp`).
pub mod mcp;
mod navigation;
pub(crate) mod protocol;
mod seam;
mod workspace;

use protocol::{path_to_uri, uri_to_path, TOKEN_MODIFIERS, TOKEN_TYPES};
use serde_json::{json, Value};
use std::io::{self, BufRead, BufReader, Write};
use std::process::ExitCode;
use workspace::Workspace;

const USAGE: &str =
    "usage: contract lsp [--stdio]\n  runs the Contract Language Server (LSP 3.17) over stdio";

/// Maximum accepted JSON-RPC payload size (16 MiB).
const MAX_CONTENT_LENGTH: usize = 16 * 1024 * 1024;

/// Run `contract lsp [--stdio]`.
pub fn run(args: &[String]) -> ExitCode {
    match args {
        [] => run_stdio(),
        [flag] if flag == "--stdio" => run_stdio(),
        [flag] if matches!(flag.as_str(), "--help" | "-h") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run_stdio() -> ExitCode {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    let mut server = Server::new();
    while let Some(msg) = read_message(&mut reader) {
        let replies = server.handle_message(&msg);
        for reply in replies {
            if write_message(&mut writer, &reply).is_err() {
                return ExitCode::FAILURE;
            }
        }
        if server.should_exit() {
            return if server.shutdown_requested() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
    }
    ExitCode::SUCCESS
}

/// Stateful LSP server. Embedders and tests drive it directly via
/// [`Server::handle_message`] without spawning a child process.
#[derive(Debug, Default)]
pub struct Server {
    ws: Workspace,
    shutdown: bool,
    exit: bool,
}

impl Server {
    /// Create a fresh server with an empty workspace.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the client has sent `exit`.
    pub fn should_exit(&self) -> bool {
        self.exit
    }

    /// Whether `shutdown` was requested before `exit`.
    pub fn shutdown_requested(&self) -> bool {
        self.shutdown
    }

    /// Handle one JSON-RPC 2.0 request or notification and return zero or more
    /// outgoing responses and notifications.
    pub fn handle_message(&mut self, msg: &Value) -> Vec<Value> {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        let Some(id) = id else {
            return self.handle_notification(method, &params);
        };

        let result = match method {
            "initialize" => Ok(self.initialize(&params)),
            "shutdown" => {
                self.shutdown = true;
                Ok(Value::Null)
            }
            "textDocument/diagnostic" => Ok(self.pull_diagnostics(&params)),
            "workspace/diagnostic" => Ok(hierarchy::workspace_diagnostics(&mut self.ws)),
            "workspace/willRenameFiles" => Ok(hierarchy::will_rename_files(&mut self.ws, &params)),
            "textDocument/hover" => Ok(navigation::hover(&mut self.ws, &params)),
            "textDocument/definition" => Ok(navigation::definition(&mut self.ws, &params)),
            "textDocument/typeDefinition" => Ok(navigation::type_definition(&mut self.ws, &params)),
            "textDocument/references" => Ok(navigation::references(&mut self.ws, &params)),
            "textDocument/documentHighlight" => {
                Ok(navigation::document_highlight(&mut self.ws, &params))
            }
            "textDocument/prepareRename" => Ok(navigation::prepare_rename(&mut self.ws, &params)),
            "textDocument/rename" => navigation::rename(&mut self.ws, &params),
            "textDocument/documentSymbol" => Ok(navigation::document_symbol(&mut self.ws, &params)),
            "workspace/symbol" => Ok(navigation::workspace_symbol(&mut self.ws, &params)),
            "textDocument/codeLens" => Ok(navigation::code_lens(&mut self.ws, &params)),
            "textDocument/documentLink" => Ok(hierarchy::document_links(&mut self.ws, &params)),
            "textDocument/documentColor" => Ok(hierarchy::document_colors(&mut self.ws, &params)),
            "textDocument/colorPresentation" => Ok(hierarchy::color_presentation(&params)),
            "textDocument/selectionRange" => Ok(hierarchy::selection_ranges(&mut self.ws, &params)),
            "textDocument/prepareCallHierarchy" => {
                Ok(hierarchy::prepare_call_hierarchy(&mut self.ws, &params))
            }
            "callHierarchy/incomingCalls" => Ok(hierarchy::incoming_calls(&mut self.ws, &params)),
            "callHierarchy/outgoingCalls" => Ok(hierarchy::outgoing_calls(&mut self.ws, &params)),
            "textDocument/completion" => Ok(completion::complete(&mut self.ws, &params)),
            "textDocument/signatureHelp" => Ok(completion::signature_help(&mut self.ws, &params)),
            "textDocument/semanticTokens/full" => {
                Ok(features::semantic_tokens(&mut self.ws, &params))
            }
            "textDocument/inlayHint" => Ok(features::inlay_hints(&mut self.ws, &params)),
            "textDocument/codeAction" => Ok(features::code_actions(&mut self.ws, &params)),
            "textDocument/formatting" => Ok(features::formatting(&mut self.ws, &params)),
            "textDocument/foldingRange" => Ok(features::folding_ranges(&mut self.ws, &params)),
            // Agent-oriented custom extension methods.
            "exact/vocab" => Ok(agents::vocab(&params)),
            "exact/symbols" => Ok(agents::symbols(&mut self.ws, &params)),
            "exact/diagnostics" => Ok(agents::diagnostics(&mut self.ws, &params)),
            "exact/outline" => Ok(agents::outline(&mut self.ws, &params)),
            "exact/check" => Ok(agents::check(&mut self.ws, &params)),
            "exact/seam" => Ok(
                match params
                    .get("uri")
                    .and_then(Value::as_str)
                    .and_then(uri_to_path)
                {
                    Some(p) => seam::audit_seam(&mut self.ws, &p),
                    None => json!({ "ok": false, "error": "missing uri" }),
                },
            ),
            "exact/impact" => Ok(agent_tools::impact(&mut self.ws, &params)),
            "exact/context" => Ok(agent_tools::context_slice(&mut self.ws, &params)),
            "exact/fix" => Ok(agent_tools::fix(&mut self.ws, &params)),
            "exact/refactor" => Ok(agent_tools::refactor(&mut self.ws, &params)),
            _ => Err((-32601, format!("method not found: {method}"))),
        };

        let response = match result {
            Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
            Err((code, message)) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": code, "message": message },
            }),
        };
        vec![response]
    }

    fn handle_notification(&mut self, method: &str, params: &Value) -> Vec<Value> {
        match method {
            "initialized" | "$/cancelRequest" | "$/setTrace" => Vec::new(),
            "exit" => {
                self.exit = true;
                Vec::new()
            }
            "textDocument/didOpen" => {
                let Some(td) = params.get("textDocument") else {
                    return Vec::new();
                };
                let Some(path) = td.get("uri").and_then(Value::as_str).and_then(uri_to_path) else {
                    return Vec::new();
                };
                let text = td
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let version = td.get("version").and_then(Value::as_i64);
                self.ws.open(path.clone(), text, version);
                self.publish_diagnostics(&path)
            }
            "textDocument/didChange" => {
                let Some(td) = params.get("textDocument") else {
                    return Vec::new();
                };
                let Some(path) = td.get("uri").and_then(Value::as_str).and_then(uri_to_path) else {
                    return Vec::new();
                };
                let version = td.get("version").and_then(Value::as_i64);
                let changes = params
                    .get("contentChanges")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                self.ws.update(&path, changes, version);
                self.publish_diagnostics(&path)
            }
            "textDocument/didSave" => {
                let Some(path) = params
                    .pointer("/textDocument/uri")
                    .and_then(Value::as_str)
                    .and_then(uri_to_path)
                else {
                    return Vec::new();
                };
                if let Some(text) = params.get("text").and_then(Value::as_str) {
                    let version = self.ws.docs.get(&path).and_then(|d| d.version);
                    self.ws.open(path.clone(), text.to_owned(), version);
                } else {
                    self.ws.invalidate_path(&path);
                }
                self.publish_diagnostics(&path)
            }
            "textDocument/didClose" => {
                let Some(path) = params
                    .pointer("/textDocument/uri")
                    .and_then(Value::as_str)
                    .and_then(uri_to_path)
                else {
                    return Vec::new();
                };
                self.ws.close(&path);
                let uri = path_to_uri(&path);
                self.ws.published_uris.remove(&uri);
                vec![json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/publishDiagnostics",
                    "params": { "uri": uri, "diagnostics": [] }
                })]
            }
            "workspace/didChangeWatchedFiles" => {
                self.ws.invalidate();
                let open: Vec<_> = self.ws.docs.keys().cloned().collect();
                let mut out = Vec::new();
                for path in open {
                    out.extend(self.publish_diagnostics(&path));
                }
                out
            }
            _ => Vec::new(),
        }
    }

    fn initialize(&mut self, params: &Value) -> Value {
        self.ws.set_roots(params);
        json!({
            "capabilities": {
                "positionEncoding": "utf-16",
                "textDocumentSync": {
                    "openClose": true,
                    "change": 2,
                    "save": { "includeText": true }
                },
                "hoverProvider": true,
                "definitionProvider": true,
                "typeDefinitionProvider": true,
                "referencesProvider": true,
                "documentHighlightProvider": true,
                "renameProvider": { "prepareProvider": true },
                "documentSymbolProvider": true,
                "workspaceSymbolProvider": true,
                "documentFormattingProvider": true,
                "foldingRangeProvider": true,
                "codeLensProvider": { "resolveProvider": false },
                "documentLinkProvider": { "resolveProvider": false },
                "colorProvider": true,
                "selectionRangeProvider": true,
                "callHierarchyProvider": true,
                "inlayHintProvider": true,
                "codeActionProvider": {
                    "codeActionKinds": ["quickfix", "source.organizeImports", "source.fixAll"]
                },
                "completionProvider": {
                    "triggerCharacters": [".", "=", "\"", ":", "/", "("]
                },
                "signatureHelpProvider": {
                    "triggerCharacters": ["(", ",", "="]
                },
                "semanticTokensProvider": {
                    "legend": {
                        "tokenTypes": TOKEN_TYPES,
                        "tokenModifiers": TOKEN_MODIFIERS
                    },
                    "full": true
                },
                "diagnosticProvider": {
                    "interFileDependencies": true,
                    "workspaceDiagnostics": true
                },
                "workspace": {
                    "fileOperations": {
                        "willRename": {
                            "filters": [{ "pattern": { "glob": "**/*.contract" } }]
                        }
                    }
                }
            },
            "serverInfo": {
                "name": "exact-contract-lsp",
                "version": env!("CARGO_PKG_VERSION")
            }
        })
    }

    fn publish_diagnostics(&mut self, path: &std::path::Path) -> Vec<Value> {
        let snap = self.ws.analyze(path);
        let mut out = Vec::new();
        for (file, errs) in &snap.diagnostics {
            let uri = path_to_uri(file);
            let diags = self.ws.lsp_diagnostics_for(file, errs);
            if diags.is_empty() && !self.ws.published_uris.contains(&uri) && file != path {
                continue;
            }
            if diags.is_empty() {
                self.ws.published_uris.remove(&uri);
            } else {
                self.ws.published_uris.insert(uri.clone());
            }
            let version = self.ws.docs.get(file).and_then(|d| d.version);
            let mut params = json!({ "uri": uri, "diagnostics": diags });
            if let Some(v) = version {
                params["version"] = json!(v);
            }
            out.push(json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": params,
            }));
        }
        out
    }

    fn pull_diagnostics(&mut self, params: &Value) -> Value {
        let Some(path) = params
            .pointer("/textDocument/uri")
            .and_then(Value::as_str)
            .and_then(uri_to_path)
        else {
            return json!({ "kind": "full", "items": [] });
        };
        let snap = self.ws.analyze(&path);
        let items = snap
            .diagnostics
            .get(&path)
            .map(|errs| self.ws.lsp_diagnostics_for(&path, errs))
            .unwrap_or_default();
        json!({ "kind": "full", "items": items })
    }
}

/// Read one `Content-Length`-framed JSON-RPC message from `reader`.
fn read_message(reader: &mut impl BufRead) -> Option<Value> {
    let mut content_length: Option<usize> = None;
    let mut header = String::new();
    loop {
        header.clear();
        let n = reader.read_line(&mut header).ok()?;
        if n == 0 {
            return None;
        }
        let trimmed = header.trim();
        if trimmed.is_empty() {
            if content_length.is_some() {
                break;
            }
            continue;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse::<usize>().ok();
            }
        }
    }
    let len = content_length?;
    if len == 0 || len > MAX_CONTENT_LENGTH {
        return None;
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

/// Write one `Content-Length`-framed JSON-RPC message to `writer`.
fn write_message(writer: &mut impl Write, msg: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(msg)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}
