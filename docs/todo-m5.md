# M5 — Module System

## AST (`nexl-ast`)
- [x] Add `module`, `import`, `export`, `re-export` AST nodes
- [x] Add qualified symbol support (`alias/name`) to AST (already existed: `Atom::Symbol { ns }`)

## Reader (`nexl-reader`)
- [x] Parse `(module name :exports [...] :performs [...])` form
- [x] Parse `(import mod :as alias)`, `:refer`, `:exclude`, `:rename` variants
- [x] Parse qualified symbols `foo/bar` as distinct from bare symbols

## Module Resolution (`nexl-modules` — new crate)
- [ ] Module name ↔ file path mapping (§8.11)
- [ ] Dependency graph construction from import declarations
- [ ] Topological sort of modules (§8.9 init order)
- [ ] Circular dependency detection (§8.6)

## Name Resolution
- [ ] Resolve qualified references (`alias/name`) to module exports
- [ ] Visibility enforcement: public, package-private, module-private (§8.8)
- [ ] Export validation: unexported names not importable

## Type Inference (`nexl-infer`)
- [ ] Cross-module type checking at import boundaries
- [ ] `:performs` row validation against exported function signatures

## Evaluation (`nexl-eval`)
- [ ] Multi-file evaluation with module initialization order
- [ ] Module-scoped environments

## Blocked
- [ ] Content-addressed definitions (§8.4 — depends on M18)
- [ ] Semantic versioning enforcement (§8.5 — depends on package manager, M15)
- [ ] Test submodules (§8.10 — depends on `nexl test` command, M9)
- [ ] `nexl.toml` full support (defer to M9/M15; basic path mapping only here)
