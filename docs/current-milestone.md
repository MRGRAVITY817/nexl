# Current Milestone: M7 — Error Handling

**Goal:** `Result`/`Option` ergonomics, `?` operator, `panic`, `assert!`, contracts.

**Crates:** `nexl-ast`, `nexl-reader`, `nexl-types`, `nexl-infer`, `nexl-eval`, `nexl-runtime`

**Spec sections to reference:**
- §9 Error Handling (lines 2688–2802)
- §9.3 The `?` Operator (lines 2714–2760)
- §9.4 `panic` (lines 2762–2776)
- §4.2.1 Function Contracts (lines 403–479)

**Key ADRs:**
- (none specific to M7)

**Acceptance criteria:**
- `panic` terminates with message and source location
- `assert!` checks condition, panics on false
- `assert-unreachable!` typed as `Never`, always panics
- `?` operator propagates `Err` from `Result`, unwraps `Ok`
- Contract clauses (`:requires`, `:ensures`, `:examples`) parsed and enforced in dev mode
- `try`/`catch` compiles to `match` on `Result`
- `cargo test` passes across all crates

**When done:** Update this file to point to M8.

See `docs/todo-m7.md` for the task checklist.
See `milestones.md` for the full plan.
