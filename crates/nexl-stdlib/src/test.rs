//! `test` module — testing framework.
//!
//! Provides assertion helpers, test registration, and a simple test runner.
//!
//! Functions:
//! - `test/is condition` — assert condition is true
//! - `test/assert-eq a b` — assert two values are equal
//! - `test/fail msg` — explicitly fail with a message
//! - `test/skip msg` — skip a test with a reason
//! - `test/check name values pred` — property test: run pred on each value
//! - `test/run-tests tests` — run a Vec of `[name thunk]` pairs, return report

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

// ─── Thread-local test registry ──────────────────────────────────────────────

/// A registered test: (name, thunk).
type TestEntry = (String, Value);

thread_local! {
    static TEST_REGISTRY: RefCell<Vec<TestEntry>> = const { RefCell::new(Vec::new()) };
    /// Stack of describe labels for scoped test naming (spec §7.1).
    static DESCRIBE_STACK: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Set of focused test names (tests with `:focus` flag).
    /// When non-empty, only focused tests are run by the CLI (spec §6.2).
    static FOCUS_SET: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// Map of test name → tag list for tests with `:tags` metadata.
    static TAGS_REGISTRY: RefCell<HashMap<String, Vec<String>>> = RefCell::new(HashMap::new());
    /// Stack of per-test setup thunks (one per active describe scope).
    /// Each element is a thunk registered by `setup` in that scope.
    static SETUP_STACK: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    /// Stack of per-test teardown thunks.
    static TEARDOWN_STACK: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    /// One-time setup-all thunk, called before all tests in the current describe.
    static SETUP_ALL_REGISTRY: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    /// One-time teardown-all thunk, called after all tests in the current describe.
    static TEARDOWN_ALL_REGISTRY: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    /// Whether the evaluator is running in test mode.
    /// Set to `true` by `nexl test` before evaluating files; `false` by `nexl run`.
    /// Controls whether `(submodule test ...)` bodies are evaluated (spec §8).
    static IS_TEST_MODE: RefCell<bool> = const { RefCell::new(false) };
    /// Whether the evaluator is running in bench mode.
    /// Set to `true` by `nexl bench` before evaluating files.
    /// Controls whether `(bench ...)` forms register into the bench registry.
    static IS_BENCH_MODE: RefCell<bool> = const { RefCell::new(false) };
    /// Bench registry: (name, thunk, warmup, iterations).
    static BENCH_REGISTRY: RefCell<Vec<(String, Value, usize, usize)>> =
        const { RefCell::new(Vec::new()) };
    /// Flaky registry: test name → max retry count.
    static FLAKY_REGISTRY: RefCell<HashMap<String, usize>> = RefCell::new(HashMap::new());
    /// Failed property-test seeds to persist across runs (spec §12.6).
    /// When a `check` form fails, the failing seed is pushed here.
    static FAILED_SEEDS_REGISTRY: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
    /// Seed overrides loaded from `.test-seeds` before a run.
    /// `eval_check` replays these seeds before running random trials.
    static SEED_OVERRIDES: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
    /// Whether the test runner is in `--accept` mode (spec §13).
    /// When `true`, `snap-file!` always writes its value (updating snapshots).
    static IS_ACCEPT_MODE: RefCell<bool> = const { RefCell::new(false) };
    /// Optional override for the snapshots base directory.
    /// When `None`, `snap-file!` uses `__snapshots__/` relative to CWD.
    /// Set in tests to avoid `set_current_dir` races.
    static SNAPSHOTS_BASE_DIR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Enable test mode: `(submodule test ...)` bodies will be evaluated.
pub fn set_test_mode(enabled: bool) {
    IS_TEST_MODE.with(|m| *m.borrow_mut() = enabled);
}

/// Check whether test mode is active.
pub fn is_test_mode() -> bool {
    IS_TEST_MODE.with(|m| *m.borrow())
}

/// Enable bench mode: `(bench ...)` forms will register into the bench registry.
pub fn set_bench_mode(enabled: bool) {
    IS_BENCH_MODE.with(|m| *m.borrow_mut() = enabled);
}

/// Check whether bench mode is active.
pub fn is_bench_mode() -> bool {
    IS_BENCH_MODE.with(|m| *m.borrow())
}

/// Register a benchmark entry.
pub fn bench_registry_push(name: String, thunk: Value, warmup: usize, iterations: usize) {
    BENCH_REGISTRY.with(|r| r.borrow_mut().push((name, thunk, warmup, iterations)));
}

/// Drain and return all registered benchmarks.
pub fn bench_registry_drain() -> Vec<(String, Value, usize, usize)> {
    BENCH_REGISTRY.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

/// Clear the bench registry.
pub fn bench_registry_clear() {
    BENCH_REGISTRY.with(|r| r.borrow_mut().clear());
}

/// Register a test as flaky with a max retry count.
pub fn flaky_registry_insert(name: String, retries: usize) {
    FLAKY_REGISTRY.with(|r| { r.borrow_mut().insert(name, retries); });
}

/// Get the retry count for a test (0 if not flaky).
pub fn flaky_retries(name: &str) -> usize {
    FLAKY_REGISTRY.with(|r| *r.borrow().get(name).unwrap_or(&0))
}

/// Drain the flaky registry.
pub fn flaky_registry_drain() -> HashMap<String, usize> {
    FLAKY_REGISTRY.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

/// Set the base directory for snapshots (overrides CWD-relative `__snapshots__/`).
/// Pass `None` to restore the default (CWD + `__snapshots__/`).
pub fn set_snapshots_base(dir: Option<String>) {
    SNAPSHOTS_BASE_DIR.with(|d| *d.borrow_mut() = dir);
}

/// Get the current snapshots base directory, or `None` for the CWD-relative default.
pub fn snapshots_base() -> Option<String> {
    SNAPSHOTS_BASE_DIR.with(|d| d.borrow().clone())
}

/// Enable or disable snapshot accept mode (`nexl test --accept`).
pub fn set_accept_mode(enabled: bool) {
    IS_ACCEPT_MODE.with(|m| *m.borrow_mut() = enabled);
}

/// Check whether accept mode is active.
pub fn is_accept_mode() -> bool {
    IS_ACCEPT_MODE.with(|m| *m.borrow())
}

/// Push a failing seed to the failed-seeds registry (called by `eval_check` on failure).
pub fn failed_seeds_push(seed: i64) {
    FAILED_SEEDS_REGISTRY.with(|r| r.borrow_mut().push(seed));
}

/// Drain and return all failed seeds accumulated during this run.
pub fn failed_seeds_drain() -> Vec<i64> {
    FAILED_SEEDS_REGISTRY.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

/// Set seed overrides to replay before random trials (loaded from `.test-seeds`).
pub fn set_seed_overrides(seeds: Vec<i64>) {
    SEED_OVERRIDES.with(|r| *r.borrow_mut() = seeds);
}

/// Take the seed overrides (consumes them so each run replays once).
pub fn take_seed_overrides() -> Vec<i64> {
    SEED_OVERRIDES.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

/// Add a test to the thread-local registry.
pub fn registry_push(name: String, thunk: Value) {
    TEST_REGISTRY.with(|r| r.borrow_mut().push((name, thunk)));
}

/// Take all tests from the registry (drains it).
pub fn registry_drain() -> Vec<TestEntry> {
    TEST_REGISTRY.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

/// Clear the registry without running.
pub fn registry_clear() {
    TEST_REGISTRY.with(|r| r.borrow_mut().clear());
    SETUP_STACK.with(|s| s.borrow_mut().clear());
    TEARDOWN_STACK.with(|s| s.borrow_mut().clear());
    SETUP_ALL_REGISTRY.with(|s| s.borrow_mut().clear());
    TEARDOWN_ALL_REGISTRY.with(|s| s.borrow_mut().clear());
    FLAKY_REGISTRY.with(|r| r.borrow_mut().clear());
    FAILED_SEEDS_REGISTRY.with(|r| r.borrow_mut().clear());
    SEED_OVERRIDES.with(|r| r.borrow_mut().clear());
}

/// Return how many tests are registered.
pub fn registry_len() -> usize {
    TEST_REGISTRY.with(|r| r.borrow().len())
}

/// Push a describe label onto the describe stack (spec §7.1).
pub fn describe_push(label: String) {
    DESCRIBE_STACK.with(|s| s.borrow_mut().push(label));
}

/// Pop the most recent describe label from the stack.
pub fn describe_pop() {
    DESCRIBE_STACK.with(|s| s.borrow_mut().pop());
}

/// Register a test name as focused (`:focus` flag on `deftest`).
pub fn focus_push(name: String) {
    FOCUS_SET.with(|s| s.borrow_mut().insert(name));
}

/// Return true if any focused tests have been registered.
pub fn focus_any() -> bool {
    FOCUS_SET.with(|s| !s.borrow().is_empty())
}

/// Take all focused test names and clear the focus set.
pub fn focus_drain() -> HashSet<String> {
    FOCUS_SET.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

/// Push a per-test setup thunk (called before each deftest body in the current scope).
pub fn setup_push(thunk: Value) {
    SETUP_STACK.with(|s| s.borrow_mut().push(thunk));
}

/// Pop the most-recently-pushed setup thunk (called when the describe scope exits).
pub fn setup_pop() {
    SETUP_STACK.with(|s| s.borrow_mut().pop());
}

/// Return a snapshot of the current setup stack (outermost → innermost).
pub fn setup_snapshot() -> Vec<Value> {
    SETUP_STACK.with(|s| s.borrow().clone())
}

/// Push a per-test teardown thunk (called after each deftest body in the current scope).
pub fn teardown_push(thunk: Value) {
    TEARDOWN_STACK.with(|s| s.borrow_mut().push(thunk));
}

/// Pop the most-recently-pushed teardown thunk.
pub fn teardown_pop() {
    TEARDOWN_STACK.with(|s| s.borrow_mut().pop());
}

/// Return a snapshot of the current teardown stack (outermost → innermost).
pub fn teardown_snapshot() -> Vec<Value> {
    TEARDOWN_STACK.with(|s| s.borrow().clone())
}

/// Register a one-time setup-all thunk (runs once before all tests in a describe).
pub fn setup_all_push(thunk: Value) {
    SETUP_ALL_REGISTRY.with(|s| s.borrow_mut().push(thunk));
}

/// Take and return all setup-all thunks, clearing the registry.
pub fn setup_all_drain() -> Vec<Value> {
    SETUP_ALL_REGISTRY.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

/// Register a one-time teardown-all thunk (runs once after all tests in a describe).
pub fn teardown_all_push(thunk: Value) {
    TEARDOWN_ALL_REGISTRY.with(|s| s.borrow_mut().push(thunk));
}

/// Take and return all teardown-all thunks, clearing the registry.
pub fn teardown_all_drain() -> Vec<Value> {
    TEARDOWN_ALL_REGISTRY.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

/// Register tags for a test name (`:tags` metadata on `deftest`).
pub fn tags_register(name: String, tags: Vec<String>) {
    TAGS_REGISTRY.with(|t| t.borrow_mut().insert(name, tags));
}

/// Take all tag registrations and clear the tags registry.
pub fn tags_drain() -> HashMap<String, Vec<String>> {
    TAGS_REGISTRY.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

/// Return the current describe path as a prefix string, e.g. "Outer > Inner > ".
/// Returns empty string when the stack is empty.
pub fn describe_prefix() -> String {
    DESCRIBE_STACK.with(|s| {
        let stack = s.borrow();
        if stack.is_empty() {
            String::new()
        } else {
            stack.join(" > ") + " > "
        }
    })
}

// ─── Matchers (spec §15) ──────────────────────────────────────────────────────

/// Helper: make a matcher NativeClosure from a predicate closure.
fn make_matcher(name: &'static str, f: impl Fn(&Value) -> Result<(), String> + 'static) -> Value {
    Value::NativeClosure {
        name: Rc::from(name),
        f: Rc::new(move |args: &[Value]| match args {
            [v] => f(v).map(|()| Value::Unit),
            _ => Err(format!("matcher `{name}` expects 1 argument, got {}", args.len())),
        }),
    }
}

/// `(test/eq expected)` — matcher: actual == expected.
fn eq_matcher(args: &[Value]) -> Result<Value, String> {
    match args {
        [expected] => {
            let expected = expected.clone();
            Ok(make_matcher("eq", move |v| {
                if v == &expected { Ok(()) } else { Err(format!("expected {expected}, got {v}")) }
            }))
        }
        _ => Err("`test/eq` requires 1 argument".into()),
    }
}

/// `(test/gt n)` — matcher: actual > n.
fn gt_matcher(args: &[Value]) -> Result<Value, String> {
    match args {
        [n @ (Value::Int(_) | Value::Float(_))] => {
            let n = n.clone();
            Ok(make_matcher("gt", move |v| match (v, &n) {
                (Value::Int(a), Value::Int(b)) if a > b => Ok(()),
                (Value::Float(a), Value::Float(b)) if a > b => Ok(()),
                (Value::Int(a), Value::Float(b)) if (*a as f64) > *b => Ok(()),
                (Value::Float(a), Value::Int(b)) if *a > (*b as f64) => Ok(()),
                _ => Err(format!("expected > {n}, got {v}")),
            }))
        }
        _ => Err("`test/gt` requires 1 numeric argument".into()),
    }
}

/// `(test/gte n)` — matcher: actual >= n.
fn gte_matcher(args: &[Value]) -> Result<Value, String> {
    match args {
        [n @ (Value::Int(_) | Value::Float(_))] => {
            let n = n.clone();
            Ok(make_matcher("gte", move |v| match (v, &n) {
                (Value::Int(a), Value::Int(b)) if a >= b => Ok(()),
                (Value::Float(a), Value::Float(b)) if a >= b => Ok(()),
                (Value::Int(a), Value::Float(b)) if (*a as f64) >= *b => Ok(()),
                (Value::Float(a), Value::Int(b)) if *a >= (*b as f64) => Ok(()),
                _ => Err(format!("expected >= {n}, got {v}")),
            }))
        }
        _ => Err("`test/gte` requires 1 numeric argument".into()),
    }
}

/// `(test/lt n)` — matcher: actual < n.
fn lt_matcher(args: &[Value]) -> Result<Value, String> {
    match args {
        [n @ (Value::Int(_) | Value::Float(_))] => {
            let n = n.clone();
            Ok(make_matcher("lt", move |v| match (v, &n) {
                (Value::Int(a), Value::Int(b)) if a < b => Ok(()),
                (Value::Float(a), Value::Float(b)) if a < b => Ok(()),
                (Value::Int(a), Value::Float(b)) if (*a as f64) < *b => Ok(()),
                (Value::Float(a), Value::Int(b)) if *a < (*b as f64) => Ok(()),
                _ => Err(format!("expected < {n}, got {v}")),
            }))
        }
        _ => Err("`test/lt` requires 1 numeric argument".into()),
    }
}

/// `(test/lte n)` — matcher: actual <= n.
fn lte_matcher(args: &[Value]) -> Result<Value, String> {
    match args {
        [n @ (Value::Int(_) | Value::Float(_))] => {
            let n = n.clone();
            Ok(make_matcher("lte", move |v| match (v, &n) {
                (Value::Int(a), Value::Int(b)) if a <= b => Ok(()),
                (Value::Float(a), Value::Float(b)) if a <= b => Ok(()),
                (Value::Int(a), Value::Float(b)) if (*a as f64) <= *b => Ok(()),
                (Value::Float(a), Value::Int(b)) if *a <= (*b as f64) => Ok(()),
                _ => Err(format!("expected <= {n}, got {v}")),
            }))
        }
        _ => Err("`test/lte` requires 1 numeric argument".into()),
    }
}

/// `(test/starts-with prefix)` — matcher: string starts with prefix.
fn starts_with_matcher(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(prefix)] => {
            let prefix = Rc::clone(prefix);
            Ok(make_matcher("starts-with", move |v| match v {
                Value::Str(s) if s.starts_with(prefix.as_ref()) => Ok(()),
                Value::Str(s) => Err(format!("expected string starting with {prefix:?}, got {s:?}")),
                _ => Err(format!("starts-with: expected Str, got {v}")),
            }))
        }
        _ => Err("`test/starts-with` requires 1 Str argument".into()),
    }
}

/// `(test/ends-with suffix)` — matcher: string ends with suffix.
fn ends_with_matcher(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(suffix)] => {
            let suffix = Rc::clone(suffix);
            Ok(make_matcher("ends-with", move |v| match v {
                Value::Str(s) if s.ends_with(suffix.as_ref()) => Ok(()),
                Value::Str(s) => Err(format!("expected string ending with {suffix:?}, got {s:?}")),
                _ => Err(format!("ends-with: expected Str, got {v}")),
            }))
        }
        _ => Err("`test/ends-with` requires 1 Str argument".into()),
    }
}

/// `(test/contains-str sub)` — matcher: string contains substring.
fn contains_str_matcher(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(sub)] => {
            let sub = Rc::clone(sub);
            Ok(make_matcher("contains-str", move |v| match v {
                Value::Str(s) if s.contains(sub.as_ref()) => Ok(()),
                Value::Str(s) => Err(format!("expected string containing {sub:?}, got {s:?}")),
                _ => Err(format!("contains-str: expected Str, got {v}")),
            }))
        }
        _ => Err("`test/contains-str` requires 1 Str argument".into()),
    }
}

/// `(test/has-length n)` — matcher: collection or string has exactly n elements.
fn has_length_matcher(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(n)] => {
            let n = *n as usize;
            Ok(make_matcher("has-length", move |v| {
                let len = match v {
                    Value::Vec(items) => items.len(),
                    Value::Str(s) => s.chars().count(),
                    _ => return Err(format!("has-length: expected Vec or Str, got {v}")),
                };
                if len == n { Ok(()) } else { Err(format!("expected length {n}, got {len}")) }
            }))
        }
        _ => Err("`test/has-length` requires 1 Int argument".into()),
    }
}

/// `(test/all-of [m1 m2 ...])` — matcher: all matchers pass.
fn all_of_matcher(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(matchers)] => {
            let matchers = Rc::clone(matchers);
            Ok(make_matcher("all-of", move |v| {
                for m in matchers.iter() {
                    nexl_runtime::call_value(m, std::slice::from_ref(v))?;
                }
                Ok(())
            }))
        }
        _ => Err("`test/all-of` requires a Vec of matchers".into()),
    }
}

