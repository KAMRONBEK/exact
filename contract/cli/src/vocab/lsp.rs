//! Language-server vocabulary queries over the compiler's live tables.

use super::*;

/// Built-in part summary: `(name, module, props, description)`.
pub struct PartInfo {
    /// Component name.
    pub name: String,
    /// Built-in module specifier, e.g. `exact:content`.
    pub module: &'static str,
    /// Prop lines as written in the built-in contract source.
    pub props: Vec<String>,
    /// Prose summary.
    pub about: String,
    /// Example call, when present.
    pub call: Option<String>,
}

/// Look up one vocabulary name as JSON, returning `(json, known)`.
pub fn lookup_json(name: &str) -> (Value, bool) {
    one_json(name)
}

/// Full compiler vocabulary snapshot as JSON.
pub fn all_json() -> Value {
    everything()
}

/// Compact ranked vocabulary search across tags, attributes, host commands,
/// standard functions, built-in parts, concepts, recipes, and patterns.
pub fn search_json(query: &str, limit: usize) -> Value {
    let q = query.trim().to_lowercase();
    let terms: Vec<&str> = q.split_whitespace().filter(|t| !t.is_empty()).collect();
    if terms.is_empty() {
        return json!({ "query": query, "matches": [] });
    }
    let score = |name: &str, text: &str| -> u32 {
        let name_l = name.to_lowercase();
        let text_l = text.to_lowercase();
        let mut s = 0u32;
        if name_l == q {
            s += 100;
        } else if name_l.starts_with(&q) {
            s += 60;
        } else if name_l.contains(&q) {
            s += 35;
        }
        for t in &terms {
            if name_l == *t {
                s += 40;
            } else if name_l.contains(t) {
                s += 20;
            } else if text_l.contains(t) {
                s += 8;
            }
        }
        s
    };

    let mut ranked: Vec<(u32, Value)> = Vec::new();
    for (name, tag) in vocab::tags() {
        let detail = tag_text(&tag);
        let note = vocab::tag_note(name).unwrap_or("");
        let sc = score(name, &format!("{detail} {note}"));
        if sc > 0 {
            ranked.push((
                sc,
                json!({ "kind": "tag", "name": name, "summary": detail }),
            ));
        }
    }
    for (name, attr) in vocab::attrs() {
        let detail = attr_summary(name, &attr);
        let sc = score(name, &detail);
        if sc > 0 {
            ranked.push((
                sc,
                json!({ "kind": kind(&attr), "name": name, "summary": detail }),
            ));
        }
    }
    for &cmd in HOST_COMMANDS {
        let doc = command_text(cmd);
        let sc = score(cmd, &doc);
        if sc > 0 {
            ranked.push((
                sc,
                json!({ "kind": "command", "name": cmd, "summary": doc }),
            ));
        }
    }
    for f in functions::functions() {
        let sig = f
            .signature
            .clone()
            .unwrap_or_else(|| format!("{}(…)", f.name));
        let rules: String = f
            .rows
            .iter()
            .map(|r| r.rule.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let sc = score(&f.name, &format!("{sig} {rules}"));
        if sc > 0 {
            ranked.push((
                sc,
                json!({ "kind": "function", "name": f.name, "summary": sig }),
            ));
        }
    }
    for c in concepts::concepts() {
        let joined = format!("{} {} {}", c.title, c.names.join(" "), c.answer);
        let sc = score(c.name, &joined);
        if sc > 0 {
            ranked.push((
                sc,
                json!({ "kind": "concept", "name": c.name, "title": c.title, "summary": c.answer }),
            ));
        }
    }
    for &(name, _) in recipes::RECIPES {
        let sc = score(name, name);
        if sc > 0 {
            ranked.push((
                sc,
                json!({ "kind": "recipe", "name": name, "path": format!("docs/recipes/{name}.md") }),
            ));
        }
    }
    for &(name, about) in recipes::PATTERNS {
        let sc = score(name, about);
        if sc > 0 {
            ranked.push((
                sc,
                json!({ "kind": "pattern", "name": name, "summary": about }),
            ));
        }
    }
    ranked.sort_by_key(|b| std::cmp::Reverse(b.0));
    let cap = limit.clamp(1, 50);
    let matches: Vec<Value> = ranked.into_iter().take(cap).map(|(_, v)| v).collect();
    json!({ "query": query, "count": matches.len(), "matches": matches })
}

/// Format rich Markdown hover documentation for any built-in vocabulary name
/// (tag, attribute, CSS property, event, host command, standard function,
/// event record, built-in part, CSS value, or heading `h1`–`h6`).
pub fn hover_markdown(name: &str) -> Option<String> {
    if let Some(level) = name
        .strip_prefix('h')
        .and_then(|n| n.parse::<u8>().ok())
        .filter(|n| (1..=6).contains(n))
    {
        return Some(format!(
            "```contract\n{name} \"Title\"\n```\nHeading of level {level} (`text \"Title\" role=\"heading\" aria-level={level}`)."
        ));
    }
    if let Some(e) = event(name) {
        let mut md = format!("```contract\nshape {name}\n");
        for (f, t) in &e.fields {
            md.push_str(&format!("  {f}: {t}\n"));
        }
        md.push_str("```\nBuilt-in event record.\n");
        for (_, calls) in &e.handlers {
            md.push_str(&format!("\n- {calls}"));
        }
        return Some(md);
    }
    let e = entry(name)?;
    let mut sections: Vec<String> = Vec::new();
    if let Some(p) = &e.part {
        let mut block = format!(
            "```contract\nuse {name} from \"{}\"\n```\nBuilt-in part of `{}`.\n\n{}",
            p.module, p.module, p.about
        );
        if !p.props.is_empty() {
            block.push_str("\n\n**Props:** ");
            block.push_str(&p.props.join(", "));
        }
        if let Some(call) = &p.call {
            block.push_str(&format!("\n\n```contract\n{call}\n```"));
        }
        sections.push(block);
    }
    if let Some(t) = tags::tag(name) {
        let mut block = format!("```contract\n{name}\n```\nTag — {}", tag_text(&t));
        if let Some(note) = vocab::tag_note(name) {
            block.push_str(&format!("\n\n{note}"));
        }
        sections.push(block);
    }
    if let Some(a) = tags::attr(name) {
        let mut block = format!("**`{name}`** ({} attribute)", kind(&a));
        for line in attr_detail(name, &a) {
            block.push_str(&format!("\n- {line}"));
        }
        if let Some(note) = frames::note(name) {
            block.push_str(&format!("\n\n{note}"));
        }
        if let Some(only) = only_on(name) {
            block.push_str(&format!("\n- Only on {only}"));
        }
        sections.push(block);
    }
    if let Some(cmd) = &e.command {
        sections.push(format!("```contract\n{name}(…)\n```\n{cmd}"));
    }
    if let Some(f) = &e.function {
        let mut block = match &f.signature {
            Some(sig) => format!("```contract\n{sig}\n```\nStandard function."),
            None => format!("```contract\n{name}(…)\n```\nCompiler intrinsic."),
        };
        for row in &f.rows {
            block.push_str(&format!("\n- {}: {}", row.call, row.rule));
            if !row.example.is_empty() {
                block.push_str(&format!(" — {}", row.example));
            }
        }
        if routed(f) {
            block.push_str("\n- Requires a `routes` declaration.");
        }
        sections.push(block);
    }
    if !e.value_of.is_empty() && sections.is_empty() {
        let of: Vec<String> = e.value_of.iter().map(|n| format!("`{n}`")).collect();
        sections.push(format!("CSS value admitted by {}.", of.join(", ")));
    }
    if sections.is_empty() {
        if let Some(open) = e.open {
            sections.push(open.to_owned());
        } else if let Some(c) = &e.concept {
            sections.push(format!("**{}** — {}", c.title, c.answer));
        }
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n---\n\n"))
    }
}

/// Short one-line detail for a built-in attribute (for completion lists).
pub fn attr_summary(name: &str, a: &AttrTarget) -> String {
    match a {
        AttrTarget::Styles(_) | AttrTarget::Flex | AttrTarget::Shorthand => {
            let rows = rows_of(name, a);
            rows.first()
                .map(|r| row_text(*r))
                .unwrap_or_else(|| "style".into())
        }
        AttrTarget::Prop(p) => format!("prop ({})", prop_type(*p)),
        AttrTarget::InvertedBoolProp(p) => format!("bool prop (inverse of {})", p.name()),
        AttrTarget::Handler(ev) => contract_types::handler_signature(name)
            .unwrap_or_else(|| format!("event handler ({ev})")),
        AttrTarget::Surface => "canvas surface binding".into(),
        AttrTarget::MediaMetadata => "record MediaMetadata".into(),
        AttrTarget::DataTransfer => "record DataTransfer".into(),
    }
}

/// Short one-line detail for a built-in tag (for completion lists).
pub fn tag_summary(t: &Tag) -> String {
    tag_text(t)
}

/// Accepted literal values for a built-in attribute, for value completion.
pub fn attr_values(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    match name {
        "buttonStyle" => {
            return exact_kernel::generated::BUTTON_STYLES
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
        }
        "navigationBar" => return vec!["auto".into(), "none".into(), "transparent".into()],
        "navigationPresentation" => return vec!["modal".into(), "fullScreen".into()],
        "navigationGrabber" => return vec!["auto".into(), "visible".into(), "hidden".into()],
        "navigationSizing" => return vec!["page".into(), "form".into()],
        "navigationDetent" => {
            return vec![
                "medium".into(),
                "large".into(),
                "medium large".into(),
                "fit-content".into(),
            ]
        }
        "navigationHistory" => {
            return vec![
                "back forward".into(),
                "back".into(),
                "forward".into(),
                "none".into(),
            ]
        }
        "listStyle" => {
            return vec!["inset-grouped".into(), "grouped".into(), "plain".into()];
        }
        "markup" => return vec!["markdown".into(), "none".into()],
        "wakelock" => return vec!["screen".into(), "none".into()],
        "orientationlock" => {
            return vec![
                "none".into(),
                "current".into(),
                "portrait".into(),
                "landscape".into(),
                "portrait-primary".into(),
                "portrait-secondary".into(),
                "landscape-primary".into(),
                "landscape-secondary".into(),
                "natural".into(),
                "any".into(),
            ];
        }
        "color-scheme" => return vec!["light".into(), "dark".into()],
        "appearance" => return vec!["auto".into(), "none".into()],
        "popover" => return vec!["auto".into(), "manual".into()],
        "role" => {
            return vec![
                "heading".into(),
                "tablist".into(),
                "tab".into(),
                "tabpanel".into(),
                "menu".into(),
                "menuitem".into(),
                "alertdialog".into(),
                "status".into(),
                "alert".into(),
                "region".into(),
                "button".into(),
                "list".into(),
                "listitem".into(),
                "separator".into(),
                "search".into(),
                "navigation".into(),
                "dialog".into(),
            ];
        }
        "type" => {
            return vec![
                "text".into(),
                "search".into(),
                "email".into(),
                "password".into(),
                "number".into(),
                "tel".into(),
                "url".into(),
                "checkbox".into(),
                "radio".into(),
                "range".into(),
                "date".into(),
                "time".into(),
                "datetime-local".into(),
                "color".into(),
                "file".into(),
            ];
        }
        "format" => {
            return vec![
                "relative".into(),
                "timer".into(),
                "countdown".into(),
                "date-short".into(),
                "date-medium".into(),
                "date-long".into(),
                "date-full".into(),
                "time-short".into(),
                "time-medium".into(),
            ];
        }
        "inputmode" => {
            return vec![
                "none".into(),
                "text".into(),
                "decimal".into(),
                "numeric".into(),
                "tel".into(),
                "search".into(),
                "email".into(),
                "url".into(),
            ];
        }
        "autocapitalize" => {
            return vec![
                "none".into(),
                "sentences".into(),
                "words".into(),
                "characters".into(),
            ];
        }
        "-exact-fit" => return vec!["shrink".into(), "none".into()],
        "-exact-return-presses" => return vec!["descendants".into(), "none".into()],
        "-exact-menu" => {
            return vec![
                "app".into(),
                "file".into(),
                "edit".into(),
                "view".into(),
                "go".into(),
                "window".into(),
                "help".into(),
                "about".into(),
                "status".into(),
                "dock".into(),
                "none".into(),
            ];
        }
        "scroll-start" => return vec!["start".into(), "end".into()],
        "scope" => return vec!["col".into(), "row".into()],
        _ => {}
    }
    let Some(a) = tags::attr(name) else {
        return out;
    };
    let rows = rows_of(name, &a);
    for row in rows {
        for v in row.enum_names() {
            let s = (*v).to_owned();
            if !out.contains(&s) {
                out.push(s);
            }
        }
        if vocab::takes_roles(*row) {
            let (exact, css) = vocab::color_roles();
            for r in exact {
                let s = format!("-exact-{r}");
                if !out.contains(&s) {
                    out.push(s);
                }
            }
            for c in css {
                let s = c.to_owned();
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        }
    }
    if matches!(
        a,
        AttrTarget::Prop(p) | AttrTarget::InvertedBoolProp(p) if prop_type(p) == "bool"
    ) {
        out.push("true".into());
        out.push("false".into());
    }
    out
}

/// Documentation text for a host command.
pub fn command_doc(name: &str) -> Option<String> {
    HOST_COMMANDS.contains(&name).then(|| command_text(name))
}

/// Built-in `exact:content` parts parsed from their embedded Contract sources.
pub fn builtin_parts() -> Vec<PartInfo> {
    let mut out = Vec::new();
    for &(module, source) in PARTS {
        for line in source.lines() {
            if let Some(name) = line.strip_prefix("component ") {
                let name = name.trim();
                if let Some(p) = part(name) {
                    out.push(PartInfo {
                        name: name.to_owned(),
                        module,
                        props: p.props,
                        about: p.about,
                        call: p.call,
                    });
                }
            }
        }
    }
    out
}

/// Built-in event record names and their fields from the type checker.
pub fn event_records() -> Vec<(String, Vec<(String, String)>)> {
    let Ok(file) = contract_syntax::parse("component G\n  view\n    column\n") else {
        return Vec::new();
    };
    let Ok(shapes) = contract_types::check_declarations(&file, None) else {
        return Vec::new();
    };
    shapes
        .map
        .into_iter()
        .map(|(name, fields)| {
            let fields = fields
                .into_iter()
                .map(|(f, t)| (f, t.to_string()))
                .collect();
            (name, fields)
        })
        .collect()
}
