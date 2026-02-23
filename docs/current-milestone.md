# Current Milestone: M1 — Tree-Walk Interpreter + Core Forms

**Goal:** Evaluate basic Nexl programs in a tree-walk interpreter. No types yet.

**Crates:** `nexl-eval`, `nexl-runtime`

**Spec sections to reference:**
- §4 Core Forms (lines 308–1148 of `nexl-spec.md`)
- §2 Lexical Grammar (lines 51–164) — for literal evaluation

**Acceptance criteria:**
- `cargo test` passes across all crates
- All M1 value types are representable and display correctly
- Core forms (`def`, `let`, `if`, `do`, `fn`, `defn`) evaluate correctly
- Arithmetic and comparison builtins work (Int-only and Float-only)
- `loop`/`recur` evaluates with TCO
- Fibonacci example from milestones.md runs correctly

**When done:** Update this file to point to M2.

See `docs/todo-m1.md` for the task checklist.
See `milestones.md` for the full plan.