/// `(test/any-of [m1 m2 ...])` — matcher: at least one matcher passes.
fn any_of_matcher(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(matchers)] => {
            let matchers = Rc::clone(matchers);
            Ok(make_matcher("any-of", move |v| {
                let mut last_err = String::from("any-of: no matchers provided");
                for m in matchers.iter() {
                    match nexl_runtime::call_value(m, std::slice::from_ref(v)) {
                        Ok(_) => return Ok(()),
                        Err(e) => last_err = e,
                    }
                }
                Err(last_err)
            }))
        }
        _ => Err("`test/any-of` requires a Vec of matchers".into()),
    }
}

/// `(test/not-m matcher)` — inverts a matcher.
fn not_m_matcher(args: &[Value]) -> Result<Value, String> {
    match args {
        [m] => {
            let m = m.clone();
            Ok(make_matcher("not-m", move |v| {
                match nexl_runtime::call_value(&m, std::slice::from_ref(v)) {
                    Ok(_) => Err(format!("not-m: matcher unexpectedly passed for {v}")),
                    Err(_) => Ok(()),
                }
            }))
        }
        _ => Err("`test/not-m` requires 1 matcher argument".into()),
    }
}

// ─── Primitive accessors exposed to Nexl for macro use ───────────────────────

/// `(test/describe-prefix)` — return the current describe prefix string.
fn describe_prefix_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/describe-prefix` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Str(Rc::from(describe_prefix().as_str())))
}

/// `(test/describe-push! label)` — push a describe label onto the stack.
fn describe_push_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => {
            describe_push(s.to_string());
            Ok(Value::Unit)
        }
        [other] => {
            describe_push(other.to_string());
            Ok(Value::Unit)
        }
        _ => Err(format!("`test/describe-push!` requires 1 argument, got {}", args.len())),
    }
}

/// `(test/describe-pop!)` — pop the most recent describe label.
fn describe_pop_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/describe-pop!` takes no arguments, got {}", args.len()));
    }
    describe_pop();
    Ok(Value::Unit)
}

