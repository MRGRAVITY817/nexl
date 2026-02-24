# Current Milestone: M3 — Algebraic Data Types + Pattern Matching

**Goal:** ADTs, records, destructuring, exhaustive match.

**Crates:** `nexl-types`, `nexl-infer`, `nexl-ast`

**Spec sections to reference:**
- §5.7 Algebraic Data Types (lines 1294–1378 of `nexl-spec.md`)
- §5.3 Composite Types — Tuple (lines 1204–1218)
- §4.9 `match` — Pattern Matching (lines 557–716)
- §5.6 Row Polymorphism (lines 1262–1292)

**Key ADRs:**
- ADR-005: Option and Result for absence and errors

**Acceptance criteria:**
- `cargo test` passes across all crates
- `deftype` supports sum types, record types, and parameterized ADTs
- Constructors work as functions: `Some : (Fn [a] -> (Option a))`
- Nullary constructors as polymorphic constants: `None : (Option a)`
- `match` form with exhaustiveness checking
- Pattern forms: literal, wildcard, variable, constructor, nested
- `:when` guards on match arms
- Record construction and keyword field access
- Tuple types (2–8 elements)
- `Option` and `Result` as built-in ADTs (ADR-005)
- Let destructuring with constructor, record, and tuple patterns
- Error messages follow Principle 6

**When done:** Update this file to point to M4.

See `docs/todo-m3.md` for the task checklist.
See `milestones.md` for the full plan.
