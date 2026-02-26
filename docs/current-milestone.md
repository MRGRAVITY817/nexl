# Current Milestone: M10 — Macro System

**Goal:** Hygienic macros via scope sets (`defmacro`, `defmacro-syntax`).

**Crates:** `nexl-macros` (new), `nexl-reader` (extend), `nexl-ast` (extend)

**Spec sections to reference:**
- §7 Macros (lines 1802–1941)

**Key ADRs:**
- ADR-013: Def-form renames (`defn-macro`, `defmacro-syntax`, etc.)

**Acceptance criteria:**
- `defmacro` expands hygienically
- `defmacro-syntax` supports pattern-based macros
- Built-in macros (`->`, `->>`, `when`, `unless`, `and`, `or`) expand correctly
- `cargo test` passes across all crates

**When done:** Update this file to point to M11.

See `docs/todo-m10.md` for the task checklist.
See `milestones.md` for the full plan.
