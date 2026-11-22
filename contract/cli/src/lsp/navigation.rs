//! Navigation and inspection providers: hover, definition, type definition,
//! references, document highlight, rename, document/workspace symbols, code lens.

use super::protocol::{
    is_valid_ident, line_text, location_json, normalize_path, parse_position, path_to_uri,
    range_json, span_contains, span_to_range, sym_kind, uri_to_path, utf16_to_byte_col, word_at,
};
use super::workspace::{Snapshot, Workspace};
use crate::symbols::{SymbolDef, SymbolTable};
use crate::vocab::lsp as vocab_lsp;
use contract_syntax::{File, Span};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Extract `(path, line0, utf16_col)` from a standard `TextDocumentPositionParams`.
fn td_pos(params: &Value) -> Option<(PathBuf, u32, u32)> {
    let path = params
        .pointer("/textDocument/uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)?;
    let (line0, char0) = parse_position(params.get("position"))?;
    Some((path, line0, char0))
}

/// Find the symbol definition index at `(path, line1, byte_col1)` in `table`.
fn symbol_at(
    table: &SymbolTable,
    path: &Path,
    line1: u32,
    byte_col1: u32,
) -> Option<(usize, Span)> {
    let target = normalize_path(path);
    // Prefer exact references first (more specific than enclosing declarations).
    for r in &table.references {
        if normalize_path(&r.file) == target && span_contains(r.span, line1, byte_col1) {
            return Some((r.to, r.span));
        }
    }
    for (idx, d) in table.definitions.iter().enumerate() {
        if normalize_path(&d.file) == target && span_contains(d.span, line1, byte_col1) {
            return Some((idx, d.span));
        }
    }
    None
}

/// Format Markdown hover documentation for a resolved authored [`SymbolDef`].
fn format_def_hover(def: &SymbolDef, snap: &Snapshot, table: &SymbolTable) -> String {
    let mut header = String::new();
    match def.kind {
        "component" => {
            header.push_str(&format!("component {}", def.name));
            if let Some(c) = snap
                .program
                .as_ref()
                .or(snap.parsed.as_ref())
                .and_then(|f| f.components.iter().find(|c| c.name == def.name))
            {
                if !c.props.is_empty() {
                    header.push_str("\n  props\n");
                    for p in &c.props {
                        let ty =
                            p.ty.as_ref()
                                .map(|t| format!("{t}"))
                                .unwrap_or_else(|| "?".into());
                        header.push_str(&format!("    {}: {ty}\n", p.name));
                    }
                }
                if c.slot {
                    header.push_str("  slot\n");
                }
            }
        }
        "shape" => {
            header.push_str(&format!("shape {}", def.name));
            if let Some(fields) = table.shapes.map.get(&def.name) {
                for (fname, fty) in fields {
                    header.push_str(&format!("\n  {fname}: {fty}"));
                }
            }
        }
        "fn" => {
            let sig = def.ty.as_deref().unwrap_or("(...)");
            header.push_str(&format!("fn {}{sig}", def.name));
        }
        "action" => {
            let sig = def
                .ty
                .as_deref()
                .and_then(|t| t.strip_prefix("action"))
                .unwrap_or("()");
            header.push_str(&format!("action {}{sig}", def.name));
        }
        "style" | "decoration" | "atom" => {
            header.push_str(&format!("{} {}", def.kind, def.name));
            let list = snap.program.as_ref().or(snap.parsed.as_ref());
            if let Some(s) = list.and_then(|f| match def.kind {
                "style" => f.styles.iter().find(|s| s.name == def.name),
                "decoration" => f.decorations.iter().find(|s| s.name == def.name),
                "atom" => f.atoms.iter().find(|s| s.name == def.name),
                _ => None,
            }) {
                for a in s.attrs.iter().take(12) {
                    header.push_str(&format!("\n  {} = …", a.name));
                }
            }
        }
        "field" => {
            let owner = def.owner.as_deref().unwrap_or("");
            let ty = def.ty.as_deref().unwrap_or("?");
            header.push_str(&format!("(field) {owner}.{}: {ty}", def.name));
        }
        "route" => {
            let pat = snap
                .program
                .as_ref()
                .or(snap.parsed.as_ref())
                .and_then(|f| f.routes.as_ref())
                .and_then(|r| r.rows.iter().find(|row| row.name == def.name))
                .map(|row| format!(" \"{}\"", row.pattern))
                .unwrap_or_default();
            header.push_str(&format!("route {}{pat}", def.name));
        }
        kind => {
            if let Some(ty) = &def.ty {
                header.push_str(&format!("{kind} {}: {ty}", def.name));
            } else {
                header.push_str(&format!("{kind} {}", def.name));
            }
        }
    }
    let mut md = format!("```contract\n{}\n```", header.trim_end());
    let mut meta: Vec<String> = Vec::new();
    if let Some(comp) = &def.component {
        meta.push(format!("in component `{comp}`"));
    }
    if let Some(owner) = &def.owner {
        meta.push(format!("of `{owner}`"));
    }
    if let Some(writes) = &def.writes {
        if writes.is_empty() {
            meta.push("writes: *(none)*".into());
        } else {
            let slots: Vec<String> = writes.iter().map(|s| format!("`{s}`")).collect();
            meta.push(format!("writes: {}", slots.join(", ")));
        }
    }
    if !meta.is_empty() {
        md.push_str("\n\n");
        md.push_str(&meta.join(" · "));
    }
    md
}

/// Hover documentation for Contract keywords when no authored symbol matches.
fn keyword_hover(word: &str) -> Option<&'static str> {
    match word {
        "component" => Some("`component Name` — declares a UI component. The first component in the root file is the app root."),
        "shape" => Some("`shape Name` — declares a closed record type with typed fields."),
        "fn" => Some("`fn name(a: T): R = expr` — declares a pure inline function."),
        "style" => Some("`style Name` — declares a reusable set of literal style attributes applied via `class=Name`."),
        "decoration" => Some("`decoration Name` — declares named paint attributes for `textarea` `decorations` spans."),
        "atom" => Some("`atom Name` — declares an inline Markdown pill atom kind."),
        "keyframes" => Some("`keyframes Name` — declares CSS `@keyframes` animation stops."),
        "timeline" => Some("`timeline Name` — declares a shared clock timeline for `animation-timeline`."),
        "theme" => Some("`theme` — declares design tokens in the root file."),
        "font" => Some("`font \"Family\"` — declares bundled TTF/OTF font faces."),
        "sound" => Some("`sound \"assets/clip.wav\"` — declares an app-wide WAV sound for `playSound`."),
        "routes" => Some("`routes slot` — declares the application route table and binds the router state slot."),
        "use" => Some("`use Name [as Alias] from \"./file.contract\"` — imports declarations from another Contract file, built-in module (`exact:*`), or package."),
        "members" => Some("`members \"./part.contract\"` / `members Name` — splits a component's members across files."),
        "props" => Some("`props` — declares typed inputs passed by parent components."),
        "inject" => Some("`inject` — declares typed values received from an enclosing component's `provide` section."),
        "provide" => Some("`provide` — supplies values to descendant components' `inject` declarations."),
        "slot" => Some("`slot` — allows callers to pass child nodes rendered at `children`."),
        "state" => Some("`state name [: T] = expr [persist]` — declares mutable component state."),
        "derive" => Some("`derive name = expr` — declares a pure derived value recomputed from dependencies."),
        "resource" => Some("`resource name = source(args) as shape T [else empty()]` — declares an asynchronous data query."),
        "mutation" => Some("`mutation name as shape T [queue] [refreshes r] [then act]` — declares an `option<T>` mutation slot filled by `send`."),
        "action" => Some("`action name(params)` — declares a state-updating handler; written slots are inferred."),
        "task" => Some("`task name mount | when cond [key=k]` — declares an `every(...)` or `after(...)` timer."),
        "sequence" => Some("`sequence name mount | when cond` — declares a runner-scheduled audio/MIDI/haptic timeline (`play(...)`)."),
        "view" => Some("`view` — declares the component's rendered node tree."),
        "send" => Some("`send mutation = source(args)` — dispatches a mutation request from an action."),
        "refresh" => Some("`refresh resource` — forces a resource to re-query its data source."),
        "persist" => Some("`persist` — retains a root `state` slot across app launches."),
        "queue" => Some("`queue` — serializes sends to a `mutation` so each waits for the previous reply."),
        "for" => Some("`for item[, index] in list key=expr` — renders a keyed list of nodes."),
        "match" => Some("`match opt` / `match opt { case some(v) => ..., case none => ... }` — exhaustive option pattern match."),
        _ => None,
    }
}