/// `(test/focus-register! name)` — register a focused test name.
fn focus_register_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => {
            focus_push(s.to_string());
            Ok(Value::Unit)
        }
        [other] => {
            focus_push(other.to_string());
            Ok(Value::Unit)
        }
        _ => Err(format!("`test/focus-register!` requires 1 argument, got {}", args.len())),
    }
}

/// `(test/tags-register! name tags)` — register tags for a test.
fn tags_register_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [name, Value::Vec(tag_vals)] => {
            let name_str = match name {
                Value::Str(s) => s.to_string(),
                other => other.to_string(),
            };
            let tags: Vec<String> = tag_vals
                .iter()
                .map(|v| match v {
                    Value::Str(s) => s.to_string(),
                    Value::Keyword { name, .. } => name.to_string(),
                    other => other.to_string(),
                })
                .collect();
            tags_register(name_str, tags);
            Ok(Value::Unit)
        }
        _ => Err(format!("`test/tags-register!` requires 2 arguments (name tags-vec), got {}", args.len())),
    }
}

/// `(test/flaky-register! name retries)` — register retry count for a flaky test.
fn flaky_register_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [name, Value::Int(retries)] => {
            let name_str = match name {
                Value::Str(s) => s.to_string(),
                other => other.to_string(),
            };
            flaky_registry_insert(name_str, (*retries).max(0) as usize);
            Ok(Value::Unit)
        }
        [name, Value::Bool(true)] => {
            let name_str = match name {
                Value::Str(s) => s.to_string(),
                other => other.to_string(),
            };
            flaky_registry_insert(name_str, 3);
            Ok(Value::Unit)
        }
        _ => Err(format!("`test/flaky-register!` requires 2 arguments (name retries), got {}", args.len())),
    }
}

