# M2: Type Inference (Core)

## Done

## In Progress

## Todo

### nexl-types — Type Representation
- [ ] Add `nexl-types` crate to workspace
- [ ] `Type` enum: Int, Float, Ratio, Bool, Char, Str, Keyword, Symbol, Unit, Never
- [ ] Fixed-width numeric types: Int8, Int16, Int32, Int64, U8, U16, U32, U64, F32, F64
- [ ] `Type::Var(TypeVar)` — unification variables
- [ ] `Type::Fn { params, ret }` — function types `(Fn [A B] -> C)`
- [ ] `Display` impl for types (human-readable format)
- [ ] `TypeVar` generation (unique IDs via counter)
- [ ] Substitution: `Subst` type (TypeVar → Type map) with `apply` method
- [ ] `Scheme` — polymorphic type (forall quantifier for let-generalization)

### nexl-types — Unification
- [ ] `unify(a, b, subst)` — Robinson unification of two types
- [ ] Occurs check (prevents infinite types)
- [ ] Error recovery: continue after first unification failure
- [ ] Type error representation with source spans

### nexl-infer — Bidirectional Inference Engine
- [ ] Add `nexl-infer` crate to workspace
- [ ] Context type: typing environment (name → Scheme), substitution state
- [ ] Synthesize mode: literal → type (Int literal → Int, Float → Float, etc.)
- [ ] Synthesize mode: variable lookup (instantiate polymorphic schemes)
- [ ] Check mode: expression against expected type
- [ ] Infer `def` form — bind inferred type in context
- [ ] Infer `let` form — sequential bindings with let-generalization
- [ ] Infer `do` form — check each expr, return type of last
- [ ] Infer `if` form — condition must be Bool, branches must unify
- [ ] Infer `fn` form — fresh type vars for params, infer body
- [ ] Infer `defn` form — sugar for def + fn, same as fn
- [ ] Infer function application — callee must be Fn, unify arg types
- [ ] Infer `loop`/`recur` — loop vars typed from init, recur must match
- [ ] Polymorphic let-generalization (generalize unbound type vars)
- [ ] Literal suffix inference: `42i32` → Int32, `3.14f32` → F32

### nexl-infer — Type Annotations
- [ ] Parse `: Type` annotations on `def` bindings
- [ ] Parse `: Type` annotations on `defn` params and `-> RetType`
- [ ] Parse `: Type` annotations on `let` bindings
- [ ] Check annotations: unify annotation with inferred type

### nexl-infer — Error Messages (Principle 6)
- [ ] "Expected X but got Y" with source span
- [ ] "Cannot add Int and Float. Use (->float n) to convert." (ADR-006)
- [ ] Suggest fixes for common mismatches
- [ ] Multiple errors collected (don't stop at first)

### Test Suite
- [ ] Unit tests for `Type` construction and display
- [ ] Unit tests for substitution and occurs check
- [ ] Unit tests for unification (success and failure cases)
- [ ] Unit tests for literal type synthesis
- [ ] Unit tests for each core form inference
- [ ] Unit tests for let-generalization (polymorphism)
- [ ] Unit tests for type annotation checking
- [ ] Unit tests for cross-type arithmetic error (ADR-006)
- [ ] Integration test: infer `fibonacci` type as `(Fn [Int] -> Int)`
- [ ] Integration test: type error on `(add 1 "hello")`

## Blocked
(none)