/// `textDocument/hover`
pub fn hover(ws: &mut Workspace, params: &Value) -> Value {
    let Some((path, line0, char0)) = td_pos(params) else {
        return Value::Null;
    };
    let Some(text) = ws.text(&path) else {
        return Value::Null;
    };
    let line = line_text(&text, line0);
    let byte_col1 = utf16_to_byte_col(line, char0);
    let snap = ws.analyze(&path);

    if let Some((md, _)) = super::seam::seam_hit_at(ws, &path, &text, line0, char0) {
        return json!({ "contents": { "kind": "markdown", "value": md } });
    }

    if let Some(table) = &snap.symbols {
        if let Some((idx, span)) = symbol_at(table, &path, line0 + 1, byte_col1) {
            let def = &table.definitions[idx];
            let mut md = format_def_hover(def, &snap, table);
            if def.kind == "source" {
                if let Some(dir) = super::seam::app_dir(ws, &path) {
                    if let Some(hit) = super::seam::find_data_source(&dir, &def.name) {
                        md.push_str(&format!(
                            "\n\n**Implementation** (`{}:{}`):\n```\n{}\n```",
                            hit.file.file_name().unwrap_or_default().to_string_lossy(),
                            hit.line0 + 1,
                            hit.preview
                        ));
                    }
                }
            }
            return json!({
                "contents": { "kind": "markdown", "value": md },
                "range": span_to_range(&text, span),
            });
        }
    }

    let Some((word, start_col, end_col)) = word_at(&text, line0, char0) else {
        return Value::Null;
    };
    let word_span = Span {
        line: line0 + 1,
        col: start_col,
        end_col,
        source_id: 0,
    };

    // Fallback: partial AST declarations when type checking failed on edits.
    if let Some(file) = snap.parsed.as_ref().or(snap.program.as_ref()) {
        if let Some(md) = ast_fallback_hover(file, &word) {
            return json!({
                "contents": { "kind": "markdown", "value": md },
                "range": span_to_range(&text, word_span),
            });
        }
    }

    if let Some(md) = vocab_lsp::hover_markdown(&word) {
        return json!({
            "contents": { "kind": "markdown", "value": md },
            "range": span_to_range(&text, word_span),
        });
    }
    if let Some(kw) = keyword_hover(&word) {
        return json!({
            "contents": { "kind": "markdown", "value": kw },
            "range": span_to_range(&text, word_span),
        });
    }
    Value::Null
}

