//! Agent-oriented custom LSP extension requests (`exact/*`).
//!
//! Coding agents that connect to `contract lsp` can use both standard LSP 3.17
//! requests and these compact, synchronous `exact/*` queries without spawning
//! a new compiler process per lookup.

use super::protocol::{normalize_path, path_to_uri, uri_to_path};
use super::workspace::Workspace;
use crate::vocab::lsp as vocab_lsp;
use serde_json::{json, Value};
use std::path::PathBuf;

/// `exact/vocab`: look up one entry (`{ "name": "button" }`), a batch
/// (`{ "names": ["list", "reach-margin"] }`), a ranked search
/// (`{ "query": "modal sheet", "limit": 5 }`), or the full vocabulary snapshot.
pub fn vocab(params: &Value) -> Value {
    if let Some(query) = params.get("query").and_then(Value::as_str) {
        let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
        return vocab_lsp::search_json(query, limit);
    }
    if let Some(names) = params.get("names").and_then(Value::as_array) {
        let entries: Vec<Value> = names
            .iter()
            .filter_map(Value::as_str)
            .map(|n| {
                let (entry, known) = vocab_lsp::lookup_json(n);
                json!({ "name": n, "known": known, "entry": entry })
            })
            .collect();
        return json!({ "entries": entries });
    }
    if let Some(name) = params.get("name").and_then(Value::as_str) {
        let (entry, known) = vocab_lsp::lookup_json(name);
        json!({ "known": known, "entry": entry })
    } else {
        vocab_lsp::all_json()
    }
}

/// `exact/symbols`: return resolved definitions (including inferred types and
/// action `writes`) and cross-file references for `{ "uri": "file://...", "name"?: "..." }`.
pub fn symbols(ws: &mut Workspace, params: &Value) -> Value {
    let Some(path) = params
        .get("uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return json!({ "definitions": [], "references": [] });
    };
    let filter = params.get("name").and_then(Value::as_str);
    let snap = ws.analyze(&path);
    let Some(table) = &snap.symbols else {
        return json!({ "definitions": [], "references": [] });
    };
    let mut selected: Vec<Option<usize>> = vec![None; table.definitions.len()];
    let mut defs = Vec::new();
    for (i, d) in table.definitions.iter().enumerate() {
        if filter.is_some_and(|n| d.name != n) {
            continue;
        }
        selected[i] = Some(defs.len());
        defs.push(json!({
            "kind": d.kind,
            "name": d.name,
            "file": d.file.to_string_lossy(),
            "uri": path_to_uri(&d.file),
            "line": d.span.line,
            "col": d.span.col,
            "endCol": d.span.end_col,
            "component": d.component,
            "owner": d.owner,
            "type": d.ty,
            "writes": d.writes,
        }));
    }
    let mut refs = Vec::new();
    for r in &table.references {
        let Some(to) = selected[r.to] else { continue };
        let d = &table.definitions[r.to];
        refs.push(json!({
            "to": to,
            "kind": d.kind,
            "name": d.name,
            "file": r.file.to_string_lossy(),
            "uri": path_to_uri(&r.file),
            "line": r.span.line,
            "col": r.span.col,
            "endCol": r.span.end_col,
        }));
    }
    json!({
        "root": path_to_uri(&snap.root),
        "definitions": defs,
        "references": refs,
    })
}

/// `exact/diagnostics`: synchronous structured errors and warnings for `{ "uri", "allFiles"?: bool }`.
pub fn diagnostics(ws: &mut Workspace, params: &Value) -> Value {
    let Some(path) = params
        .get("uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return json!({ "ok": false, "diagnostics": [] });
    };
    let all_files = params
        .get("allFiles")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let snap = ws.analyze(&path);
    let target = normalize_path(&path);
    let mut items = Vec::new();
    let mut error_count = 0usize;
    let mut warning_count = 0usize;
    for (file, errs) in &snap.diagnostics {
        if !all_files && normalize_path(file) != target {
            continue;
        }
        for e in errs {
            if e.is_warning() {
                warning_count += 1;
            } else {
                error_count += 1;
            }
            items.push(serde_json::from_str::<Value>(&e.to_json()).unwrap_or(Value::Null));
        }
    }
    let fixes = ws
        .text(&path)
        .map(|src| {
            let errs = snap.diagnostics.get(&path).cloned().unwrap_or_default();
            let lsp_diags = ws.lsp_diagnostics_for(&path, &errs);
            super::features::quick_fixes_for_file(ws, &path, &src, &lsp_diags)
        })
        .unwrap_or_default();
    json!({
        "ok": error_count == 0,
        "root": snap.root.to_string_lossy(),
        "files": snap.files.iter().map(|f| f.to_string_lossy()).collect::<Vec<_>>(),
        "errors": error_count,
        "warnings": warning_count,
        "diagnostics": items,
        "fixes": fixes,
    })
}

