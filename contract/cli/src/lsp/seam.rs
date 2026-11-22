//! Cross-seam intelligence linking `.contract` files to `app.ts`, Rust `data/`,
//! `strings/*.json`, `app.forma`, `app.json`, `package.json`, and `*.test.contract`.

use super::protocol::{line_text, normalize_path, path_to_uri, range_json, utf16_to_byte_col};
use super::workspace::Workspace;
use contract_syntax::{Expr, File, Node, Stmt};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// One located symbol in a companion file (`app.ts`, `strings/en.json`, `app.forma`, etc.).
#[derive(Debug, Clone)]
pub struct SeamHit {
    /// Absolute path of the companion file.
    pub file: PathBuf,
    /// 0-based line number.
    pub line0: u32,
    /// 0-based start UTF-16 column.
    pub start_col0: u32,
    /// 0-based end UTF-16 column.
    pub end_col0: u32,
    /// Short preview value or signature line.
    pub preview: String,
}

impl SeamHit {
    /// Convert to an LSP `Location`.
    pub fn to_location(&self) -> Value {
        json!({
            "uri": path_to_uri(&self.file),
            "range": range_json(self.line0, self.start_col0, self.line0, self.end_col0),
        })
    }
}

/// Resolve the app root directory for `contract_path` (the directory holding
/// `app.contract` or `app.json`, falling back to `contract_path.parent()`).
pub fn app_dir(ws: &Workspace, contract_path: &Path) -> Option<PathBuf> {
    let root = ws.find_root(contract_path);
    root.parent()
        .or_else(|| contract_path.parent())
        .map(normalize_path)
}

/// Find a data source definition (`resource`/`send` target) in `app.ts` or `data/src/*.rs`.
pub fn find_data_source(dir: &Path, name: &str) -> Option<SeamHit> {
    let ts = dir.join("app.ts");
    if let Ok(src) = std::fs::read_to_string(&ts) {
        if let Some(hit) = scan_ts_export(&ts, &src, name) {
            return Some(hit);
        }
    }
    let rust_dir = dir.join("data").join("src");
    if let Ok(entries) = std::fs::read_dir(&rust_dir) {
        let mut rs_files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "rs"))
            .collect();
        rs_files.sort();
        for rs in rs_files {
            if let Ok(src) = std::fs::read_to_string(&rs) {
                if let Some(hit) = scan_rust_source(&rs, &src, name) {
                    return Some(hit);
                }
            }
        }
    }
    None
}

fn scan_ts_export(path: &Path, src: &str, name: &str) -> Option<SeamHit> {
    for (idx, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        let matched = [
            format!("export async function {name}("),
            format!("export function {name}("),
            format!("export const {name} ="),
            format!("export const {name}:"),
            format!("export let {name} ="),
            format!("function {name}("),
            format!("async function {name}("),
            format!("async {name}("),
            format!("{name}("),
            format!("{name}:"),
            format!("case \"{name}\":"),
            format!("case '{name}':"),
        ]
        .iter()
        .any(|pat| trimmed.starts_with(pat));
        if matched {
            let col = line.find(name).unwrap_or(0) as u32;
            return Some(SeamHit {
                file: normalize_path(path),
                line0: idx as u32,
                start_col0: col,
                end_col0: col + name.len() as u32,
                preview: trimmed.to_owned(),
            });
        }
    }
    None
}

fn scan_rust_source(path: &Path, src: &str, name: &str) -> Option<SeamHit> {
    let quoted = format!("\"{name}\"");
    for (idx, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.contains(&quoted) && (trimmed.contains("=>") || trimmed.starts_with(&quoted)) {
            let col = line.find(&quoted).unwrap_or(0) as u32;
            return Some(SeamHit {
                file: normalize_path(path),
                line0: idx as u32,
                start_col0: col,
                end_col0: col + quoted.len() as u32,
                preview: trimmed.to_owned(),
            });
        }
        if trimmed.starts_with(&format!("fn {name}("))
            || trimmed.starts_with(&format!("pub fn {name}("))
        {
            let col = line.find(name).unwrap_or(0) as u32;
            return Some(SeamHit {
                file: normalize_path(path),
                line0: idx as u32,
                start_col0: col,
                end_col0: col + name.len() as u32,
                preview: trimmed.to_owned(),
            });
        }
    }
    None
}

