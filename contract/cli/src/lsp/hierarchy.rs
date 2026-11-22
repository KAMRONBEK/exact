//! Rich editor LSP providers: Call Hierarchy, Document Links, Document Colors,
//! Selection Ranges, Workspace Pull Diagnostics, and File Rename edits.

use super::protocol::{
    byte_col_to_utf16, line_text, normalize_path, parse_position, path_to_uri, range_json,
    span_contains, span_to_range, sym_kind, uri_to_path, utf16_to_byte_col,
};
use super::workspace::Workspace;
use contract_syntax::{Node, Stmt};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// `textDocument/documentLink` — clickable links for `use`, `members`, `font`, `sound`, and `image` paths.
pub fn document_links(ws: &mut Workspace, params: &Value) -> Value {
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
    let Some(dir) = path.parent() else {
        return Value::Array(Vec::new());
    };
    let mut links = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        let mut cursor = 0usize;
        while let Some(rel_q) = line[cursor..].find('"') {
            let start = cursor + rel_q + 1;
            let Some(rel_end) = line[start..].find('"') else {
                break;
            };
            let end = start + rel_end;
            let spec = &line[start..end];
            cursor = end + 1;

            let target =
                if spec.starts_with("./") || spec.starts_with("../") || spec.starts_with("assets/")
                {
                    let candidate = dir.join(spec);
                    candidate.is_file().then_some(candidate)
                } else {
                    None
                };
            if let Some(t) = target {
                let sc = byte_col_to_utf16(line, start as u32 + 1);
                let ec = byte_col_to_utf16(line, end as u32 + 1);
                links.push(json!({
                    "range": range_json(idx as u32, sc, idx as u32, ec),
                    "target": path_to_uri(&t),
                    "tooltip": format!("Open {}", t.display()),
                }));
            }
        }
    }
    Value::Array(links)
}

/// Parse `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa` into normalized RGBA `[0.0..=1.0]`.
fn parse_hex_color(hex: &str) -> Option<(f64, f64, f64, f64)> {
    let h = hex.strip_prefix('#')?;
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |s: &str| u8::from_str_radix(s, 16).ok().map(|v| f64::from(v) / 255.0);
    let nib = |s: &str| {
        u8::from_str_radix(s, 16)
            .ok()
            .map(|v| f64::from(v * 17) / 255.0)
    };
    match h.len() {
        3 => Some((nib(&h[0..1])?, nib(&h[1..2])?, nib(&h[2..3])?, 1.0)),
        4 => Some((
            nib(&h[0..1])?,
            nib(&h[1..2])?,
            nib(&h[2..3])?,
            nib(&h[3..4])?,
        )),
        6 => Some((byte(&h[0..2])?, byte(&h[2..4])?, byte(&h[4..6])?, 1.0)),
        8 => Some((
            byte(&h[0..2])?,
            byte(&h[2..4])?,
            byte(&h[4..6])?,
            byte(&h[6..8])?,
        )),
        _ => None,
    }
}

/// `textDocument/documentColor` — inline color swatches for hex literals in Contract files.
pub fn document_colors(ws: &mut Workspace, params: &Value) -> Value {
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
    let mut colors = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let code = match line.find("//") {
            Some(c) if !line[..c].contains('"') => &line[..c],
            _ => line,
        };
        let bytes = code.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'#' {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
                let next_ok = !bytes
                    .get(i)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
                if next_ok {
                    let slice = &code[start..i];
                    if let Some((r, g, b, a)) = parse_hex_color(slice) {
                        let sc = byte_col_to_utf16(line, start as u32 + 1);
                        let ec = byte_col_to_utf16(line, i as u32 + 1);
                        colors.push(json!({
                            "range": range_json(idx as u32, sc, idx as u32, ec),
                            "color": { "red": r, "green": g, "blue": b, "alpha": a },
                        }));
                    }
                }
            } else {
                i += 1;
            }
        }
    }
    Value::Array(colors)
}

