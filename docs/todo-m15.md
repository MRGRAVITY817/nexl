# M15 — Advanced Toolchain

## Package Manager Foundation

## Documentation

## Sandbox
- [x] Implement `nexl sandbox` with `--allow-*` flags
- [x] Map CLI flags to effect capabilities in evaluator
- [x] Deny uncovered effects at runtime (Stage 0: runtime check)

## Audit
- [ ] Implement `nexl audit` — scan for FFI trust boundaries (`defextern`)
- [ ] Report which effects each dependency performs (transitive)

## Blocked
- [ ] (none)

## Done
- [x] Create `nexl-lsp` crate with tower-lsp scaffold
- [x] Implement `textDocument/publishDiagnostics` (parse errors + type errors)
- [x] Implement `textDocument/hover` (type signature + docstring)
- [x] Implement `textDocument/definition` (go-to-definition)
- [x] Implement `textDocument/completion` (symbols in scope)
- [x] Wire `nexl lsp` subcommand in CLI
- [x] Create `nexl-pkg` crate with `project.nexl` schema (EDN format)
- [x] Implement `project.nexl` parser (package name, version, deps, prefix)
- [x] Implement dependency resolution (flat, no version conflicts for Stage 0)
- [x] Wire `nexl pkg add/remove/lock` subcommands in CLI
- [x] Content-addressed definition store (SQLite-backed hash→artifact)
- [x] Create `nexl-doc` crate with HTML generation
- [x] Extract doc comments, type signatures, effects, contracts from source
- [x] Generate per-module HTML pages with cross-links
- [x] Wire `nexl doc` subcommand in CLI
