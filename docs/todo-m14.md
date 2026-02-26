# M14 — Standard Library

## Scaffold
- [x] Create `nexl-stdlib` crate with per-module organization
- [x] Wire `nexl-stdlib` into `nexl-eval` standard environment

## Core Modules (§11.1)
- [x] `core` module: `identity`, `comp`, `partial`, `constantly`, `juxt`, `apply`
- [x] `str` module: `split`, `join`, `trim`, `trim-start`, `trim-end`, `upper`, `lower`, `starts-with?`, `ends-with?`, `contains?`, `replace`, `index-of`, `blank?`, `chars`, `graphemes`
- [x] `math` module: `abs`, `floor`, `ceil`, `round`, `pow`, `sqrt`, `log`, `exp`, trig fns, `min`, `max`, `clamp`, `pi`, `e`
- [x] `conv` module: `->int`, `->float`, `->str`, widening (total) + narrowing (Option)
- [x] `io` module: `println`, `print`, `read-file`, `write-file`, `path-join` (Stage 0: direct I/O)
- [x] `json` module: `parse`, `stringify` (hand-rolled recursive descent parser)
- [x] `time` module: `now`, `millis` (Stage 0: std::time)
- [x] `crypto` module: `hash`, `constant-time=` (Stage 0: DefaultHasher; SHA deferred)
- [x] `log` module: `debug`, `info`, `warn`, `error` (Stage 0: eprintln)
- [x] `test` module: `is`, `assert-eq` (Stage 0: basic assertions; deftest/check deferred)
- [x] `net` module: stub (requires async effects)
- [x] `async` module: `sleep` (Stage 0: thread::sleep; full concurrency deferred)

## Integration
- [x] Register all stdlib modules with qualified names (e.g. `str/split`)
- [x] End-to-end test: compile a real program using stdlib

## Blocked
- [ ] (none)
