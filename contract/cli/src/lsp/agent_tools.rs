//! Agent-oriented compiler intelligence: state/action impact graphs (`exact/impact`),
//! token-budgeted context slicing (`exact/context`), deterministic one-shot fixes
//! (`exact/fix`), and structural refactors (`exact/refactor`).

use super::features::quick_fixes_for_file;
use super::navigation;
use super::protocol::{apply_content_changes, normalize_path, path_to_uri, uri_to_path};
use super::workspace::Workspace;
use contract_syntax::{Node, Stmt};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// `exact/impact`: state/action/resource/mutation/shape/component dataflow and impact graph.
pub fn impact(ws: &mut Workspace, params: &Value) -> Value {
    let Some(path) = params
        .get("uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return json!({ "ok": false, "error": "missing uri" });
    };
    let symbol_filter = params.get("symbol").and_then(Value::as_str);
    let snap = ws.analyze(&path);
    let (Some(table), Some(prog)) = (
        &snap.symbols,
        snap.program.as_ref().or(snap.parsed.as_ref()),
    ) else {
        return json!({ "ok": false, "error": "program did not type-check; fix errors first" });
    };

    // Map each state/mutation/resource slot -> actions that write it and sites that reference it.
    let mut slot_writers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut action_graph = Vec::new();
    for c in &prog.components {
        for a in &c.actions {
            let writes = table
                .definitions
                .iter()
                .find(|d| {
                    d.kind == "action"
                        && d.name == a.name
                        && d.component.as_deref() == Some(c.name.as_str())
                })
                .and_then(|d| d.writes.clone())
                .unwrap_or_default();
            for w in &writes {
                slot_writers
                    .entry(format!("{}.{w}", c.name))
                    .or_default()
                    .push(format!("{}.{}", c.name, a.name));
            }
            let (mut calls, mut sends, mut refreshes, mut commands) = (
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
            );
            collect_stmt_edges(
                &a.body,
                &mut calls,
                &mut sends,
                &mut refreshes,
                &mut commands,
            );
            if symbol_filter.is_none_or(|f| {
                a.name == f
                    || writes.iter().any(|w| w == f)
                    || sends.contains(f)
                    || refreshes.contains(f)
                    || calls.contains(f)
            }) {
                action_graph.push(json!({
                    "component": c.name,
                    "action": a.name,
                    "writes": writes,
                    "calls": calls.into_iter().collect::<Vec<_>>(),
                    "sends": sends.into_iter().collect::<Vec<_>>(),
                    "refreshes": refreshes.into_iter().collect::<Vec<_>>(),
                    "commands": commands.into_iter().collect::<Vec<_>>(),
                }));
            }
        }
    }

    // Component parent -> child edges.
    let mut component_edges = Vec::new();
    for c in &prog.components {
        let mut children = BTreeSet::new();
        collect_child_components(&c.view, &mut children);
        if symbol_filter.is_none_or(|f| c.name == f || children.contains(f)) {
            component_edges.push(json!({
                "component": c.name,
                "uses": children.into_iter().collect::<Vec<_>>(),
            }));
        }
    }

    // Targeted symbol summary when `symbol` is specified.
    let target = symbol_filter.map(|name| {
        let defs: Vec<Value> = table
            .definitions
            .iter()
            .enumerate()
            .filter(|(_, d)| d.name == name)
            .map(|(idx, d)| {
                let refs: Vec<Value> = table
                    .references
                    .iter()
                    .filter(|r| r.to == idx)
                    .map(|r| {
                        json!({
                            "file": r.file.to_string_lossy(),
                            "uri": path_to_uri(&r.file),
                            "line": r.span.line,
                            "col": r.span.col,
                        })
                    })
                    .collect();
                let key = d
                    .component
                    .as_deref()
                    .map(|c| format!("{c}.{name}"))
                    .unwrap_or_default();
                json!({
                    "kind": d.kind,
                    "name": d.name,
                    "component": d.component,
                    "type": d.ty,
                    "file": d.file.to_string_lossy(),
                    "line": d.span.line,
                    "writers": slot_writers.get(&key).cloned().unwrap_or_default(),
                    "referenceCount": refs.len(),
                    "references": refs,
                })
            })
            .collect();
        Value::Array(defs)
    });

    json!({
        "ok": true,
        "root": path_to_uri(&snap.root),
        "symbol": symbol_filter,
        "target": target,
        "slotWriters": slot_writers,
        "actions": action_graph,
        "components": component_edges,
    })
}