fn ast_fallback_hover(file: &File, word: &str) -> Option<String> {
    if let Some(s) = file.shapes.iter().find(|s| s.name == word) {
        let mut out = format!("```contract\nshape {}\n", s.name);
        for f in &s.fields {
            out.push_str(&format!("  {}: {}\n", f.name, f.ty));
        }
        out.push_str("```");
        return Some(out);
    }
    if let Some(f) = file.fns.iter().find(|f| f.name == word) {
        let ps: Vec<String> = f
            .params
            .iter()
            .map(|p| {
                format!(
                    "{}: {}",
                    p.name,
                    p.ty.as_ref()
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "?".into())
                )
            })
            .collect();
        return Some(format!(
            "```contract\nfn {}({}): {}\n```",
            f.name,
            ps.join(", "),
            f.ret
        ));
    }
    if let Some(c) = file.components.iter().find(|c| c.name == word) {
        let mut out = format!("```contract\ncomponent {}\n", c.name);
        if !c.props.is_empty() {
            out.push_str("  props\n");
            for p in &c.props {
                let ty =
                    p.ty.as_ref()
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "?".into());
                out.push_str(&format!("    {}: {ty}\n", p.name));
            }
        }
        out.push_str("```");
        return Some(out);
    }
    None
}

/// Resolve a string literal on `line` if `(byte_col1)` is inside `"..."` for
/// `use ... from "..."` or `members "..."` or `font`/`sound`.
fn path_string_at(path: &Path, line: &str, byte_col1: u32) -> Option<PathBuf> {
    let idx = byte_col1.saturating_sub(1) as usize;
    let bytes = line.as_bytes();
    if idx > bytes.len() {
        return None;
    }
    // Find enclosing `"..."`.
    let left = line[..idx.min(line.len())].rfind('"')?;
    let right_rel = line[left + 1..].find('"')?;
    let right = left + 1 + right_rel;
    if idx < left || idx > right {
        return None;
    }
    let spec = &line[left + 1..right];
    if spec.starts_with("./") || spec.starts_with("../") {
        let target = path.parent()?.join(spec);
        if target.is_file() {
            return Some(normalize_path(&target));
        }
    }
    None
}

