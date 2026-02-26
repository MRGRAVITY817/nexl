# Current Milestone: M11 — Protocols + Advanced Types

**Goal:** Protocols, implementations, compiler-dispatched ops, and advanced type features.

**Crates:** `nexl-types`, `nexl-infer`, `nexl-ast`

**Spec sections to reference:**
- §5.9–§5.12 (opaque types, protocols, Any, refinements)
- §7.6 (named patterns)

**Key ADRs:**
- ADR-008: No HKT, compiler-dispatched map/filter

**Acceptance criteria:**
- `defprotocol` and `impl` work end-to-end
- Compiler-dispatched ops (`map`, `filter`, `reduce`) resolve statically
- Opaque types and `deftype-alias` supported
- Any escape hatch and refinement types implemented
- Named patterns (`defpattern`) usable in `match`

**When done:** Update this file to point to M12.

See `docs/todo-m11.md` for the task checklist.
See `milestones.md` for the full plan.
