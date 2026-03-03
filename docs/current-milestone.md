# Current: M27 — nexl.test in Nexl (Macro Self-Hosting)

**Goal:** Move nexl.test from Rust special forms in `eval.rs` to Nexl macros in
`test.nx`. Proves Nexl's macro system is production-grade by dogfooding it.
Zero nexl.test special forms in `eval.rs` when done.

Part of Stage 2 (M23–M28): Real-World Readiness.
M23 (WASI Integration & Interop) completed 2026-02-28.
M24 (Hello Production Stack) completed 2026-02-28.
M25 (Developer Experience & Toolchain Polish) completed 2026-02-28.
M26 (nexl.test: Effect-Powered Testing Library) completed 2026-03-02.

See `docs/todo-m27.md` for the task checklist.
See `docs/todo-m28.md` for the next milestone (Flagship + 1.0).
