# M31 — Concurrency, System & Stdlib Finalization

## Goal
Add concurrency primitives (channels, async, child processes), enhance the system
interface, and finalize the stdlib with integration tests and documentation.
After this milestone, the stdlib spec is fully implemented.

Reference: `docs/stdlib-spec.md`

## `channel` Module (New — Rust)

- [ ] **CSP-style channels** — ~10 functions
  - Creation: `new` (unbuffered), `buffered`
  - Send/receive: `send!`, `recv!`, `try-send!`, `try-recv!`
  - Lifecycle: `close!`, `closed?`
  - Special: `select!` (special form — wait on multiple channels)
  - Integration: `into-iter` (consume channel as lazy Iter)
  - Stage 0: backed by `std::sync::mpsc`

## `async` Module (Enhance — Rust)

- [ ] **Concurrency primitives** — ~6 new functions
  - `spawn`: run function concurrently, returns Future
  - `await`: block until Future completes
  - `timeout`: run with time limit
  - `all`: wait for all Futures
  - `race`: wait for first Future to complete
  - `defer`: guaranteed cleanup (like Go's defer)
  - Stage 0: backed by OS threads

## `process` Module (New — Rust)

- [ ] **Child process management** — ~7 functions
  - `run`, `run-with`: run and wait for completion
  - `spawn`, `wait`, `kill`: async process management
  - `stdin-write`: write to spawned process stdin
  - `pid`: process ID
  - Types: `Output {:exit-code Int :stdout Str :stderr Str}`
  - Types: `ProcessOpts {:cmd Str :args (Vec Str) :cwd (Option Str) :env (Option (Map Str Str)) :stdin (Option Str)}`

## Enhanced `sys` (Rust)

- [ ] **System interface** — ~5 new functions
  - `os`: operating system name
  - `arch`: CPU architecture
  - `cpu-count`: number of CPUs
  - `cwd`: current working directory
  - `home-dir`: user home directory
  - `exe-path`: path to current executable

## Enhanced `log` (Rust)

- [ ] **Logging enhancements** — ~2 new functions
  - `with-logger`: custom log sink function
  - `context`: retrieve current context fields

## Stdlib Finalization

- [ ] **Integration tests** — comprehensive test suite for all new modules
  - One test file per module: `tests/stdlib/{module}.nx`
  - Cover every function with at least one test
  - Edge cases for Option/Result combinators
  - Lazy iteration correctness (Iter ADT)
  - Threading macro variant tests (some->, ok->, cond->)

- [ ] **Stdlib documentation** — docstrings for every public function
  - Triple-quoted docstrings with usage examples
  - `nexl doc` generates complete stdlib API reference
  - Cross-references between related functions

- [ ] **Performance baselines** — benchmark critical paths
  - Collection operation throughput (map, filter, reduce)
  - Iter vs eager comparison on large datasets
  - Crypto operation benchmarks (sha256, hmac)
  - JSON encode/decode throughput
