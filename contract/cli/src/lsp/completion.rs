//! Context-aware autocompletion and signature help for Contract.

use super::protocol::{comp_kind, line_text, parse_position, uri_to_path, utf16_to_byte_col};
use super::workspace::{Snapshot, Workspace};
use crate::functions;
use crate::vocab::lsp as vocab_lsp;
use contract_lower::vocab as lower_vocab;
use contract_syntax::{File, HOST_COMMANDS};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Default)]
struct Collector {
    items: Vec<Value>,
    seen: BTreeSet<String>,
}

impl Collector {
    fn push(&mut self, item: Value) {
        if let Some(label) = item.get("label").and_then(Value::as_str) {
            if self.seen.insert(label.to_owned()) {
                self.items.push(item);
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn finish(self) -> Value {
        json!({ "isIncomplete": false, "items": self.items })
    }
}

/// `textDocument/completion`
pub fn complete(ws: &mut Workspace, params: &Value) -> Value {
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
    let mut snap = ws.analyze(&path);
    if snap.symbols.is_none() {
        let patched: String = text
            .split('\n')
            .enumerate()
            .map(|(i, l)| if i as u32 == line0 { "" } else { l })
            .collect::<Vec<_>>()
            .join("\n");
        let recovered = ws.check_with_overlays(&path, vec![(path.clone(), patched)]);
        if recovered.symbols.is_some() {
            snap = recovered;
        }
    }
    let line = line_text(&text, line0);
    let byte_idx = utf16_to_byte_col(line, char0)
        .saturating_sub(1)
        .min(line.len() as u32) as usize;
    let prefix = &line[..byte_idx];
    let trimmed = prefix.trim_start();
    let indent = prefix.len() - trimmed.len();

    let mut out = Collector::default();

    // 1. Inside `use ... from "..."` path specifier.
    if trimmed.starts_with("use ") && trimmed.contains(" from \"") && !after_closing_quote(trimmed)
    {
        complete_use_paths(&path, &mut |i| out.push(i));
        return out.finish();
    }

    // 2. Inside `use | from "./file.contract"` exported names.
    if trimmed.starts_with("use ") && !trimmed.contains(" from ") {
        if let Some(spec) = extract_from_spec(line) {
            complete_use_exports(&path, spec, ws, &mut |i| out.push(i));
            if !out.is_empty() {
                return out.finish();
            }
        }
    }

    // 3. Cross-seam string contexts (`t("|")`, `var(--|)`, `hatch="|"`, `.test.contract` `tap "|"`).
    if super::completion_ext::complete_seam_string(ws, &path, prefix, &mut |i| out.push(i)) {
        return out.finish();
    }

    // 3b. Inside `image "symbol:|"` symbol roles.
    if let Some(q) = prefix.rfind('"') {
        let inside = &prefix[q + 1..];
        if inside.starts_with("symbol:") {
            for role in exact_kernel::generated::SYMBOL_ROLES {
                out.push(json!({
                    "label": format!("symbol:{role}"),
                    "kind": comp_kind::ENUM_MEMBER,
                    "detail": "Built-in symbol role",
                    "insertText": role,
                }));
            }
            return out.finish();
        }
    }

    // 4. Member access after `.` (`expr.|` including chained `a.b.c.`).
    if let Some(fields) =
        super::completion_ext::resolve_chained_dot_fields(prefix, &text, line0, &snap)
    {
        for (fname, fty) in fields {
            out.push(json!({
                "label": fname,
                "kind": comp_kind::FIELD,
                "detail": fty,
            }));
        }
        return out.finish();
    }
    if dot_receiver(prefix).is_some() {
        complete_dot_methods(&mut |i| out.push(i));
        return out.finish();
    }

    // 5. Attribute value position (`attr=|` or `attr="|"`).
    if let Some((attr_name, quoted)) = attr_value_context(prefix) {
        complete_attr_values(&attr_name, quoted, &snap, &text, line0, &mut |i| {
            out.push(i)
        });
        if !out.is_empty() {
            return out.finish();
        }
    }

    // 6. Type position (`: |`, `as shape |`, `option<|`).
    if is_type_position(prefix) {
        complete_types(&snap, &mut |i| out.push(i));
        super::completion_ext::complete_auto_imports(
            ws,
            &path,
            &text,
            &snap,
            false,
            true,
            &mut |i| out.push(i),
        );
        return out.finish();
    }

    // 7. Inside `ComponentName(...)` or `ShapeName(...)` named arguments.
    if let Some(callee) = call_named_arg_context(prefix) {
        complete_named_args(&callee, &snap, &mut |i| out.push(i));
        if !out.is_empty() {
            return out.finish();
        }
    }

    // 8. Top-level (column 0).
    if indent == 0 {
        complete_top_level(&mut |i| out.push(i));
        return out.finish();
    }

    // 9. Determine enclosing section from lines above.
    let section = enclosing_section(&text, line0, indent);
    match section {
        Section::Component => {
            complete_component_sections(&mut |i| out.push(i));
        }
        Section::Style | Section::Keyframes => {
            complete_style_attrs(&mut |i| out.push(i));
        }
        Section::View => {
            if is_line_head(trimmed) {
                complete_view_heads(&snap, &mut |i| out.push(i));
                super::completion_ext::complete_auto_imports(
                    ws,
                    &path,
                    &text,
                    &snap,
                    true,
                    false,
                    &mut |i| out.push(i),
                );
            } else {
                complete_element_attrs(&mut |i| out.push(i));
                complete_in_scope_values(&snap, &text, line0, &mut |i| out.push(i));
            }
        }
        Section::Action => {
            if is_line_head(trimmed) {
                complete_action_statements(&snap, &text, line0, &mut |i| out.push(i));
            }
            complete_in_scope_values(&snap, &text, line0, &mut |i| out.push(i));
            complete_stdlib(&mut |i| out.push(i));
        }
        Section::Shape | Section::Props => {
            // Field or prop name: no forced completions unless after `:` (handled above).
        }
        Section::Other => {
            complete_in_scope_values(&snap, &text, line0, &mut |i| out.push(i));
            complete_stdlib(&mut |i| out.push(i));
        }
    }

    out.finish()
}

fn after_closing_quote(s: &str) -> bool {
    s.matches('"').count() >= 2
}

fn extract_from_spec(line: &str) -> Option<&str> {
    let rest = line.split_once(" from \"")?.1;
    rest.split('"').next()
}

fn complete_use_paths(path: &Path, push: &mut impl FnMut(Value)) {
    for (spec, detail) in [
        (
            "exact:content",
            "Built-in UI parts (EmptyState, RoundButton, …)",
        ),
        ("exact:motion", "Built-in motion helpers"),
    ] {
        push(json!({
            "label": spec,
            "kind": comp_kind::MODULE,
            "detail": detail,
        }));
    }
    for pkg in super::seam::available_exact_packages(path) {
        push(json!({
            "label": pkg,
            "kind": comp_kind::MODULE,
            "detail": "Installed Exact package",
        }));
    }
    if let Some(dir) = path.parent() {
        let self_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".contract")
                    && !name.ends_with(".test.contract")
                    && name != self_name
                {
                    push(json!({
                        "label": format!("./{name}"),
                        "kind": comp_kind::FILE,
                        "detail": "Local Contract file",
                    }));
                }
            }
        }
    }
}

