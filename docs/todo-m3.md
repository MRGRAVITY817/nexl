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
- [x] Parse `(deftype Name | Ctor1 | (Ctor2 a))` — sum type declarations
- [x] Parse `(deftype Name {:field Type})` — record type declarations
- [x] Parse `(deftype Name [a] | ...)` — parameterized ADTs
- [x] Register type definition and constructors in typing environment
- [x] Nullary constructors as polymorphic constants: `None : (Option a)`
- [x] N-ary constructors as functions: `Some : (Fn [a] -> (Option a))`
- [x] Record constructors: `Point : (Fn [{:x Float :y Float}] -> Point)`

### nexl-infer — Constructor Application & Field Access
- [x] Infer constructor application: `(Some 42)` → `(Option Int)`
- [x] Infer nullary constructor usage: `None` → `(Option a)` (fresh var)
- [x] Infer record construction: `(Point {:x 1.0 :y 2.0})` → `Point`
- [x] Infer keyword field access: `(:x point)` → `Float`

### nexl-ast — Pattern AST
- [x] `Pattern` enum: Wildcard, Var, Literal, Constructor, Record, Tuple, Or, As
- [x] Pattern parser: AST nodes → Pattern (from match arm position)

### nexl-infer — match Form
- [x] Parse `(match expr arm1 arm2 ...)` — extract scrutinee + pattern/body pairs
- [x] Infer scrutinee type
- [x] Check each pattern against scrutinee type
- [x] Unify all arm body types to a common return type
- [x] `:when` guard — guard must be Bool
- [x] Wildcard and variable patterns
- [x] Literal patterns (Int, Str, Bool, Keyword)
- [x] Constructor patterns: `(Some x)`, `None`
- [x] Nested patterns

### nexl-infer — Exhaustiveness Checking
- [x] Missing patterns → compile error
- [x] Redundant patterns → warning
- [x] Exhaustiveness for simple enums (Color, Bool)
- [x] Exhaustiveness for parameterized ADTs (Option, Result)

### nexl-infer — let Destructuring
- [x] Constructor patterns in let: `(let [(Some v) maybe-val] ...)`
- [x] Record destructuring: `(let [{:keys [x y]} point] ...)`
- [x] Tuple destructuring: `(let [[a b] pair] ...)`
- [x] Non-exhaustive let pattern → compile error

### Built-in ADTs
- [x] `Option` type definition (ADR-005)
- [x] `Result` type definition (ADR-005)

### Test Suite
- [x] Unit tests for ADT type construction, display, unification
- [x] Unit tests for Record and Tuple types
- [x] Unit tests for deftype form parsing and env registration
- [x] Unit tests for constructor application inference
- [x] Unit tests for match form inference
- [x] Unit tests for exhaustiveness checking
- [x] Unit tests for let destructuring
- [x] Integration test: deftype + match end-to-end

## Blocked
(none)