/// `(test/setup-push! thunk)` — push a per-test setup hook.
fn setup_push_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [thunk] => {
            setup_push(thunk.clone());
            Ok(Value::Unit)
        }
        _ => Err(format!("`test/setup-push!` requires 1 thunk argument, got {}", args.len())),
    }
}

/// `(test/teardown-push! thunk)` — push a per-test teardown hook.
fn teardown_push_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [thunk] => {
            teardown_push(thunk.clone());
            Ok(Value::Unit)
        }
        _ => Err(format!("`test/teardown-push!` requires 1 thunk argument, got {}", args.len())),
    }
}

/// `(test/setup-all-push! thunk)` — push a one-time setup-all hook.
fn setup_all_push_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [thunk] => {
            setup_all_push(thunk.clone());
            Ok(Value::Unit)
        }
        _ => Err(format!("`test/setup-all-push!` requires 1 thunk argument, got {}", args.len())),
    }
}

/// `(test/teardown-all-push! thunk)` — push a one-time teardown-all hook.
fn teardown_all_push_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [thunk] => {
            teardown_all_push(thunk.clone());
            Ok(Value::Unit)
        }
        _ => Err(format!("`test/teardown-all-push!` requires 1 thunk argument, got {}", args.len())),
    }
}

/// `(test/current-setup-hooks)` — return snapshot of current setup hooks as Vec.
fn current_setup_hooks_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/current-setup-hooks` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Vec(Rc::new(setup_snapshot())))
}

/// `(test/current-teardown-hooks)` — return snapshot of current teardown hooks as Vec.
fn current_teardown_hooks_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/current-teardown-hooks` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Vec(Rc::new(teardown_snapshot())))
}

/// `(test/setup-stack-len)` — number of hooks currently on the setup stack.
fn setup_stack_len_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/setup-stack-len` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Int(setup_snapshot().len() as i64))
}

/// `(test/teardown-stack-len)` — number of hooks currently on the teardown stack.
fn teardown_stack_len_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/teardown-stack-len` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Int(teardown_snapshot().len() as i64))
}

/// `(test/setup-pop!)` — remove the top hook from the setup stack.
fn setup_pop_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/setup-pop!` takes no arguments, got {}", args.len()));
    }
    setup_pop();
    Ok(Value::Unit)
}

/// `(test/teardown-pop!)` — remove the top hook from the teardown stack.
fn teardown_pop_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/teardown-pop!` takes no arguments, got {}", args.len()));
    }
    teardown_pop();
    Ok(Value::Unit)
}

/// `(test/persist-seed! seed)` — push a failing check seed to the registry.
fn persist_seed_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(seed)] => {
            failed_seeds_push(*seed);
            Ok(Value::Unit)
        }
        _ => Err(format!("`test/persist-seed!` requires 1 Int argument, got {}", args.len())),
    }
}

