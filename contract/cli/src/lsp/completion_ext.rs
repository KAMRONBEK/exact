//! Auto-import completions, chained dot-member resolution, and cross-seam
//! completions (`t("...")`, Forma tokens, `hatch="..."`, `.test.contract`).

use super::protocol::{comp_kind, line_text, range_json};
use super::seam;
use super::workspace::{Snapshot, Workspace};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

/// Resolve chained member expressions like `order.customer.` or `items.0.` to
/// the receiver's record shape fields, falling back to `for x in xs` local inference.
pub fn resolve_chained_dot_fields(
    prefix: &str,
    text: &str,
    line0: u32,
    snap: &Snapshot,
) -> Option<Vec<(String, String)>> {
    let trimmed = prefix.trim_end();
    let before_word = trimmed.trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
    let before_dot = before_word.strip_suffix('.')?;
    let chain_start = before_dot
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'))
        .map(|i| i + 1)
        .unwrap_or(0);
    let chain = &before_dot[chain_start..];
    let parts: Vec<&str> = chain.split('.').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }

    let table = snap.symbols.as_ref()?;
    // Determine root type of `parts[0]` from symbols or enclosing `for var in list` line.
    let mut cur_ty = table
        .definitions
        .iter()
        .find(|d| d.name == parts[0])
        .and_then(|d| d.ty.clone())
        .or_else(|| infer_for_local_type(parts[0], text, line0, table))?;

    for seg in &parts[1..] {
        if seg.bytes().all(|b| b.is_ascii_digit()) {
            // Numeric list index (`rows.0.`).
            cur_ty = cur_ty.strip_suffix("[]")?.to_owned();
            continue;
        }
        let shape_name = cur_ty
            .strip_prefix("option<")
            .and_then(|s| s.strip_suffix('>'))
            .unwrap_or(&cur_ty);
        let fields = table.shapes.map.get(shape_name)?;
        let (_, next_ty) = fields.iter().find(|(f, _)| f == seg)?;
        cur_ty = next_ty.to_string();
    }

    let fields = table.shapes.map.get(cur_ty.as_str())?;
    Some(
        fields
            .iter()
            .map(|(f, t)| (f.clone(), t.to_string()))
            .collect(),
    )
}

fn infer_for_local_type(
    var: &str,
    text: &str,
    line0: u32,
    table: &crate::symbols::SymbolTable,
) -> Option<String> {
    let needle = format!("for {var} in ");
    let needle_comma = format!("for {var},");
    for l in (0..line0).rev() {
        let trimmed = line_text(text, l).trim_start();
        let rest = if let Some(r) = trimmed.strip_prefix(&needle) {
            r
        } else if let Some(after) = trimmed.strip_prefix(&needle_comma) {
            after.split_once(" in ")?.1
        } else {
            continue;
        };
        let list_ident = rest.split_whitespace().next()?;
        let list_ty = table
            .definitions
            .iter()
            .find(|d| d.name == list_ident)
            .and_then(|d| d.ty.as_deref())?;
        return list_ty.strip_suffix("[]").map(str::to_owned);
    }
    None
}

/// Complete cross-seam string contexts: `t("|")`, `var(--|)`, `hatch="|"`, and `.test.contract` `tap "|"`.
pub fn complete_seam_string(
    ws: &mut Workspace,
    path: &Path,
    prefix: &str,
    push: &mut impl FnMut(Value),
) -> bool {
    let Some(dir) = seam::app_dir(ws, path) else {
        return false;
    };
    let Some(q) = prefix.rfind('"') else {
        return false;
    };
    let before_q = prefix[..q].trim_end();

    // 1. `t("key")` -> keys from `strings/en.json`.
    if before_q.ends_with("t(") {
        if let Some((_, map)) = seam::load_strings(&dir) {
            for (k, (_, val)) in map {
                push(json!({
                    "label": k,
                    "kind": comp_kind::CONSTANT,
                    "detail": format!("\"{val}\""),
                }));
            }
            return true;
        }
    }

    // 2. Direct Forma token attributes (`color="|"`, `background-color="|"`, etc.).
    if before_q.ends_with("color=")
        || before_q.ends_with("background-color=")
        || before_q.ends_with("border-color=")
        || before_q.ends_with("fill=")
        || before_q.ends_with("stroke=")
    {
        if let Some((_, map)) = seam::load_forma_tokens(&dir) {
            for (tok, (_, val)) in map {
                push(json!({
                    "label": tok,
                    "kind": comp_kind::COLOR,
                    "detail": format!("app.forma: {val}"),
                }));
            }
            // Fall through (return false) so built-in color roles are also offered.
        }
    }

    // 3. `hatch="|"` -> `app.json` hatches.
    if before_q.ends_with("hatch=") {
        let m = seam::load_manifest_seam(&dir);
        if !m.hatches.is_empty() {
            for h in m.hatches {
                push(json!({
                    "label": h,
                    "kind": comp_kind::ENUM_MEMBER,
                    "detail": "app.json hatch",
                }));
            }
            return true;
        }
    }

    // 4. `.test.contract` `tap "|"` / `type "|"` -> `testId`s from sibling `app.contract`.
    if Workspace::is_test_file(path)
        && (before_q.ends_with("tap") || before_q.ends_with("type") || before_q.ends_with("into"))
    {
        let app = dir.join("app.contract");
        if app.is_file() {
            let snap = ws.analyze(&app);
            if let Some(table) = &snap.symbols {
                for d in &table.definitions {
                    if matches!(d.kind, "testId" | "id") {
                        push(json!({
                            "label": d.name,
                            "kind": comp_kind::VALUE,
                            "detail": format!("{} in {}", d.kind, d.component.as_deref().unwrap_or("App")),
                        }));
                    }
                }
                return true;
            }
        }
    }

    false
}

