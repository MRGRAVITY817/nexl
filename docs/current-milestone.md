# Current Milestone: M4 — Persistent Collections

**Goal:** Add persistent data structures (vectors, maps, sets) with type inference support and core operations.

**Crates:** `nexl-runtime`, `nexl-infer`, `nexl-types`

**Spec sections to reference:**
- §5.3 Composite Types (lines 1204–1218) — `Vec`, `Map`, `Set`, `List`, `Tuple`
- §4 (core forms) for collection literals and operations

**Key ADRs:** none yet (consult `decisions/` as they appear)

**Acceptance criteria:**
- Persistent `Vec`, `Map`, and `Set` value forms and types
- Inference for collection literals and common ops (lookup, assoc/update, membership)
- Sequence destructuring for collections where specified in §4
- Works with existing ADT/pattern machinery; `cargo test` passes

**When done:** Update this file to point to M5.

See `docs/todo-m4.md` (to be created) for the task checklist.
See `milestones.md` for the full plan.