fn complete_use_exports(path: &Path, spec: &str, ws: &Workspace, push: &mut impl FnMut(Value)) {
    if spec == "exact:content" {
        for part in vocab_lsp::builtin_parts() {
            push(json!({
                "label": part.name,
                "kind": comp_kind::CLASS,
                "detail": format!("exact:content component — {}", part.about),
            }));
        }
        return;
    }
    let Some(target) = path.parent().map(|d| d.join(spec)) else {
        return;
    };
    let Some(src) = ws.text(&target) else {
        return;
    };
    let file = match contract_syntax::parse_source_partial(&src, 0) {
        Ok(f) => f,
        Err(p) => match p.file {
            Some(f) => *f,
            None => return,
        },
    };
    for c in &file.components {
        push(json!({ "label": c.name, "kind": comp_kind::CLASS, "detail": "component" }));
    }
    for s in &file.shapes {
        push(json!({ "label": s.name, "kind": comp_kind::STRUCT, "detail": "shape" }));
    }
    for f in &file.fns {
        push(json!({ "label": f.name, "kind": comp_kind::FUNCTION, "detail": "fn" }));
    }
    for s in &file.styles {
        push(json!({ "label": s.name, "kind": comp_kind::VALUE, "detail": "style" }));
    }
    for k in &file.keyframes {
        push(json!({ "label": k.name, "kind": comp_kind::VALUE, "detail": "keyframes" }));
    }
}

fn dot_receiver(prefix: &str) -> Option<String> {
    let trimmed = prefix.trim_end();
    // Strip partial identifier being typed after `.`.
    let before_word = trimmed.trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
    let before_dot = before_word.strip_suffix('.')?;
    let recv_start = before_dot
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .map(|i| i + 1)
        .unwrap_or(0);
    let recv = &before_dot[recv_start..];
    (!recv.is_empty()).then(|| recv.to_owned())
}

