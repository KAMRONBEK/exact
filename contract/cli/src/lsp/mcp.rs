//! Native MCP (Model Context Protocol) server over stdio JSON-RPC 2.0
//! (`contract mcp [--stdio]`), sharing the warm LSP [`Workspace`] engine.

use super::agent_tools;
use super::agents;
use super::navigation;
use super::protocol::path_to_uri;
use super::seam;
use super::workspace::Workspace;
use serde_json::{json, Value};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage: contract mcp [--stdio]\n  runs the Contract MCP (Model Context Protocol) server over stdio JSON-RPC 2.0";

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Run `contract mcp [--stdio]`.
pub fn run(args: &[String]) -> ExitCode {
    match args {
        [] => {}
        [flag] if flag == "--stdio" => {}
        [flag] if matches!(flag.as_str(), "--help" | "-h") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    }
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    let mut server = McpServer::new();
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let Ok(msg) = serde_json::from_str::<Value>(trimmed) else {
                    continue;
                };
                if let Some(reply) = server.handle_message(&msg) {
                    if writeln!(writer, "{reply}").is_err() || writer.flush().is_err() {
                        return ExitCode::FAILURE;
                    }
                }
            }
            Err(_) => return ExitCode::FAILURE,
        }
    }
    ExitCode::SUCCESS
}

/// Stateful MCP server over the warm Contract compiler workspace.
#[derive(Debug, Default)]
pub struct McpServer {
    ws: Workspace,
}

impl McpServer {
    /// Create a fresh MCP server.
    pub fn new() -> Self {
        Self::default()
    }

    /// Handle one MCP JSON-RPC 2.0 message, returning `Some(response)` for requests.
    pub fn handle_message(&mut self, msg: &Value) -> Option<Value> {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id")?.clone();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": {
                    "name": "exact-contract-mcp",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => self.call_tool(&params),
            _ => Err((-32601, format!("method not found: {method}"))),
        };

        Some(match result {
            Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
            Err((code, message)) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": code, "message": message },
            }),
        })
    }

    fn call_tool(&mut self, params: &Value) -> Result<Value, (i32, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((-32602, "missing tool name".to_owned()))?;
        let mut args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));

        // Allow agents to pass either `path` or `uri` seamlessly.
        if args.get("uri").is_none() {
            if let Some(p) = args.get("path").and_then(Value::as_str) {
                args["uri"] = json!(path_to_uri(Path::new(p)));
            }
        }

        let out = match name {
            "contract_check" => agents::check(&mut self.ws, &args),
            "contract_diagnostics" => agents::diagnostics(&mut self.ws, &args),
            "contract_outline" => agents::outline(&mut self.ws, &args),
            "contract_context" => agent_tools::context_slice(&mut self.ws, &args),
            "contract_vocab" => agents::vocab(&args),
            "contract_symbols" => agents::symbols(&mut self.ws, &args),
            "contract_impact" => agent_tools::impact(&mut self.ws, &args),
            "contract_seam" => match args
                .get("uri")
                .and_then(Value::as_str)
                .and_then(super::protocol::uri_to_path)
            {
                Some(p) => seam::audit_seam(&mut self.ws, &p),
                None => json!({ "ok": false, "error": "missing path or uri" }),
            },
            "contract_hover" => {
                let line = args.get("line").and_then(Value::as_u64).unwrap_or(1);
                let col = args.get("col").and_then(Value::as_u64).unwrap_or(1);
                navigation::hover(
                    &mut self.ws,
                    &json!({
                        "textDocument": { "uri": args["uri"] },
                        "position": { "line": line.saturating_sub(1), "character": col.saturating_sub(1) },
                    }),
                )
            }
            "contract_fix" => agent_tools::fix(&mut self.ws, &args),
            "contract_refactor" => agent_tools::refactor(&mut self.ws, &args),
            _ => return Err((-32602, format!("unknown tool: {name}"))),
        };

        let is_error = out.get("ok") == Some(&Value::Bool(false)) && out.get("error").is_some();
        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&out).unwrap_or_else(|_| out.to_string()),
            }],
            "structuredContent": out,
            "isError": is_error,
        }))
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "contract_check",
            "description": "Compile-check a Contract app with optional in-memory file overlays (without writing to disk) and return structured diagnostics + machine-applicable quick fixes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute path to a .contract file" },
                    "overlays": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Map of file paths or file:// URIs to proposed source text" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "contract_outline",
            "description": "Return a compact, token-efficient structural overview of a Contract file (imports, routes, shapes, functions, styles, components, states, derives, resources, mutations, actions with inferred writes, tasks).",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }
        },
        {
            "name": "contract_context",
            "description": "Return a minimal, self-contained context slice around a symbol or 1-based line: the declaration source, referenced shapes/styles/functions, and caller locations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "symbol": { "type": "string" },
                    "line": { "type": "integer" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "contract_vocab",
            "description": "Query the Contract compiler vocabulary: pass `name` for one item, `names` for batch lookup, or `query` for ranked search across tags, attributes, CSS properties, events, commands, functions, concepts, recipes, and patterns.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "names": { "type": "array", "items": { "type": "string" } },
                    "query": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "contract_symbols",
            "description": "Return compiler-resolved definitions (with inferred types and action writes) and cross-file references for a Contract app.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "name": { "type": "string", "description": "Optional exact symbol name filter" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "contract_impact",
            "description": "Return the state/action/component dataflow and impact graph: which actions write which slots, send which mutations, refresh which resources, call which actions, and which components use which children.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "symbol": { "type": "string" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "contract_seam",
            "description": "Audit all app seams in one call: .contract <-> app.ts/data/ sources & app.contract.d.ts freshness, strings/en.json missing/unused keys, app.forma tokens, app.json hatches/modules, and .test.contract testIds.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }
        },
        {
            "name": "contract_hover",
            "description": "Get hover type, signature, writes, or compiler vocabulary documentation at a 1-based line and column in a Contract file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "line": { "type": "integer" },
                    "col": { "type": "integer" }
                },
                "required": ["path", "line", "col"]
            }
        },
        {
            "name": "contract_fix",
            "description": "Compute (and optionally apply when `apply: true`) deterministic quick fixes: missing `use` imports, HTML tag/attr renames, retired keywords, tabs -> spaces, and optional canonical formatting.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "apply": { "type": "boolean" },
                    "format": { "type": "boolean" }
                },
                "required": ["path"]
            }
        },
        {
            "name": "contract_refactor",
            "description": "Perform safe compiler-backed refactors: `rename_symbol` or `scaffold_missing_props`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["rename_symbol", "scaffold_missing_props"] },
                    "path": { "type": "string" },
                    "apply": { "type": "boolean" },
                    "symbol": { "type": "string" },
                    "newName": { "type": "string" }
                },
                "required": ["kind", "path"]
            }
        }
    ])
}