/// `textDocument/definition`
pub fn definition(ws: &mut Workspace, params: &Value) -> Value {
    let Some((path, line0, char0)) = td_pos(params) else {
        return Value::Null;
    };
    let Some(text) = ws.text(&path) else {
        return Value::Null;
    };
    let line = line_text(&text, line0);
    let byte_col1 = utf16_to_byte_col(line, char0);

    // Jump to target file when clicking a relative path string (`use ... from "./x.contract"`, `members "./x.contract"`).
    if let Some(target) = path_string_at(&path, line, byte_col1) {
        return json!({
            "uri": path_to_uri(&target),
            "range": range_json(0, 0, 0, 0),
        });
    }

    // Cross-seam jump (`t("key")` -> `strings/en.json`, `var(--tok)` -> `app.forma`, `.test.contract` -> `app.contract`).
    if let Some((_, hit)) = super::seam::seam_hit_at(ws, &path, &text, line0, char0) {
        return hit.to_location();
    }

    let snap = ws.analyze(&path);
    if let Some(table) = &snap.symbols {
        if let Some((idx, _)) = symbol_at(table, &path, line0 + 1, byte_col1) {
            let def = &table.definitions[idx];
            if def.kind == "source" {
                if let Some(dir) = super::seam::app_dir(ws, &path) {
                    if let Some(hit) = super::seam::find_data_source(&dir, &def.name) {
                        return hit.to_location();
                    }
                }
            }
            if def.file.is_absolute() {
                let dtext = ws.text(&def.file).unwrap_or_default();
                return location_json(&def.file, &dtext, def.span);
            }
        }
    }

    // Fallback on broken files: find local top-level declaration matching word.
    if let (Some((word, _, _)), Some(file)) = (word_at(&text, line0, char0), snap.parsed.as_ref()) {
        if let Some(span) = find_local_decl_span(file, &word) {
            return location_json(&path, &text, span);
        }
    }
    Value::Null
}

fn find_local_decl_span(file: &File, word: &str) -> Option<Span> {
    let names = &file.names;
    file.components
        .iter()
        .find(|c| c.name == word)
        .map(|c| names.name(c.span))
        .or_else(|| {
            file.shapes
                .iter()
                .find(|s| s.name == word)
                .map(|s| names.name(s.span))
        })
        .or_else(|| {
            file.fns
                .iter()
                .find(|f| f.name == word)
                .map(|f| names.name(f.span))
        })
        .or_else(|| {
            file.styles
                .iter()
                .find(|s| s.name == word)
                .map(|s| names.name(s.span))
        })
        .or_else(|| {
            file.keyframes
                .iter()
                .find(|k| k.name == word)
                .map(|k| names.name(k.span))
        })
}

/// Extract the underlying shape name from a rendered type string like
/// `"Station"`, `"Station[]"`, `"option<Station>"`, or `"option<Station[]>"`.
fn unwrap_shape_name(ty: &str) -> Option<&str> {
    let mut cur = ty.trim();
    loop {
        if let Some(inner) = cur
            .strip_prefix("option<")
            .and_then(|s| s.strip_suffix('>'))
        {
            cur = inner.trim();
        } else if let Some(inner) = cur.strip_suffix("[]") {
            cur = inner.trim();
        } else {
            break;
        }
    }
    if matches!(
        cur,
        "" | "string" | "number" | "bool" | "unit" | "?" | "action"
    ) || cur.starts_with("action(")
        || cur.starts_with("enum<")
    {
        None
    } else {
        Some(cur)
    }
}

