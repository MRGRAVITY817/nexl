# Current Milestone: M15 — Advanced Toolchain

**Goal:** LSP, package manager, documentation, sandbox mode.

**Crates:** `nexl-lsp`, `nexl-pkg`, `nexl-doc`, plus CLI subcommands

**Spec sections to reference:**
- §14.1 CLI commands (lines ~3227–3255)
- §14.7 Documentation (lines ~3465–3473)
- §8.4 Content-addressed definitions (lines ~2521–2560)
- §8.11 Package vs module (lines ~2667–2684)
- LSP section (lines ~3443–3463)

**Key design points:**
- LSP and compiler share the same analysis engine (incremental queries)
- Effects ARE the capability system → sandbox is a thin layer
- Content-addressed definitions for packages
- Semver enforcement via API diffing

**Acceptance criteria:**
- `nexl lsp` provides diagnostics, hover, go-to-def, completions
- `nexl pkg add/remove/lock` manages dependencies
- `nexl doc` generates HTML documentation
- `nexl sandbox` restricts effects via CLI flags

**When done:** Update this file to point to M16.

See `docs/todo-m15.md` for the task checklist.
See `milestones.md` for the full plan.
