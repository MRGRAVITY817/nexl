# Current Milestone: M5 — Module System

**Goal:** Multi-file compilation with imports, exports, visibility, and qualified access.

**Crates:** `nexl-ast`, `nexl-reader`, `nexl-modules` (new), `nexl-infer`, `nexl-eval`

**Spec sections to reference:**
- §8 Module System (lines 2447–2686) — module declarations, imports, visibility, init order
- §8.1 Module Declaration (lines 2449–2474)
- §8.2 Importing Modules (lines 2476–2504)
- §8.3 Namespace (lines 2506–2517) — qualified access
- §8.6 Circular Dependencies (line 2562)
- §8.8 Visibility (lines 2598–2622) — public, package-private, module-private
- §8.9 Module Initialization Order (lines 2624–2632)
- §8.11 Package ↔ Module Relationship (lines 2667–2684)

**Key ADRs:** none yet (consult `decisions/` as they appear)

**Acceptance criteria:**
- `(module ...)` and `(import ...)` forms parse to AST nodes
- Qualified symbols (`alias/name`) are distinct from bare symbols
- Module dependency graph with topological sort and cycle detection
- Visibility enforcement (public, package-private, module-private)
- Cross-module type checking at import boundaries
- Multi-file evaluation with correct initialization order
- `cargo test` passes across all crates

**When done:** Update this file to point to M6.

See `docs/todo-m5.md` for the task checklist.
See `milestones.md` for the full plan.
