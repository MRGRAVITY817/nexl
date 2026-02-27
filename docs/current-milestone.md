# Current Milestone: M17 — Optimization

**Goal:** Make compiled code fast.

**Crates:** `nexl-ir`, `nexl-wasm`, `nexl-native`, plus potential new optimizer crate

**Spec sections to reference:**
- §12 Compilation Model

**Key design points:**
- Inlining small functions at call sites
- Escape analysis: stack-allocate closures/collections that don't escape
- Perceus reuse analysis: in-place mutation for uniquely-owned persistent data
- Dead code elimination
- Constant folding
- Specialization: monomorphize polymorphic functions
- Optional WASM GC backend
- Arena mode (`--gc none`) for short-lived WASM plugins

**Acceptance criteria:**
- Inlining pass reduces call overhead for small functions
- Escape analysis identifies stack-allocatable values
- DCE removes unreachable definitions
- Constant folding evaluates compile-time constants
- Benchmarks show measurable improvement

**When done:** Update this file to point to M18.

See `docs/todo-m17.md` for the task checklist.
See `milestones.md` for the full plan.
