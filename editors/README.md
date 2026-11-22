# Editor and Agent Support for Exact Contract

The Contract compiler ships both a **Language Server Protocol (LSP 3.17)**
server and a native **Model Context Protocol (MCP)** server:

```sh
contract lsp --stdio                   # standard LSP over stdio
contract mcp --stdio                   # native MCP server for coding agents
# or, from any Exact checkout or app:
bun scripts/exact.mjs contract lsp --stdio
```

Build a fast local binary once from the repository root:

```sh
cargo build -p contract --profile host-dev
# binary at ./target/host-dev/contract
```

## Editors

- **VS Code / Cursor / Windsurf / VSCodium**: see [vscode/README.md](vscode/README.md).
  Ships TextMate grammars for `.contract` and `.forma`, snippets, problem
  matcher, and the zero-dependency LSP client (`vscode/src/extension.js`).
- **Neovim (0.11+)**: copy or source [neovim/exact-contract.lua](neovim/exact-contract.lua).
  Registers `exact-contract` and configures `vim.lsp.config.exact_contract`
  with LSP semantic token highlighting, navigation, completion, and formatting.
- **Helix**: merge [helix/languages.toml](helix/languages.toml) into
  `~/.config/helix/languages.toml`.
- **Zed**: see [zed/languages/exact-contract/config.toml](zed/languages/exact-contract/config.toml).
- **Sublime Text / Shiki / Bat / GitHub Linguist**: reuse
  [vscode/syntaxes/exact-contract.tmLanguage.json](vscode/syntaxes/exact-contract.tmLanguage.json)
  and [vscode/syntaxes/exact-forma.tmLanguage.json](vscode/syntaxes/exact-forma.tmLanguage.json)
  directly.
- **Emacs (`eglot`)**:
  ```elisp
  (with-eval-after-load 'eglot
    (add-to-list 'eglot-server-programs '((exact-contract-mode) . ("contract" "lsp" "--stdio"))))
  ```

## Serving Coding Agents

### 1. Native MCP Server (`contract mcp --stdio`)

Drop into any MCP client config (keeps one warm incremental compiler workspace
with zero cold-start per call):

```json
{
  "mcpServers": {
    "exact-contract": {
      "command": "/path/to/exact/target/host-dev/contract",
      "args": ["mcp", "--stdio"]
    }
  }
}
```

Tools exposed by `contract mcp`:

- `contract_check` — compile-check a file with optional in-memory `overlays` (never touches disk), returning structured diagnostics and quick fixes.
- `contract_outline` — compact structural summary (imports, routes, shapes, fns, styles, components, states, actions + `writes`, tests).
- `contract_context` — token-budgeted context slice around a `symbol` or `line` (declaration source, referenced shapes/styles/fns/components, callers).
- `contract_vocab` — exact lookup (`name`), batch lookup (`names`), or ranked keyword search (`query`) over tags, attributes, CSS properties, events, host commands, stdlib functions, concepts, recipes, and patterns.
- `contract_symbols` — resolved definitions (with inferred types and action `writes`) and cross-file references.
- `contract_impact` — state/action/component dataflow and symbol blast-radius graph.
- `contract_seam` — cross-seam audit between `.contract`, `app.ts`/`data/src/*.rs`, `strings/*.json`, `app.forma`, and `app.json`.
- `contract_hover` — type, docs, and seam info at a 1-based `(line, col)`.
- `contract_fix` — preview (`apply: false`) or apply (`apply: true`) deterministic quick fixes + optional canonical formatting.
- `contract_refactor` — compiler-backed `rename_symbol` and `scaffold_missing_props`.

### 2. Standard LSP 3.17 + `exact/*` Custom Methods

Every MCP tool is also exposed as a custom JSON-RPC method on `contract lsp`:
`exact/vocab`, `exact/symbols`, `exact/diagnostics`, `exact/outline`,
`exact/check`, `exact/seam`, `exact/impact`, `exact/context`, `exact/fix`, and
`exact/refactor`. Pull diagnostics (`textDocument/diagnostic` and
`workspace/diagnostic`) are supported alongside push `publishDiagnostics`.

From Node or Bun, import `LspConnection` and `discoverServer` directly from
[vscode/src/lsp-client.js](vscode/src/lsp-client.js):

```js
const { discoverServer, LspConnection } = require('./editors/vscode/src/lsp-client.js');
const { command, args } = discoverServer([process.cwd()]);
const lsp = new LspConnection(command, args);
await lsp.initialize({ rootUri: `file://${process.cwd()}` });
const slice = await lsp.request('exact/context', {
  uri: `file://${process.cwd()}/apps/caltrain/app.contract`,
  symbol: 'App',
});
await lsp.stop();
```
