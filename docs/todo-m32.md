# M32 — Flagship Project & 1.0 Preparation

## Goal
Build a substantial real-world Nexl project that exercises the full language
and enriched stdlib, then harden the language and toolchain for a 1.0 release.

(Delayed from original M28 to follow stdlib implementation.)

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

- [x] **nexl-functions** — Effect-sandboxed HTTP function host (flagship)
  - `nexl functions deploy <file.nx>` — register handler with name, capability, route
  - `nexl functions list` — list deployed functions
  - `nexl functions serve [--port 8080]` — HTTP server routing requests to handlers
  - `nexl functions logs <name>` — view invocation logs
  - `nexl functions invoke <name>` — direct invocation without HTTP
  - Capability levels: pure / read-only / full (sandbox policy enforcement)
  - HTML dashboard at `GET /` listing all functions
  - Registry at `.nexl-functions/registry.json`
  - Logs at `.nexl-functions/logs/<name>.jsonl`
  - Note: Stage 0 uses tree-walk evaluator; Wasmtime backend is Stage 1

- [x] **1.0 stability contract** — define stable vs experimental surface
  - Written at `docs/stability.md`
  - Stable: syntax, core forms, type system, effect system, stdlib, CLI, WIT interop
  - Experimental: WASI 0.3 async, native backend, specific optimizations
  - Backward compatibility promise documented
  - Edition mechanism design documented

- [x] **Migration guide & changelog** — pre-1.0 → 1.0
  - Written at `docs/migration.md`
  - Documents all breaking changes (naming, Unit vs nil, booleans, effects, match)
  - Automated `nexl migrate` tool spec
  - Stage 0 → 1 → 2 changelog

- [ ] **Public documentation site**
  - Language guide (tutorial progression)
  - Standard library API reference (auto-generated)
  - Effect system guide
  - WASM interop guide
  - Cookbook integration
  - Deferred: requires static site generator and hosting

- [x] **Community infrastructure**
  - `CONTRIBUTING.md` — development setup, workflow, good-first-issues
  - `CODE_OF_CONDUCT.md` — based on Contributor Covenant 2.1
  - Issue tracker labels, Discord, and GitHub Discussions are operational concerns

- [x] **AI/LLM readiness**
  - `examples/` directory with 14 canonical programs covering all language features
  - `CLAUDE.md` context file for AI coding assistants (already exists)
  - `docs/stability.md` and `docs/migration.md` for grounding
  - Example topics: hello, functions, collections, Option/Result, matching, JSON, HTTP, DB, testing, effects, crypto, channels, file I/O, nexl-functions handler