fn collect_stmt_edges(
    stmts: &[Stmt],
    calls: &mut BTreeSet<String>,
    sends: &mut BTreeSet<String>,
    refreshes: &mut BTreeSet<String>,
    commands: &mut BTreeSet<String>,
) {
    for s in stmts {
        match s {
            Stmt::Command { name, .. } => {
                if contract_syntax::HOST_COMMANDS.contains(&name.as_str()) {
                    commands.insert(name.clone());
                } else {
                    calls.insert(name.clone());
                }
            }
            Stmt::Call { action, .. } => {
                calls.insert(action.clone());
            }
            Stmt::Send { target, .. } => {
                sends.insert(target.clone());
            }
            Stmt::Refresh { target, .. } => {
                refreshes.insert(target.clone());
            }
            Stmt::If {
                then, otherwise, ..
            } => {
                collect_stmt_edges(then, calls, sends, refreshes, commands);
                collect_stmt_edges(otherwise, calls, sends, refreshes, commands);
            }
            Stmt::Match { some, none, .. } => {
                collect_stmt_edges(&some.1, calls, sends, refreshes, commands);
                collect_stmt_edges(none, calls, sends, refreshes, commands);
            }
            Stmt::Let { .. } | Stmt::Assign { .. } => {}
        }
    }
}

fn collect_child_components(roots: &[Node], out: &mut BTreeSet<String>) {
    for n in Node::walk(roots) {
        if let Node::Use { name, .. } = n {
            out.insert(name.clone());
        }
    }
}

/// Extract the indented block starting at 1-based `start_line` from `text`.
fn extract_block(text: &str, start_line1: u32) -> (u32, u32, String) {
    let lines: Vec<&str> = text.lines().collect();
    let start0 = start_line1.saturating_sub(1) as usize;
    let Some(head) = lines.get(start0) else {
        return (start_line1, start_line1, String::new());
    };
    let base_indent = head.len() - head.trim_start().len();
    let mut end0 = start0;
    for (i, raw) in lines.iter().enumerate().skip(start0 + 1) {
        let trimmed = raw.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let ind = raw.len() - trimmed.len();
        if ind <= base_indent {
            break;
        }
        end0 = i;
    }
    let slice = lines[start0..=end0].join("\n");
    (start_line1, end0 as u32 + 1, slice)
}

/// `exact/context`: token-budgeted context slice around `{ uri, symbol? | line? }`.
pub fn context_slice(ws: &mut Workspace, params: &Value) -> Value {
    let Some(path) = params
        .get("uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return json!({ "ok": false, "error": "missing uri" });
    };
    let Some(text) = ws.text(&path) else {
        return json!({ "ok": false, "error": "cannot read file" });
    };
    let snap = ws.analyze(&path);
    let symbol = params.get("symbol").and_then(Value::as_str);
    let line_param = params.get("line").and_then(Value::as_u64).map(|l| l as u32);

    let (target_file, target_line1, target_kind, target_name) = if let Some(sym) = symbol {
        if let Some(table) = &snap.symbols {
            if let Some(def) = table
                .definitions
                .iter()
                .find(|d| d.name == sym && d.file.is_absolute())
            {
                (def.file.clone(), def.span.line, def.kind, def.name.clone())
            } else {
                return json!({ "ok": false, "error": format!("symbol `{sym}` not found") });
            }
        } else {
            return json!({ "ok": false, "error": "symbols unavailable; check diagnostics first" });
        }
    } else if let Some(l1) = line_param {
        // Find innermost declaration starting at or before `l1`.
        let lines: Vec<&str> = text.lines().collect();
        let mut decl_line = l1.max(1);
        for i in (0..=(l1.saturating_sub(1) as usize).min(lines.len().saturating_sub(1))).rev() {
            if !lines[i].starts_with(' ')
                && !lines[i].trim().is_empty()
                && !lines[i].starts_with("//")
            {
                decl_line = i as u32 + 1;
                break;
            }
        }
        (
            path.clone(),
            decl_line,
            "declaration",
            format!("line {decl_line}"),
        )
    } else {
        return json!({ "ok": false, "error": "provide `symbol` or `line`" });
    };

    let ttext = ws.text(&target_file).unwrap_or_default();
    let (start_line, end_line, snippet) = extract_block(&ttext, target_line1);

    // Collect referenced declarations (shapes, styles, fns, components) within this block.
    let mut deps = Vec::new();
    let mut callers = Vec::new();
    if let Some(table) = &snap.symbols {
        let tnorm = normalize_path(&target_file);
        let mut seen = BTreeSet::new();
        for r in &table.references {
            if normalize_path(&r.file) == tnorm
                && r.span.line >= start_line
                && r.span.line <= end_line
            {
                let def = &table.definitions[r.to];
                if matches!(def.kind, "shape" | "fn" | "style" | "component")
                    && def.file.is_absolute()
                    && seen.insert(r.to)
                {
                    let dtext = ws.text(&def.file).unwrap_or_default();
                    let (ds, de, dcode) = extract_block(&dtext, def.span.line);
                    deps.push(json!({
                        "kind": def.kind,
                        "name": def.name,
                        "file": def.file.to_string_lossy(),
                        "startLine": ds,
                        "endLine": de,
                        "source": dcode,
                    }));
                }
            }
        }
        if let Some(tidx) = table
            .definitions
            .iter()
            .position(|d| d.name == target_name && normalize_path(&d.file) == tnorm)
        {
            for r in table.references.iter().filter(|r| r.to == tidx) {
                callers.push(json!({
                    "file": r.file.to_string_lossy(),
                    "line": r.span.line,
                    "col": r.span.col,
                }));
            }
        }
    }

    json!({
        "ok": true,
        "target": {
            "kind": target_kind,
            "name": target_name,
            "file": target_file.to_string_lossy(),
            "uri": path_to_uri(&target_file),
            "startLine": start_line,
            "endLine": end_line,
            "source": snippet,
        },
        "dependencies": deps,
        "callers": callers,
    })
}

