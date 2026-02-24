# M4 — Persistent Collections

## Type System (`nexl-types`)
- [x] Add `Vec`, `Map`, `Set` type constructors to `Type` enum
- [x] Add substitution, free-vars, display, and unification support for collection types

## Runtime Values (`nexl-runtime`)
- [x] Add `Vec`, `Map`, `Set` value variants to `Value` enum
- [x] Implement Display, PartialEq, type_name for collection values

## Evaluation (`nexl-eval`)
- [x] Evaluate vector literals `[1 2 3]` → `Value::Vec`
- [x] Evaluate map literals `{:a 1 :b 2}` → `Value::Map`
- [ ] Evaluate set literals `#{1 2 3}` → `Value::Set`

## Type Inference (`nexl-infer`)
- [ ] Infer vector literals as `(Vec a)` (homogeneous elements)
- [ ] Infer map literals as `(Map k v)` (homogeneous keys and values)
- [ ] Infer set literals as `(Set a)` (homogeneous elements)
- [ ] Distinguish `(Tuple a b)` from `(Vec a)` based on context

## Collection Operations — Built-in Functions
- [ ] Vec: `get`, `put`, `append`, `count`, `first`, `rest`, `last`, `slice`
- [ ] Map: `get`, `put`, `remove`, `keys`, `vals`, `entries`, `contains?`, `count`
- [ ] Set: `add`, `remove`, `contains?`, `count`, `union`, `intersection`, `difference`
- [ ] Type inference for collection operations

## Sequence Operations (stretch — may defer to later)
- [ ] `map`, `filter`, `reduce` (compiler-dispatched)
- [ ] `each`, `times` forms
- [ ] `for` / `for!` comprehensions

## Blocked
- [ ] List type (low priority — primarily used in macros, defer to M10)
- [ ] Iter/lazy sequences (depends on effect system for fusion, defer)
- [ ] Transients (depends on escape analysis, defer)
