# M2: Type Inference (Core)

## Done

## In Progress

## Todo

### nexl-types — Type Representation
- [x] Add `nexl-types` crate to workspace
- [x] `Type` enum: Int, Float, Ratio, Bool, Char, Str, Keyword, Symbol, Unit, Never
- [x] Fixed-width numeric types: Int8, Int16, Int32, Int64, U8, U16, U32, U64, F32, F64
- [x] `Type::Var(TypeVar)` — unification variables
- [x] `Type::Fn { params, ret }` — function types `(Fn [A B] -> C)`
- [x] `Display` impl for types (human-readable format)
- [x] `TypeVar` generation (unique IDs via counter)
- [x] Substitution: `Subst` type (TypeVar → Type map) with `apply` method
- [x] `Scheme` — polymorphic type (forall quantifier for let-generalization)

### nexl-types — Unification
- [x] `unify(a, b, subst)` — Robinson unification of two types
- [x] Occurs check (prevents infinite types)
- [x] Error recovery: continue after first unification failure
- [x] Type error representation with source spans

### nexl-infer — Bidirectional Inference Engine
- [x] Add `nexl-infer` crate to workspace
- [x] Context type: typing environment (name → Scheme), substitution state
- [x] Synthesize mode: literal → type (Int literal → Int, Float → Float, etc.)
- [x] Synthesize mode: variable lookup (instantiate polymorphic schemes)
- [x] Check mode: expression against expected type
- [x] Infer `def` form — bind inferred type in context
- [x] Infer `let` form — sequential bindings with let-generalization
- [x] Infer `do` form — check each expr, return type of last
- [x] Infer `if` form — condition must be Bool, branches must unify
- [x] Infer `fn` form — fresh type vars for params, infer body
- [x] Infer `defn` form — sugar for def + fn, same as fn
- [x] Infer function application — callee must be Fn, unify arg types
- [x] Infer `loop`/`recur` — loop vars typed from init, recur must match
- [x] Polymorphic let-generalization (generalize unbound type vars)
- [x] Literal suffix inference: `42i32` → Int32, `3.14f32` → F32

### nexl-infer — Type Annotations
- [x] Parse `: Type` annotations on `def` bindings
- [x] Parse `: Type` annotations on `defn` params and `-> RetType`
- [x] Parse `: Type` annotations on `let` bindings
- [x] Check annotations: unify annotation with inferred type

### nexl-infer — Error Messages (Principle 6)
- [x] "Expected X but got Y" with source span
- [ ] "Cannot add Int and Float. Use (->float n) to convert." (ADR-006)
- [ ] Suggest fixes for common mismatches
- [ ] Multiple errors collected (don't stop at first)

### Test Suite
- [x] Unit tests for `Type` construction and display
- [x] Unit tests for substitution and occurs check
- [x] Unit tests for unification (success and failure cases)
- [x] Unit tests for literal type synthesis
- [ ] Unit tests for each core form inference
- [ ] Unit tests for let-generalization (polymorphism)
- [ ] Unit tests for type annotation checking
- [ ] Unit tests for cross-type arithmetic error (ADR-006)
- [ ] Integration test: infer `fibonacci` type as `(Fn [Int] -> Int)`
- [ ] Integration test: type error on `(add 1 "hello")`

## Blocked
(none)