/// Map from key/token name to `(zero_based_line, value_preview)`.
pub type KeyedEntries = BTreeMap<String, (u32, String)>;

/// Read `strings/en.json` (or the first JSON file under `strings/`) as `(path, key -> (line0, value))`.
pub fn load_strings(dir: &Path) -> Option<(PathBuf, KeyedEntries)> {
    let strings_dir = dir.join("strings");
    let primary = strings_dir.join("en.json");
    let file = if primary.is_file() {
        primary
    } else {
        std::fs::read_dir(&strings_dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "json"))?
    };
    let text = std::fs::read_to_string(&file).ok()?;
    let parsed: Value = serde_json::from_str(&text).ok()?;
    let obj = parsed.as_object()?;
    let mut map = BTreeMap::new();
    for (k, v) in obj {
        let val = match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let needle = format!("\"{k}\"");
        let line0 = text.lines().position(|l| l.contains(&needle)).unwrap_or(0) as u32;
        map.insert(k.clone(), (line0, val));
    }
    Some((normalize_path(&file), map))
}

/// Read design tokens declared in `app.forma` (or `app.json`'s `forma` path)
/// via `exact_forma::parse` as `(path, token -> (line0, value))`.
pub fn load_forma_tokens(dir: &Path) -> Option<(PathBuf, KeyedEntries)> {
    let path = crate::design_path(&dir.join("app.contract"))?;
    let text = std::fs::read_to_string(&path).ok()?;
    let design = exact_forma::parse(&text).ok()?;
    let mut map = BTreeMap::new();
    for t in design.tokens.iter().chain(&design.icons) {
        map.insert(
            t.name.value.clone(),
            (t.name.line.saturating_sub(1), t.value.value.clone()),
        );
    }
    for d in &design.dials {
        map.insert(
            d.name.value.clone(),
            (d.name.line.saturating_sub(1), d.value.value.clone()),
        );
    }
    Some((normalize_path(&path), map))
}

/// Read `app.json` hatches, native module tags, data-* keys, and widgets.
#[derive(Debug, Default)]
pub struct AppManifestSeam {
    /// Declared hatch words (`app.json` `hatches`).
    pub hatches: Vec<String>,
    /// Declared native module tags (`app.json` `modules`).
    pub modules: Vec<String>,
    /// Declared `data-*` keys (`app.json` `data`).
    pub data_keys: Vec<String>,
    /// Declared widget kinds.
    pub widgets: Vec<String>,
}

/// Parse `app.json` beside `dir` when present.
pub fn load_manifest_seam(dir: &Path) -> AppManifestSeam {
    let mut out = AppManifestSeam::default();
    let Ok(text) = std::fs::read_to_string(dir.join("app.json")) else {
        return out;
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return out;
    };
    let collect_keys = |field: &str| -> Vec<String> {
        match &v[field] {
            Value::Array(arr) => arr
                .iter()
                .filter_map(|i| i.as_str().map(str::to_owned))
                .collect(),
            Value::Object(map) => map.keys().cloned().collect(),
            _ => Vec::new(),
        }
    };
    out.hatches = collect_keys("hatches");
    out.modules = collect_keys("modules");
    out.data_keys = collect_keys("data");
    out.widgets = collect_keys("widgets");
    out
}

/// Find `@exact/*` packages installed in the nearest `package.json` above `path`.
pub fn available_exact_packages(path: &Path) -> Vec<String> {
    let mut cur = path.parent();
    for _ in 0..8 {
        let Some(dir) = cur else { break };
        let pkg = dir.join("package.json");
        if let Ok(text) = std::fs::read_to_string(&pkg) {
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                let mut names: BTreeSet<String> = BTreeSet::new();
                for section in ["dependencies", "devDependencies"] {
                    if let Some(obj) = v.get(section).and_then(Value::as_object) {
                        for k in obj.keys() {
                            if k.starts_with("@exact/") {
                                names.insert(k.clone());
                            }
                        }
                    }
                }
                if !names.is_empty() {
                    return names.into_iter().collect();
                }
            }
        }
        cur = dir.parent();
    }
    Vec::new()
}