/// `(test/take-seed-overrides)` — drain and return seed override list as Vec.
fn take_seed_overrides_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/take-seed-overrides` takes no arguments, got {}", args.len()));
    }
    let seeds = take_seed_overrides();
    Ok(Value::Vec(Rc::new(seeds.into_iter().map(Value::Int).collect())))
}

/// `(test/accept-mode?)` — is --accept mode active?
fn accept_mode_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/accept-mode?` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Bool(is_accept_mode()))
}

/// `(test/snapshots-dir)` — return snapshots base directory (or "." if default).
fn snapshots_dir_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/snapshots-dir` takes no arguments, got {}", args.len()));
    }
    let dir = snapshots_base().unwrap_or_else(|| ".".to_string());
    Ok(Value::Str(Rc::from(dir.as_str())))
}

/// `(test/bench-register! name iterations warmup thunk)` — register a benchmark.
fn bench_register_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [name, Value::Int(iterations), Value::Int(warmup), thunk] => {
            let name_str = match name {
                Value::Str(s) => s.to_string(),
                other => other.to_string(),
            };
            bench_registry_push(name_str, thunk.clone(), (*warmup).max(0) as usize, (*iterations).max(1) as usize);
            Ok(Value::Unit)
        }
        _ => Err(format!("`test/bench-register!` requires 4 arguments (name iterations warmup thunk), got {}", args.len())),
    }
}

/// `(test/test-mode?)` — is test mode active?
fn test_mode_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/test-mode?` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Bool(is_test_mode()))
}

/// `(test/is-bench-mode?)` — is bench mode active?
fn is_bench_mode_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`test/is-bench-mode?` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Bool(is_bench_mode()))
}

/// `(test/try-call! thunk)` — call `thunk` and catch any exception.
///
/// Returns `(Ok result)` if the thunk completes normally, or `(Err msg)` if
/// it raises any runtime error (including `panic`, `test/fail`, etc.).
///
/// Used by the `throws?` macro to intercept exceptions at the Nexl level:
/// ```nexl
/// (throws? (panic "oops"))
/// ;; expands to (match (test/try-call! (fn [] (panic "oops"))) ...)
/// ```
fn try_call_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [thunk] => {
            let result = nexl_runtime::call_value(thunk, &[]);
            Ok(match result {
                Ok(v) => Value::Adt {
                    type_name: Rc::from("Result"),
                    ctor: Rc::from("Ok"),
                    fields: Rc::new(vec![v]),
                },
                Err(e) => Value::Adt {
                    type_name: Rc::from("Result"),
                    ctor: Rc::from("Err"),
                    fields: Rc::new(vec![Value::Str(Rc::from(e.as_str()))]),
                },
            })
        }
        _ => Err(format!("`test/try-call!` requires 1 thunk argument, got {}", args.len())),
    }
}

/// `(test/str? v)` — return `true` if `v` is a `Str`, `false` otherwise.
fn is_str_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => Ok(Value::Bool(matches!(v, Value::Str(_)))),
        _ => Err(format!("`test/str?` requires 1 argument, got {}", args.len())),
    }
}

// ─── Stdlib entries ───────────────────────────────────────────────────────────

/// Return all `test` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("is", is as fn(&[Value]) -> Result<Value, String>),
        ("assert-eq", assert_eq_fn),
        ("fail", fail_fn),
        ("skip", skip_fn),
        ("check", check_fn),
        ("run-tests", run_tests_fn),
        ("register!", register_fn),
        ("run-registered", run_registered_fn),
        ("clear-registry!", clear_registry_fn),
        // Matchers (spec §15)
        ("eq", eq_matcher),
        ("gt", gt_matcher),
        ("gte", gte_matcher),
        ("lt", lt_matcher),
        ("lte", lte_matcher),
        ("starts-with", starts_with_matcher),
        ("ends-with", ends_with_matcher),
        ("contains-str", contains_str_matcher),
        ("has-length", has_length_matcher),
        ("all-of", all_of_matcher),
        ("any-of", any_of_matcher),
        ("not-m", not_m_matcher),
        // Macro support primitives (Phase 0 — M27)
        ("describe-prefix",       describe_prefix_fn),
        ("describe-push!",        describe_push_fn),
        ("describe-pop!",         describe_pop_fn),
        ("focus-register!",       focus_register_fn),
        ("tags-register!",        tags_register_fn),
        ("flaky-register!",       flaky_register_fn),
        ("setup-push!",           setup_push_fn),
        ("teardown-push!",        teardown_push_fn),
        ("setup-all-push!",       setup_all_push_fn),
        ("teardown-all-push!",    teardown_all_push_fn),
        ("current-setup-hooks",   current_setup_hooks_fn),
        ("current-teardown-hooks", current_teardown_hooks_fn),
        ("persist-seed!",         persist_seed_fn),
        ("take-seed-overrides",   take_seed_overrides_fn),
        ("accept-mode?",          accept_mode_fn),
        ("snapshots-dir",         snapshots_dir_fn),
        ("bench-register!",       bench_register_fn),
        ("test-mode?",            test_mode_fn),
        ("is-bench-mode?",        is_bench_mode_fn),
        ("try-call!",             try_call_fn),
        ("str?",                  is_str_fn),
        ("setup-stack-len",       setup_stack_len_fn),
        ("teardown-stack-len",    teardown_stack_len_fn),
        ("setup-pop!",            setup_pop_fn),
        ("teardown-pop!",         teardown_pop_fn),
    ]
}

/// Build a Keyword value with no namespace.
fn kw(name: &str) -> Value {
    Value::Keyword {
        ns: None,
        name: Rc::from(name),
    }
}

/// `(test/is condition)` — assert that condition is true.
fn is(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Bool(true)] => Ok(Value::Unit),
        [Value::Bool(false)] => Err("assertion failed: (test/is false)".into()),
        [other] => Err(format!(
            "`test/is` expected Bool, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`test/is` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(test/assert-eq a b)` — assert that two values are equal.
fn assert_eq_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [a, b] => {
            if a == b {
                Ok(Value::Unit)
            } else {
                Err(format!("assertion failed: expected {a} to equal {b}"))
            }
        }
        _ => Err(format!(
            "`test/assert-eq` requires exactly 2 arguments, got {}",
            args.len()
        )),
    }
}