fn complete_dot_methods(push: &mut impl FnMut(Value)) {
    // Method-form stdlib functions (`length`, `filter`, `map`, `trim`, `slice`, ...).
    push(json!({
        "label": "length",
        "kind": comp_kind::PROPERTY,
        "detail": "number — list or string length",
    }));
    for f in functions::functions() {
        if contract_syntax::idioms::is_method(&f.name) {
            let sig = f.signature.as_deref().unwrap_or("");
            push(json!({
                "label": f.name,
                "kind": comp_kind::METHOD,
                "detail": sig,
                "insertText": format!("{}($0)", f.name),
                "insertTextFormat": 2,
            }));
        }
    }
}

/// Detect `attr=|` or `attr="partial|` and return `(attr_name, inside_quotes)`.
fn attr_value_context(prefix: &str) -> Option<(String, bool)> {
    let trimmed =
        prefix.trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let (before_eq, quoted) = if let Some(rest) = trimmed.strip_suffix('"') {
        (rest.trim_end().strip_suffix('=')?, true)
    } else {
        let rest = trimmed.strip_suffix('=')?;
        if rest.ends_with(['=', '!', '<', '>']) {
            return None;
        }
        (rest, false)
    };
    let before_eq = before_eq.trim_end();
    let start = before_eq
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'))
        .map(|i| i + 1)
        .unwrap_or(0);
    let name = &before_eq[start..];
    (!name.is_empty()).then(|| (name.to_owned(), quoted))
}

fn complete_attr_values(
    attr: &str,
    quoted: bool,
    snap: &Snapshot,
    text: &str,
    line0: u32,
    push: &mut impl FnMut(Value),
) {
    let file = snap.program.as_ref().or(snap.parsed.as_ref());
    // Event handler -> component's actions and action props/injects.
    if contract_analyze::HANDLERS.contains(&attr) {
        let comp_name = enclosing_component_name(text, line0);
        if let Some(f) = file {
            if let Some(c) = comp_name
                .as_deref()
                .and_then(|n| f.components.iter().find(|c| c.name == n))
                .or_else(|| f.components.first())
            {
                for a in &c.actions {
                    push(json!({
                        "label": a.name,
                        "kind": comp_kind::METHOD,
                        "detail": "action",
                    }));
                }
                for p in c.props.iter().chain(&c.injects) {
                    if p.ty.as_ref().is_some_and(|t| t.to_string() == "action") {
                        push(json!({
                            "label": p.name,
                            "kind": comp_kind::PROPERTY,
                            "detail": "action prop",
                        }));
                    }
                }
            }
        }
        return;
    }

    if attr == "class" {
        if let Some(f) = file {
            for s in &f.styles {
                push(json!({
                    "label": s.name,
                    "kind": comp_kind::CLASS,
                    "detail": "style",
                }));
            }
        }
        return;
    }
    if attr == "font-family" {
        if let Some(f) = file {
            for font in &f.fonts {
                let insert = if quoted {
                    font.name.clone()
                } else {
                    format!("\"{}\"", font.name)
                };
                push(json!({
                    "label": font.name,
                    "kind": comp_kind::VALUE,
                    "detail": "declared font",
                    "insertText": insert,
                }));
            }
        }
    }
    if matches!(
        attr,
        "animation-name" | "animation" | "-exact-exit-animation"
    ) {
        if let Some(f) = file {
            for k in &f.keyframes {
                let insert = if quoted {
                    k.name.clone()
                } else {
                    format!("\"{}\"", k.name)
                };
                push(json!({
                    "label": k.name,
                    "kind": comp_kind::VALUE,
                    "detail": "keyframes",
                    "insertText": insert,
                }));
            }
        }
    }
    if attr == "animation-timeline" {
        if let Some(f) = file {
            for t in &f.timelines {
                push(json!({
                    "label": t.name,
                    "kind": comp_kind::VALUE,
                    "detail": "timeline",
                }));
            }
        }
    }

    for val in vocab_lsp::attr_values(attr) {
        let is_bool = val == "true" || val == "false";
        let is_color = val.starts_with("-exact-");
        let insert = if quoted || is_bool {
            val.clone()
        } else {
            format!("\"{val}\"")
        };
        push(json!({
            "label": val,
            "kind": if is_color { comp_kind::COLOR } else { comp_kind::ENUM_MEMBER },
            "detail": format!("value of {attr}"),
            "insertText": insert,
        }));
    }
}

fn is_type_position(prefix: &str) -> bool {
    let trimmed = prefix.trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_');
    let trimmed = trimmed.trim_end();
    trimmed.ends_with(':')
        || trimmed.ends_with("as shape")
        || trimmed.ends_with("option<")
        || trimmed.ends_with("enum<")
}