/// `textDocument/colorPresentation` — format picked RGBA back into hex color notation.
pub fn color_presentation(params: &Value) -> Value {
    let r = (params
        .pointer("/color/red")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        * 255.0)
        .round() as u8;
    let g = (params
        .pointer("/color/green")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        * 255.0)
        .round() as u8;
    let b = (params
        .pointer("/color/blue")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        * 255.0)
        .round() as u8;
    let a = (params
        .pointer("/color/alpha")
        .and_then(Value::as_f64)
        .unwrap_or(1.0)
        * 255.0)
        .round() as u8;
    let hex = if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    };
    let mut item = json!({ "label": hex });
    if let Some(range) = params.get("range") {
        item["textEdit"] = json!({ "range": range, "newText": hex });
    }
    json!([item])
}

/// `textDocument/selectionRange` — nested selection hierarchy: word -> line -> indent block -> top-level decl -> document.
pub fn selection_ranges(ws: &mut Workspace, params: &Value) -> Value {
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
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let positions = params
        .get("positions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    let mut out = Vec::new();
    for pos in positions {
        let Some((line0, char0)) = parse_position(Some(pos)) else {
            continue;
        };
        let mut chain: Vec<Value> = Vec::new();

        // 1. Word at cursor.
        if let Some((_, sc, ec)) = super::protocol::word_at(&text, line0, char0) {
            let l = line_text(&text, line0);
            chain.push(range_json(
                line0,
                byte_col_to_utf16(l, sc),
                line0,
                byte_col_to_utf16(l, ec),
            ));
        }

        // 2. Trimmed line content.
        let l = line_text(&text, line0);
        let indent = l.len() - l.trim_start().len();
        let line_end_u16 = l.encode_utf16().count() as u32;
        let line_start_u16 = byte_col_to_utf16(l, indent as u32 + 1);
        chain.push(range_json(line0, line_start_u16, line0, line_end_u16));

        // 3. Enclosing indentation blocks going outward to column 0.
        let mut cur_line = line0 as usize;
        let mut cur_indent = indent;
        loop {
            let end_line = block_end(&lines, cur_line, cur_indent);
            let end_u16 = lines
                .get(end_line)
                .map(|s| s.encode_utf16().count() as u32)
                .unwrap_or(0);
            chain.push(range_json(cur_line as u32, 0, end_line as u32, end_u16));
            if cur_indent == 0 {
                break;
            }
            let Some((parent_line, parent_indent)) = enclosing_parent(&lines, cur_line, cur_indent)
            else {
                break;
            };
            cur_line = parent_line;
            cur_indent = parent_indent;
        }

        // 4. Whole document.
        chain.push(super::protocol::full_document_range(&text));

        // Deduplicate consecutive identical ranges and nest innermost -> outermost.
        chain.dedup();
        let mut nested: Option<Value> = None;
        for r in chain.into_iter().rev() {
            nested = Some(match nested {
                Some(parent) => json!({ "range": r, "parent": parent }),
                None => json!({ "range": r }),
            });
        }
        if let Some(n) = nested {
            out.push(n);
        }
    }
    Value::Array(out)
}

fn block_end(lines: &[&str], start: usize, base_indent: usize) -> usize {
    let mut last = start;
    for (i, raw) in lines.iter().enumerate().skip(start + 1) {
        let trimmed = raw.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let ind = raw.len() - trimmed.len();
        if ind <= base_indent {
            break;
        }
        last = i;
    }
    last
}

fn enclosing_parent(lines: &[&str], line: usize, indent: usize) -> Option<(usize, usize)> {
    for i in (0..line).rev() {
        let raw = lines[i];
        let trimmed = raw.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let ind = raw.len() - trimmed.len();
        if ind < indent {
            return Some((i, ind));
        }
    }
    None
}

/// `textDocument/prepareCallHierarchy` — supports `action`, `fn`, and `component`.
pub fn prepare_call_hierarchy(ws: &mut Workspace, params: &Value) -> Value {
    let Some(path) = params
        .pointer("/textDocument/uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return Value::Array(Vec::new());
    };
    let Some((line0, char0)) = parse_position(params.get("position")) else {
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
    let target = normalize_path(&path);
    let def_idx = table
        .references
        .iter()
        .find(|r| normalize_path(&r.file) == target && span_contains(r.span, line0 + 1, byte_col1))
        .map(|r| r.to)
        .or_else(|| {
            table.definitions.iter().position(|d| {
                normalize_path(&d.file) == target && span_contains(d.span, line0 + 1, byte_col1)
            })
        });
    let Some(idx) = def_idx else {
        return Value::Array(Vec::new());
    };
    let def = &table.definitions[idx];
    if !matches!(def.kind, "action" | "component" | "fn") || !def.file.is_absolute() {
        return Value::Array(Vec::new());
    }
    let dtext = ws.text(&def.file).unwrap_or_default();
    let range = span_to_range(&dtext, def.span);
    let kind = match def.kind {
        "component" => sym_kind::CLASS,
        "fn" => sym_kind::FUNCTION,
        _ => sym_kind::METHOD,
    };
    json!([{
        "name": def.name,
        "kind": kind,
        "detail": def.component.clone().unwrap_or_else(|| def.kind.to_owned()),
        "uri": path_to_uri(&def.file),
        "range": range,
        "selectionRange": range,
        "data": { "kind": def.kind, "name": def.name, "component": def.component },
    }])
}

/// `callHierarchy/incomingCalls`
pub fn incoming_calls(ws: &mut Workspace, params: &Value) -> Value {
    let Some(item) = params.get("item") else {
        return Value::Array(Vec::new());
    };
    let Some(path) = item
        .get("uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return Value::Array(Vec::new());
    };
    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
    let kind = item
        .pointer("/data/kind")
        .and_then(Value::as_str)
        .unwrap_or("action");
    let snap = ws.analyze(&path);
    let Some(table) = &snap.symbols else {
        return Value::Array(Vec::new());
    };
    let Some(target_idx) = table
        .definitions
        .iter()
        .position(|d| d.kind == kind && d.name == name)
    else {
        return Value::Array(Vec::new());
    };
    let mut grouped: BTreeMap<usize, Vec<Value>> = BTreeMap::new();
    for r in table.references.iter().filter(|r| r.to == target_idx) {
        if !r.file.is_absolute() {
            continue;
        }
        // Attribute each reference to the innermost enclosing action/component/fn block in that file.
        let rfile = normalize_path(&r.file);
        let rtext = ws.text(&r.file).unwrap_or_default();
        let rlines: Vec<&str> = rtext.lines().collect();
        let ref_line0 = r.span.line.saturating_sub(1) as usize;
        let owner_idx = table
            .definitions
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                if !matches!(d.kind, "action" | "component" | "fn")
                    || normalize_path(&d.file) != rfile
                    || d.span.line > r.span.line
                {
                    return false;
                }
                let start0 = d.span.line.saturating_sub(1) as usize;
                let base_indent = rlines
                    .get(start0)
                    .map(|l| l.len() - l.trim_start().len())
                    .unwrap_or(0);
                let end0 = block_end(&rlines, start0, base_indent);
                ref_line0 <= end0
            })
            .max_by_key(|(_, d)| d.span.line)
            .map(|(i, _)| i);
        if let Some(oidx) = owner_idx {
            grouped
                .entry(oidx)
                .or_default()
                .push(span_to_range(&rtext, r.span));
        }
    }
    let calls: Vec<Value> = grouped
        .into_iter()
        .map(|(oidx, from_ranges)| {
            let d = &table.definitions[oidx];
            let dtext = ws.text(&d.file).unwrap_or_default();
            let range = span_to_range(&dtext, d.span);
            let k = match d.kind {
                "component" => sym_kind::CLASS,
                "fn" => sym_kind::FUNCTION,
                _ => sym_kind::METHOD,
            };
            json!({
                "from": {
                    "name": d.name,
                    "kind": k,
                    "detail": d.component.clone().unwrap_or_else(|| d.kind.to_owned()),
                    "uri": path_to_uri(&d.file),
                    "range": range,
                    "selectionRange": range,
                    "data": { "kind": d.kind, "name": d.name, "component": d.component },
                },
                "fromRanges": from_ranges,
            })
        })
        .collect();
    Value::Array(calls)
}

