# M5 — Module System

## AST (`nexl-ast`)
- [x] Add `module`, `import`, `export`, `re-export` AST nodes
- [x] Add qualified symbol support (`alias/name`) to AST (already existed: `Atom::Symbol { ns }`)

## Reader (`nexl-reader`)
- [x] Parse `(module name :exports [...] :performs [...])` form
- [x] Parse `(import mod :as alias)`, `:refer`, `:exclude`, `:rename` variants
- [x] Parse qualified symbols `foo/bar` as distinct from bare symbols

## Module Resolution (`nexl-modules` — new crate)
- [x] Module name ↔ file path mapping (§8.11)
- [x] Dependency graph construction from import declarations
- [x] Topological sort of modules (§8.9 init order)
- [x] Circular dependency detection (§8.6)

## Name Resolution
- [x] Resolve qualified references (`alias/name`) to module exports
- [x] Visibility enforcement: public, package-private, module-private (§8.8)
- [x] Export validation: unexported names not importable

## Type Inference (`nexl-infer`)
- [x] Cross-module type checking at import boundaries
- [x] `:performs` row validation against exported function signatures

## Evaluation (`nexl-eval`)
- [x] Multi-file evaluation with module initialization order
- [x] Module-scoped environments

## Blocked
- [ ] Content-addressed definitions (§8.4 — depends on M18)
- [ ] Semantic versioning enforcement (§8.5 — depends on package manager, M15)
- [ ] Test submodules (§8.10 — depends on `nexl test` command, M9)
- [ ] `nexl.toml` full support (defer to M9/M15; basic path mapping only here)