/// `exact/outline`: token-efficient structural summary of a Contract file for agents.
pub fn outline(ws: &mut Workspace, params: &Value) -> Value {
    let Some(path) = params
        .get("uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return Value::Null;
    };
    let snap = ws.analyze(&path);
    let Some(file) = snap.parsed.as_ref() else {
        return json!({ "uri": path_to_uri(&path), "parsed": false });
    };
    let table = snap.symbols.as_ref();
    let uses: Vec<Value> = file
        .uses
        .iter()
        .map(|u| {
            json!({
                "from": u.path,
                "names": u.names.iter().map(|n| n.local()).collect::<Vec<_>>(),
                "line": u.span.line,
            })
        })
        .collect();
    let shapes: Vec<Value> = file
        .shapes
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "line": s.span.line,
                "fields": s.fields.iter().map(|f| json!({ "name": f.name, "type": f.ty.to_string() })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let fns: Vec<Value> = file
        .fns
        .iter()
        .map(|f| {
            json!({
                "name": f.name,
                "line": f.span.line,
                "params": f.params.iter().map(|p| json!({
                    "name": p.name,
                    "type": p.ty.as_ref().map(|t| t.to_string()),
                })).collect::<Vec<_>>(),
                "returns": f.ret.to_string(),
            })
        })
        .collect();
    let components: Vec<Value> = file
        .components
        .iter()
        .chain(file.members.iter())
        .map(|c| {
            let def_ty = |kind: &str, name: &str| {
                table.and_then(|t| {
                    t.definitions
                        .iter()
                        .find(|d| {
                            d.kind == kind
                                && d.name == name
                                && d.component.as_deref() == Some(c.name.as_str())
                        })
                        .and_then(|d| d.ty.clone())
                })
            };
            let def_writes = |name: &str| {
                table.and_then(|t| {
                    t.definitions
                        .iter()
                        .find(|d| {
                            d.kind == "action"
                                && d.name == name
                                && d.component.as_deref() == Some(c.name.as_str())
                        })
                        .and_then(|d| d.writes.clone())
                })
            };
            json!({
                "name": c.name,
                "line": c.span.line,
                "slot": c.slot,
                "props": c.props.iter().map(|p| json!({ "name": p.name, "type": p.ty.as_ref().map(|t| t.to_string()) })).collect::<Vec<_>>(),
                "injects": c.injects.iter().map(|p| json!({ "name": p.name, "type": p.ty.as_ref().map(|t| t.to_string()) })).collect::<Vec<_>>(),
                "states": c.states.iter().map(|s| json!({ "name": s.name, "type": def_ty("state", &s.name), "persist": c.persists(&s.name), "line": s.span.line })).collect::<Vec<_>>(),
                "derives": c.derives.iter().map(|d| json!({ "name": d.name, "type": def_ty("derive", &d.name), "line": d.span.line })).collect::<Vec<_>>(),
                "resources": c.resources.iter().map(|r| json!({ "name": r.name, "source": r.source, "shape": r.shape.to_string(), "line": r.span.line })).collect::<Vec<_>>(),
                "mutations": c.mutations.iter().map(|m| json!({ "name": m.name, "shape": m.shape.to_string(), "mode": format!("{:?}", m.mode), "line": m.span.line })).collect::<Vec<_>>(),
                "actions": c.actions.iter().map(|a| json!({ "name": a.name, "type": def_ty("action", &a.name), "writes": def_writes(&a.name), "line": a.span.line })).collect::<Vec<_>>(),
                "tasks": c.tasks.iter().map(|t| json!({ "name": t.name, "line": t.span.line })).collect::<Vec<_>>(),
                "members": c.members.lines.iter().map(|m| &m.path).collect::<Vec<_>>(),
            })
        })
        .collect();
    let routes = file.routes.as_ref().map(|r| {
        json!({
            "slot": r.slot,
            "rows": r.rows.iter().map(|row| json!({
                "name": row.name,
                "pattern": row.pattern,
                "tab": row.tab,
                "notfound": row.notfound,
            })).collect::<Vec<_>>(),
        })
    });
    json!({
        "uri": path_to_uri(&path),
        "root": path_to_uri(&snap.root),
        "parsed": true,
        "uses": uses,
        "routes": routes,
        "shapes": shapes,
        "styles": file.styles.iter().map(|s| &s.name).collect::<Vec<_>>(),
        "keyframes": file.keyframes.iter().map(|k| &k.name).collect::<Vec<_>>(),
        "fns": fns,
        "components": components,
        "tests": file.tests.iter().map(|t| &t.name).collect::<Vec<_>>(),
    })
}

/// `exact/check`: compile-check `{ "uri", "overlays"?: { "<uri-or-path>": "<text>" } }`
/// without mutating open editor buffers, returning structured diagnostics.
pub fn check(ws: &mut Workspace, params: &Value) -> Value {
    let Some(path) = params
        .get("uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return json!({ "ok": false, "diagnostics": [] });
    };
    let mut overlays: Vec<(PathBuf, String)> = Vec::new();
    if let Some(map) = params.get("overlays").and_then(Value::as_object) {
        for (k, v) in map {
            let Some(text) = v.as_str() else { continue };
            let p = uri_to_path(k).unwrap_or_else(|| PathBuf::from(k));
            overlays.push((p, text.to_owned()));
        }
    }
    let target_overlay = overlays
        .iter()
        .find(|(p, _)| normalize_path(p) == normalize_path(&path))
        .map(|(_, s)| s.clone());
    let snap = ws.check_with_overlays(&path, overlays);
    let mut items = Vec::new();
    let mut error_count = 0usize;
    let mut warning_count = 0usize;
    for errs in snap.diagnostics.values() {
        for e in errs {
            if e.is_warning() {
                warning_count += 1;
            } else {
                error_count += 1;
            }
            items.push(serde_json::from_str::<Value>(&e.to_json()).unwrap_or(Value::Null));
        }
    }
    let fixes = target_overlay
        .or_else(|| ws.text(&path))
        .map(|src| {
            let errs = snap.diagnostics.get(&path).cloned().unwrap_or_default();
            let lsp_diags = ws.lsp_diagnostics_for(&path, &errs);
            super::features::quick_fixes_for_file(ws, &path, &src, &lsp_diags)
        })
        .unwrap_or_default();
    json!({
        "ok": error_count == 0,
        "root": snap.root.to_string_lossy(),
        "errors": error_count,
        "warnings": warning_count,
        "diagnostics": items,
        "fixes": fixes,
    })
}
