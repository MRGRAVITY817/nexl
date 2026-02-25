# Current Milestone: M6 — Algebraic Effect System

**Goal:** `defeffect`, `handle`, effect row inference, and evidence-passing compilation.

**Crates:** `nexl-ast`, `nexl-reader`, `nexl-types`, `nexl-infer`, `nexl-effects` (new), `nexl-runtime`

**Spec sections to reference:**
- §6 Effects (lines 1520–1800)
- §6.3 Effect operations vs module-qualified calls (lines 1605–1630)
- §6.4 `handle` (lines 1632–1715)
- §6.5 Continuation handlers (lines 1717–1763)

**Key ADRs:**
- ADR-003: One-shot continuations

**Acceptance criteria:**
- `defeffect` declarations are parsed and registered
- Effect rows tracked in function types (`! [E1 E2 | r]`)
- `handle` type-checks and removes handled effects from rows
- Evidence passing in lowered representation
- Built-in effects (`Console`, `FileSystem`, `Time`, `Random`) wired in runtime
- `cargo test` passes across all crates

**When done:** Update this file to point to M7.

See `docs/todo-m6.md` for the task checklist.
See `milestones.md` for the full plan.