/// `(test/fail msg)` — explicitly fail a test with a message.
fn fail_fn(args: &[Value]) -> Result<Value, String> {
    let msg = match args {
        [Value::Str(s)] => s.to_string(),
        [other] => other.to_string(),
        _ => return Err(format!("`test/fail` requires 1 argument, got {}", args.len())),
    };
    Err(format!("test failed: {msg}"))
}

/// `(test/skip msg)` — skip a test, returning a Skip ADT.
fn skip_fn(args: &[Value]) -> Result<Value, String> {
    let reason = match args {
        [Value::Str(s)] => s.clone(),
        [other] => Rc::from(other.to_string().as_str()),
        _ => return Err(format!("`test/skip` requires 1 argument, got {}", args.len())),
    };
    Ok(Value::Adt {
        type_name: Rc::from("TestResult"),
        ctor: Rc::from("Skip"),
        fields: Rc::new(vec![Value::Str(reason)]),
    })
}

/// `(test/check name values pred)` — property-based test.
///
/// Calls `pred(v)` for each `v` in `values`. Each call must return `Bool(true)`.
/// Returns `Unit` on success or an error on the first failing value.
fn check_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(name), Value::Vec(values), pred] => {
            for (i, v) in values.iter().enumerate() {
                let result = nexl_runtime::call_value(pred, std::slice::from_ref(v))
                    .map_err(|e| format!("check `{name}` failed: {e}"))?;
                match result {
                    Value::Bool(true) => {}
                    Value::Bool(false) => {
                        return Err(format!(
                            "check `{name}` failed at index {i}: predicate returned false for {v}"
                        ));
                    }
                    other => {
                        return Err(format!(
                            "check `{name}` predicate must return Bool, got {} for {v}",
                            other.type_name()
                        ));
                    }
                }
            }
            Ok(Value::Unit)
        }
        _ if args.len() != 3 => Err(format!(
            "`test/check` requires 3 arguments (name values pred), got {}",
            args.len()
        )),
        _ => Err(format!(
            "`test/check` expected (Str Vec Fn), got ({}, {}, {})",
            args[0].type_name(),
            args[1].type_name(),
            args[2].type_name()
        )),
    }
}

/// `(test/register! name thunk)` — register a test in the thread-local registry.
///
/// Typically used in conjunction with `nexl test` which evaluates a file and
/// then calls `test/run-registered` to execute all registered tests.
fn register_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [name, thunk] => {
            let name_str = match name {
                Value::Str(s) => s.to_string(),
                other => other.to_string(),
            };
            registry_push(name_str, thunk.clone());
            Ok(Value::Unit)
        }
        _ => Err(format!(
            "`test/register!` requires 2 arguments (name thunk), got {}",
            args.len()
        )),
    }
}

/// `(test/run-registered)` — run all tests in the thread-local registry.
///
/// Drains the registry (tests are removed after running). Returns the same
/// report Map as `test/run-tests`.
fn run_registered_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!(
            "`test/run-registered` takes no arguments, got {}",
            args.len()
        ));
    }
    let tests = registry_drain();
    let test_vec: Vec<Value> = tests
        .into_iter()
        .map(|(name, thunk)| {
            Value::Vec(Rc::new(vec![
                Value::Str(Rc::from(name.as_str())),
                thunk,
            ]))
        })
        .collect();
    run_tests_fn(&[Value::Vec(Rc::new(test_vec))])
}

/// `(test/clear-registry!)` — discard all registered tests without running them.
fn clear_registry_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!(
            "`test/clear-registry!` takes no arguments, got {}",
            args.len()
        ));
    }
    registry_clear();
    Ok(Value::Unit)
}

