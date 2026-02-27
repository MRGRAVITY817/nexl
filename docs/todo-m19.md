# M19 — Core Evaluator Completeness

## Deliverables

- [x] 1. `match` form in evaluator
  - Literal patterns: Int, Float, Bool, Str, Keyword, Unit
  - Constructor patterns: `(Some x)`, `(None)`, `(Ok v)`, `(Err e)`
  - Map destructuring: `{:key pat}`
  - Wildcard `_` and binding patterns
  - Nested patterns (at least 1 level deep)
  - Tuple patterns: `[a b c]`
  - Or patterns: `(| p1 p2)`
  - Runtime error on no match

- [x] 2. `cond` form in evaluator
  - `(cond test1 expr1 test2 expr2 ... :else default)`

- [x] 3. Short-circuit `and` / `or`
  - Changed from eager NativeFn to special forms
  - `(and a b c)` stops at first falsy
  - `(or a b c)` stops at first truthy

- [x] 4. Fix variadic rest args
  - `(fn [x & rest] rest)` binds `rest` to a Vec of remaining args

- [x] 5. `deftype` in evaluator
  - Register constructors: `(deftype Color | Red | Green | Blue)`
  - Parameterized: `(deftype MyOption [a] | MyNone | (MySome a))`

- [x] 6. Multi-body `do` audit
  - Confirmed: `defn`, `let`, `fn`, `loop` all support multiple body expressions