/// `textDocument/typeDefinition`
pub fn type_definition(ws: &mut Workspace, params: &Value) -> Value {
    let Some((path, line0, char0)) = td_pos(params) else {
        return Value::Null;
    };
    let Some(text) = ws.text(&path) else {
        return Value::Null;
    };
    let byte_col1 = utf16_to_byte_col(line_text(&text, line0), char0);
    let snap = ws.analyze(&path);
    let Some(table) = &snap.symbols else {
        return Value::Null;
    };
    let Some((idx, _)) = symbol_at(table, &path, line0 + 1, byte_col1) else {
        return Value::Null;
    };
    let def = &table.definitions[idx];
    let shape_name = if def.kind == "shape" {
        Some(def.name.as_str())
    } else {
        def.ty.as_deref().and_then(unwrap_shape_name)
    };
    let Some(shape_name) = shape_name else {
        return Value::Null;
    };
    if let Some(shape_def) = table
        .definitions
        .iter()
        .find(|d| d.kind == "shape" && d.name == shape_name)
    {
        if shape_def.file.is_absolute() {
            let dtext = ws.text(&shape_def.file).unwrap_or_default();
            return location_json(&shape_def.file, &dtext, shape_def.span);
        }
    }
    Value::Null
}

/// `textDocument/references`
pub fn references(ws: &mut Workspace, params: &Value) -> Value {
    let Some((path, line0, char0)) = td_pos(params) else {
        return Value::Array(Vec::new());
    };
    let Some(text) = ws.text(&path) else {
        return Value::Array(Vec::new());
    };
    let byte_col1 = utf16_to_byte_col(line_text(&text, line0), char0);
    let include_decl = params
        .pointer("/context/includeDeclaration")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let snap = ws.analyze(&path);
    let Some(table) = &snap.symbols else {
        return Value::Array(Vec::new());
    };
    let Some((idx, _)) = symbol_at(table, &path, line0 + 1, byte_col1) else {
        return Value::Array(Vec::new());
    };
    let mut locs = Vec::new();
    let def = &table.definitions[idx];
    if include_decl && def.file.is_absolute() {
        let dtext = ws.text(&def.file).unwrap_or_default();
        locs.push(location_json(&def.file, &dtext, def.span));
    }
    for r in table.references.iter().filter(|r| r.to == idx) {
        if r.file.is_absolute() {
            let rtext = ws.text(&r.file).unwrap_or_default();
            locs.push(location_json(&r.file, &rtext, r.span));
        }
    }
    Value::Array(locs)
}

/// `textDocument/documentHighlight`
pub fn document_highlight(ws: &mut Workspace, params: &Value) -> Value {
    let Some((path, line0, char0)) = td_pos(params) else {
        return Value::Array(Vec::new());
    };
    let Some(text) = ws.text(&path) else {
        return Value::Array(Vec::new());
    };
    let byte_col1 = utf16_to_byte_col(line_text(&text, line0), char0);
    let snap = ws.analyze(&path);
    let Some(table) = &snap.symbols else {
        return Value::Array(Vec::new());
    };
    let Some((idx, _)) = symbol_at(table, &path, line0 + 1, byte_col1) else {
        return Value::Array(Vec::new());
    };
    let target = normalize_path(&path);
    let mut out = Vec::new();
    let def = &table.definitions[idx];
    if normalize_path(&def.file) == target {
        out.push(json!({ "range": span_to_range(&text, def.span), "kind": 3 }));
    }
    for r in table.references.iter().filter(|r| r.to == idx) {
        if normalize_path(&r.file) == target {
            out.push(json!({ "range": span_to_range(&text, r.span), "kind": 2 }));
        }
    }
    Value::Array(out)
}

/// `textDocument/prepareRename`
pub fn prepare_rename(ws: &mut Workspace, params: &Value) -> Value {
    let Some((path, line0, char0)) = td_pos(params) else {
        return Value::Null;
    };
    let Some(text) = ws.text(&path) else {
        return Value::Null;
    };
    let byte_col1 = utf16_to_byte_col(line_text(&text, line0), char0);
    let snap = ws.analyze(&path);
    let Some(table) = &snap.symbols else {
        return Value::Null;
    };
    let Some((idx, span)) = symbol_at(table, &path, line0 + 1, byte_col1) else {
        return Value::Null;
    };
    let def = &table.definitions[idx];
    // Do not rename external built-ins, data sources, or string literals (`id`, `font`, `route`).
    if !def.file.is_absolute() || matches!(def.kind, "source" | "font" | "id" | "testId") {
        return Value::Null;
    }
    json!({
        "range": span_to_range(&text, span),
        "placeholder": def.name,
    })
}