/// `(test/run-tests tests)` — run a Vec of `[name thunk]` pairs.
///
/// Each thunk is called with no arguments.
/// - `Unit` result → pass
/// - `Err(msg)` result → fail
/// - `Skip(reason)` Adt → skip
///
/// Returns a Map: `{:passed n, :failed n, :skipped n, :failures [[name msg] ...]}`.
fn run_tests_fn(args: &[Value]) -> Result<Value, String> {
    let tests = match args {
        [Value::Vec(tests)] => tests.clone(),
        [other] => {
            return Err(format!(
                "`test/run-tests` expected Vec, got {}",
                other.type_name()
            ))
        }
        _ => {
            return Err(format!(
                "`test/run-tests` requires 1 argument, got {}",
                args.len()
            ))
        }
    };

    let mut passed: i64 = 0;
    let mut failed: i64 = 0;
    let mut skipped: i64 = 0;
    let mut failures: Vec<Value> = Vec::new();

    for test in tests.iter() {
        let (name, thunk) = match test {
            Value::Vec(pair) if pair.len() == 2 => (pair[0].clone(), pair[1].clone()),
            other => {
                return Err(format!(
                    "`test/run-tests` each test must be a 2-element Vec [name thunk], got {other}"
                ))
            }
        };
        let name_str = match &name {
            Value::Str(s) => s.to_string(),
            other => other.to_string(),
        };

        match nexl_runtime::call_value(&thunk, &[]) {
            Ok(Value::Unit) => {
                passed += 1;
            }
            Ok(Value::Adt { ctor, fields, .. }) if ctor.as_ref() == "Skip" => {
                skipped += 1;
                let _ = fields; // reason available but not counted as failure
            }
            Ok(other) => {
                // Any non-Unit, non-Skip result counts as a pass if no error
                let _ = other;
                passed += 1;
            }
            Err(msg) => {
                failed += 1;
                failures.push(Value::Vec(Rc::new(vec![
                    Value::Str(Rc::from(name_str.as_str())),
                    Value::Str(Rc::from(msg.as_str())),
                ])));
            }
        }
    }

    Ok(Value::Map(Rc::new(
        vec![
            (kw("passed"), Value::Int(passed)),
            (kw("failed"), Value::Int(failed)),
            (kw("skipped"), Value::Int(skipped)),
            (kw("failures"), Value::Vec(Rc::new(failures))),
        ]
        .into(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn test_is_true() {
        assert_eq!(is(&[Value::Bool(true)]).unwrap(), Value::Unit);
    }

    #[test]
    fn test_is_false() {
        assert!(is(&[Value::Bool(false)]).is_err());
    }

    #[test]
    fn test_assert_eq_equal() {
        assert_eq!(
            assert_eq_fn(&[Value::Int(42), Value::Int(42)]).unwrap(),
            Value::Unit
        );
    }

    #[test]
    fn test_assert_eq_not_equal() {
        assert!(assert_eq_fn(&[Value::Int(1), Value::Int(2)]).is_err());
    }

    #[test]
    fn test_assert_eq_str() {
        assert_eq!(
            assert_eq_fn(&[Value::Str(Rc::from("hello")), Value::Str(Rc::from("hello")),]).unwrap(),
            Value::Unit
        );
    }

    // ── Test: fail ───────────────────────────────────────────────────────────

    #[test]
    fn test_fail_returns_err() {
        let result = fail_fn(&[Value::Str(Rc::from("oops"))]);
        assert!(result.is_err(), "fail should return Err");
    }

    #[test]
    fn test_fail_includes_message() {
        let err = fail_fn(&[Value::Str(Rc::from("oops"))]).unwrap_err();
        assert!(err.contains("oops"), "error should contain message: {err}");
    }

    // ── Test: skip ───────────────────────────────────────────────────────────

    #[test]
    fn test_skip_returns_skip_adt() {
        let result = skip_fn(&[Value::Str(Rc::from("not ready"))]).unwrap();
        match result {
            Value::Adt { ctor, .. } => assert_eq!(ctor.as_ref(), "Skip"),
            other => panic!("expected Skip Adt, got {other}"),
        }
    }

    // ── Test: register! / run-registered / clear-registry! ──────────────────

    #[test]
    fn test_register_adds_to_registry() {
        registry_clear();
        let thunk = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "t",
            f: |_| Ok(Value::Unit),
        }));
        register_fn(&[Value::Str(Rc::from("my-test")), thunk]).unwrap();
        assert_eq!(registry_len(), 1);
        registry_clear();
    }

    #[test]
    fn test_run_registered_empty() {
        registry_clear();
        let report = run_registered_fn(&[]).unwrap();
        let map = match report { Value::Map(m) => m, other => panic!("{other}") };
        let get = |key: &str| -> Value {
            map.iter()
                .find(|(k, _)| matches!(k, Value::Keyword { name, .. } if name.as_ref() == key))
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        assert_eq!(get("passed"), Value::Int(0));
        assert_eq!(get("failed"), Value::Int(0));
    }

    #[test]
    fn test_run_registered_with_pass() {
        registry_clear();
        let pass_thunk = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "pass",
            f: |_| Ok(Value::Unit),
        }));
        register_fn(&[Value::Str(Rc::from("t1")), pass_thunk]).unwrap();
        let report = run_registered_fn(&[]).unwrap();
        let map = match report { Value::Map(m) => m, other => panic!("{other}") };
        let passed = map.iter()
            .find(|(k, _)| matches!(k, Value::Keyword { name, .. } if name.as_ref() == "passed"))
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(passed, Value::Int(1));
    }

    #[test]
    fn test_run_registered_with_fail() {
        registry_clear();
        let fail_thunk = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "fail",
            f: |_| Err("boom".to_string()),
        }));
        register_fn(&[Value::Str(Rc::from("t-fail")), fail_thunk]).unwrap();
        let report = run_registered_fn(&[]).unwrap();
        let map = match report { Value::Map(m) => m, other => panic!("{other}") };
        let failed = map.iter()
            .find(|(k, _)| matches!(k, Value::Keyword { name, .. } if name.as_ref() == "failed"))
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(failed, Value::Int(1));
    }

    #[test]
    fn test_clear_registry() {
        let thunk = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "t",
            f: |_| Ok(Value::Unit),
        }));
        register_fn(&[Value::Str(Rc::from("x")), thunk]).unwrap();
        clear_registry_fn(&[]).unwrap();
        assert_eq!(registry_len(), 0);
    }

    // ── Test: focus ──────────────────────────────────────────────────────────

    #[test]
    fn focus_push_tracks_focused_name() {
        focus_drain(); // clear any leftover state
        focus_push("my-test".to_string());
        assert!(focus_any(), "focus_any should be true after push");
        let set = focus_drain();
        assert!(set.contains("my-test"), "drain should contain the pushed name");
    }

    #[test]
    fn focus_any_false_when_empty() {
        focus_drain(); // ensure empty
        assert!(!focus_any(), "focus_any should be false when no focused tests");
    }

    #[test]
    fn focus_drain_clears_set() {
        focus_drain();
        focus_push("x".to_string());
        focus_drain();
        assert!(!focus_any(), "focus_any should be false after drain");
    }

    // ── Test: tags ───────────────────────────────────────────────────────────

    #[test]
    fn tags_register_and_drain() {
        tags_drain(); // clear any state
        tags_register("my-test".to_string(), vec!["db".to_string(), "slow".to_string()]);
        let map = tags_drain();
        assert!(map.contains_key("my-test"), "should have registered test");
        let tags = &map["my-test"];
        assert!(tags.contains(&"db".to_string()));
        assert!(tags.contains(&"slow".to_string()));
    }

    #[test]
    fn tags_drain_clears_registry() {
        tags_drain();
        tags_register("t".to_string(), vec!["unit".to_string()]);
        tags_drain();
        let map = tags_drain();
        assert!(map.is_empty(), "tags should be cleared after drain");
    }

    // ── Test: check ──────────────────────────────────────────────────────────

    #[test]
    fn test_check_passes_when_all_true() {
        // Predicate: always returns Bool(true)
        let pred = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "always-true",
            f: |_args| Ok(Value::Bool(true)),
        }));
        let values = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        let result = check_fn(&[Value::Str(Rc::from("test")), values, pred]).unwrap();
        assert_eq!(result, Value::Unit);
    }

    #[test]
    fn test_check_fails_on_false() {
        let pred = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "always-false",
            f: |_args| Ok(Value::Bool(false)),
        }));
        let values = Value::Vec(Rc::new(vec![Value::Int(42)]));
        let err = check_fn(&[Value::Str(Rc::from("my-prop")), values, pred]).unwrap_err();
        assert!(err.contains("my-prop"), "error should name the check: {err}");
    }

    // ── Test: run-tests ──────────────────────────────────────────────────────

    #[test]
    fn test_run_tests_all_pass() {
        let pass_thunk = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "pass",
            f: |_| Ok(Value::Unit),
        }));
        let tests = Value::Vec(Rc::new(vec![
            Value::Vec(Rc::new(vec![
                Value::Str(Rc::from("test-a")),
                pass_thunk.clone(),
            ])),
            Value::Vec(Rc::new(vec![
                Value::Str(Rc::from("test-b")),
                pass_thunk,
            ])),
        ]));
        let report = run_tests_fn(&[tests]).unwrap();
        match &report {
            Value::Map(m) => {
                let passed = m.iter()
                    .find(|(k, _)| matches!(k, Value::Keyword { name, .. } if name.as_ref() == "passed"))
                    .map(|(_, v)| v.clone());
                assert_eq!(passed, Some(Value::Int(2)));
            }
            other => panic!("expected report Map, got {other}"),
        }
    }

    #[test]
    fn test_run_tests_counts_failures() {
        let fail_thunk = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "fail",
            f: |_| Err("assertion failed".to_string()),
        }));
        let pass_thunk = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "pass",
            f: |_| Ok(Value::Unit),
        }));
        let tests = Value::Vec(Rc::new(vec![
            Value::Vec(Rc::new(vec![Value::Str(Rc::from("pass")), pass_thunk])),
            Value::Vec(Rc::new(vec![Value::Str(Rc::from("fail")), fail_thunk])),
        ]));
        let report = run_tests_fn(&[tests]).unwrap();
        let map = match report { Value::Map(m) => m, other => panic!("{other}") };
        let get = |key: &str| -> Value {
            map.iter()
                .find(|(k, _)| matches!(k, Value::Keyword { name, .. } if name.as_ref() == key))
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        assert_eq!(get("passed"), Value::Int(1));
        assert_eq!(get("failed"), Value::Int(1));
    }

    #[test]
    fn test_run_tests_returns_report_map() {
        let report = run_tests_fn(&[Value::Vec(Rc::new(vec![]))]).unwrap();
        match &report {
            Value::Map(m) => {
                let keys: Vec<String> = m
                    .iter()
                    .filter_map(|(k, _)| match k {
                        Value::Keyword { name, .. } => Some(name.to_string()),
                        _ => None,
                    })
                    .collect();
                assert!(keys.contains(&"passed".to_string()));
                assert!(keys.contains(&"failed".to_string()));
                assert!(keys.contains(&"skipped".to_string()));
                assert!(keys.contains(&"failures".to_string()));
            }
            other => panic!("expected Map, got {other}"),
        }
    }

    // ── Test: Phase 0 macro-support primitives ───────────────────────────────

    #[test]
    fn describe_prefix_returns_empty_when_no_stack() {
        DESCRIBE_STACK.with(|s| s.borrow_mut().clear());
        let result = describe_prefix_fn(&[]).unwrap();
        assert_eq!(result, Value::Str(Rc::from("")));
    }

    #[test]
    fn describe_push_pop_round_trips() {
        DESCRIBE_STACK.with(|s| s.borrow_mut().clear());
        describe_push_fn(&[Value::Str(Rc::from("Suite"))]).unwrap();
        let prefix = describe_prefix_fn(&[]).unwrap();
        assert_eq!(prefix, Value::Str(Rc::from("Suite > ")));
        describe_pop_fn(&[]).unwrap();
        let after = describe_prefix_fn(&[]).unwrap();
        assert_eq!(after, Value::Str(Rc::from("")));
    }

    #[test]
    fn setup_push_and_current_hooks() {
        SETUP_STACK.with(|s| s.borrow_mut().clear());
        let thunk = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn { name: "noop", f: |_| Ok(Value::Unit) }));
        setup_push_fn(&[thunk]).unwrap();
        let hooks = current_setup_hooks_fn(&[]).unwrap();
        match hooks {
            Value::Vec(v) => assert_eq!(v.len(), 1),
            other => panic!("expected Vec, got {other}"),
        }
        SETUP_STACK.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn teardown_push_and_current_hooks() {
        TEARDOWN_STACK.with(|s| s.borrow_mut().clear());
        let thunk = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn { name: "noop", f: |_| Ok(Value::Unit) }));
        teardown_push_fn(&[thunk]).unwrap();
        let hooks = current_teardown_hooks_fn(&[]).unwrap();
        match hooks {
            Value::Vec(v) => assert_eq!(v.len(), 1),
            other => panic!("expected Vec, got {other}"),
        }
        TEARDOWN_STACK.with(|s| s.borrow_mut().clear());
    }

    #[test]
    fn persist_seed_and_take_overrides() {
        FAILED_SEEDS_REGISTRY.with(|r| r.borrow_mut().clear());
        SEED_OVERRIDES.with(|r| r.borrow_mut().clear());
        persist_seed_fn(&[Value::Int(42)]).unwrap();
        persist_seed_fn(&[Value::Int(99)]).unwrap();
        // Set seeds as overrides (mimic CLI behaviour)
        let seeds = failed_seeds_drain();
        set_seed_overrides(seeds);
        let overrides = take_seed_overrides_fn(&[]).unwrap();
        match overrides {
            Value::Vec(v) => {
                assert_eq!(v.len(), 2);
                assert!(v.contains(&Value::Int(42)));
                assert!(v.contains(&Value::Int(99)));
            }
            other => panic!("expected Vec, got {other}"),
        }
    }

    #[test]
    fn accept_mode_fn_returns_bool() {
        set_accept_mode(false);
        assert_eq!(accept_mode_fn(&[]).unwrap(), Value::Bool(false));
        set_accept_mode(true);
        assert_eq!(accept_mode_fn(&[]).unwrap(), Value::Bool(true));
        set_accept_mode(false);
    }

    #[test]
    fn test_mode_fn_returns_bool() {
        set_test_mode(false);
        assert_eq!(test_mode_fn(&[]).unwrap(), Value::Bool(false));
        set_test_mode(true);
        assert_eq!(test_mode_fn(&[]).unwrap(), Value::Bool(true));
        set_test_mode(false);
    }

    #[test]
    fn snapshots_dir_fn_returns_dot_by_default() {
        set_snapshots_base(None);
        assert_eq!(snapshots_dir_fn(&[]).unwrap(), Value::Str(Rc::from(".")));
    }

    #[test]
    fn bench_register_fn_adds_entry() {
        bench_registry_clear();
        let thunk = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn { name: "b", f: |_| Ok(Value::Unit) }));
        bench_register_fn(&[Value::Str(Rc::from("my-bench")), Value::Int(100), Value::Int(10), thunk]).unwrap();
        let entries = bench_registry_drain();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "my-bench");
    }
}
