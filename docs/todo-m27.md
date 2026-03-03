# M27 — nexl.test in Nexl (Macro Self-Hosting)

## Goal
Move nexl.test from Rust special forms in `eval.rs` to Nexl macros in `test.nx`.
Zero nexl.test special forms in `eval.rs` when done.

## Phase 0: Primitive Scaffolding (Rust)

- [x] **test.rs primitives** — expose ~15 new Nexl-callable functions
  - `test/describe-prefix` — returns current describe prefix string
  - `test/describe-push!` / `test/describe-pop!` — push/pop describe stack
  - `test/focus-register!` — register focused test name
  - `test/tags-register!` — register tags for a test
  - `test/flaky-register!` — register retry count for flaky test
  - `test/setup-push!` / `test/teardown-push!` — push lifecycle hooks
  - `test/setup-all-push!` / `test/teardown-all-push!` — push one-time hooks
  - `test/current-setup-hooks` / `test/current-teardown-hooks` — snapshot hook stacks
  - `test/persist-seed!` — push failing check seed
  - `test/take-seed-overrides` — drain seed override list
  - `test/accept-mode?` — is --accept flag set?
  - `test/snapshots-dir` — get snapshots base directory
  - `test/bench-register!` — register benchmark entry
  - `test/test-mode?` — is test mode active?
  - `test/str?` — runtime type check for Str values
  - `test/try-call!` — call a thunk; returns (Ok unit) or (Err msg)

- [x] **gen_mod.rs primitive** — `gen/lcg-next seed` → next seed Int

- [x] **io.rs primitives** — `io/read-file`, `io/write-file`, `io/create-dir-all`

## Phase 1: Macro System Integration

- [x] **Expose `Expander` publicly in nexl-macros**
  - Public struct, `expand_forms`, `Default` impl

- [x] **Fix 1: Nested list patterns in `defmacro-syntax`** — `crates/nexl-macros/src/expand.rs`
  - `PatternNode` enum + `parse_pattern_node` + `match_pattern_node` with fallthrough
  - Added `PatternNode::AnyMap(String)` for `{:& binding}` map patterns

- [x] **Fix 2: `syntax-str` in `expand_quasiquote`** — `crates/nexl-macros/src/expand.rs`
  - Intercepts `(syntax-str <binding>)` at quasiquote depth 1

- [x] **Integrate macro expansion into eval pipeline** — `crates/nexl-cli/src/main.rs`
  - `macro_expand()`: embeds test.nx via `include_str!`, returns `(expanded, prelude_forms)`
  - Prelude forms evaluated before user code in all 3 command handlers
  - `eval_forms` in lib.rs also runs macro expansion + prelude evaluation

- [x] **`defn` with namespaced symbols** — `crates/nexl-eval/src/eval.rs`
  - `eval_defn` accepts `ns: Some(ns)` symbols
  - `Env::add_to_module_alias` for copy-on-write module extension

## Phase 2: Simple Macros (write in test.nx, delete from eval.rs)

- [x] **`throws?`** macro in test.nx → deleted `eval_throws_q` from eval.rs

- [x] **`setup` / `teardown` / `setup-all` / `teardown-all`** macros → deleted `eval_lifecycle_hook`

- [x] **`is-match`** macro → deleted `eval_is_match`

## Phase 3: `describe` and `bench`

- [x] **`describe`** macro → deleted `eval_describe`

- [x] **`bench`** macro → deleted `eval_bench`

## Phase 4: `deftest`

- [x] **`deftest`** macro → deleted `eval_deftest`
  - Metadata via map arg `{:focus true :tags [...] :flaky N :skip "reason"}`

## Phase 5: `is`

- [x] **`is`** macro → deleted `eval_is` + `diff_hint` from eval.rs
  - Clauses: binary ops, 1-arg predicate, 2-arg dispatch (str? check), simple bool

## Phase 6: `check` — Property Testing

- [x] **`check`** macro → deleted `eval_check` and `shrink_check` from eval.rs
  - `gen/seed-seq` as a Nexl function (loop + gen/lcg-next)
  - `test/check-run!` as a Nexl function handling seed loop

## Phase 7: `snap-file!`

- [x] **`snap-file!`** regular function → deleted `eval_snap_file` from eval.rs
  - Uses `io/read-file`, `io/write-file`, `io/create-dir-all`

## Phase 8: Delete & Verify

- [x] **Grep check**: zero live `eval_*` nexl.test functions remain in eval.rs (only comments)
- [x] `cargo test --workspace` — all green
- [x] `cargo test -p nexl-cli --test e2e` — all green
- [x] `cargo clippy` — clean on changed crates
- [x] **New E2E fixture** `crates/nexl-cli/tests/fixtures/nexl_test_macros.nx` — exercises is, throws?, check (binary ops, message form, property testing)