fn complete_types(snap: &Snapshot, push: &mut impl FnMut(Value)) {
    for (name, detail, insert) in [
        ("string", "UTF-8 string", "string"),
        ("number", "IEEE 754 number", "number"),
        ("bool", "boolean", "bool"),
        ("action", "action reference prop", "action"),
        ("unit", "empty result", "unit"),
        (
            "option",
            "optional value `option<T>`",
            "option<${1:string}>",
        ),
        (
            "enum",
            "finite string vocabulary `enum<\"a\", \"b\">`",
            "enum<\"${1:a}\", \"${2:b}\">",
        ),
    ] {
        let snippet = insert.contains('$');
        push(json!({
            "label": name,
            "kind": comp_kind::KEYWORD,
            "detail": detail,
            "insertText": insert,
            "insertTextFormat": if snippet { 2 } else { 1 },
        }));
    }
    if let Some(file) = snap.program.as_ref().or(snap.parsed.as_ref()) {
        for s in &file.shapes {
            push(json!({
                "label": s.name,
                "kind": comp_kind::STRUCT,
                "detail": format!("shape ({} fields)", s.fields.len()),
            }));
        }
    }
    for (name, fields) in vocab_lsp::event_records() {
        push(json!({
            "label": name,
            "kind": comp_kind::STRUCT,
            "detail": format!("built-in record ({} fields)", fields.len()),
        }));
    }
}

