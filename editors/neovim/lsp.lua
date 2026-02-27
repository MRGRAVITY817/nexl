-- Nexl LSP configuration for nvim-lspconfig
--
-- Usage: Add to your Neovim config (init.lua or lua/plugins/nexl.lua):
--
--   require('lspconfig.configs').nexl = require('path.to.this.lsp')
--   require('lspconfig').nexl.setup({})
--
-- Or with lazy.nvim, add the editors/neovim directory to your runtimepath.

local util = require('lspconfig.util')

return {
  default_config = {
    cmd = { 'nexl', 'lsp' },
    filetypes = { 'nexl' },
    root_dir = util.root_pattern('project.nexl', '.git'),
    single_file_support = true,
    settings = {},
  },
  docs = {
    description = [[
https://github.com/nexl-lang/nexl

Nexl language server providing diagnostics, hover, go-to-definition,
completion, and formatting for `.nxl` files.

Install the `nexl` binary and ensure it is on your PATH.
]],
    default_config = {
      root_dir = [[root_pattern("project.nexl", ".git")]],
    },
  },
}