/// `textDocument/rename`
pub fn rename(ws: &mut Workspace, params: &Value) -> Result<Value, (i32, String)> {
    let Some((path, line0, char0)) = td_pos(params) else {
        return Ok(Value::Null);
    };
    let Some(new_name) = params.get("newName").and_then(Value::as_str) else {
        return Err((-32602, "missing newName".into()));
    };
    if !is_valid_ident(new_name) {
        return Err((
            -32602,
            format!("`{new_name}` is not a valid Contract identifier"),
        ));
    }
    let Some(text) = ws.text(&path) else {
        return Ok(Value::Null);
    };
    let byte_col1 = utf16_to_byte_col(line_text(&text, line0), char0);
    let snap = ws.analyze(&path);
    let Some(table) = &snap.symbols else {
        return Err((-32603, "fix syntax or type errors before renaming".into()));
    };
    let Some((idx, _)) = symbol_at(table, &path, line0 + 1, byte_col1) else {
        return Ok(Value::Null);
    };
    let def = &table.definitions[idx];
    if !def.file.is_absolute() || matches!(def.kind, "source" | "font" | "id" | "testId") {
        return Err((-32602, format!("cannot rename {} `{}`", def.kind, def.name)));
    }
    let mut changes: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut push_edit = |file: &Path, span: Span| {
        if !file.is_absolute() {
            return;
        }
        let ftext = ws.text(file).unwrap_or_default();
        let uri = path_to_uri(file);
        let range = span_to_range(&ftext, span);
        let list = changes.entry(uri).or_default();
        if !list.iter().any(|e| e["range"] == range) {
            list.push(json!({ "range": range, "newText": new_name }));
        }
    };
    push_edit(&def.file, def.span);
    for r in table.references.iter().filter(|r| r.to == idx) {
        // Alias-aware rename: if a file imported this symbol via `use Old as Alias`,
        // update the imported name in `use`, without clobbering occurrences written as `Alias`.
        let ftext = ws.text(&r.file).unwrap_or_default();
        let rline = line_text(&ftext, r.span.line.saturating_sub(1));
        let start = r.span.col.saturating_sub(1) as usize;
        let end = r.span.end_col.saturating_sub(1) as usize;
        let written = rline.get(start..end).unwrap_or("");
        if written == def.name || written.is_empty() {
            push_edit(&r.file, r.span);
        }
    }
    Ok(json!({ "changes": changes }))
}

/// `textDocument/documentSymbol`
pub fn document_symbol(ws: &mut Workspace, params: &Value) -> Value {
    let Some(path) = params
        .pointer("/textDocument/uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return Value::Array(Vec::new());
    };
    let Some(text) = ws.text(&path) else {
        return Value::Array(Vec::new());
    };
    let snap = ws.analyze(&path);
    let Some(file) = snap.parsed.as_ref() else {
        return Value::Array(Vec::new());
    };
    Value::Array(file_document_symbols(file, &text))
}

