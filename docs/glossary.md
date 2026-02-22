# Nexl Glossary

Terms specific to Nexl and its implementation.

| Term | Meaning |
|------|---------|
| **Effect row** | The `! [Eff1 Eff2]` annotation on function signatures declaring which effects may occur |
| **Handler** | A `handle` form that intercepts effect operations and provides implementations |
| **Capability** | Effect-based permission; granted/restricted via `handle` (see spec §8.2) |
| **Refinement type** | Subset type with compile-time predicate checking |
| **Row polymorphism** | Extensible record/effect types with `\| r` rest variable |
| **One-shot continuation** | A continuation that can be resumed at most once (ADR-003) |
| **Evidence passing** | Compilation strategy where effect handlers become dictionary arguments |
| **Perceus RC** | Reference-counting memory management with precise drop insertion (Koka-inspired) |
| **Scope sets** | Macro hygiene model from Flatt 2016 (ADR-009) |
| **Reader** | The lexer + S-expression parser phase (text → AST) |
| **Form** | Any S-expression: atom or list |
| **Special form** | A built-in syntactic construct (`if`, `let`, `fn`, etc.) recognized by the compiler |
| **ADT** | Algebraic Data Type — defined with `deftype` using `\|` prefix variants |
| **Unit** | The single value of the `Unit` type, written `unit` — not nil/null (ADR-001) |
