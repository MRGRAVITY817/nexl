# M7 — Error Handling

## AST / Reader
- [x] Parse `panic` form
- [ ] Parse `assert!` and `assert-unreachable!` forms
- [ ] Parse `?` postfix operator (question-mark suffix on expressions)
- [ ] Parse contract clauses (`:requires`, `:ensures`, `:examples`) on `defn`
- [ ] Parse `try`/`catch` form

## Types / Inference
- [ ] `panic` typed as `Never`; `assert!` typed as `Unit`; `assert-unreachable!` typed as `Never`
- [ ] `?` operator type checking (unwrap Result, early return on Err)
- [ ] Contract clause type checking (`:requires`/`:ensures` must be Bool expressions)

## Eval / Runtime
- [x] `panic` evaluation (runtime termination with message + source location)
- [ ] `assert!` / `assert-unreachable!` evaluation
- [ ] `?` operator evaluation (early return from function on Err)
- [ ] Contract enforcement in dev mode (`:requires` before body, `:ensures` after)
- [ ] `try`/`catch` evaluation (desugar to match on Result)

## Blocked
- [ ] (none yet)