/// Check if `(line0, char0)` is inside a `t("key")`, Forma token literal, or `.test.contract`
/// `tap "testId"` / `after action` reference, returning a cross-seam hit.
pub fn seam_hit_at(
    ws: &mut Workspace,
    path: &Path,
    text: &str,
    line0: u32,
    char0: u32,
) -> Option<(String, SeamHit)> {
    let dir = app_dir(ws, path)?;
    let line = line_text(text, line0);
    let byte_idx = utf16_to_byte_col(line, char0)
        .saturating_sub(1)
        .min(line.len() as u32) as usize;

    // 1. Quoted-string lookups: `t("key")` in `strings/en.json`, Forma tokens, and `.test.contract` `testId`.
    if let Some((quoted, start_q)) = enclosing_quoted(line, byte_idx) {
        let before = line[..start_q].trim_end();
        if before.ends_with("t(") {
            if let Some((sfile, map)) = load_strings(&dir) {
                if let Some((l0, val)) = map.get(quoted) {
                    return Some((
                        format!("**Localized string** `{quoted}`\n\n`{val}`"),
                        SeamHit {
                            file: sfile,
                            line0: *l0,
                            start_col0: 0,
                            end_col0: 0,
                            preview: val.clone(),
                        },
                    ));
                }
            }
        }
        // 2b. Direct Forma design token literal (e.g. `color="text.primary"`, `background-color="surface.page"`).
        if let Some((tfile, map)) = load_forma_tokens(&dir) {
            if let Some((l0, val)) = map.get(quoted) {
                return Some((
                    format!("**Design token** `{quoted}`\n\n```forma\n{quoted} = {val}\n```"),
                    SeamHit {
                        file: tfile,
                        line0: *l0,
                        start_col0: 0,
                        end_col0: 0,
                        preview: val.clone(),
                    },
                ));
            }
        }
        // 3. In `.test.contract`, `tap "testId"` / `type "testId"` -> jump to `testId="..."` in `app.contract`.
        if Workspace::is_test_file(path) {
            let app = dir.join("app.contract");
            if app.is_file() {
                let snap = ws.analyze(&app);
                if let Some(table) = &snap.symbols {
                    if let Some(def) = table
                        .definitions
                        .iter()
                        .find(|d| matches!(d.kind, "testId" | "id") && d.name == quoted)
                    {
                        return Some((
                            format!(
                                "`{}` `{quoted}` in `{}`",
                                def.kind,
                                def.component.as_deref().unwrap_or("")
                            ),
                            SeamHit {
                                file: normalize_path(&def.file),
                                line0: def.span.line.saturating_sub(1),
                                start_col0: def.span.col.saturating_sub(1),
                                end_col0: def.span.end_col.saturating_sub(1),
                                preview: quoted.to_owned(),
                            },
                        ));
                    }
                }
            }
        }
    }

    // 4. In `.test.contract`, `after actionName` / `never refuses actionName` -> jump to `action` in `app.contract`.
    if Workspace::is_test_file(path) {
        if let Some((word, _, _)) = super::protocol::word_at(text, line0, char0) {
            let app = dir.join("app.contract");
            if app.is_file() {
                let snap = ws.analyze(&app);
                if let Some(table) = &snap.symbols {
                    if let Some(def) = table
                        .definitions
                        .iter()
                        .find(|d| d.kind == "action" && d.name == word)
                    {
                        return Some((
                            format!(
                                "`action {word}` in `{}`",
                                def.component.as_deref().unwrap_or("")
                            ),
                            SeamHit {
                                file: normalize_path(&def.file),
                                line0: def.span.line.saturating_sub(1),
                                start_col0: def.span.col.saturating_sub(1),
                                end_col0: def.span.end_col.saturating_sub(1),
                                preview: word,
                            },
                        ));
                    }
                }
            }
        }
    }

    None
}

fn enclosing_quoted(line: &str, byte_idx: usize) -> Option<(&str, usize)> {
    let left = line[..byte_idx.min(line.len())].rfind('"')?;
    let rel_right = line[left + 1..].find('"')?;
    let right = left + 1 + rel_right;
    (byte_idx >= left && byte_idx <= right).then_some((&line[left + 1..right], left))
}

