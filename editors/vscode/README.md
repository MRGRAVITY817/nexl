# Nexl VS Code / Cursor Extension

Provides syntax highlighting for `.nx` files in VS Code and Cursor.

## Install

**Option A — symlink (recommended for local development):**

```bash
ln -s /Users/tripboi/Projects/nexl/editors/vscode \
      ~/.vscode/extensions/nexl-vscode
# or for Cursor:
ln -s /Users/tripboi/Projects/nexl/editors/vscode \
      ~/.cursor/extensions/nexl-vscode
```

Restart the editor after linking.

**Option B — copy:**

Copy this directory to `~/.vscode/extensions/nexl-vscode` (or `~/.cursor/extensions/nexl-vscode`).

## What it provides

- `.nx` files are recognized as the **Nexl** language (not Clojure)
- Multiline string literals are highlighted as a single unit
- String escape sequences (`\n`, `\t`, `\r`, `\\`, `\"`, `\{`) are highlighted
- String interpolation holes `{expr}` are highlighted
- Keywords (`:name`), numbers, booleans, type names, special forms, comments
- Bracket auto-closing and `; line comment` support
