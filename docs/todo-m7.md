# M7 — Error Handling

## AST / Reader
- [x] Parse `panic` form
- [x] Parse `assert!` and `assert-unreachable!` forms
- [x] Parse `?` postfix operator (question-mark suffix on expressions)
- [x] Parse contract clauses (`:requires`, `:ensures`, `:examples`) on `defn`
- [x] Parse `try`/`catch` form

## Types / Inference
- [x] `panic` typed as `Never`; `assert!` typed as `Unit`; `assert-unreachable!` typed as `Never`
- [x] `?` operator type checking (unwrap `Ok`/early-return `Err` for `Result`; unwrap `Some`/early-return `None` for `Option`; mixing the two in the same function is a compile error)
- [x] Contract clause type checking (`:requires`/`:ensures` must be Bool expressions)

## Eval / Runtime
- [x] `panic` evaluation (runtime termination with message + source location)
- [x] `assert!` / `assert-unreachable!` evaluation
- [x] `?` operator evaluation (early return from function on `Err` for `Result`; early return on `None` for `Option`)
- [x] Contract enforcement in dev mode (`:requires` before body, `:ensures` after)
- [x] `try`/`catch` evaluation (desugar to match on Result)

## Blocked
- [ ] (none yet)