/// Suggest unimported components/shapes/fns/styles from sibling `.contract` files
/// with `additionalTextEdits` that insert or extend the `use … from "…"` line.
pub fn complete_auto_imports(
    ws: &Workspace,
    path: &Path,
    text: &str,
    snap: &Snapshot,
    want_components: bool,
    want_types: bool,
    push: &mut impl FnMut(Value),
) {
    let Some(dir) = path.parent() else { return };
    let self_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // Names already declared locally or imported.
    let mut existing: BTreeSet<String> = BTreeSet::new();
    let mut existing_uses: Vec<(u32, String, String)> = Vec::new();
    if let Some(parsed) = &snap.parsed {
        for c in &parsed.components {
            existing.insert(c.name.clone());
        }
        for s in &parsed.shapes {
            existing.insert(s.name.clone());
        }
        for f in &parsed.fns {
            existing.insert(f.name.clone());
        }
        for s in &parsed.styles {
            existing.insert(s.name.clone());
        }
        for u in &parsed.uses {
            for n in &u.names {
                existing.insert(n.local().to_owned());
            }
            let l0 = u.span.line.saturating_sub(1);
            existing_uses.push((l0, u.path.clone(), line_text(text, l0).to_owned()));
        }
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().into_owned();
        if !fname.ends_with(".contract") || fname.ends_with(".test.contract") || fname == self_name
        {
            continue;
        }
        let spec = format!("./{fname}");
        let Some(other_src) = ws.text(&entry.path()) else {
            continue;
        };
        let other = match contract_syntax::parse_source_partial(&other_src, 0) {
            Ok(f) => f,
            Err(p) => match p.file {
                Some(f) => *f,
                None => continue,
            },
        };
        let make_import_edit = |sym: &str| -> Value {
            if let Some((l0, _, old_line)) = existing_uses.iter().find(|(_, p, _)| p == &spec) {
                if let Some(pos) = old_line.find(" from ") {
                    let updated = format!("{}, {sym}{}", &old_line[..pos], &old_line[pos..]);
                    return json!({
                        "range": range_json(*l0, 0, *l0, old_line.encode_utf16().count() as u32),
                        "newText": updated,
                    });
                }
            }
            json!({
                "range": range_json(0, 0, 0, 0),
                "newText": format!("use {sym} from \"{spec}\"\n"),
            })
        };

        if want_components {
            for c in &other.components {
                if existing.contains(&c.name) {
                    continue;
                }
                let props: Vec<String> = c
                    .props
                    .iter()
                    .enumerate()
                    .map(|(i, p)| format!("{}=${{{}}}", p.name, i + 1))
                    .collect();
                push(json!({
                    "label": c.name,
                    "kind": comp_kind::CLASS,
                    "detail": format!("auto-import from {spec}"),
                    "insertText": format!("{}({})", c.name, props.join(", ")),
                    "insertTextFormat": 2,
                    "additionalTextEdits": [make_import_edit(&c.name)],
                }));
            }
        }
        if want_types {
            for s in &other.shapes {
                if existing.contains(&s.name) {
                    continue;
                }
                push(json!({
                    "label": s.name,
                    "kind": comp_kind::STRUCT,
                    "detail": format!("shape — auto-import from {spec}"),
                    "additionalTextEdits": [make_import_edit(&s.name)],
                }));
            }
        }
    }
}