/// `exact/fix`: compute (and optionally apply when `apply: true`) all deterministic quick fixes.
pub fn fix(ws: &mut Workspace, params: &Value) -> Value {
    let Some(uri) = params.get("uri").and_then(Value::as_str) else {
        return json!({ "ok": false, "error": "missing uri" });
    };
    let Some(path) = uri_to_path(uri) else {
        return json!({ "ok": false, "error": "invalid uri" });
    };
    let apply = params
        .get("apply")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let format = params
        .get("format")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let Some(before) = ws.text(&path) else {
        return json!({ "ok": false, "error": "cannot read file" });
    };

    let snap = ws.analyze(&path);
    let errs = snap.diagnostics.get(&path).cloned().unwrap_or_default();
    let lsp_diags = ws.lsp_diagnostics_for(&path, &errs);
    let actions = quick_fixes_for_file(ws, &path, &before, &lsp_diags);

    let mut applied_titles = Vec::new();
    let mut after = before.clone();

    // Apply preferred quick fixes in order (starting with missing `use` imports if present).
    for act in &actions {
        if act.get("kind").and_then(Value::as_str) == Some("source.organizeImports") {
            continue;
        }
        if !act
            .get("isPreferred")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        if let Some(edits) = act
            .pointer(&format!("/edit/changes/{}", escape_ptr(uri)))
            .and_then(Value::as_array)
        {
            let changes: Vec<Value> = edits
                .iter()
                .map(|e| json!({ "range": e["range"], "text": e["newText"] }))
                .collect();
            apply_content_changes(&mut after, &changes);
            if let Some(t) = act.get("title").and_then(Value::as_str) {
                applied_titles.push(t.to_owned());
            }
            // Only apply one full-document rewrite per pass to keep ranges valid.
            if applied_titles.len() == 1 && after != before {
                break;
            }
        }
    }
    if format {
        if let Ok(formatted) = contract_syntax::fmt::format(&after) {
            if formatted != after {
                after = formatted;
                applied_titles.push("Format Contract source".into());
            }
        }
    }

    let changed = after != before;
    if apply && changed {
        if ws.docs.contains_key(&path) {
            let v = ws.docs.get(&path).and_then(|d| d.version);
            ws.open(path.clone(), after.clone(), v);
        }
        if let Err(e) = std::fs::write(&path, &after) {
            return json!({ "ok": false, "error": format!("write failed: {e}") });
        }
        ws.invalidate_path(&path);
    }

    json!({
        "ok": true,
        "uri": uri,
        "changed": changed,
        "applied": apply && changed,
        "fixes": applied_titles,
        "availableActions": actions,
        "text": after,
    })
}

