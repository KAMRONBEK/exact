//! Semantic tokens, inlay hints, code actions (auto-imports & quick fixes),
//! canonical formatting, and folding ranges.

use super::protocol::{
    byte_col_to_utf16, full_document_range, line_text, mods, normalize_path, range_json, tok,
    uri_to_path,
};
use super::workspace::Workspace;
use crate::sources::{self, with_contract_texts};
use contract_lower::vocab as lower_vocab;
use contract_syntax::{Lexer, Span, TokenKind, HOST_COMMANDS};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;

/// `textDocument/semanticTokens/full`
pub fn semantic_tokens(ws: &mut Workspace, params: &Value) -> Value {
    let Some(path) = params
        .pointer("/textDocument/uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return json!({ "data": [] });
    };
    let data = compute_semantic_tokens_data(ws, &path);
    json!({ "data": data })
}

fn compute_semantic_tokens_data(ws: &mut Workspace, path: &Path) -> Vec<u32> {
    let Some(text) = ws.text(path) else {
        return Vec::new();
    };
    let snap = ws.analyze(path);
    let target = normalize_path(path);

    // Map `(line, col)` to resolved symbol `(kind, is_decl)` from the compiler's graph.
    let mut resolved: HashMap<(u32, u32), (&'static str, bool)> = HashMap::new();
    if let Some(table) = &snap.symbols {
        for def in &table.definitions {
            if normalize_path(&def.file) == target {
                resolved.insert((def.span.line, def.span.col), (def.kind, true));
            }
        }
        for r in &table.references {
            if normalize_path(&r.file) == target {
                let kind = table.definitions[r.to].kind;
                resolved
                    .entry((r.span.line, r.span.col))
                    .or_insert((kind, false));
            }
        }
    }

    let mut raw_tokens: Vec<(u32, u32, u32, u32, u32)> = Vec::new();
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();

    // 1. Collect lexer tokens via recovering tokenization so partial edits still highlight.
    let tokens = Lexer::tokenize(&text, 1).unwrap_or_default();
    for (i, t) in tokens.iter().enumerate() {
        if t.span.line == 0 || t.span.end_col <= t.span.col {
            continue;
        }
        let line0 = t.span.line - 1;
        let line = lines.get(line0 as usize).copied().unwrap_or("");
        let start = byte_col_to_utf16(line, t.span.col);
        let end = byte_col_to_utf16(line, t.span.end_col);
        let len = end.saturating_sub(start);
        if len == 0 {
            continue;
        }
        let next_kind = tokens.get(i + 1).map(|n| &n.kind);
        let prev_kind = i
            .checked_sub(1)
            .and_then(|p| tokens.get(p))
            .map(|p| &p.kind);

        let classified = match &t.kind {
            TokenKind::Number(_) => Some((tok::NUMBER, 0)),
            TokenKind::Str(_) | TokenKind::Template(_) => Some((tok::STRING, 0)),
            TokenKind::Ident(w) => {
                if let Some(&(kind, is_decl)) = resolved.get(&(t.span.line, t.span.col)) {
                    Some(symbol_token(kind, is_decl))
                } else {
                    classify_ident(w, t.span, prev_kind, next_kind)
                }
            }
            TokenKind::Punct(p) => {
                if matches!(
                    *p,
                    "==" | "!="
                        | "<="
                        | ">="
                        | "&&"
                        | "||"
                        | "=>"
                        | "+"
                        | "-"
                        | "*"
                        | "/"
                        | "%"
                        | "<"
                        | ">"
                        | "!"
                        | "?"
                ) {
                    Some((tok::OPERATOR, 0))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some((tt, tm)) = classified {
            raw_tokens.push((line0, start, len, tt, tm));
        }
    }

    // 2. Collect line comments (skipped by lexer).
    for (idx, line) in lines.iter().enumerate() {
        if let Some(col_byte) = find_comment_start(line) {
            let start = byte_col_to_utf16(line, col_byte as u32 + 1);
            let total = line.encode_utf16().count() as u32;
            let len = total.saturating_sub(start);
            if len > 0 {
                raw_tokens.push((idx as u32, start, len, tok::COMMENT, 0));
            }
        }
    }

    raw_tokens.sort_by_key(|&(l, s, ..)| (l, s));
    raw_tokens.dedup_by_key(|t| (t.0, t.1));

    // Encode LSP relative semantic tokens array (5 u32s per token).
    let mut data: Vec<u32> = Vec::with_capacity(raw_tokens.len() * 5);
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;
    for (line0, start, len, tt, tm) in raw_tokens {
        let delta_line = line0 - prev_line;
        let delta_start = if delta_line == 0 {
            start.saturating_sub(prev_start)
        } else {
            start
        };
        data.extend_from_slice(&[delta_line, delta_start, len, tt, tm]);
        prev_line = line0;
        prev_start = start;
    }

    data
}

fn symbol_token(kind: &str, is_decl: bool) -> (u32, u32) {
    let decl = if is_decl { mods::DECLARATION } else { 0 };
    match kind {
        "component" => (tok::CLASS, decl),
        "shape" => (tok::STRUCT, decl),
        "fn" => (tok::FUNCTION, decl),
        "action" => (tok::METHOD, decl),
        "style" | "decoration" | "atom" => (tok::TYPE, decl),
        "keyframes" | "timeline" => (tok::MACRO, decl),
        "prop" | "inject" | "field" | "provide" => (tok::PROPERTY, decl | mods::READONLY),
        "derive" | "resource" => (tok::VARIABLE, decl | mods::READONLY),
        "state" | "mutation" => (tok::VARIABLE, decl | mods::MODIFICATION),
        "parameter" => (tok::PARAMETER, decl),
        "local" => (tok::VARIABLE, decl),
        "route" | "task" => (tok::EVENT, decl),
        "source" => (tok::FUNCTION, 0),
        "font" | "id" | "testId" => (tok::STRING, decl),
        _ => (tok::VARIABLE, decl),
    }
}

fn classify_ident(
    w: &str,
    span: Span,
    prev: Option<&TokenKind>,
    next: Option<&TokenKind>,
) -> Option<(u32, u32)> {
    let at_line_start = matches!(
        prev,
        None | Some(TokenKind::Newline | TokenKind::Indent | TokenKind::Dedent)
    );
    let followed_by_eq = matches!(next, Some(TokenKind::Punct("=")));
    let followed_by_paren = matches!(next, Some(TokenKind::Punct("(")));

    if followed_by_eq {
        if contract_analyze::HANDLERS.contains(&w) {
            return Some((tok::EVENT, mods::DEFAULT_LIBRARY));
        }
        return Some((tok::PROPERTY, 0));
    }
    if matches!(
        w,
        "component"
            | "shape"
            | "fn"
            | "style"
            | "decoration"
            | "atom"
            | "keyframes"
            | "timeline"
            | "color-profile"
            | "theme"
            | "font"
            | "sound"
            | "routes"
            | "use"
            | "members"
            | "test"
            | "always"
            | "after"
            | "never"
    ) && span.col == 1
    {
        return Some((tok::KEYWORD, 0));
    }
    if at_line_start
        && matches!(
            w,
            "props"
                | "inject"
                | "provide"
                | "slot"
                | "state"
                | "derive"
                | "resource"
                | "mutation"
                | "action"
                | "task"
                | "sequence"
                | "tool"
                | "params"
                | "parts"
                | "view"
                | "send"
                | "refresh"
                | "let"
                | "tab"
                | "notfound"
        )
    {
        return Some((tok::KEYWORD, 0));
    }
    if matches!(
        w,
        "if" | "else"
            | "for"
            | "in"
            | "when"
            | "match"
            | "case"
            | "as"
            | "and"
            | "or"
            | "not"
            | "from"
            | "to"
            | "with"
            | "then"
            | "refreshes"
            | "queue"
            | "persist"
            | "mount"
            | "children"
            | "key"
    ) {
        return Some((tok::KEYWORD, 0));
    }
    if matches!(w, "true" | "false" | "none") {
        return Some((tok::ENUM_MEMBER, mods::DEFAULT_LIBRARY));
    }
    if matches!(
        w,
        "string" | "number" | "bool" | "unit" | "action" | "option" | "enum"
    ) {
        return Some((tok::TYPE, mods::DEFAULT_LIBRARY));
    }
    if followed_by_paren && HOST_COMMANDS.contains(&w) {
        return Some((tok::FUNCTION, mods::DEFAULT_LIBRARY));
    }
    if followed_by_paren
        && (exact_plan::Stdlib::from_name(w).is_some()
            || matches!(
                w,
                "some" | "pending" | "failed" | "failure" | "path" | "t" | "empty"
            ))
    {
        return Some((tok::FUNCTION, mods::DEFAULT_LIBRARY));
    }
    if at_line_start && contract_lower::tags::tag(w).is_some() {
        return Some((tok::CLASS, mods::DEFAULT_LIBRARY));
    }
    if w.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return Some((tok::TYPE, 0));
    }
    None
}

fn find_comment_start(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut in_str = false;
    let mut in_tpl = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
        } else if in_tpl {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'`' {
                in_tpl = false;
            }
        } else if b == b'"' {
            in_str = true;
        } else if b == b'`' {
            in_tpl = true;
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `textDocument/inlayHint`
pub fn inlay_hints(ws: &mut Workspace, params: &Value) -> Value {
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
    let range_start = params
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let range_end = params
        .pointer("/range/end/line")
        .and_then(Value::as_u64)
        .unwrap_or(u64::from(u32::MAX)) as u32;
    let target = normalize_path(&path);
    let mut hints = Vec::new();

    for def in &table.definitions {
        if normalize_path(&def.file) != target || def.span.line == 0 {
            continue;
        }
        let line0 = def.span.line - 1;
        if line0 < range_start || line0 > range_end {
            continue;
        }
        let line = line_text(&text, line0);
        let end_char = byte_col_to_utf16(line, def.span.end_col.max(def.span.col));

        // Type inlay hint on `state`, `derive`, `local`, and untyped action `parameter`.
        if matches!(def.kind, "state" | "derive" | "local" | "parameter") {
            if let Some(ty) = &def.ty {
                // Skip if the user already wrote an explicit `: T` annotation right after the name.
                let after = line
                    .get((def.span.end_col.saturating_sub(1) as usize).min(line.len())..)
                    .unwrap_or("")
                    .trim_start();
                if !after.starts_with(':') {
                    hints.push(json!({
                        "position": { "line": line0, "character": end_char },
                        "label": format!(": {ty}"),
                        "kind": 1,
                        "paddingLeft": false,
                        "paddingRight": true,
                    }));
                }
            }
        }
        // Action inferred-writes inlay hint at end of action signature line.
        if def.kind == "action" {
            if let Some(writes) = &def.writes {
                if !writes.is_empty() {
                    let eol = line.encode_utf16().count() as u32;
                    hints.push(json!({
                        "position": { "line": line0, "character": eol },
                        "label": format!("  // writes {}", writes.join(", ")),
                        "kind": 2,
                        "paddingLeft": true,
                    }));
                }
            }
        }
    }
    Value::Array(hints)
}

/// `textDocument/codeAction`
pub fn code_actions(ws: &mut Workspace, params: &Value) -> Value {
    let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
        return Value::Array(Vec::new());
    };
    let Some(path) = uri_to_path(uri) else {
        return Value::Array(Vec::new());
    };
    let Some(text) = ws.text(&path) else {
        return Value::Array(Vec::new());
    };
    let diags = params
        .pointer("/context/diagnostics")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    Value::Array(quick_fixes_for_file(ws, &path, &text, diags))
}

/// Compute all deterministic quick fixes and import actions for `path`.
pub fn quick_fixes_for_file(
    ws: &Workspace,
    path: &Path,
    text: &str,
    diags: &[Value],
) -> Vec<Value> {
    let uri = super::protocol::path_to_uri(path);
    let mut actions: Vec<Value> = Vec::new();

    // 1. Missing `use` lines (both quickfix for `contract-use-missing` and `source.organizeImports`).
    if let Some(edit_text) = compute_use_fix(ws, path, text) {
        let edit = json!({
            "changes": {
                uri.clone(): [{
                    "range": full_document_range(text),
                    "newText": edit_text,
                }]
            }
        });
        let has_missing = diags
            .iter()
            .any(|d| d.get("code").and_then(Value::as_str) == Some("contract-use-missing"));
        if has_missing {
            actions.push(json!({
                "title": "Add missing `use` imports",
                "kind": "quickfix",
                "isPreferred": true,
                "edit": edit.clone(),
            }));
        }
        actions.push(json!({
            "title": "Organize / add missing Contract imports",
            "kind": "source.organizeImports",
            "edit": edit,
        }));
    }

    // 2. Diagnostic-driven quick fixes.
    for diag in diags {
        let code = diag.get("code").and_then(Value::as_str).unwrap_or("");
        let msg = diag.get("message").and_then(Value::as_str).unwrap_or("");
        let Some(range) = diag.get("range") else {
            continue;
        };
        match code {
            "syntax-tab-indent" => {
                if let Some(line0) = range.pointer("/start/line").and_then(Value::as_u64) {
                    let line = line_text(text, line0 as u32);
                    let fixed = line.replace('\t', "  ");
                    actions.push(json!({
                        "title": "Replace tabs with spaces",
                        "kind": "quickfix",
                        "isPreferred": true,
                        "diagnostics": [diag],
                        "edit": {
                            "changes": {
                                uri.clone(): [{
                                    "range": range_json(line0 as u32, 0, line0 as u32, line.encode_utf16().count() as u32),
                                    "newText": fixed,
                                }]
                            }
                        }
                    }));
                }
            }
            "syntax-list-type" => {
                if let Some(line0) = range.pointer("/start/line").and_then(Value::as_u64) {
                    let line = line_text(text, line0 as u32);
                    if let Some(fixed) = rewrite_list_type(line) {
                        actions.push(json!({
                            "title": "Convert `list<T>` to `T[]`",
                            "kind": "quickfix",
                            "isPreferred": true,
                            "diagnostics": [diag],
                            "edit": {
                                "changes": {
                                    uri.clone(): [{
                                        "range": range_json(line0 as u32, 0, line0 as u32, line.encode_utf16().count() as u32),
                                        "newText": fixed,
                                    }]
                                }
                            }
                        }));
                    }
                }
            }
            "syntax-method-form" => {
                if let Some(rep) = backticked_after(msg, "write `") {
                    actions.push(json!({
                        "title": format!("Use method form `{rep}`"),
                        "kind": "quickfix",
                        "isPreferred": true,
                        "diagnostics": [diag],
                        "edit": { "changes": { uri.clone(): [{ "range": range, "newText": rep }] } }
                    }));
                }
            }
            "syntax-missing-prop" | "type-missing-prop" => {
                if let Some(line0) = range.pointer("/start/line").and_then(Value::as_u64) {
                    let line = line_text(text, line0 as u32);
                    let missing: Vec<&str> = msg
                        .split_once("needs ")
                        .map(|(_, rest)| {
                            rest.split(',')
                                .filter_map(|p| first_backticked(p.trim()))
                                .collect()
                        })
                        .unwrap_or_default();
                    if !missing.is_empty() {
                        if let Some(close) = line.rfind(')') {
                            let inside = line
                                .find('(')
                                .map(|o| line[o + 1..close].trim())
                                .unwrap_or("");
                            let prefix = if inside.is_empty() { "" } else { ", " };
                            let added = missing
                                .iter()
                                .map(|p| format!("{p}=\"\""))
                                .collect::<Vec<_>>()
                                .join(", ");
                            let col_u16 = byte_col_to_utf16(line, close as u32 + 1);
                            actions.push(json!({
                                "title": format!("Add missing prop(s): {}", missing.join(", ")),
                                "kind": "quickfix",
                                "isPreferred": true,
                                "diagnostics": [diag],
                                "edit": {
                                    "changes": {
                                        uri.clone(): [{
                                            "range": range_json(line0 as u32, col_u16, line0 as u32, col_u16),
                                            "newText": format!("{prefix}{added}"),
                                        }]
                                    }
                                }
                            }));
                        }
                    }
                }
            }
            "syntax-view-keyword" => {
                let replacement = if msg.contains("`for`") {
                    Some("for")
                } else if msg.contains("`if`") {
                    Some("if")
                } else {
                    None
                };
                if let Some(rep) = replacement {
                    actions.push(json!({
                        "title": format!("Replace with `{rep}`"),
                        "kind": "quickfix",
                        "isPreferred": true,
                        "diagnostics": [diag],
                        "edit": { "changes": { uri.clone(): [{ "range": range, "newText": rep }] } }
                    }));
                }
            }
            "lower-unknown-tag" => {
                if let Some(bad) = first_backticked(msg) {
                    if let Some((_, hint)) = lower_vocab::html_tags()
                        .into_iter()
                        .find(|(n, _)| *n == bad)
                    {
                        if let Some(rep) = first_backticked(hint) {
                            actions.push(json!({
                                "title": format!("Replace `{bad}` with `{rep}`"),
                                "kind": "quickfix",
                                "isPreferred": true,
                                "diagnostics": [diag],
                                "edit": { "changes": { uri.clone(): [{ "range": range, "newText": rep }] } }
                            }));
                        }
                    }
                }
            }
            "lower-unknown-attr" | "lower-renamed-attr" => {
                if let Some(bad) = first_backticked(msg) {
                    if let Some((_, rep)) =
                        lower_vocab::renamed().into_iter().find(|(n, _)| *n == bad)
                    {
                        actions.push(json!({
                            "title": format!("Rename `{bad}` to `{rep}`"),
                            "kind": "quickfix",
                            "isPreferred": true,
                            "diagnostics": [diag],
                            "edit": { "changes": { uri.clone(): [{ "range": range, "newText": rep }] } }
                        }));
                    }
                }
            }
            _ => {
                if let Some(rep) = backticked_after(msg, "did you mean `") {
                    actions.push(json!({
                        "title": format!("Replace with `{rep}`"),
                        "kind": "quickfix",
                        "isPreferred": true,
                        "diagnostics": [diag],
                        "edit": { "changes": { uri.clone(): [{ "range": range, "newText": rep }] } }
                    }));
                }
            }
        }
    }

    actions
}

fn backticked_after<'a>(s: &'a str, marker: &str) -> Option<&'a str> {
    let rest = s.split_once(marker)?.1;
    let inner = rest.split('`').next()?;
    (!inner.is_empty()).then_some(inner)
}

fn rewrite_list_type(line: &str) -> Option<String> {
    let start = line.find("list<")?;
    let mut depth = 0usize;
    let mut end = None;
    for (i, ch) in line[start + 5..].char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                if depth == 0 {
                    end = Some(start + 5 + i);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let end = end?;
    let inner = &line[start + 5..end];
    Some(format!("{}{}[]{}", &line[..start], inner, &line[end + 1..]))
}

fn first_backticked(s: &str) -> Option<&str> {
    let rest = s.split_once('`')?.1;
    let inner = rest.split('`').next()?;
    (!inner.is_empty()).then_some(inner)
}

fn compute_use_fix(ws: &Workspace, path: &Path, text: &str) -> Option<String> {
    let root = ws.find_root(path);
    let root_src = ws.text(&root)?;
    let app_root = root
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .ok()?;
    let target = normalize_path(path);
    with_contract_texts(ws.overlays(), || {
        let fixes = sources::use_fixes(&root, &root_src, &app_root).ok()?;
        let fix = fixes
            .into_iter()
            .find(|f| normalize_path(&f.path) == target && !f.lines.is_empty())?;
        let after = sources::apply_uses(text, &fix.lines);
        (after != text).then_some(after)
    })
}

/// `textDocument/formatting`
pub fn formatting(ws: &mut Workspace, params: &Value) -> Value {
    let Some(path) = params
        .pointer("/textDocument/uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return Value::Array(Vec::new());
    };
    let Some(before) = ws.text(&path) else {
        return Value::Array(Vec::new());
    };
    let Ok(after) = contract_syntax::fmt::format(&before) else {
        return Value::Array(Vec::new());
    };
    if before == after {
        return Value::Array(Vec::new());
    }
    json!([{
        "range": full_document_range(&before),
        "newText": after,
    }])
}

/// `textDocument/foldingRange`
pub fn folding_ranges(ws: &mut Workspace, params: &Value) -> Value {
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
    let lines: Vec<&str> = text.lines().collect();
    let mut ranges: Vec<Value> = Vec::new();
    let mut stack: Vec<(usize, u32)> = Vec::new();
    let mut last_non_empty = 0u32;
    let mut comment_start: Option<u32> = None;

    for (idx, raw) in lines.iter().enumerate() {
        let line0 = idx as u32;
        let trimmed = raw.trim_start();
        if trimmed.is_empty() {
            if let Some(cs) = comment_start.take() {
                if line0 > cs + 1 {
                    ranges
                        .push(json!({ "startLine": cs, "endLine": line0 - 1, "kind": "comment" }));
                }
            }
            continue;
        }
        if trimmed.starts_with("//") {
            comment_start.get_or_insert(line0);
            continue;
        }
        if let Some(cs) = comment_start.take() {
            if line0 > cs + 1 {
                ranges.push(json!({ "startLine": cs, "endLine": line0 - 1, "kind": "comment" }));
            }
        }
        let indent = raw.len() - trimmed.len();
        while let Some(&(prev_ind, start_line)) = stack.last() {
            if indent <= prev_ind {
                stack.pop();
                if last_non_empty > start_line {
                    ranges.push(json!({
                        "startLine": start_line,
                        "endLine": last_non_empty,
                    }));
                }
            } else {
                break;
            }
        }
        stack.push((indent, line0));
        last_non_empty = line0;
    }
    if let Some(cs) = comment_start.take() {
        let end = lines.len().saturating_sub(1) as u32;
        if end > cs {
            ranges.push(json!({ "startLine": cs, "endLine": end, "kind": "comment" }));
        }
    }
    while let Some((_, start_line)) = stack.pop() {
        if last_non_empty > start_line {
            ranges.push(json!({
                "startLine": start_line,
                "endLine": last_non_empty,
            }));
        }
    }
    Value::Array(ranges)
}
