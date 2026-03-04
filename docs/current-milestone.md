# Current: M28 — Stdlib Core & Enrichment

**Goal:** Enrich existing Rust stdlib modules with missing essentials, add the
`option`, `result`, and `core` modules, and build the infrastructure for writing
stdlib modules in Nexl.

Part of the Stdlib Implementation arc (M28–M31), followed by Flagship & 1.0 (M32).

M23 (WASI Integration & Interop) completed 2026-02-28.
M24 (Hello Production Stack) completed 2026-02-28.
M25 (Developer Experience & Toolchain Polish) completed 2026-02-28.
M26 (nexl.test: Effect-Powered Testing Library) completed 2026-03-02.
M27 (nexl.test in Nexl — Macro Self-Hosting) completed 2026-03-04.

## Milestone Roadmap

| Milestone | Focus | Modules | Language |
|-----------|-------|---------|----------|
| **M28** | Core & Enrichment | builtins, str, math, conv, core, option, result | Rust + Nexl |
| **M29** | Collections & Iteration | vec, map, set, iter, char, regex, threading macros | Rust + Nexl |
| **M30** | Production Stack | path, uri, csv, toml, base64, uuid, bit, atom, crypto, http, time, random, error types | Rust |
| **M31** | Concurrency & Finalization | channel, async, process, sys, log, integration tests, docs | Rust |
| **M32** | Flagship Project & 1.0 | nexl-functions, stability contract, docs site, community | Mixed |

See `docs/todo-m28.md` for the task checklist.
See `docs/stdlib-spec.md` for the full stdlib specification.