fn escape_ptr(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

/// `exact/refactor`: compiler-backed structural refactors (`rename_symbol`,
/// `scaffold_missing_props`).
pub fn refactor(ws: &mut Workspace, params: &Value) -> Value {
    let kind = params.get("kind").and_then(Value::as_str).unwrap_or("");
    let apply = params
        .get("apply")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match kind {
        "rename_symbol" => refactor_rename(ws, params, apply),
        "scaffold_missing_props" => refactor_scaffold_props(ws, params, apply),
        _ => json!({
            "ok": false,
            "error": format!("unknown refactor kind `{kind}`; supported: rename_symbol, scaffold_missing_props")
        }),
    }
}

fn refactor_rename(ws: &mut Workspace, params: &Value, apply: bool) -> Value {
    let Some(uri) = params.get("uri").and_then(Value::as_str) else {
        return json!({ "ok": false, "error": "missing uri" });
    };
    let Some(new_name) = params.get("newName").and_then(Value::as_str) else {
        return json!({ "ok": false, "error": "missing newName" });
    };
    // Allow addressing by either `position` or `symbol` name.
    let position = if let Some(pos) = params.get("position") {
        pos.clone()
    } else if let Some(sym) = params.get("symbol").and_then(Value::as_str) {
        let Some(path) = uri_to_path(uri) else {
            return json!({ "ok": false, "error": "invalid uri" });
        };
        let snap = ws.analyze(&path);
        let Some(def) = snap
            .symbols
            .as_ref()
            .and_then(|t| t.definitions.iter().find(|d| d.name == sym))
        else {
            return json!({ "ok": false, "error": format!("symbol `{sym}` not found") });
        };
        json!({
            "line": def.span.line.saturating_sub(1),
            "character": def.span.col.saturating_sub(1),
        })
    } else {
        return json!({ "ok": false, "error": "provide `position` or `symbol`" });
    };

    let req_params = json!({
        "textDocument": { "uri": uri },
        "position": position,
        "newName": new_name,
    });
    match navigation::rename(ws, &req_params) {
        Ok(edit) => {
            if apply {
                apply_workspace_edit(ws, &edit);
            }
            json!({ "ok": true, "applied": apply, "edit": edit })
        }
        Err((_, msg)) => json!({ "ok": false, "error": msg }),
    }
}

fn refactor_scaffold_props(ws: &mut Workspace, params: &Value, apply: bool) -> Value {
    let Some(uri) = params.get("uri").and_then(Value::as_str) else {
        return json!({ "ok": false, "error": "missing uri" });
    };
    let Some(path) = uri_to_path(uri) else {
        return json!({ "ok": false, "error": "invalid uri" });
    };
    let Some(before) = ws.text(&path) else {
        return json!({ "ok": false, "error": "cannot read file" });
    };
    let snap = ws.analyze(&path);
    let errs = snap.diagnostics.get(&path).cloned().unwrap_or_default();
    let lsp_diags = ws.lsp_diagnostics_for(&path, &errs);
    let actions = quick_fixes_for_file(ws, &path, &before, &lsp_diags);
    let prop_actions: Vec<Value> = actions
        .into_iter()
        .filter(|a| {
            a.get("title")
                .and_then(Value::as_str)
                .is_some_and(|t| t.starts_with("Add missing prop"))
        })
        .collect();
    if apply {
        for act in &prop_actions {
            if let Some(edit) = act.get("edit") {
                apply_workspace_edit(ws, edit);
            }
        }
    }
    json!({
        "ok": true,
        "applied": apply,
        "count": prop_actions.len(),
        "actions": prop_actions,
    })
}

fn apply_workspace_edit(ws: &mut Workspace, edit: &Value) {
    let Some(changes) = edit.get("changes").and_then(Value::as_object) else {
        return;
    };
    for (uri, edits) in changes {
        let Some(path) = uri_to_path(uri) else {
            continue;
        };
        let Some(mut text) = ws.text(&path) else {
            continue;
        };
        let Some(arr) = edits.as_array() else {
            continue;
        };
        // Apply edits in reverse document order so earlier ranges stay valid.
        let mut sorted: Vec<&Value> = arr.iter().collect();
        sorted.sort_by(|a, b| {
            let al = a
                .pointer("/range/start/line")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let ac = a
                .pointer("/range/start/character")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let bl = b
                .pointer("/range/start/line")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let bc = b
                .pointer("/range/start/character")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            (bl, bc).cmp(&(al, ac))
        });
        let converted: Vec<Value> = sorted
            .into_iter()
            .map(|e| json!({ "range": e["range"], "text": e["newText"] }))
            .collect();
        apply_content_changes(&mut text, &converted);
        if ws.docs.contains_key(&path) {
            let v = ws.docs.get(&path).and_then(|d| d.version);
            ws.open(path.clone(), text.clone(), v);
        }
        let _ = std::fs::write(&path, &text);
        ws.invalidate_path(&path);
    }
}
