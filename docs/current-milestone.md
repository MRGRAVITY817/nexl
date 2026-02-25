# Current Milestone: M8 — WASM Backend

**Goal:** Compile Nexl programs to `.wasm` files that run in Wasmtime/browser.

**Crates:** `nexl-ir`, `nexl-wasm`, `nexl-memory`, `nexl-vm`, `nexl-cli`

**Spec sections to reference:**
- §12 Compilation Model (lines 2833–2926)

**Key ADRs:**
- (none specific to M8)

**Acceptance criteria:**
- IR design: lowered representation (closures → env structs, match → decision trees, `?` → jumps)
- WASM core module codegen (functions, closures, ADTs, strings, collections)
- Memory management — Perceus RC (dup/drop insertion, reuse analysis)
- Effect runtime in WASM (evidence vectors, tail-resumptive handlers)
- `cargo test` passes across all crates

**When done:** Update this file to point to M9.

See `docs/todo-m8.md` for the task checklist.
See `milestones.md` for the full plan.