fn view_exprs(roots: &[Node]) -> Vec<&Expr> {
    let mut exprs = vec![];
    for n in Node::walk(roots) {
        exprs.extend(n.exprs());
    }
    exprs
}

/// Collect all `t("key")` keys used in a Contract AST.
fn collect_t_keys(file: &File, out: &mut BTreeSet<String>) {
    fn walk_expr(e: &Expr, out: &mut BTreeSet<String>) {
        match e {
            Expr::Call(name, args, _) => {
                if name == "t" {
                    if let Some(Expr::Str(key, _)) = args.first() {
                        out.insert(key.clone());
                    }
                }
                for a in args {
                    walk_expr(a, out);
                }
            }
            Expr::List(xs, _) => xs.iter().for_each(|x| walk_expr(x, out)),
            Expr::Template(parts, _) => {
                for p in parts {
                    if let contract_syntax::TemplatePart::Expr(x) = p {
                        walk_expr(x, out);
                    }
                }
            }
            Expr::NamedArg(_, x, _)
            | Expr::Typed(x, _, _)
            | Expr::Some(x, _)
            | Expr::Unary(_, x, _)
            | Expr::Member(x, _, _)
            | Expr::Arrow { body: x, .. } => walk_expr(x, out),
            Expr::Binary(_, a, b, _) => {
                walk_expr(a, out);
                walk_expr(b, out);
            }
            Expr::Ternary(a, b, c, _) => {
                walk_expr(a, out);
                walk_expr(b, out);
                walk_expr(c, out);
            }
            Expr::Match {
                subject,
                some,
                none,
                ..
            } => {
                walk_expr(subject, out);
                walk_expr(some, out);
                walk_expr(none, out);
            }
            Expr::Let { value, body, .. } => {
                walk_expr(value, out);
                walk_expr(body, out);
            }
            _ => {}
        }
    }
    fn walk_stmts(stmts: &[Stmt], out: &mut BTreeSet<String>) {
        for s in stmts {
            match s {
                Stmt::Let { expr, .. } | Stmt::Assign { expr, .. } => walk_expr(expr, out),
                Stmt::Command { args, .. } | Stmt::Call { args, .. } | Stmt::Send { args, .. } => {
                    args.iter().for_each(|a| walk_expr(a, out))
                }
                Stmt::If {
                    cond,
                    then,
                    otherwise,
                    ..
                } => {
                    walk_expr(cond, out);
                    walk_stmts(then, out);
                    walk_stmts(otherwise, out);
                }
                Stmt::Match {
                    subject,
                    some,
                    none,
                    ..
                } => {
                    walk_expr(subject, out);
                    walk_stmts(&some.1, out);
                    walk_stmts(none, out);
                }
                Stmt::Refresh { .. } => {}
            }
        }
    }
    for f in &file.fns {
        walk_expr(&f.body, out);
    }
    for c in &file.components {
        for b in c.states.iter().chain(&c.derives).chain(&c.provides) {
            walk_expr(&b.expr, out);
        }
        for a in &c.actions {
            walk_stmts(&a.body, out);
        }
        for e in view_exprs(&c.view) {
            walk_expr(e, out);
        }
    }
}

