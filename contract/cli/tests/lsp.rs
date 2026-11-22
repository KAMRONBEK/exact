//! End-to-end integration tests for `contract lsp` (LSP 3.17 + `exact/*` agent APIs).

use contract::lsp::mcp::McpServer;
use contract::lsp::Server;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

fn temp_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "exact-contract-lsp-{tag}-{}-{n}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn uri(path: &Path) -> String {
    let norm = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    format!("file://{}", norm.to_string_lossy().replace('\\', "/"))
}

fn req(server: &mut Server, id: i64, method: &str, params: Value) -> Value {
    let replies = server.handle_message(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    }));
    assert_eq!(replies.len(), 1, "expected 1 reply for {method}");
    assert!(
        replies[0].get("error").is_none(),
        "{method} failed: {}",
        replies[0]
    );
    replies[0]["result"].clone()
}

fn notify(server: &mut Server, method: &str, params: Value) -> Vec<Value> {
    server.handle_message(&json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    }))
}

#[test]
fn initialize_advertises_full_capabilities_and_shuts_down() {
    let mut server = Server::new();
    let res = req(&mut server, 1, "initialize", json!({ "rootUri": null }));
    let caps = &res["capabilities"];
    assert_eq!(caps["hoverProvider"], true);
    assert_eq!(caps["definitionProvider"], true);
    assert_eq!(caps["typeDefinitionProvider"], true);
    assert_eq!(caps["referencesProvider"], true);
    assert_eq!(caps["documentHighlightProvider"], true);
    assert_eq!(caps["documentSymbolProvider"], true);
    assert_eq!(caps["workspaceSymbolProvider"], true);
    assert_eq!(caps["documentFormattingProvider"], true);
    assert_eq!(caps["foldingRangeProvider"], true);
    assert_eq!(caps["inlayHintProvider"], true);
    assert!(caps["completionProvider"]["triggerCharacters"].is_array());
    assert!(caps["semanticTokensProvider"]["legend"]["tokenTypes"].is_array());
    assert!(caps["diagnosticProvider"].is_object());

    notify(&mut server, "initialized", json!({}));
    req(&mut server, 2, "shutdown", Value::Null);
    assert!(server.shutdown_requested());
    notify(&mut server, "exit", Value::Null);
    assert!(server.should_exit());
}

