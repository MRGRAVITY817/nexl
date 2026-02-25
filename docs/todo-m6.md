# M6 — Algebraic Effect System

## AST / Reader
- [x] Parse `defeffect` declarations
- [x] Parse `handle` forms (simple + continuation)
- [x] Parse effect rows in type annotations (`! [E | r]`)

## Types / Inference
- [x] Extend type representation with effect rows
- [x] Row unification and polymorphism (`! [E | r]`)
- [x] Effect inference for function bodies
- [x] `handle` removes handled effects from rows
- [x] Module `:performs` effect checking

## Effects / Runtime
- [x] Evidence passing representation
- [ ] Continuation handlers (one-shot, ADR-003)
- [ ] Built-in effects: `Console`, `FileSystem`, `Time`, `Random`

## Blocked
- [ ] (none yet)
