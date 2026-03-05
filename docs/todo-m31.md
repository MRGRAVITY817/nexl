# M31 — Concurrency, System & Stdlib Finalization

## Goal
Add concurrency primitives (channels, async, child processes), enhance the system
interface, and finalize the stdlib with integration tests and documentation.
After this milestone, the stdlib spec is fully implemented.

Reference: `docs/stdlib-spec.md`

## `channel` Module (New — Rust)

- [x] **CSP-style channels** — 8 functions (Stage 0 single-threaded)
  - Creation: `new` (unbuffered capacity 1), `buffered`
  - Send/receive: `send!`, `recv!`, `try-send!`, `try-recv!`
  - Lifecycle: `close!`, `closed?`
  - Note: `select!` deferred (requires evaluator special form), `into-iter` deferred (requires Iter integration)

## `async` Module (Enhance — Rust)

- [x] **Concurrency primitives** — 6 new functions (Stage 0: synchronous execution)
  - `spawn`: execute thunk, wrap result in `(Future val)`
  - `await`: unwrap a Future
  - `timeout`: run thunk, return `(Ok val)` (Stage 0: timeout never fires)
  - `all`: run Vec of thunks, return Vec of results
  - `race`: return first thunk result (Stage 0: first in list)
  - `defer`: run thunk, always run cleanup, return thunk result

## `process` Module (New — Rust)

- [x] **Child process management** — 6 functions
  - `run`: run shell command, return `(Result Output Str)`
  - `run-with`: run with ProcessOpts map `{:cmd :args :cwd :env :stdin}`
  - `spawn`: spawn and return handle (Stage 0: runs to completion)
  - `wait`: wait for handle, return `(Result Output Str)`
  - `kill`: mark process as killed
  - `pid`: current process ID
  - `Output` record: `{:exit-code Int :stdout Str :stderr Str}`

## Enhanced `sys` (Rust)

- [x] **System interface** — 6 new functions
  - `os`: operating system name (`"macos"`, `"linux"`, `"windows"`)
  - `arch`: CPU architecture (`"aarch64"`, `"x86_64"`)
  - `cpu-count`: number of available CPUs
  - `cwd`: current working directory path
  - `home-dir`: user home directory as `(Option Str)`
  - `exe-path`: path to current executable as `(Option Str)`

## Enhanced `log` (Rust)

- [x] **Logging enhancements** — 2 new functions
  - `with-logger`: run body with custom log sink `(Fn [Str] -> Unit)`; restores previous logger
  - `context`: return current merged context fields as `(Map Keyword Str)`

## Stdlib Finalization

- [x] **Integration tests** — e2e test fixtures for all M31 modules
  - `channel_stdlib.nx` — channel create, send, recv, close lifecycle
  - `async_stdlib.nx` — sleep, spawn/await, timeout, all, race, defer
  - `process_stdlib.nx` — pid, run echo, exit code capture
  - `sys_stdlib.nx` — os, arch, cpu-count, cwd, home-dir, exe-path
  - `log_stdlib.nx` — with-logger custom sink, context retrieval

- [ ] **Stdlib documentation** — docstrings for every public function
  - Triple-quoted docstrings with usage examples
  - `nexl doc` generates complete stdlib API reference
  - Cross-references between related functions
  - Deferred: nexl doc generator is M32 scope

- [ ] **Performance baselines** — benchmark critical paths
  - Deferred: formal benchmarking infrastructure is M32 scope