#[test]
fn diagnostics_hover_navigation_rename_inlay_semantic_and_formatting() {
    let dir = temp_dir("core");
    let card_path = dir.join("card.contract");
    let app_path = dir.join("app.contract");

    let card_src = "\
shape Item
  id: string
  label: string

component Card
  props
    item: Item
    onPick: action
  view
    button item.label press=onPick
";
    let app_src = "\
use Card, Item from \"./card.contract\"

style Pill
  padding=12

fn formatCount(n: number): string = `${n} items`

component App
  state count = 0
  state picked = Item(id=\"1\", label=\"First\")
  derive summary = formatCount(count)

  action bump
    count = count + 1

  view
    column class=Pill
      text summary
      Card(item=picked, onPick=bump)
";
    fs::write(&card_path, card_src).unwrap();
    fs::write(&app_path, app_src).unwrap();

    let mut server = Server::new();
    req(
        &mut server,
        1,
        "initialize",
        json!({ "rootUri": uri(&dir) }),
    );
    notify(&mut server, "initialized", json!({}));

    let app_uri = uri(&app_path);
    let card_uri = uri(&card_path);
    let notes = notify(
        &mut server,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": app_uri,
                "languageId": "exact-contract",
                "version": 1,
                "text": app_src,
            }
        }),
    );
    let app_pub = notes
        .iter()
        .find(|n| n["params"]["uri"] == app_uri)
        .expect("published diagnostics for app");
    let errs: Vec<_> = app_pub["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["severity"] == 1)
        .collect();
    assert!(errs.is_empty(), "expected clean app, got {errs:?}");

    // Pull diagnostics (LSP 3.17).
    let pulled = req(
        &mut server,
        2,
        "textDocument/diagnostic",
        json!({ "textDocument": { "uri": app_uri } }),
    );
    assert_eq!(pulled["kind"], "full");

    // Hover on inferred derive `summary` (line 10, `derive summary = ...`).
    let hover = req(
        &mut server,
        3,
        "textDocument/hover",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 10, "character": 10 },
        }),
    );
    let md = hover["contents"]["value"].as_str().unwrap_or("");
    assert!(md.contains("derive summary: string"), "hover was: {md}");

    // Hover on action `bump` includes inferred `writes: count`.
    let hover_act = req(
        &mut server,
        4,
        "textDocument/hover",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 12, "character": 10 },
        }),
    );
    let md_act = hover_act["contents"]["value"].as_str().unwrap_or("");
    assert!(
        md_act.contains("writes:") && md_act.contains("count"),
        "action hover: {md_act}"
    );

    // Hover on built-in tag `column` (line 16).
    let hover_tag = req(
        &mut server,
        5,
        "textDocument/hover",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 16, "character": 6 },
        }),
    );
    assert!(hover_tag["contents"]["value"]
        .as_str()
        .unwrap_or("")
        .contains("Tag"));

    // Cross-file go-to-definition from `Card(...)` (line 18) -> card.contract.
    let def = req(
        &mut server,
        6,
        "textDocument/definition",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 18, "character": 8 },
        }),
    );
    assert_eq!(def["uri"], card_uri);
    assert_eq!(def["range"]["start"]["line"], 4);

    // Go-to-definition on `"./card.contract"` path string (line 0).
    let def_file = req(
        &mut server,
        7,
        "textDocument/definition",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 0, "character": 25 },
        }),
    );
    assert_eq!(def_file["uri"], card_uri);

    // Go-to-type-definition from `picked` (line 9, `state picked = Item(...)`) -> `shape Item` in card.contract.
    let type_def = req(
        &mut server,
        8,
        "textDocument/typeDefinition",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 9, "character": 10 },
        }),
    );
    assert_eq!(type_def["uri"], card_uri);
    assert_eq!(type_def["range"]["start"]["line"], 0);

    // Find references to `count` (line 8) -> definition + derive + action reads/writes.
    let refs = req(
        &mut server,
        9,
        "textDocument/references",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 8, "character": 10 },
            "context": { "includeDeclaration": true },
        }),
    );
    assert!(refs.as_array().unwrap().len() >= 3, "refs: {refs}");

    // Document highlight on `count`.
    let hl = req(
        &mut server,
        10,
        "textDocument/documentHighlight",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 8, "character": 10 },
        }),
    );
    assert!(hl.as_array().unwrap().len() >= 3);

    // PrepareRename & Rename `count` -> `total`.
    let prep = req(
        &mut server,
        11,
        "textDocument/prepareRename",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 8, "character": 10 },
        }),
    );
    assert_eq!(prep["placeholder"], "count");
    let renamed = req(
        &mut server,
        12,
        "textDocument/rename",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 8, "character": 10 },
            "newName": "total",
        }),
    );
    assert!(renamed["changes"][&app_uri].as_array().unwrap().len() >= 3);

    // Document symbols & Workspace symbols.
    let doc_syms = req(
        &mut server,
        13,
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": app_uri } }),
    );
    let names: Vec<&str> = doc_syms
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert!(names.contains(&"Pill") && names.contains(&"formatCount") && names.contains(&"App"));

    let ws_syms = req(
        &mut server,
        14,
        "workspace/symbol",
        json!({ "query": "Card" }),
    );
    assert!(ws_syms
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"] == "Card"));

    // Semantic tokens, Inlay hints, CodeLens, Folding ranges.
    let sem = req(
        &mut server,
        15,
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": app_uri } }),
    );
    assert!(!sem["data"].as_array().unwrap().is_empty());

    let hints = req(
        &mut server,
        16,
        "textDocument/inlayHint",
        json!({
            "textDocument": { "uri": app_uri },
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 30, "character": 0 } },
        }),
    );
    assert!(
        hints
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["label"].as_str().unwrap_or("").contains(": number")),
        "inlay hints: {hints}"
    );

    let lenses = req(
        &mut server,
        17,
        "textDocument/codeLens",
        json!({ "textDocument": { "uri": app_uri } }),
    );
    assert!(lenses
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["command"]["title"]
            .as_str()
            .unwrap_or("")
            .contains("writes count")));

    let folds = req(
        &mut server,
        18,
        "textDocument/foldingRange",
        json!({ "textDocument": { "uri": app_uri } }),
    );
    assert!(!folds.as_array().unwrap().is_empty());

    // Signature help on `Card(` (line 18, right after `Card(`).
    let sig = req(
        &mut server,
        19,
        "textDocument/signatureHelp",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 18, "character": 11 },
        }),
    );
    assert!(sig["signatures"][0]["label"]
        .as_str()
        .unwrap_or("")
        .contains("item: Item"));

    // Formatting (introduce ugly spacing via incremental didChange, then format).
    notify(
        &mut server,
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": app_uri, "version": 2 },
            "contentChanges": [{
                "range": { "start": { "line": 8, "character": 0 }, "end": { "line": 8, "character": 17 } },
                "text": "  state   count=0",
            }],
        }),
    );
    let edits = req(
        &mut server,
        20,
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": app_uri },
            "options": { "tabSize": 2, "insertSpaces": true },
        }),
    );
    assert!(!edits.as_array().unwrap().is_empty());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn completions_code_actions_and_agent_extensions() {
    let dir = temp_dir("agents");
    let badge_path = dir.join("badge.contract");
    let app_path = dir.join("app.contract");

    fs::write(
        &badge_path,
        "component Badge\n  props\n    label: string\n  view\n    text label\n",
    )
    .unwrap();
    // Missing `use Badge from "./badge.contract"` plus HTML tag `div` on purpose.
    let bad_app = "\
component App
  state count = 0
  action inc
    count = count + 1
  view
    column
      Badge(label=\"Hi\")
      button \"Go\" buttonStyle=
      div
";
    fs::write(&app_path, bad_app).unwrap();

    let mut server = Server::new();
    req(
        &mut server,
        1,
        "initialize",
        json!({ "rootUri": uri(&dir) }),
    );
    let app_uri = uri(&app_path);
    let badge_uri = uri(&badge_path);

    // Open clean version first to test completions.
    let clean_app = "\
use Badge from \"./badge.contract\"

component App
  state count = 0
  action inc
    count = count + 1
  view
    column
      Badge(label=\"Hi\")
      button \"Go\" buttonStyle=
";
    notify(
        &mut server,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": app_uri,
                "languageId": "exact-contract",
                "version": 1,
                "text": clean_app,
            }
        }),
    );

    // Value completion after `buttonStyle=`.
    let comp = req(
        &mut server,
        2,
        "textDocument/completion",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 9, "character": 30 },
        }),
    );
    let labels: Vec<&str> = comp["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["label"].as_str())
        .collect();
    assert!(
        labels.contains(&"filled") && labels.contains(&"bordered"),
        "labels: {labels:?}"
    );

    // Code action for missing `use` + unknown HTML tag `div`.
    let missing_use_app = "\
use Badge from \"./badge.contract\"

component App
  view
    div
";
    let notes = notify(
        &mut server,
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": app_uri, "version": 2 },
            "contentChanges": [{ "text": missing_use_app }],
        }),
    );
    let diags = notes[0]["params"]["diagnostics"].clone();
    let actions = req(
        &mut server,
        3,
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": app_uri },
            "range": { "start": { "line": 4, "character": 4 }, "end": { "line": 4, "character": 7 } },
            "context": { "diagnostics": diags },
        }),
    );
    assert!(
        actions
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["title"].as_str().unwrap_or("").contains("Replace `div`")),
        "code actions: {actions}"
    );

    // Agent extension: `exact/vocab`.
    let vocab = req(&mut server, 4, "exact/vocab", json!({ "name": "button" }));
    assert_eq!(vocab["known"], true);
    assert_eq!(vocab["entry"]["tag"]["name"], "button");

    // Agent extension: `exact/check` with in-memory overlay (does NOT mutate open buffer!).
    let checked = req(
        &mut server,
        5,
        "exact/check",
        json!({
            "uri": app_uri,
            "overlays": {
                app_uri.clone(): "use Badge from \"./badge.contract\"\n\ncomponent App\n  view\n    column\n      Badge(label=\"OK\")\n"
            }
        }),
    );
    assert_eq!(checked["ok"], true, "checked: {checked}");
    assert_eq!(checked["errors"], 0);

    // Open clean version again and test `exact/outline` + `exact/symbols` + `exact/diagnostics`.
    notify(
        &mut server,
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": app_uri, "version": 3 },
            "contentChanges": [{
                "text": "use Badge from \"./badge.contract\"\n\ncomponent App\n  state n = 1\n  action up\n    n = n + 1\n  view\n    column\n      Badge(label=\"OK\")\n"
            }],
        }),
    );
    let outline = req(&mut server, 6, "exact/outline", json!({ "uri": app_uri }));
    assert_eq!(outline["parsed"], true);
    assert_eq!(outline["components"][0]["name"], "App");
    assert_eq!(
        outline["components"][0]["actions"][0]["writes"],
        json!(["n"])
    );

    let syms = req(&mut server, 7, "exact/symbols", json!({ "uri": app_uri }));
    assert!(syms["definitions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["name"] == "Badge" && d["uri"] == badge_uri));

    let diag_res = req(
        &mut server,
        8,
        "exact/diagnostics",
        json!({ "uri": app_uri }),
    );
    assert_eq!(diag_res["ok"], true);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn links_colors_selection_call_hierarchy_and_file_rename() {
    let dir = temp_dir("phase2-lsp");
    let helper_path = dir.join("helper.contract");
    let app_path = dir.join("app.contract");

    let helper_src = "\
shape Owner
  name: string

shape Ticket
  id: string
  owner: Owner

fn formatTicket(t: Ticket): string = t.owner.name

component Row
  props
    ticket: Ticket
  view
    text formatTicket(ticket)
";
    let app_src = "\
use Ticket, Row from \"./helper.contract\"

style Accent
  background-color=\"#3366ff\"

component App
  state count = 0
  resource active = loadTicket() as shape Ticket
  action ping
    count = count + 1
  view
    column class=Accent
      Row(ticket=active)
      button \"Go\" press=ping
";
    fs::write(&helper_path, helper_src).unwrap();
    fs::write(&app_path, app_src).unwrap();

    let mut server = Server::new();
    let init = req(
        &mut server,
        1,
        "initialize",
        json!({ "rootUri": uri(&dir) }),
    );
    let caps = &init["capabilities"];
    assert_eq!(caps["colorProvider"], true);
    assert_eq!(caps["selectionRangeProvider"], true);
    assert_eq!(caps["callHierarchyProvider"], true);
    assert_eq!(caps["semanticTokensProvider"]["full"], true);
    assert_eq!(caps["diagnosticProvider"]["workspaceDiagnostics"], true);

    let app_uri = uri(&app_path);
    let helper_uri = uri(&helper_path);
    notify(
        &mut server,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": app_uri,
                "languageId": "exact-contract",
                "version": 1,
                "text": app_src,
            }
        }),
    );

    // Document links: clickable `./helper.contract`.
    let links = req(
        &mut server,
        2,
        "textDocument/documentLink",
        json!({ "textDocument": { "uri": app_uri } }),
    );
    assert!(
        links
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["target"].as_str() == Some(helper_uri.as_str())),
        "links: {links}"
    );

    // Document colors + color presentation.
    let colors = req(
        &mut server,
        3,
        "textDocument/documentColor",
        json!({ "textDocument": { "uri": app_uri } }),
    );
    assert_eq!(colors.as_array().unwrap().len(), 1, "colors: {colors}");
    let pres = req(
        &mut server,
        4,
        "textDocument/colorPresentation",
        json!({
            "color": { "red": 1.0, "green": 0.0, "blue": 0.5, "alpha": 1.0 },
            "range": colors[0]["range"],
        }),
    );
    assert_eq!(pres[0]["label"], "#ff0080");

    // Selection ranges: nested word -> line -> block -> document.
    let sel = req(
        &mut server,
        5,
        "textDocument/selectionRange",
        json!({
            "textDocument": { "uri": app_uri },
            "positions": [{ "line": 10, "character": 6 }],
        }),
    );
    assert!(sel[0]["parent"]["parent"].is_object(), "sel: {sel}");

    // Call hierarchy: prepare on `ping` (line 8, col 10), then incomingCalls finds `App`.
    let items = req(
        &mut server,
        6,
        "textDocument/prepareCallHierarchy",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 8, "character": 10 },
        }),
    );
    assert_eq!(items[0]["name"], "ping", "items: {items}");
    let incoming = req(
        &mut server,
        7,
        "callHierarchy/incomingCalls",
        json!({ "item": items[0] }),
    );
    assert!(
        incoming
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["from"]["name"] == "App"),
        "incoming: {incoming}"
    );

    // Outgoing calls from `App` component include `Row`.
    let app_item = req(
        &mut server,
        8,
        "textDocument/prepareCallHierarchy",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 5, "character": 11 },
        }),
    );
    let outgoing = req(
        &mut server,
        9,
        "callHierarchy/outgoingCalls",
        json!({ "item": app_item[0] }),
    );
    assert!(
        outgoing
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["to"]["name"] == "Row"),
        "outgoing: {outgoing}"
    );

    // Workspace pull diagnostics.
    let ws_diag = req(&mut server, 10, "workspace/diagnostic", json!({}));
    assert!(!ws_diag["items"].as_array().unwrap().is_empty());

    // Semantic tokens full.
    let sem_full = req(
        &mut server,
        11,
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": app_uri } }),
    );
    assert!(!sem_full["data"].as_array().unwrap().is_empty());

    // File rename updates `use ... from "./helper.contract"`.
    let renamed_helper = dir.join("tickets.contract");
    let rename_edit = req(
        &mut server,
        12,
        "workspace/willRenameFiles",
        json!({
            "files": [{
                "oldUri": helper_uri,
                "newUri": uri(&renamed_helper),
            }]
        }),
    );
    let app_edits = rename_edit["changes"][&app_uri].as_array().unwrap();
    assert_eq!(app_edits[0]["newText"], "./tickets.contract");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cross_seam_navigation_completions_auto_import_and_quick_fixes() {
    let dir = temp_dir("phase2-seam");
    fs::create_dir_all(dir.join("strings")).unwrap();
    fs::write(
        dir.join("strings").join("en.json"),
        "{\n  \"greeting\": \"Hello world\",\n  \"farewell\": \"Goodbye\"\n}\n",
    )
    .unwrap();
    fs::write(
        dir.join("app.forma"),
        "forma seam\n\ntheme\n  brand.primary = #2255cc  scheme=fixed\n  surface.page = #ffffff  scheme=fixed\n",
    )
    .unwrap();
    fs::write(
        dir.join("app.ts"),
        "export const queries = {\n  loadItem: () => ({ id: '1', owner: { handle: '@ada' } }),\n};\n",
    )
    .unwrap();
    fs::write(
        dir.join("app.json"),
        "{\"name\":\"SeamApp\",\"app\":{\"id\":\"com.exact.seam\",\"name\":\"SeamApp\"},\"hatches\":[\"map\"]}\n",
    )
    .unwrap();
    fs::write(
        dir.join("pill.contract"),
        "component Pill\n  props\n    label: string\n  view\n    text label\n",
    )
    .unwrap();

    let app_path = dir.join("app.contract");
    let app_src = "\
shape Owner
  handle: string

shape Item
  id: string
  owner: Owner

style Card
  color=\"brand.primary\"

component App
  resource item = loadItem() as shape Item
  view
    column class=Card
      text t(\"greeting\")
      text item.owner.
";
    fs::write(&app_path, app_src).unwrap();

    let mut server = Server::new();
    req(
        &mut server,
        1,
        "initialize",
        json!({ "rootUri": uri(&dir) }),
    );
    let app_uri = uri(&app_path);
    notify(
        &mut server,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": app_uri,
                "languageId": "exact-contract",
                "version": 1,
                "text": app_src,
            }
        }),
    );

    // Cross-seam definition: `t("greeting")` -> strings/en.json.
    let str_def = req(
        &mut server,
        2,
        "textDocument/definition",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 14, "character": 16 },
        }),
    );
    assert!(
        str_def["uri"]
            .as_str()
            .unwrap_or("")
            .ends_with("strings/en.json"),
        "str_def: {str_def}"
    );

    // Cross-seam hover: `color="brand.primary"` -> shows `#2255cc` from `app.forma`.
    let tok_hover = req(
        &mut server,
        3,
        "textDocument/hover",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 8, "character": 12 },
        }),
    );
    assert!(
        tok_hover["contents"]["value"]
            .as_str()
            .unwrap_or("")
            .contains("#2255cc"),
        "tok_hover: {tok_hover}"
    );

    // Chained dot completion: `item.owner.` -> `handle`.
    let dot_comp = req(
        &mut server,
        4,
        "textDocument/completion",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 15, "character": 22 },
        }),
    );
    let dot_labels: Vec<&str> = dot_comp["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["label"].as_str())
        .collect();
    assert!(dot_labels.contains(&"handle"), "dot_labels: {dot_labels:?}");

    // Auto-import completion: `Pi` offers `Pill` with `additionalTextEdits` adding `use Pill`.
    let auto_imp_src = app_src.replace("text item.owner.", "Pi");
    notify(
        &mut server,
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": app_uri, "version": 2 },
            "contentChanges": [{ "text": auto_imp_src }],
        }),
    );
    let imp_comp = req(
        &mut server,
        5,
        "textDocument/completion",
        json!({
            "textDocument": { "uri": app_uri },
            "position": { "line": 15, "character": 8 },
        }),
    );
    let pill_item = imp_comp["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == "Pill")
        .expect("auto-import Pill completion");
    assert!(
        pill_item["additionalTextEdits"][0]["newText"]
            .as_str()
            .unwrap_or("")
            .contains("use Pill from \"./pill.contract\""),
        "pill_item: {pill_item}"
    );

    // Cross-seam audit (`exact/seam`).
    let clean_seam_src = app_src.replace("      text item.owner.\n", "");
    notify(
        &mut server,
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": app_uri, "version": 3 },
            "contentChanges": [{ "text": clean_seam_src }],
        }),
    );
    let seam = req(&mut server, 6, "exact/seam", json!({ "uri": app_uri }));
    assert_eq!(seam["ok"], true, "seam: {seam}");
    assert_eq!(seam["strings"]["definedCount"], 2);
    assert_eq!(seam["forma"]["tokenCount"], 2);

    // `list<T>` -> `T[]` quick fix.
    let list_bad = "shape Bag\n  items: list<string>\n\ncomponent App\n  view\n    text \"x\"\n";
    let notes = notify(
        &mut server,
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": app_uri, "version": 4 },
            "contentChanges": [{ "text": list_bad }],
        }),
    );
    let diags = notes[0]["params"]["diagnostics"].clone();
    let fixes = req(
        &mut server,
        7,
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": app_uri },
            "range": { "start": { "line": 1, "character": 9 }, "end": { "line": 1, "character": 21 } },
            "context": { "diagnostics": diags },
        }),
    );
    assert!(
        fixes.as_array().unwrap().iter().any(|a| {
            a["title"] == "Convert `list<T>` to `T[]`"
                && a["edit"]["changes"][&app_uri][0]["newText"]
                    .as_str()
                    .unwrap_or("")
                    .contains("string[]")
        }),
        "fixes: {fixes}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn agent_tools_vocab_search_impact_context_fix_refactor_and_mcp_server() {
    let dir = temp_dir("phase2-agent-mcp");
    let app_path = dir.join("app.contract");
    let app_src = "\
shape Todo
  id: string
  title: string

component App
  resource items = loadTodos() as shape Todo[]
  state filter = \"\"
  action clearFilter
    filter = \"\"
  view
    column
      for item in items key=item.id
        TodoRow(todo=item)

component TodoRow
  props
    todo: Todo
  view
    text todo.title
";
    fs::write(&app_path, app_src).unwrap();
    let app_uri = uri(&app_path);

    // 1. Batch & search `exact/vocab`, `exact/impact`, `exact/context`, `exact/refactor`.
    let mut server = Server::new();
    req(
        &mut server,
        1,
        "initialize",
        json!({ "rootUri": uri(&dir) }),
    );
    notify(
        &mut server,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": app_uri,
                "languageId": "exact-contract",
                "version": 1,
                "text": app_src,
            }
        }),
    );

    let batch = req(
        &mut server,
        2,
        "exact/vocab",
        json!({ "names": ["button", "navigate"] }),
    );
    assert_eq!(batch["entries"].as_array().unwrap().len(), 2);

    let search = req(
        &mut server,
        3,
        "exact/vocab",
        json!({ "query": "scroll", "limit": 5 }),
    );
    assert!(search["count"].as_u64().unwrap() >= 1, "search: {search}");

    let impact = req(
        &mut server,
        4,
        "exact/impact",
        json!({ "uri": app_uri, "symbol": "Todo" }),
    );
    assert_eq!(impact["ok"], true, "impact: {impact}");
    assert!(
        impact["target"][0]["referenceCount"].as_u64().unwrap() >= 2,
        "impact: {impact}"
    );

    let ctx = req(
        &mut server,
        5,
        "exact/context",
        json!({ "uri": app_uri, "symbol": "App" }),
    );
    assert_eq!(ctx["ok"], true, "ctx: {ctx}");
    assert!(ctx["target"]["source"]
        .as_str()
        .unwrap()
        .contains("component App"));
    assert!(
        ctx["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["name"] == "TodoRow"),
        "ctx: {ctx}"
    );

    let renamed = req(
        &mut server,
        6,
        "exact/refactor",
        json!({
            "kind": "rename_symbol",
            "uri": app_uri,
            "symbol": "Todo",
            "newName": "TaskItem",
            "apply": false,
        }),
    );
    assert_eq!(renamed["ok"], true, "renamed: {renamed}");
    let edits = renamed["edit"]["changes"][&app_uri].as_array().unwrap();
    assert!(edits.len() >= 3, "edits: {edits:?}");
    assert!(edits.iter().all(|e| e["newText"] == "TaskItem"));

    // 2. Native MCP server (`contract mcp`).
    let mut mcp = McpServer::new();
    let mcp_init = mcp
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }))
        .expect("mcp init response");
    assert_eq!(
        mcp_init["result"]["serverInfo"]["name"],
        "exact-contract-mcp"
    );

    let tools = mcp
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }))
        .expect("tools list");
    let tool_names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(tool_names.contains(&"contract_check"));
    assert!(tool_names.contains(&"contract_context"));
    assert!(tool_names.contains(&"contract_impact"));
    assert!(tool_names.contains(&"contract_seam"));
    assert!(tool_names.contains(&"contract_refactor"));

    let call = mcp
        .handle_message(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "contract_context",
                "arguments": { "path": app_path.to_string_lossy(), "symbol": "TodoRow" }
            }
        }))
        .expect("tools/call");
    assert_eq!(call["result"]["isError"], false);
    assert!(call["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("component TodoRow"));

    let _ = fs::remove_dir_all(&dir);
}
