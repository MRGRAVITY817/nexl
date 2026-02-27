# Nexl Neovim Support

Editor integration for [Nexl](../../) in Neovim.

## Installation

### Option 1: Add to runtimepath

Add the `editors/neovim` directory to your Neovim runtimepath:

```lua
-- init.lua
vim.opt.rtp:append('/path/to/nexl/editors/neovim')
```

This gives you filetype detection, indentation settings, and syntax highlighting.

### Option 2: Symlink into your config

```sh
ln -s /path/to/nexl/editors/neovim/ftdetect ~/.config/nvim/ftdetect
ln -s /path/to/nexl/editors/neovim/ftplugin ~/.config/nvim/ftplugin
ln -s /path/to/nexl/editors/neovim/syntax   ~/.config/nvim/syntax
```

## LSP Setup

Requires [nvim-lspconfig](https://github.com/neovim/nvim-lspconfig) and the
`nexl` binary on your PATH.

```lua
local lspconfig = require('lspconfig')
local configs = require('lspconfig.configs')

if not configs.nexl then
  configs.nexl = require('editors.neovim.lsp')  -- adjust path as needed
end

lspconfig.nexl.setup({})
```

This provides:
- Diagnostics (parse errors + type errors)
- Hover (type info + docstrings)
- Go-to-definition
- Completion
- Format on save (via `nexl fmt` or the LSP formatting handler)

### Format on save

```lua
vim.api.nvim_create_autocmd('BufWritePre', {
  pattern = '*.nx',
  callback = function()
    vim.lsp.buf.format({ async = false })
  end,
})
```

## Syntax Highlighting

The included `syntax/nexl.vim` provides regex-based fallback highlighting.
For full structural highlighting, install
[tree-sitter-nexl](https://github.com/nexl-lang/tree-sitter-nexl) and
configure `nvim-treesitter`:

```lua
require('nvim-treesitter.configs').setup({
  ensure_installed = { 'nexl' },
  highlight = { enable = true },
})
```

## What's Included

| File | Purpose |
|---|---|
| `ftdetect/nexl.vim` | Detect `.nx` files as `filetype=nexl` |
| `ftplugin/nexl.vim` | Indent, comment, and keyword settings |
| `syntax/nexl.vim` | Regex-based syntax highlighting |
| `lsp.lua` | nvim-lspconfig server definition |
