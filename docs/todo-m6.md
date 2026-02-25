# M6 — Algebraic Effect System

## AST / Reader
- [ ] Parse `defeffect` declarations
- [ ] Parse `handle` forms (simple + continuation)
- [ ] Parse effect rows in type annotations (`! [E | r]`)

## Types / Inference
- [ ] Extend type representation with effect rows
- [ ] Row unification and polymorphism (`! [E | r]`)
- [ ] Effect inference for function bodies
- [ ] `handle` removes handled effects from rows
- [ ] Module `:performs` effect checking

## Effects / Runtime
- [ ] Evidence passing representation
- [ ] Continuation handlers (one-shot, ADR-003)
- [ ] Built-in effects: `Console`, `FileSystem`, `Time`, `Random`

## Blocked
- [ ] (none yet)