/// `callHierarchy/outgoingCalls`
pub fn outgoing_calls(ws: &mut Workspace, params: &Value) -> Value {
    let Some(item) = params.get("item") else {
        return Value::Array(Vec::new());
    };
    let Some(path) = item
        .get("uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return Value::Array(Vec::new());
    };
    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
    let kind = item
        .pointer("/data/kind")
        .and_then(Value::as_str)
        .unwrap_or("action");
    let snap = ws.analyze(&path);
    let (Some(table), Some(prog)) = (
        &snap.symbols,
        snap.program.as_ref().or(snap.parsed.as_ref()),
    ) else {
        return Value::Array(Vec::new());
    };

    let mut targets: Vec<(&str, String)> = Vec::new();
    match kind {
        "component" => {
            if let Some(c) = prog.components.iter().find(|c| c.name == name) {
                collect_used_components(&c.view, &mut targets);
            }
        }
        "action" => {
            let comp = item.pointer("/data/component").and_then(Value::as_str);
            if let Some(c) = prog.components.iter().find(|c| {
                comp.is_none_or(|cn| c.name == cn) && c.actions.iter().any(|a| a.name == name)
            }) {
                if let Some(a) = c.actions.iter().find(|a| a.name == name) {
                    fn walk_stmts(stmts: &[Stmt], out: &mut Vec<(&'static str, String)>) {
                        for s in stmts {
                            match s {
                                Stmt::Command { name, .. }
                                    if !contract_syntax::HOST_COMMANDS.contains(&name.as_str()) =>
                                {
                                    out.push(("action", name.clone()));
                                }
                                Stmt::Call { action, .. } => out.push(("action", action.clone())),
                                Stmt::If {
                                    then, otherwise, ..
                                } => {
                                    walk_stmts(then, out);
                                    walk_stmts(otherwise, out);
                                }
                                Stmt::Match { some, none, .. } => {
                                    walk_stmts(&some.1, out);
                                    walk_stmts(none, out);
                                }
                                _ => {}
                            }
                        }
                    }
                    walk_stmts(&a.body, &mut targets);
                }
            }
        }
        _ => {}
    }

    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (tk, tn) in targets {
        if !seen.insert((tk, tn.clone())) {
            continue;
        }
        if let Some(def) = table
            .definitions
            .iter()
            .find(|d| d.kind == tk && d.name == tn && d.file.is_absolute())
        {
            let dtext = ws.text(&def.file).unwrap_or_default();
            let range = span_to_range(&dtext, def.span);
            let k = if tk == "component" {
                sym_kind::CLASS
            } else {
                sym_kind::METHOD
            };
            out.push(json!({
                "to": {
                    "name": def.name,
                    "kind": k,
                    "detail": def.component.clone().unwrap_or_else(|| def.kind.to_owned()),
                    "uri": path_to_uri(&def.file),
                    "range": range,
                    "selectionRange": range,
                    "data": { "kind": def.kind, "name": def.name, "component": def.component },
                },
                "fromRanges": [],
            }));
        }
    }
    Value::Array(out)
}

fn collect_used_components(roots: &[Node], out: &mut Vec<(&'static str, String)>) {
    out.extend(Node::walk(roots).filter_map(|n| match n {
        Node::Use { name, .. } => Some(("component", name.clone())),
        _ => None,
    }));
}

/// `workspace/diagnostic` — pull diagnostics across all `.contract` roots in the workspace.
pub fn workspace_diagnostics(ws: &mut Workspace) -> Value {
    let files = ws.contract_files();
    let mut roots = std::collections::BTreeSet::new();
    for f in files {
        roots.insert(ws.find_root(&f));
    }
    let mut seen_uris = std::collections::BTreeSet::new();
    let mut reports = Vec::new();
    for root in roots {
        let snap = ws.analyze(&root);
        for (file, errs) in &snap.diagnostics {
            let uri = path_to_uri(file);
            if !seen_uris.insert(uri.clone()) {
                continue;
            }
            let items = ws.lsp_diagnostics_for(file, errs);
            reports.push(json!({
                "kind": "full",
                "uri": uri,
                "version": ws.docs.get(file).and_then(|d| d.version),
                "items": items,
            }));
        }
    }
    json!({ "items": reports })
}

/// `workspace/willRenameFiles` — update relative `use … from "…"` and `members "…"` specifiers.
pub fn will_rename_files(ws: &mut Workspace, params: &Value) -> Value {
    let Some(files) = params.get("files").and_then(Value::as_array) else {
        return Value::Null;
    };
    let mut renames: Vec<(PathBuf, PathBuf)> = Vec::new();
    for f in files {
        let (Some(old), Some(new)) = (
            f.get("oldUri")
                .and_then(Value::as_str)
                .and_then(uri_to_path),
            f.get("newUri")
                .and_then(Value::as_str)
                .and_then(uri_to_path),
        ) else {
            continue;
        };
        if old.extension().is_some_and(|e| e == "contract") {
            renames.push((old, new));
        }
    }
    if renames.is_empty() {
        return Value::Null;
    }

    let mut changes: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for candidate in ws.contract_files() {
        let Some(text) = ws.text(&candidate) else {
            continue;
        };
        let Some(cdir) = candidate.parent() else {
            continue;
        };
        let uri = path_to_uri(&candidate);
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if !(trimmed.starts_with("use ") || trimmed.starts_with("members ")) {
                continue;
            }
            let Some(q1) = line.find('"') else { continue };
            let Some(rel_q2) = line[q1 + 1..].find('"') else {
                continue;
            };
            let q2 = q1 + 1 + rel_q2;
            let spec = &line[q1 + 1..q2];
            if !(spec.starts_with("./") || spec.starts_with("../")) {
                continue;
            }
            let resolved = normalize_path(&cdir.join(spec));
            if let Some((_, new_target)) = renames.iter().find(|(old, _)| *old == resolved) {
                let rel = relative_spec(cdir, new_target);
                let sc = byte_col_to_utf16(line, q1 as u32 + 2);
                let ec = byte_col_to_utf16(line, q2 as u32 + 1);
                changes.entry(uri.clone()).or_default().push(json!({
                    "range": range_json(idx as u32, sc, idx as u32, ec),
                    "newText": rel,
                }));
            }
        }
    }
    if changes.is_empty() {
        Value::Null
    } else {
        json!({ "changes": changes })
    }
}

fn relative_spec(from_dir: &Path, to_file: &Path) -> String {
    let from_parts: Vec<_> = from_dir.components().collect();
    let to_parts: Vec<_> = to_file.components().collect();
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut rel = String::new();
    let ups = from_parts.len().saturating_sub(common);
    if ups == 0 {
        rel.push_str("./");
    } else {
        for _ in 0..ups {
            rel.push_str("../");
        }
    }
    for (i, c) in to_parts[common..].iter().enumerate() {
        if i > 0 {
            rel.push('/');
        }
        if let Component::Normal(s) = c {
            rel.push_str(&s.to_string_lossy());
        }
    }
    rel
}
