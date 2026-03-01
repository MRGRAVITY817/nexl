# M26 — Flagship Project & 1.0 Preparation

## Tasks

- [x] **Triple-quoted and raw string literals** — `"""..."""` auto-dedent, `r"..."` / `r#"..."#` verbatim
  - `StringKind` enum (Regular, Triple, Raw) in nexl-reader; `dedent_triple` in reader
  - 18 new tests (6 triple, 6 raw, 6 dedent); all existing tests green
  - 18 stdlib `.nx` docstrings converted to triple-quoted (section headers flush-left in hover)
  - nexl-spec §2.4 + Appendix D updated; tree-sitter `raw_string_literal` rule added

- [x] **nexl-market** — Multi-vendor marketplace (DDD) at `/Users/tripboi/Projects/nexl-market/`
  - 5 bounded contexts: Catalog, Orders, Payments, Delivery, Accounts
  - deftype records + sum types, `:requires`/`:ensures` contracts
  - SQLite CRUD across all contexts
  - `log/with` structured logging, `env/load-dotenv`, `json/pretty`
  - 14 tests across 3 test files (all green)
  - `nexl doc` generates HTML API docs
  - Full order lifecycle integration demo in `src/main.nx`

- [ ] **nexl-functions** — Effect-sandboxed WASM plugin host (flagship)
  - HTTP trigger: incoming request → run function → return response
  - Capability levels: pure / read-only / full
  - `nexl functions deploy` command
  - Dashboard: deployed functions, capabilities, invocation logs
  - Runs locally via Wasmtime

- [ ] **1.0 stability contract** — define stable vs experimental surface
  - Stable: syntax, core forms, type system, effect system, stdlib, CLI, WIT interop
  - Experimental: WASI 0.3 async, native backend, specific optimizations
  - Backward compatibility promise
  - Edition mechanism design

- [ ] **Migration guide & changelog** — pre-1.0 → 1.0
  - Document every breaking change
  - `nexl migrate` tool for automated fixups
  - Stage 0 → Stage 1 → Stage 2 changelog

- [ ] **Public documentation site**
  - Language guide (tutorial progression)
  - Standard library API reference (auto-generated)
  - Effect system guide
  - WASM interop guide
  - Cookbook integration

- [ ] **Community infrastructure**
  - GitHub Discussions or Discord
  - Good-first-issue labels
  - Contributing guide
  - Code of conduct

- [ ] **AI/LLM readiness**
  - Context files for AI coding assistants
  - Canonical example corpus (50+ programs)
  - Verify Claude can write basic Nexl from documentation context