/// `exact/seam`: full cross-seam integrity audit for the app containing `path`.
pub fn audit_seam(ws: &mut Workspace, path: &Path) -> Value {
    let snap = ws.analyze(path);
    let Some(dir) = app_dir(ws, path) else {
        return json!({ "ok": false, "error": "cannot determine app directory" });
    };

    // 1. Data sources: declared in Contract vs implemented in `app.ts` / `data/src/*.rs`.
    let mut sources_json = Vec::new();
    let mut missing_sources = Vec::new();
    if let Some(table) = &snap.symbols {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for d in &table.definitions {
            if d.kind != "source" || !seen.insert(d.name.clone()) {
                continue;
            }
            let runner_owned = exact_plan::runner_owned_source(&d.name);
            let impl_hit = if runner_owned {
                None
            } else {
                find_data_source(&dir, &d.name)
            };
            if !runner_owned && impl_hit.is_none() {
                missing_sources.push(d.name.clone());
            }
            sources_json.push(json!({
                "name": d.name,
                "runnerOwned": runner_owned,
                "implemented": runner_owned || impl_hit.is_some(),
                "location": impl_hit.as_ref().map(SeamHit::to_location),
                "preview": impl_hit.map(|h| h.preview),
            }));
        }
    }

    // Check `app.contract.d.ts` freshness when `app.ts` exists.
    let dts_status = if dir.join("app.ts").is_file() {
        let dts_path = dir.join("app.contract.d.ts");
        let fresh = crate::compile_path_all_unchecked(&snap.root, false)
            .ok()
            .and_then(|(plan, _)| {
                crate::typescript_exporting(
                    &plan,
                    crate::design_exports(&snap.root).as_deref(),
                    &[],
                )
                .ok()
            })
            .map(|expected| {
                std::fs::read_to_string(&dts_path)
                    .ok()
                    .is_some_and(|actual| actual == expected)
            });
        json!({ "present": dts_path.is_file(), "fresh": fresh })
    } else {
        json!({ "present": false, "fresh": null })
    };

    // 2. Strings keys (`t("...")` vs `strings/en.json`).
    let mut used_keys = BTreeSet::new();
    if let Some(prog) = snap.program.as_ref().or(snap.parsed.as_ref()) {
        collect_t_keys(prog, &mut used_keys);
    }
    let strings_info = match load_strings(&dir) {
        Some((sfile, map)) => {
            let defined: BTreeSet<String> = map.keys().cloned().collect();
            let missing: Vec<&String> = used_keys.difference(&defined).collect();
            let unused: Vec<&String> = defined.difference(&used_keys).collect();
            json!({
                "file": sfile.to_string_lossy(),
                "usedCount": used_keys.len(),
                "definedCount": defined.len(),
                "missingKeys": missing,
                "unusedKeys": unused,
            })
        }
        None => json!({
            "file": null,
            "usedCount": used_keys.len(),
            "missingKeys": used_keys.iter().collect::<Vec<_>>(),
        }),
    };

    // 3. Forma design tokens.
    let forma_info = match load_forma_tokens(&dir) {
        Some((tfile, map)) => json!({
            "file": tfile.to_string_lossy(),
            "tokenCount": map.len(),
            "tokens": map.keys().collect::<Vec<_>>(),
        }),
        None => json!({ "file": null, "tokenCount": 0 }),
    };

    // 4. Manifest (`app.json`).
    let manifest = load_manifest_seam(&dir);

    // 5. Authored test files (`*.test.contract`): check referenced `testId`s against `app.contract`.
    let declared_test_ids: BTreeSet<String> = snap
        .symbols
        .as_ref()
        .map(|t| {
            t.definitions
                .iter()
                .filter(|d| matches!(d.kind, "testId" | "id"))
                .map(|d| d.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let mut test_files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if Workspace::is_test_file(&p) {
                if let Ok(tsrc) = std::fs::read_to_string(&p) {
                    let test_count = crate::tests(&tsrc).map(|t| t.len()).unwrap_or(0);
                    let prop_count = crate::properties(&tsrc).map(|p| p.len()).unwrap_or(0);
                    test_files.push(json!({
                        "file": p.to_string_lossy(),
                        "tests": test_count,
                        "properties": prop_count,
                    }));
                }
            }
        }
    }

    json!({
        "ok": missing_sources.is_empty(),
        "appDir": dir.to_string_lossy(),
        "root": snap.root.to_string_lossy(),
        "dataSeam": {
            "hasTypeScript": dir.join("app.ts").is_file(),
            "hasRust": dir.join("data/Cargo.toml").is_file(),
            "dts": dts_status,
            "sources": sources_json,
            "missingSources": missing_sources,
        },
        "strings": strings_info,
        "forma": forma_info,
        "manifest": {
            "hatches": manifest.hatches,
            "modules": manifest.modules,
            "dataKeys": manifest.data_keys,
            "widgets": manifest.widgets,
        },
        "tests": {
            "declaredTestIds": declared_test_ids.into_iter().collect::<Vec<_>>(),
            "files": test_files,
        }
    })
}
