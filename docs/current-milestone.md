# Current Milestone: M12 — Concurrency

**Goal:** Structured concurrency with `fork`/`join`, channels, and `par-let`.

**Crates:** `nexl-effects`, `nexl-runtime`, `nexl-infer`

**Spec sections to reference:**
- §10.3–§10.6 (channels, atoms, par-let)

**Key ADRs:**
- ADR-007: Atoms outside effect system

**Acceptance criteria:**
- `Concurrent` effect with `fork`/`join`/`yield`
- `fork`, `join`, `race`, `timeout`
- Channels and atoms
- `par-let` and `go` sugar
- `sleep` and a deterministic test handler

**When done:** Update this file to point to M13.

See `docs/todo-m12.md` for the task checklist.
See `milestones.md` for the full plan.
