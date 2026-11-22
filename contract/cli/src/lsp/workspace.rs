//! Workspace state, in-memory document overlays, app-root discovery, and
//! cached compiler analysis snapshots.

use super::protocol::{normalize_path, path_to_uri, span_to_range, uri_to_path};
use crate::sources::{self, with_contract_texts};
use crate::symbols::{self, SymbolTable};
use crate::CompileError;
use contract_syntax::File;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// One open editor buffer.
#[derive(Debug, Clone)]
pub struct Document {
    /// Text as last synchronized by the client.
    pub text: String,
    /// Client document version, when provided.
    pub version: Option<i64>,
}

/// Analysis result for a compilation root (typically `app.contract` or the
/// open file itself).
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Entry file compiled as the root of this snapshot.
    pub root: PathBuf,
    /// Parsed AST of the single target file (even when syntax recovery ran).
    pub parsed: Option<File>,
    /// Merged AST of the whole program rooted at `root`, when loading succeeded.
    pub program: Option<File>,
    /// Resolved symbol table (definitions, references, types, action writes).
    pub symbols: Option<SymbolTable>,
    /// Diagnostics grouped by normalized file path.
    pub diagnostics: BTreeMap<PathBuf, Vec<CompileError>>,
    /// Every source file reached from `root`.
    pub files: Vec<PathBuf>,
}

/// Mutable state of the language server across open documents and workspace roots.
#[derive(Debug, Default)]
pub struct Workspace {
    /// Workspace root folders reported during `initialize`.
    pub roots: Vec<PathBuf>,
    /// Unsaved in-memory document buffers, keyed by normalized path.
    pub docs: HashMap<PathBuf, Document>,
    /// Paths to which diagnostics were last published (so cleared files get `[]`).
    pub published_uris: BTreeSet<String>,
    /// Cached snapshots keyed by compilation root path.
    cache: HashMap<PathBuf, Snapshot>,
}

impl Workspace {
    /// Record workspace root folders from `initialize`.
    pub fn set_roots(&mut self, params: &Value) {
        self.roots.clear();
        if let Some(folders) = params.get("workspaceFolders").and_then(Value::as_array) {
            for f in folders {
                if let Some(p) = f.get("uri").and_then(Value::as_str).and_then(uri_to_path) {
                    self.roots.push(p);
                }
            }
        }
        if self.roots.is_empty() {
            if let Some(p) = params
                .get("rootUri")
                .and_then(Value::as_str)
                .and_then(uri_to_path)
            {
                self.roots.push(p);
            } else if let Some(p) = params.get("rootPath").and_then(Value::as_str) {
                self.roots.push(normalize_path(Path::new(p)));
            }
        }
    }

    /// Open a document in memory and invalidate affected cached snapshots.
    pub fn open(&mut self, path: PathBuf, text: String, version: Option<i64>) {
        self.invalidate_path(&path);
        self.docs.insert(path, Document { text, version });
    }

    /// Update an open document and invalidate affected cached snapshots.
    pub fn update(&mut self, path: &Path, changes: &[Value], version: Option<i64>) {
        if let Some(doc) = self.docs.get_mut(path) {
            super::protocol::apply_content_changes(&mut doc.text, changes);
            doc.version = version;
        } else if let Some(text) = changes
            .last()
            .and_then(|c| c.get("text"))
            .and_then(Value::as_str)
        {
            self.docs.insert(
                path.to_path_buf(),
                Document {
                    text: text.to_owned(),
                    version,
                },
            );
        }
        self.invalidate_path(path);
    }

    /// Close a document (falling back to disk for future reads).
    pub fn close(&mut self, path: &Path) {
        self.docs.remove(path);
        self.invalidate_path(path);
    }

    /// Invalidate all cached snapshots (e.g. after watched file changes).
    pub fn invalidate(&mut self) {
        self.cache.clear();
    }

    /// Invalidate only cached snapshots whose source graph or directory includes `path`.
    pub fn invalidate_path(&mut self, path: &Path) {
        let target = normalize_path(path);
        let target_dir = target.parent().map(Path::to_path_buf);
        self.cache.retain(|root, snap| {
            let touches = *root == target
                || snap.files.contains(&target)
                || target_dir
                    .as_deref()
                    .is_some_and(|d| root.parent() == Some(d));
            !touches
        });
    }

    /// Current text of `path`, preferring the open in-memory buffer over disk.
    pub fn text(&self, path: &Path) -> Option<String> {
        let key = normalize_path(path);
        if let Some(doc) = self.docs.get(&key) {
            return Some(doc.text.clone());
        }
        std::fs::read_to_string(&key).ok()
    }

