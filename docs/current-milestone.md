# Current Milestone: M9 — CLI + REPL

**Goal:** A complete `nexl` CLI with REPL, `run`, `check`, and `build` subcommands.

**Crates:** `nexl-cli` (extend), `nexl-repl` (new)

**Spec sections to reference:**
- §14 Toolchain (lines 3203–end)

**Key ADRs:**
- (none specific to M9)

**Acceptance criteria:**
- `nexl run <file>` compiles and runs a Nexl program
- `nexl check <file>` type-checks without emitting
- `nexl repl` provides an interactive REPL
- `cargo test` passes across all crates

**When done:** Update this file to point to M10.

See `docs/todo-m9.md` for the task checklist.
See `milestones.md` for the full plan.