/// Build hierarchical LSP `DocumentSymbol` nodes for one parsed Contract file.
pub fn file_document_symbols(file: &File, text: &str) -> Vec<Value> {
    let names = &file.names;
    let mut out = Vec::new();
    let make =
        |name: &str, detail: &str, kind: u32, span: Span, sel: Span, children: Vec<Value>| {
            let mut sym = json!({
                "name": name,
                "detail": detail,
                "kind": kind,
                "range": span_to_range(text, span),
                "selectionRange": span_to_range(text, sel),
            });
            if !children.is_empty() {
                sym["children"] = Value::Array(children);
            }
            sym
        };

    if let Some(routes) = &file.routes {
        let kids: Vec<Value> = routes
            .rows
            .iter()
            .map(|r| {
                make(
                    &r.name,
                    &r.pattern,
                    sym_kind::KEY,
                    r.span,
                    names.name(r.span),
                    Vec::new(),
                )
            })
            .collect();
        out.push(make(
            &routes.slot,
            "routes",
            sym_kind::NAMESPACE,
            routes.span,
            names.name(routes.span),
            kids,
        ));
    }
    for font in &file.fonts {
        out.push(make(
            &font.name,
            "font",
            sym_kind::CONSTANT,
            font.span,
            names.name(font.span),
            Vec::new(),
        ));
    }
    for sound in &file.sounds {
        out.push(make(
            &sound.source,
            "sound",
            sym_kind::CONSTANT,
            sound.span,
            sound.span,
            Vec::new(),
        ));
    }
    for shape in &file.shapes {
        let kids: Vec<Value> = shape
            .fields
            .iter()
            .map(|f| {
                make(
                    &f.name,
                    &f.ty.to_string(),
                    sym_kind::FIELD,
                    f.span,
                    f.span,
                    Vec::new(),
                )
            })
            .collect();
        out.push(make(
            &shape.name,
            "shape",
            sym_kind::STRUCT,
            shape.span,
            names.name(shape.span),
            kids,
        ));
    }
    for f in &file.fns {
        let detail = format!(
            "({}) -> {}",
            f.params
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            f.ret
        );
        out.push(make(
            &f.name,
            &detail,
            sym_kind::FUNCTION,
            f.span,
            names.name(f.span),
            Vec::new(),
        ));
    }
    for s in &file.styles {
        out.push(make(
            &s.name,
            "style",
            sym_kind::INTERFACE,
            s.span,
            names.name(s.span),
            Vec::new(),
        ));
    }
    for d in &file.decorations {
        out.push(make(
            &d.name,
            "decoration",
            sym_kind::INTERFACE,
            d.span,
            names.name(d.span),
            Vec::new(),
        ));
    }
    for a in &file.atoms {
        out.push(make(
            &a.name,
            "atom",
            sym_kind::INTERFACE,
            a.span,
            names.name(a.span),
            Vec::new(),
        ));
    }
    for k in &file.keyframes {
        out.push(make(
            &k.name,
            "keyframes",
            sym_kind::EVENT,
            k.span,
            names.name(k.span),
            Vec::new(),
        ));
    }
    for t in &file.timelines {
        out.push(make(
            &t.name,
            "timeline",
            sym_kind::EVENT,
            t.span,
            names.name(t.span),
            Vec::new(),
        ));
    }
    let components = file.components.iter().chain(file.members.iter());
    for c in components {
        let mut kids = Vec::new();
        for p in &c.props {
            let ty = p.ty.as_ref().map(|t| t.to_string()).unwrap_or_default();
            kids.push(make(
                &p.name,
                &ty,
                sym_kind::PROPERTY,
                p.span,
                p.span,
                Vec::new(),
            ));
        }
        for p in &c.injects {
            let ty = p.ty.as_ref().map(|t| t.to_string()).unwrap_or_default();
            kids.push(make(
                &p.name,
                &ty,
                sym_kind::PROPERTY,
                p.span,
                p.span,
                Vec::new(),
            ));
        }
        for s in &c.states {
            kids.push(make(
                &s.name,
                "state",
                sym_kind::VARIABLE,
                s.span,
                names.name(s.span),
                Vec::new(),
            ));
        }
        for d in &c.derives {
            kids.push(make(
                &d.name,
                "derive",
                sym_kind::CONSTANT,
                d.span,
                names.name(d.span),
                Vec::new(),
            ));
        }
        for r in &c.resources {
            kids.push(make(
                &r.name,
                &format!("resource as {}", r.shape),
                sym_kind::FIELD,
                r.span,
                names.name(r.span),
                Vec::new(),
            ));
        }
        for m in &c.mutations {
            kids.push(make(
                &m.name,
                &format!("mutation as {}", m.shape),
                sym_kind::FIELD,
                m.span,
                names.name(m.span),
                Vec::new(),
            ));
        }
        for a in &c.actions {
            kids.push(make(
                &a.name,
                "action",
                sym_kind::METHOD,
                a.span,
                names.name(a.span),
                Vec::new(),
            ));
        }
        for t in &c.tasks {
            kids.push(make(
                &t.name,
                "task",
                sym_kind::EVENT,
                t.span,
                names.name(t.span),
                Vec::new(),
            ));
        }
        for sq in &c.sequences {
            kids.push(make(
                &sq.name,
                "sequence",
                sym_kind::EVENT,
                sq.span,
                names.name(sq.span),
                Vec::new(),
            ));
        }
        out.push(make(
            &c.name,
            if file.members.is_some() {
                "members"
            } else {
                "component"
            },
            sym_kind::CLASS,
            c.span,
            names.name(c.span),
            kids,
        ));
    }
    for t in &file.tests {
        out.push(make(
            &t.name,
            "test",
            sym_kind::FUNCTION,
            t.span,
            t.span,
            Vec::new(),
        ));
    }
    for p in &file.properties {
        out.push(make(
            &p.name,
            "property",
            sym_kind::EVENT,
            p.span,
            p.span,
            Vec::new(),
        ));
    }
    out
}

