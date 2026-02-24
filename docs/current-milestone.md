# Current Milestone: M2 — Type Inference (Core)

**Goal:** Bidirectional type inference for all M1 forms. Type errors with helpful messages.

**Crates:** `nexl-types`, `nexl-infer`

**Spec sections to reference:**
- §5 Type System (lines 1150–1518 of `nexl-spec.md`)
- §5.2 Primitive Types
- §5.4 Type Inference (bidirectional)
- §5.5 Type Annotations

**Key ADRs:**
- ADR-006: Cross-type arithmetic is a compile error

**Acceptance criteria:**
- `cargo test` passes across all crates
- Primitive types are representable: Int, Float, Ratio, Bool, Char, Str, Keyword, Symbol, Unit, Never
- Function types: `(Fn [A B] -> C)`
- Type variables with unification (occurs check, substitution)
- Bidirectional inference: check mode + synthesize mode
- Polymorphic `let` generalization (Hindley-Milner)
- Literal type inference (Int default, Float default, suffixed literals)
- Cross-type arithmetic detected as type error (ADR-006)
- Type annotations on `def`, `defn`, `let` are checked
- Error messages follow Principle 6 (expected vs found, with suggestions)
- Example: `(defn add [x y] (+ x y))` infers `(Fn [Int Int] -> Int)`
- Example: `(add 1 "hello")` produces a clear type error

**When done:** Update this file to point to M3.

See `docs/todo-m2.md` for the task checklist.
See `milestones.md` for the full plan.
