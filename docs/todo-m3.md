# M3: Algebraic Data Types + Pattern Matching

## Done

## In Progress

## Todo

### nexl-types — ADT Type Representation
- [x] `Type::Adt { name, args }` — applied ADT type (e.g. `(Option Int)`, `Color`)
- [x] `TypeDef` struct — type name, type params, list of constructors
- [x] `Constructor` struct — constructor name, field types (positional)
- [x] `Display` impl for `Type::Adt`
- [x] Unification of `Type::Adt` — same name, unify args pairwise
- [x] `Subst::apply` and `free_vars` for `Type::Adt`

### nexl-types — Record & Tuple Types
- [x] `Type::Record { name, fields }` — named record type (nominal, not structural)
- [x] `Type::Tuple(Vec<Type>)` — heterogeneous product, 2–8 elements
- [x] `Display` impl for Record and Tuple types
- [x] Unification for Record and Tuple types
- [x] `Subst::apply` and `free_vars` for Record and Tuple

### nexl-infer — deftype Form
- [ ] Parse `(deftype Name | Ctor1 | (Ctor2 a))` — sum type declarations
- [ ] Parse `(deftype Name {:field Type})` — record type declarations
- [ ] Parse `(deftype Name [a] | ...)` — parameterized ADTs
- [ ] Register type definition and constructors in typing environment
- [ ] Nullary constructors as polymorphic constants: `None : (Option a)`
- [ ] N-ary constructors as functions: `Some : (Fn [a] -> (Option a))`
- [ ] Record constructors: `Point : (Fn [{:x Float :y Float}] -> Point)`

### nexl-infer — Constructor Application & Field Access
- [ ] Infer constructor application: `(Some 42)` → `(Option Int)`
- [ ] Infer nullary constructor usage: `None` → `(Option a)` (fresh var)
- [ ] Infer record construction: `(Point {:x 1.0 :y 2.0})` → `Point`
- [ ] Infer keyword field access: `(:x point)` → `Float`

### nexl-ast — Pattern AST
- [ ] `Pattern` enum: Wildcard, Var, Literal, Constructor, Record, Tuple, Or, As
- [ ] Pattern parser: AST nodes → Pattern (from match arm position)

### nexl-infer — match Form
- [ ] Parse `(match expr arm1 arm2 ...)` — extract scrutinee + pattern/body pairs
- [ ] Infer scrutinee type
- [ ] Check each pattern against scrutinee type
- [ ] Unify all arm body types to a common return type
- [ ] `:when` guard — guard must be Bool
- [ ] Wildcard and variable patterns
- [ ] Literal patterns (Int, Str, Bool, Keyword)
- [ ] Constructor patterns: `(Some x)`, `None`
- [ ] Nested patterns

### nexl-infer — Exhaustiveness Checking
- [ ] Missing patterns → compile error
- [ ] Redundant patterns → warning
- [ ] Exhaustiveness for simple enums (Color, Bool)
- [ ] Exhaustiveness for parameterized ADTs (Option, Result)

### nexl-infer — let Destructuring
- [ ] Constructor patterns in let: `(let [(Some v) maybe-val] ...)`
- [ ] Record destructuring: `(let [{:keys [x y]} point] ...)`
- [ ] Tuple destructuring: `(let [[a b] pair] ...)`
- [ ] Non-exhaustive let pattern → compile error

### Built-in ADTs
- [ ] `Option` type definition (ADR-005)
- [ ] `Result` type definition (ADR-005)

### Test Suite
- [ ] Unit tests for ADT type construction, display, unification
- [ ] Unit tests for Record and Tuple types
- [ ] Unit tests for deftype form parsing and env registration
- [ ] Unit tests for constructor application inference
- [ ] Unit tests for match form inference
- [ ] Unit tests for exhaustiveness checking
- [ ] Unit tests for let destructuring
- [ ] Integration test: deftype + match end-to-end

## Blocked
(none)