/// `workspace/symbol`
pub fn workspace_symbol(ws: &mut Workspace, params: &Value) -> Value {
    let query = params
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let files = ws.contract_files();
    let mut results = Vec::new();
    for path in files {
        let Some(text) = ws.text(&path) else {
            continue;
        };
        let parsed = match contract_syntax::parse_source_partial(&text, 0) {
            Ok(f) => f,
            Err(p) => match p.file {
                Some(f) => *f,
                None => continue,
            },
        };
        let names = &parsed.names;
        let mut push = |name: &str, kind: u32, span: Span, container: Option<&str>| {
            if !query.is_empty() && !name.to_lowercase().contains(&query) {
                return;
            }
            let mut item = json!({
                "name": name,
                "kind": kind,
                "location": location_json(&path, &text, span),
            });
            if let Some(c) = container {
                item["containerName"] = json!(c);
            }
            results.push(item);
        };
        for s in &parsed.shapes {
            push(&s.name, sym_kind::STRUCT, names.name(s.span), None);
        }
        for f in &parsed.fns {
            push(&f.name, sym_kind::FUNCTION, names.name(f.span), None);
        }
        for s in &parsed.styles {
            push(&s.name, sym_kind::INTERFACE, names.name(s.span), None);
        }
        for k in &parsed.keyframes {
            push(&k.name, sym_kind::EVENT, names.name(k.span), None);
        }
        for c in parsed.components.iter().chain(parsed.members.iter()) {
            push(&c.name, sym_kind::CLASS, names.name(c.span), None);
            for a in &c.actions {
                push(&a.name, sym_kind::METHOD, names.name(a.span), Some(&c.name));
            }
            for s in &c.states {
                push(
                    &s.name,
                    sym_kind::VARIABLE,
                    names.name(s.span),
                    Some(&c.name),
                );
            }
            for r in &c.resources {
                push(&r.name, sym_kind::FIELD, names.name(r.span), Some(&c.name));
            }
        }
        if results.len() >= 200 {
            break;
        }
    }
    Value::Array(results)
}

/// `textDocument/codeLens`
pub fn code_lens(ws: &mut Workspace, params: &Value) -> Value {
    let Some(path) = params
        .pointer("/textDocument/uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return Value::Array(Vec::new());
    };
    let Some(text) = ws.text(&path) else {
        return Value::Array(Vec::new());
    };
    let snap = ws.analyze(&path);
    let Some(table) = &snap.symbols else {
        return Value::Array(Vec::new());
    };
    let target = normalize_path(&path);
    let mut lenses = Vec::new();
    for (idx, def) in table.definitions.iter().enumerate() {
        if normalize_path(&def.file) != target {
            continue;
        }
        if matches!(
            def.kind,
            "component" | "shape" | "fn" | "style" | "keyframes" | "action"
        ) {
            let count = table.references.iter().filter(|r| r.to == idx).count();
            let mut label = format!(
                "{count} {}",
                if count == 1 {
                    "reference"
                } else {
                    "references"
                }
            );
            if let Some(writes) = &def.writes {
                if !writes.is_empty() {
                    label.push_str(&format!(" · writes {}", writes.join(", ")));
                }
            }
            lenses.push(json!({
                "range": span_to_range(&text, def.span),
                "command": {
                    "title": label,
                    "command": "",
                },
            }));
        }
    }
    Value::Array(lenses)
}