    /// Collect all in-memory overrides for [`with_contract_texts`].
    pub fn overlays(&self) -> Vec<(PathBuf, String)> {
        self.docs
            .iter()
            .map(|(p, d)| (p.clone(), d.text.clone()))
            .collect()
    }

    /// Whether `path` is an authored test file (`*.test.contract`).
    pub fn is_test_file(path: &Path) -> bool {
        path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".test.contract"))
    }

    /// Find the compilation root for `path`: an enclosing `app.contract` whose
    /// source graph reaches `path`, else `path` itself.
    pub fn find_root(&self, path: &Path) -> PathBuf {
        let target = normalize_path(path);
        if Self::is_test_file(&target) {
            return target;
        }
        if target
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "app.contract")
        {
            return target;
        }
        let overlays = self.overlays();
        let mut dir = target.parent();
        for _ in 0..4 {
            let Some(d) = dir else { break };
            let candidate = d.join("app.contract");
            if candidate.is_file() || self.docs.contains_key(&normalize_path(&candidate)) {
                let candidate_norm = normalize_path(&candidate);
                let reaches = with_contract_texts(overlays.clone(), || {
                    let g = crate::source_graph(&candidate_norm);
                    g.sources.iter().any(|s| normalize_path(&s.path) == target)
                });
                if reaches {
                    return candidate_norm;
                }
            }
            if d.join("app.json").is_file() || self.roots.iter().any(|r| r == d) {
                break;
            }
            dir = d.parent();
        }
        target
    }

    /// Analyze `path` (reusing the cached snapshot for its compilation root).
    pub fn analyze(&mut self, path: &Path) -> Snapshot {
        let target = normalize_path(path);
        let root = self.find_root(&target);
        if let Some(cached) = self.cache.get(&root) {
            let mut snap = cached.clone();
            if root != target {
                snap.parsed = self.parse_single(&target);
            }
            return snap;
        }
        let snap = self.compute_snapshot(&root, &target, &self.overlays());
        self.cache.insert(root, snap.clone());
        snap
    }

    /// Run an ephemeral check of `path` with optional extra file overlays
    /// without mutating open editor buffers (for `exact/check`).
    pub fn check_with_overlays(&self, path: &Path, extra: Vec<(PathBuf, String)>) -> Snapshot {
        let target = normalize_path(path);
        let mut merged: HashMap<PathBuf, String> = self
            .docs
            .iter()
            .map(|(p, d)| (p.clone(), d.text.clone()))
            .collect();
        for (p, t) in extra {
            merged.insert(normalize_path(&p), t);
        }
        let overlays: Vec<(PathBuf, String)> = merged.into_iter().collect();
        let root = if Self::is_test_file(&target) {
            target.clone()
        } else {
            self.find_root(&target)
        };
        self.compute_snapshot(&root, &target, &overlays)
    }

    fn parse_single(&self, path: &Path) -> Option<File> {
        let src = self.text(path)?;
        match contract_syntax::parse_source_partial(&src, 0) {
            Ok(f) => Some(f),
            Err(partial) => partial.file.map(|f| *f),
        }
    }

    fn compute_snapshot(
        &self,
        root: &Path,
        target: &Path,
        overlays: &[(PathBuf, String)],
    ) -> Snapshot {
        let target_src = overlays
            .iter()
            .find(|(p, _)| p == target)
            .map(|(_, s)| s.clone())
            .or_else(|| self.text(target))
            .unwrap_or_default();
        let parsed = match contract_syntax::parse_source_partial(&target_src, 0) {
            Ok(f) => Some(f),
            Err(partial) => partial.file.map(|f| *f),
        };

        if Self::is_test_file(root) {
            return self.analyze_test_file(root, &target_src, parsed, overlays);
        }

        let root_src = overlays
            .iter()
            .find(|(p, _)| p == root)
            .map(|(_, s)| s.clone())
            .or_else(|| self.text(root))
            .unwrap_or_default();

        with_contract_texts(overlays.to_vec(), || {
            let graph = crate::source_graph(root);
            let mut files: Vec<PathBuf> = graph
                .sources
                .iter()
                .filter(|s| s.path.is_absolute())
                .map(|s| normalize_path(&s.path))
                .collect();
            if !files.contains(&root.to_path_buf()) {
                files.insert(0, root.to_path_buf());
            }

            let mut diagnostics: BTreeMap<PathBuf, Vec<CompileError>> = BTreeMap::new();
            for f in &files {
                diagnostics.entry(f.clone()).or_default();
            }

            let (compiled, warnings) = crate::compile_path_all_warned(root, false);
            if let Err(errors) = compiled {
                for err in errors {
                    let f = err
                        .file
                        .as_deref()
                        .map(normalize_path)
                        .unwrap_or_else(|| root.to_path_buf());
                    diagnostics.entry(f).or_default().push(err);
                }
            }
            for w in warnings {
                let f = w
                    .file
                    .as_deref()
                    .map(normalize_path)
                    .unwrap_or_else(|| root.to_path_buf());
                diagnostics.entry(f).or_default().push(w);
            }

            let app_root = root
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from("."));
            let mut refused = sources::Refused::default();
            let program = sources::load_partial(root, &root_src, &app_root, &mut refused)
                .ok()
                .map(|(f, _)| f);

            let symbols = symbols::symbol_table_source(root, &root_src).ok();

            Snapshot {
                root: root.to_path_buf(),
                parsed,
                program,
                symbols,
                diagnostics,
                files,
            }
        })
    }

    fn analyze_test_file(
        &self,
        root: &Path,
        src: &str,
        parsed: Option<File>,
        overlays: &[(PathBuf, String)],
    ) -> Snapshot {
        let mut diagnostics: BTreeMap<PathBuf, Vec<CompileError>> = BTreeMap::new();
        let entry = diagnostics.entry(root.to_path_buf()).or_default();
        match crate::tests(src).and_then(|_| crate::properties(src)) {
            Ok(props) => {
                if !props.is_empty() {
                    let paired = root.with_file_name(
                        root.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .replace(".test.contract", ".contract"),
                    );
                    let app = root.with_file_name("app.contract");
                    let app = if app.is_file() { app } else { paired };
                    if app.is_file() {
                        with_contract_texts(overlays.to_vec(), || {
                            if let Err(errs) = crate::check_properties_path(&app, &props) {
                                for mut e in errs {
                                    if e.file.is_none() {
                                        e.file = Some(root.into());
                                    }
                                    entry.push(e);
                                }
                            }
                        });
                    }
                }
            }
            Err(mut e) => {
                if e.file.is_none() {
                    e.file = Some(root.into());
                }
                entry.push(e);
            }
        }
        Snapshot {
            root: root.to_path_buf(),
            parsed,
            program: None,
            symbols: None,
            diagnostics,
            files: vec![root.to_path_buf()],
        }
    }

    /// Convert compiler errors/warnings for `path` into LSP `Diagnostic` JSON items.
    pub fn lsp_diagnostics_for(&self, path: &Path, items: &[CompileError]) -> Vec<Value> {
        let text = self.text(path).unwrap_or_default();
        items
            .iter()
            .map(|e| {
                let severity = if !e.is_warning() {
                    1
                } else if crate::information(e) {
                    3
                } else {
                    2
                };
                let mut diag = json!({
                    "range": span_to_range(&text, e.span),
                    "severity": severity,
                    "code": e.id,
                    "source": "exact-contract",
                    "message": e.message,
                });
                if !e.related.is_empty() {
                    let related: Vec<Value> = e
                        .related
                        .iter()
                        .map(|r| {
                            let rpath = r.file.as_deref().unwrap_or(path);
                            let rtext = self.text(rpath).unwrap_or_default();
                            json!({
                                "location": {
                                    "uri": path_to_uri(rpath),
                                    "range": span_to_range(&rtext, r.span),
                                },
                                "message": r.note,
                            })
                        })
                        .collect();
                    if !related.is_empty() {
                        diag["relatedInformation"] = Value::Array(related);
                    }
                }
                diag
            })
            .collect()
    }

    /// Discover `.contract` files in workspace roots (capped for fast workspace search).
    pub fn contract_files(&self) -> Vec<PathBuf> {
        let mut out: BTreeSet<PathBuf> = self.docs.keys().cloned().collect();
        for root in &self.roots {
            collect_contracts(root, &mut out, 0);
        }
        out.into_iter().collect()
    }
}

fn collect_contracts(dir: &Path, out: &mut BTreeSet<PathBuf>, depth: usize) {
    if depth > 6 || out.len() >= 512 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if matches!(
            name,
            "target" | "node_modules" | ".git" | ".shells" | "vendor" | "dist"
        ) || name.starts_with('.')
        {
            continue;
        }
        if path.is_dir() {
            collect_contracts(&path, out, depth + 1);
        } else if name.ends_with(".contract") {
            out.insert(normalize_path(&path));
            if out.len() >= 512 {
                return;
            }
        }
    }
}
