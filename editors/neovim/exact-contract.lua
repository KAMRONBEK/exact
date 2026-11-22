-- Exact Contract filetype and LSP 3.17 configuration for Neovim 0.11+.
-- Drop into ~/.config/nvim/after/plugin/exact-contract.lua or require() from init.lua.

vim.filetype.add({
  extension = {
    contract = "exact-contract",
  },
})

local function find_contract_cmd(root_dir)
  if root_dir then
    for _, rel in ipairs({
      "target/host-dev/contract",
      "target/release/contract",
      "target/debug/contract",
    }) do
      local bin = root_dir .. "/" .. rel
      if vim.fn.executable(bin) == 1 then
        return { bin, "lsp", "--stdio" }
      end
    end
    local repo_cli = root_dir .. "/scripts/exact.mjs"
    if vim.fn.filereadable(repo_cli) == 1 then
      return { "bun", repo_cli, "contract", "lsp", "--stdio" }
    end
    local app_cli = root_dir .. "/exact.mjs"
    if vim.fn.filereadable(app_cli) == 1 then
      return { "bun", app_cli, "contract", "lsp", "--stdio" }
    end
  end
  return { "contract", "lsp", "--stdio" }
end

if vim.lsp.config then
  vim.lsp.config.exact_contract = {
    cmd = function(dispatchers, config)
      local cmd = find_contract_cmd(config.root_dir)
      return vim.lsp.rpc.start(cmd, dispatchers)
    end,
    filetypes = { "exact-contract" },
    root_markers = { "app.json", "app.contract", "Cargo.toml", ".git" },
  }
  vim.lsp.enable("exact_contract")
end

vim.api.nvim_create_autocmd("FileType", {
  pattern = "exact-contract",
  callback = function()
    vim.bo.commentstring = "// %s"
    vim.bo.shiftwidth = 2
    vim.bo.tabstop = 2
    vim.bo.expandtab = true
  end,
})
