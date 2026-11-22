//! Typed symbol table extracted from `symbols.rs` for the LSP and MCP server.

use super::{authored_action_hint, partial, types, Graph, Resolver};
use crate::{sources, CompileError};
use contract_syntax::scope::Kind;
use contract_syntax::Span;
use contract_types::{Scope, Types};
use std::path::{Path, PathBuf};

/// One resolved definition in a Contract program.
#[derive(Debug, Clone)]
pub(crate) struct SymbolDef {
    pub(crate) kind: &'static str,
    pub(crate) name: String,
    pub(crate) file: PathBuf,
    pub(crate) span: Span,
    pub(crate) component: Option<String>,
    pub(crate) owner: Option<String>,
    pub(crate) writes: Option<Vec<String>>,
    pub(crate) ty: Option<String>,
}

/// One resolved reference to a [`SymbolDef`] by index in [`SymbolTable::definitions`].
#[derive(Debug, Clone)]
pub(crate) struct SymbolRef {
    pub(crate) file: PathBuf,
    pub(crate) span: Span,
    pub(crate) to: usize,
}

/// Resolved definitions, references, and checked shapes for a Contract root.
#[derive(Debug, Clone)]
pub(crate) struct SymbolTable {
    pub(crate) definitions: Vec<SymbolDef>,
    pub(crate) references: Vec<SymbolRef>,
    pub(crate) shapes: contract_types::Shapes,
}

/// Build a [`SymbolTable`] from an in-memory source string for `path`.
pub(crate) fn symbol_table_source(path: &Path, src: &str) -> Result<SymbolTable, CompileError> {
    let root = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| CompileError {
            pass: "use",
            id: "contract-use-unreadable".into(),
            message: e.to_string(),
            span: Span::default(),
            file: Some(path.into()),
            related: Box::new([]),
        })?;
    let (file, sources) = sources::load(path, src, &root).map_err(|mut all| all.swap_remove(0))?;
    let strings = crate::strings::load(&root, path).map_err(|mut all| all.swap_remove(0))?;
    let (types, expanded) = if file.components.is_empty() {
        (
            Types {
                shapes: contract_types::check_declarations(&file, strings)
                    .map_err(|e| sources.resolve(e.into()))?,
                components: Vec::new(),
            },
            None,
        )
    } else {
        let checked = contract_types::check_all(&file, false, contract_lower::tags::style, strings)
            .map_err(|mut all| {
                sources.resolve(authored_action_hint(&file, all.swap_remove(0).into()))
            })?;
        (checked.types, Some(checked.expanded))
    };
    let aliases = types::aliases(&types.shapes, true);
    let mut r = Resolver {
        file: &file,
        types: &types,
        aliases: &aliases,
        typed: true,
        left_out: Vec::new(),
        whole: true,
        graph: Graph {
            hidden: partial::shadows(&file, &[]),
            editor: true,
            ..Graph::default()
        },
        members: None,
        component: None,
        scope: Scope::default(),
        locals: Vec::new(),
        owner: None,
    };
    r.declarations();
    for import in &sources.imports {
        let kinds: &[&str] = match import.kind {
            Kind::Component => &["component"],
            Kind::Call => &["shape", "fn"],
            Kind::Style => &["style"],
            Kind::Decoration => &["decoration"],
            Kind::Atom => &["atom"],
            Kind::Keyframes => &["keyframes"],
            Kind::Timeline => &[],
        };
        if let Some(to) = r.graph.unique(kinds, &import.name, None, None) {
            r.graph.refer(import.span, to);
        }
    }
    r.file(expanded.as_ref().map(|e| &e.root));
    let definitions = r
        .graph
        .definitions
        .into_iter()
        .map(|d| SymbolDef {
            kind: d.kind,
            name: d.name,
            file: sources.path(d.span).to_path_buf(),
            span: d.span,
            component: d.component,
            owner: d.owner,
            writes: d.writes,
            ty: d.ty,
        })
        .collect();
    let references = r
        .graph
        .references
        .into_iter()
        .map(|rf| SymbolRef {
            file: sources.path(rf.span).to_path_buf(),
            span: rf.span,
            to: rf.to,
        })
        .collect();
    Ok(SymbolTable {
        definitions,
        references,
        shapes: types.shapes,
    })
}
