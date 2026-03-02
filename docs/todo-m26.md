# M26 — nexl.test: Effect-Powered Testing Library

## Phase 0: `defhandler` — Language-Level Named Effect Handlers

- [x] `defhandler` AST node in nexl-ast
- [x] `defhandler` parsing in nexl-reader (impl-style flat syntax, uppercase = effect section)
- [x] `defhandler` evaluation in nexl-eval (simple, continuation, parameterized, multi-effect)
- [x] `handle [HandlerName]` — install named handler by reference
- [x] `handle [(HandlerName args)]` — install parameterized handler
- [x] `defhandler` type inference in nexl-infer (completeness checking, effect row)
- [x] `defhandler` LSP support (hover, go-to-definition)
- [x] E2E tests for defhandler (simple, continuation, parameterized, multi-effect, nested)

## Phase 1: Core (MVP)

- [x] `is` macro — power-assert expansion for `=`, `not=`, predicates, `<`/`>`
- [x] `deftest` macro — registration + test runner discovery
- [x] `describe` macro — nesting, scoped naming
- [x] `throws?` assertion
- [x] Updated `nexl test` CLI — discovery, filtering (`--filter`), output formatting
- [x] `:skip` and `:focus` support on `deftest`
- [x] Backward compat: keep old `test/` API working alongside new API

## Phase 2: Data, Patterns & Lifecycle

- [x] `is-match` — pattern matching assertions with destructuring, guards, pins
- [x] `each` — table-driven tests (data-driven)
- [x] `:let` clause on `describe`
- [x] `:tags` support with CLI `--tags` filtering
- [x] String/collection diff output in error messages
- [x] `setup`/`teardown`/`setup-all`/`teardown-all` lifecycle hooks

## Phase 3: Effect-Based Mocking

- [x] `call-log` test utility (recording wrapper for effect operations)
- [x] Capability-aware test sandboxing (unhandled effects = compile error)
- [x] `SequentialExecutor` for deterministic concurrent testing
- [x] `submodule test` support (compile-time exclusion from release)

## Phase 4: Property Testing

- [x] Generator primitives and combinators
- [x] `check` form inside `deftest`
- [x] Integrated shrinking (generators carry shrink trees)
- [ ] `Arbitrary` protocol with auto-derive for ADTs/records
- [x] Failure persistence (`.test-seeds`)

## Phase 5: Snapshots, Doctests & Contracts

- [ ] `snap!` inline snapshots with source rewriting (deferred — requires source rewriting)
- [x] `snap-file!` file-based snapshots
- [x] `--accept` CLI command (deferred `--review` — interactive diff out of scope)
- [x] Contract-driven testing (`:examples` auto-execution)
- [x] Doctest `>>>` parsing from docstrings

## Phase 6: Polish & Performance

- [ ] `--watch` mode with smart re-runs
- [ ] `--parallel` cross-module execution
- [x] `--format json` output
- [x] `bench` form and `nexl bench` command
- [x] Matcher protocol and built-in matchers
- [ ] `--coverage` support
- [x] `:flaky`, `:timeout` annotations