/// Detect `Callee(arg1=1, |` returning `Some("Callee")` when cursor is in a
/// capitalized call's argument position (component use or shape constructor).
fn call_named_arg_context(prefix: &str) -> Option<String> {
    let mut depth = 0i32;
    let mut open_pos = None;
    for (idx, ch) in prefix.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                if depth == 0 {
                    open_pos = Some(idx);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    let open = open_pos?;
    let before = prefix[..open].trim_end();
    let start = before
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .map(|i| i + 1)
        .unwrap_or(0);
    let callee = &before[start..];
    if !callee
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
    {
        return None;
    }
    // Only suggest argument names before `=`, not while typing a value after `=`.
    let inside = &prefix[open + 1..];
    let after_comma = inside.rsplit(',').next().unwrap_or(inside);
    if after_comma.contains('=') {
        return None;
    }
    Some(callee.to_owned())
}

fn complete_named_args(callee: &str, snap: &Snapshot, push: &mut impl FnMut(Value)) {
    let Some(file) = snap.program.as_ref().or(snap.parsed.as_ref()) else {
        return;
    };
    if let Some(c) = file.components.iter().find(|c| c.name == callee) {
        for p in &c.props {
            let ty = p.ty.as_ref().map(|t| t.to_string()).unwrap_or_default();
            push(json!({
                "label": p.name,
                "kind": comp_kind::PROPERTY,
                "detail": format!("prop: {ty}"),
                "insertText": format!("{}=", p.name),
            }));
        }
    }
    if let Some(s) = file.shapes.iter().find(|s| s.name == callee) {
        for f in &s.fields {
            push(json!({
                "label": f.name,
                "kind": comp_kind::FIELD,
                "detail": format!("field: {}", f.ty),
                "insertText": format!("{}=", f.name),
            }));
        }
    }
}

fn complete_top_level(push: &mut impl FnMut(Value)) {
    for (label, detail, snippet) in [
        (
            "component",
            "Declare a UI component",
            "component ${1:Name}\n  view\n    ${0:column}",
        ),
        (
            "shape",
            "Declare a record shape",
            "shape ${1:Name}\n  ${2:id}: ${0:string}",
        ),
        (
            "fn",
            "Declare a pure inline function",
            "fn ${1:name}(${2:x}: ${3:number}): ${4:string} = ${0}",
        ),
        (
            "style",
            "Declare a named style",
            "style ${1:Name}\n  ${0:padding=12}",
        ),
        (
            "decoration",
            "Declare textarea decoration paint",
            "decoration ${1:Name}\n  ${0:color=\"-exact-accent\"}",
        ),
        (
            "atom",
            "Declare a Markdown inline atom",
            "atom ${1:Name} ${0:background-color=\"-exact-secondary-background\"}",
        ),
        (
            "keyframes",
            "Declare CSS keyframes",
            "keyframes ${1:Name}\n  from opacity=0\n  to opacity=1",
        ),
        ("timeline", "Declare a clock timeline", "timeline ${1:Name}"),
        (
            "routes",
            "Declare the app route table",
            "routes ${1:nav}\n  tab ${2:home} \"/\"\n  notfound",
        ),
        (
            "use",
            "Import declarations from another file or module",
            "use ${1:Name} from \"${2:./file.contract}\"",
        ),
        (
            "members",
            "Split component members into a file",
            "members ${1:App}\n  ${0}",
        ),
        (
            "font",
            "Declare a bundled font family",
            "font \"${1:Family}\" = \"${2:assets/font.ttf}\"",
        ),
        (
            "sound",
            "Declare a bundled WAV sound",
            "sound \"${1:assets/clip.wav}\"",
        ),
        (
            "theme",
            "Declare inline theme tokens",
            "theme\n  ${1:accent} = \"${2:#007aff}\"",
        ),
        (
            "color-profile",
            "Declare a CSS color profile",
            "color-profile --${1:name}\n  src = \"${2:assets/profile.icc}\"",
        ),
        (
            "test",
            "Declare an authored end-to-end test",
            "test \"${1:name}\"\n  ${0:expect text \"Hello\"}",
        ),
        (
            "always",
            "Declare an invariant property",
            "always \"${1:name}\"\n  ${0}",
        ),
        (
            "after",
            "Declare a post-action property",
            "after ${1:action} \"${2:name}\"\n  ${0}",
        ),
    ] {
        push(json!({
            "label": label,
            "kind": comp_kind::SNIPPET,
            "detail": detail,
            "insertText": snippet,
            "insertTextFormat": 2,
        }));
    }
}

fn complete_component_sections(push: &mut impl FnMut(Value)) {
    for (label, detail, snippet) in [
        (
            "props",
            "Typed component inputs",
            "props\n  ${1:title}: ${0:string}",
        ),
        (
            "inject",
            "Values injected from an ancestor's provide",
            "inject\n  ${1:name}: ${0:string}",
        ),
        (
            "provide",
            "Provide values to descendant injects",
            "provide\n  ${0:name}",
        ),
        ("slot", "Accept child nodes rendered via `children`", "slot"),
        (
            "state",
            "Mutable component state slot",
            "state ${1:name} = ${0:0}",
        ),
        ("derive", "Pure derived value", "derive ${1:name} = ${0}"),
        (
            "resource",
            "Async data resource",
            "resource ${1:data} = ${2:load}() as shape ${3:Data}\n  else empty()",
        ),
        (
            "mutation",
            "Async mutation slot",
            "mutation ${1:save} as shape ${2:Result}",
        ),
        (
            "action",
            "State-updating action",
            "action ${1:handle}()\n  ${0}",
        ),
        (
            "task",
            "Scheduled timer task",
            "task ${1:ticker} mount\n  every(${2:1000}, ${0:tick})",
        ),
        (
            "sequence",
            "Scheduled audio/MIDI/haptic sequence",
            "sequence ${1:seq} when ${2:playing}\n  play(${0:tracks}, bpm=120)",
        ),
        (
            "members",
            "Include a members file",
            "members \"${1:./part.contract}\"",
        ),
        ("view", "Component view tree", "view\n  ${0:column}"),
    ] {
        let is_snip = snippet.contains('$');
        push(json!({
            "label": label,
            "kind": if is_snip { comp_kind::SNIPPET } else { comp_kind::KEYWORD },
            "detail": detail,
            "insertText": snippet,
            "insertTextFormat": if is_snip { 2 } else { 1 },
        }));
    }
}

fn complete_view_heads(snap: &Snapshot, push: &mut impl FnMut(Value)) {
    // Control constructs and pseudo-parts.
    for (label, detail, snippet) in [
        ("if", "Conditional view branch", "if ${1:cond}\n  ${0}"),
        ("else", "Fallback view branch", "else\n  ${0}"),
        (
            "for",
            "Keyed list iteration",
            "for ${1:item} in ${2:items} key=${3:item.id}\n  ${0}",
        ),
        (
            "match",
            "Exhaustive option match",
            "match ${1:opt}\n  case some(${2:v})\n    ${3}\n  case none\n    ${0}",
        ),
        ("case", "Match arm", "case ${1:some(v)}\n  ${0}"),
        ("children", "Render component slot children", "children"),
        (
            "::placeholder",
            "Input placeholder color block",
            "::placeholder\n  color=\"${0:-exact-placeholder-text}\"",
        ),
        (
            ":active",
            "Pressed state paint block",
            ":active\n  opacity=${0:0.7}",
        ),
        (
            ":hover",
            "Hover state paint block",
            ":hover\n  background-color=\"${0:-exact-secondary-background}\"",
        ),
        (
            ":focus-visible",
            "Focus ring paint block",
            ":focus-visible\n  border-color=\"${0:-exact-accent}\"",
        ),
        (
            ":disabled",
            "Disabled state paint block",
            ":disabled\n  opacity=${0:0.5}",
        ),
        (
            ":checked",
            "Checked state paint block",
            ":checked\n  background-color=\"${0:-exact-accent}\"",
        ),
    ] {
        let is_snip = snippet.contains('$');
        push(json!({
            "label": label,
            "kind": if is_snip { comp_kind::SNIPPET } else { comp_kind::KEYWORD },
            "detail": detail,
            "insertText": snippet,
            "insertTextFormat": if is_snip { 2 } else { 1 },
        }));
    }
    // Declared/imported components.
    if let Some(file) = snap.program.as_ref().or(snap.parsed.as_ref()) {
        for c in &file.components {
            let props: Vec<String> = c
                .props
                .iter()
                .enumerate()
                .map(|(i, p)| format!("{}=${{{}}}", p.name, i + 1))
                .collect();
            let snippet = format!("{}({})", c.name, props.join(", "));
            push(json!({
                "label": c.name,
                "kind": comp_kind::CLASS,
                "detail": format!("component ({} props)", c.props.len()),
                "insertText": snippet,
                "insertTextFormat": 2,
            }));
        }
    }
    // Built-in tags + h1..h6 headings.
    for (name, tag) in lower_vocab::tags() {
        push(json!({
            "label": name,
            "kind": comp_kind::CLASS,
            "detail": vocab_lsp::tag_summary(&tag),
        }));
    }
    for level in 1..=6 {
        push(json!({
            "label": format!("h{level}"),
            "kind": comp_kind::CLASS,
            "detail": format!("Heading level {level}"),
        }));
    }
}

fn complete_style_attrs(push: &mut impl FnMut(Value)) {
    for (name, target) in lower_vocab::attrs() {
        if matches!(
            target,
            contract_lower::tags::AttrTarget::Styles(_)
                | contract_lower::tags::AttrTarget::Flex
                | contract_lower::tags::AttrTarget::Shorthand
        ) {
            push(json!({
                "label": name,
                "kind": comp_kind::PROPERTY,
                "detail": vocab_lsp::attr_summary(name, &target),
                "insertText": format!("{name}="),
            }));
        }
    }
}

fn complete_element_attrs(push: &mut impl FnMut(Value)) {
    for (name, target) in lower_vocab::attrs() {
        let is_event = matches!(target, contract_lower::tags::AttrTarget::Handler(_));
        push(json!({
            "label": name,
            "kind": if is_event { comp_kind::EVENT } else { comp_kind::PROPERTY },
            "detail": vocab_lsp::attr_summary(name, &target),
            "insertText": format!("{name}="),
        }));
    }
}

fn complete_action_statements(
    snap: &Snapshot,
    text: &str,
    line0: u32,
    push: &mut impl FnMut(Value),
) {
    for (label, detail, snippet) in [
        ("let", "Local variable binding", "let ${1:name} = ${0}"),
        (
            "send",
            "Dispatch a mutation",
            "send ${1:mutation} = ${2:source}(${0})",
        ),
        (
            "refresh",
            "Force-refresh a resource",
            "refresh ${0:resource}",
        ),
        ("if", "Conditional statement", "if ${1:cond}\n  ${0}"),
        ("else", "Else branch", "else\n  ${0}"),
        (
            "match",
            "Match option value",
            "match ${1:opt}\n  case some(${2:v})\n    ${3}\n  case none\n    ${0}",
        ),
    ] {
        push(json!({
            "label": label,
            "kind": comp_kind::SNIPPET,
            "detail": detail,
            "insertText": snippet,
            "insertTextFormat": 2,
        }));
    }
    for &cmd in HOST_COMMANDS {
        push(json!({
            "label": cmd,
            "kind": comp_kind::FUNCTION,
            "detail": vocab_lsp::command_doc(cmd).unwrap_or_else(|| "host command".into()),
            "insertText": format!("{cmd}($0)"),
            "insertTextFormat": 2,
        }));
    }
    let _ = (snap, text, line0);
}

fn complete_in_scope_values(snap: &Snapshot, text: &str, line0: u32, push: &mut impl FnMut(Value)) {
    let comp_name = enclosing_component_name(text, line0);
    if let Some(table) = &snap.symbols {
        for def in &table.definitions {
            let visible = match (&def.component, &comp_name) {
                (None, _) => matches!(def.kind, "fn" | "shape" | "style"),
                (Some(dc), Some(cc)) => {
                    dc == cc && !matches!(def.kind, "id" | "testId" | "provide")
                }
                _ => false,
            };
            if !visible {
                continue;
            }
            let kind = match def.kind {
                "fn" => comp_kind::FUNCTION,
                "action" => comp_kind::METHOD,
                "shape" => comp_kind::STRUCT,
                "style" => comp_kind::CLASS,
                "prop" | "inject" | "field" => comp_kind::PROPERTY,
                _ => comp_kind::VARIABLE,
            };
            let detail = match &def.ty {
                Some(ty) => format!("{}: {ty}", def.kind),
                None => def.kind.to_owned(),
            };
            push(json!({
                "label": def.name,
                "kind": kind,
                "detail": detail,
            }));
        }
    } else if let Some(file) = snap.parsed.as_ref() {
        for f in &file.fns {
            push(json!({ "label": f.name, "kind": comp_kind::FUNCTION, "detail": "fn" }));
        }
        for s in &file.shapes {
            push(json!({ "label": s.name, "kind": comp_kind::STRUCT, "detail": "shape" }));
        }
        if let Some(c) = comp_name
            .as_deref()
            .and_then(|n| file.components.iter().find(|c| c.name == n))
            .or_else(|| file.components.first())
        {
            for p in c.props.iter().chain(&c.injects) {
                push(json!({ "label": p.name, "kind": comp_kind::PROPERTY, "detail": "prop" }));
            }
            for s in &c.states {
                push(json!({ "label": s.name, "kind": comp_kind::VARIABLE, "detail": "state" }));
            }
            for d in &c.derives {
                push(json!({ "label": d.name, "kind": comp_kind::VARIABLE, "detail": "derive" }));
            }
            for r in &c.resources {
                push(json!({ "label": r.name, "kind": comp_kind::VARIABLE, "detail": "resource" }));
            }
            for m in &c.mutations {
                push(json!({ "label": m.name, "kind": comp_kind::VARIABLE, "detail": "mutation" }));
            }
            for a in &c.actions {
                push(json!({ "label": a.name, "kind": comp_kind::METHOD, "detail": "action" }));
            }
        }
    }
    for (lit, detail) in [
        ("true", "bool"),
        ("false", "bool"),
        ("none", "option<T>"),
        ("some", "some(value)"),
    ] {
        push(json!({ "label": lit, "kind": comp_kind::CONSTANT, "detail": detail }));
    }
}

fn complete_stdlib(push: &mut impl FnMut(Value)) {
    for f in functions::functions() {
        if contract_syntax::idioms::is_method(&f.name) {
            continue;
        }
        let sig = f
            .signature
            .clone()
            .unwrap_or_else(|| format!("{}(…)", f.name));
        push(json!({
            "label": f.name,
            "kind": comp_kind::FUNCTION,
            "detail": sig,
            "insertText": format!("{}($0)", f.name),
            "insertTextFormat": 2,
        }));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Component,
    View,
    Action,
    Style,
    Keyframes,
    Shape,
    Props,
    Other,
}

fn is_line_head(trimmed: &str) -> bool {
    !trimmed.contains(' ') && !trimmed.contains('=') && !trimmed.contains('(')
}

fn enclosing_section(text: &str, line0: u32, cur_indent: usize) -> Section {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = line0 as usize;
    let mut min_indent = cur_indent;
    while i > 0 {
        i -= 1;
        let raw = lines[i];
        let trimmed = raw.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let ind = raw.len() - trimmed.len();
        if ind < min_indent {
            min_indent = ind;
            let head = trimmed.split_whitespace().next().unwrap_or("");
            match head {
                "view" => return Section::View,
                "action" => return Section::Action,
                "style" | "decoration" | "atom" => return Section::Style,
                "keyframes" => return Section::Keyframes,
                "shape" => return Section::Shape,
                "props" | "inject" => return Section::Props,
                "component" | "members" => return Section::Component,
                _ => {}
            }
            if ind == 0 {
                break;
            }
        }
    }
    Section::Other
}

fn enclosing_component_name(text: &str, line0: u32) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = (line0 as usize).min(lines.len().saturating_sub(1));
    loop {
        let raw = lines.get(i).copied().unwrap_or("");
        if let Some(rest) = raw
            .strip_prefix("component ")
            .or_else(|| raw.strip_prefix("members "))
        {
            return rest.split_whitespace().next().map(str::to_owned);
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    None
}

/// `textDocument/signatureHelp`
pub fn signature_help(ws: &mut Workspace, params: &Value) -> Value {
    let Some(path) = params
        .pointer("/textDocument/uri")
        .and_then(Value::as_str)
        .and_then(uri_to_path)
    else {
        return Value::Null;
    };
    let Some((line0, char0)) = parse_position(params.get("position")) else {
        return Value::Null;
    };
    let Some(text) = ws.text(&path) else {
        return Value::Null;
    };
    let line = line_text(&text, line0);
    let byte_idx = utf16_to_byte_col(line, char0)
        .saturating_sub(1)
        .min(line.len() as u32) as usize;
    let prefix = &line[..byte_idx];

    // Walk backward to the nearest enclosing unclosed `(`.
    let mut depth = 0i32;
    let mut commas = 0u32;
    let mut open_pos = None;
    for (idx, ch) in prefix.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => {
                if depth == 0 {
                    open_pos = Some(idx);
                    break;
                }
                depth -= 1;
            }
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    let Some(open) = open_pos else {
        return Value::Null;
    };
    let before = prefix[..open].trim_end();
    let start = before
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .map(|i| i + 1)
        .unwrap_or(0);
    let callee = &before[start..];
    if callee.is_empty() {
        return Value::Null;
    }

    let mut snap = ws.analyze(&path);
    if snap.symbols.is_none() {
        let patched: String = text
            .split('\n')
            .enumerate()
            .map(|(i, l)| if i as u32 == line0 { "" } else { l })
            .collect::<Vec<_>>()
            .join("\n");
        let recovered = ws.check_with_overlays(&path, vec![(path.clone(), patched)]);
        if recovered.symbols.is_some() {
            snap = recovered;
        }
    }
    let file = snap.program.as_ref().or(snap.parsed.as_ref());

    if let Some(sig) = resolve_signature(callee, file, &snap, &text, line0) {
        return json!({
            "signatures": [sig],
            "activeSignature": 0,
            "activeParameter": commas,
        });
    }
    Value::Null
}

fn resolve_signature(
    callee: &str,
    file: Option<&File>,
    snap: &Snapshot,
    text: &str,
    line0: u32,
) -> Option<Value> {
    if let Some(f) = file {
        if let Some(c) = f.components.iter().find(|c| c.name == callee) {
            let params: Vec<Value> = c
                .props
                .iter()
                .map(|p| {
                    let ty =
                        p.ty.as_ref()
                            .map(|t| t.to_string())
                            .unwrap_or_else(|| "?".into());
                    json!({ "label": format!("{}: {ty}", p.name) })
                })
                .collect();
            let labels: Vec<&str> = params.iter().filter_map(|p| p["label"].as_str()).collect();
            return Some(json!({
                "label": format!("{}({})", c.name, labels.join(", ")),
                "documentation": "Component use",
                "parameters": params,
            }));
        }
        if let Some(s) = f.shapes.iter().find(|s| s.name == callee) {
            let params: Vec<Value> = s
                .fields
                .iter()
                .map(|fld| json!({ "label": format!("{}: {}", fld.name, fld.ty) }))
                .collect();
            let labels: Vec<&str> = params.iter().filter_map(|p| p["label"].as_str()).collect();
            return Some(json!({
                "label": format!("{}({})", s.name, labels.join(", ")),
                "documentation": "Shape record constructor",
                "parameters": params,
            }));
        }
        if let Some(fn_decl) = f.fns.iter().find(|d| d.name == callee) {
            let params: Vec<Value> = fn_decl
                .params
                .iter()
                .map(|p| {
                    let ty =
                        p.ty.as_ref()
                            .map(|t| t.to_string())
                            .unwrap_or_else(|| "?".into());
                    json!({ "label": format!("{}: {ty}", p.name) })
                })
                .collect();
            let labels: Vec<&str> = params.iter().filter_map(|p| p["label"].as_str()).collect();
            return Some(json!({
                "label": format!("fn {}({}): {}", fn_decl.name, labels.join(", "), fn_decl.ret),
                "parameters": params,
            }));
        }
        if let Some(comp_name) = enclosing_component_name(text, line0) {
            if let Some(c) = f.components.iter().find(|c| c.name == comp_name) {
                if let Some(a) = c.actions.iter().find(|a| a.name == callee) {
                    let params: Vec<Value> = a
                        .params
                        .iter()
                        .map(|p| {
                            let ty = p
                                .ty
                                .as_ref()
                                .map(|t| t.to_string())
                                .or_else(|| {
                                    snap.symbols.as_ref().and_then(|t| {
                                        t.definitions
                                            .iter()
                                            .find(|d| {
                                                d.kind == "parameter"
                                                    && d.name == p.name
                                                    && d.owner.as_deref() == Some(a.name.as_str())
                                            })
                                            .and_then(|d| d.ty.clone())
                                    })
                                })
                                .unwrap_or_else(|| "?".into());
                            json!({ "label": format!("{}: {ty}", p.name) })
                        })
                        .collect();
                    let labels: Vec<&str> =
                        params.iter().filter_map(|p| p["label"].as_str()).collect();
                    return Some(json!({
                        "label": format!("action {}({})", a.name, labels.join(", ")),
                        "parameters": params,
                    }));
                }
            }
        }
    }
    if let Some(func) = functions::function(callee) {
        let label = func
            .signature
            .clone()
            .unwrap_or_else(|| format!("{callee}(…)"));
        let doc = func
            .rows
            .iter()
            .map(|r| format!("{}: {}", r.call, r.rule))
            .collect::<Vec<_>>()
            .join("\n");
        return Some(json!({
            "label": label,
            "documentation": { "kind": "markdown", "value": doc },
        }));
    }
    if let Some(doc) = vocab_lsp::command_doc(callee) {
        return Some(json!({
            "label": format!("{callee}(…)"),
            "documentation": { "kind": "markdown", "value": doc },
        }));
    }
    None
}
