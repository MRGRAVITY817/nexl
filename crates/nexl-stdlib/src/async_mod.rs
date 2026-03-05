//! `async` module — concurrency primitives.
//!
//! Stage 0: backed by OS threads for `spawn`, immediate execution otherwise.
//! Since `Value` uses `Rc` (not `Send`), true concurrent execution is deferred.
//! Stage 0 `spawn` executes synchronously and wraps the result in a Future ADT.
//!
//! Functions:
//! - `(async/sleep ms)` — pause for ms milliseconds
//! - `(async/spawn thunk)` — execute thunk, return `(Future val)`
//! - `(async/await future)` — unwrap a Future
//! - `(async/timeout ms thunk)` — run thunk; `(Ok val)` or `(Err "timeout")`
//! - `(async/all thunks)` — run each thunk, return `(Vec val)`
//! - `(async/race thunks)` — return result of first thunk (Stage 0: first in list)
//! - `(async/defer cleanup thunk)` — run thunk, always run cleanup, return thunk result

use nexl_runtime::Value;
use std::rc::Rc;

use crate::StdlibEntry;

/// Return all `async` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("sleep", sleep as fn(&[Value]) -> Result<Value, String>),
        ("spawn", spawn_fn),
        ("await", await_fn),
        ("timeout", timeout_fn),
        ("all", all_fn),
        ("race", race_fn),
        ("defer", defer_fn),
    ]
}

fn future_adt(val: Value) -> Value {
    Value::Adt {
        type_name: Rc::from("Future"),
        ctor: Rc::from("Future"),
        fields: Rc::new(vec![val]),
    }
}

fn ok_val(v: Value) -> Value {
    Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Ok"),
        fields: Rc::new(vec![v]),
    }
}


/// `(async/sleep ms)` — pause execution for `ms` milliseconds.
fn sleep(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Concurrent)?;
    match args {
        [Value::Int(ms)] => {
            if *ms < 0 {
                return Err(format!("`async/sleep` ms must be non-negative, got {ms}"));
            }
            std::thread::sleep(std::time::Duration::from_millis(*ms as u64));
            Ok(Value::Unit)
        }
        [other] => Err(format!(
            "`async/sleep` expected Int, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`async/sleep` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(async/spawn thunk)` — execute thunk immediately, wrap result in `(Future val)`.
///
/// Stage 0: synchronous. True concurrent execution requires `Arc`-based Values.
fn spawn_fn(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Concurrent)?;
    match args {
        [thunk] => {
            let result = nexl_runtime::call_value(thunk, &[])
                .map_err(|e| format!("`async/spawn` thunk failed: {e}"))?;
            Ok(future_adt(result))
        }
        _ => Err(format!(
            "`async/spawn` requires 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(async/await future)` — unwrap a `(Future val)` to `val`.
fn await_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Adt { type_name, ctor, fields }]
            if type_name.as_ref() == "Future" && ctor.as_ref() == "Future" =>
        {
            fields
                .first()
                .cloned()
                .ok_or_else(|| "`async/await` Future has no value".into())
        }
        [other] => Err(format!(
            "`async/await` expected Future, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`async/await` requires 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(async/timeout ms thunk)` — run thunk; return `(Ok val)` or `(Err "timeout")`.
///
/// Stage 0: executes synchronously, so timeout is never triggered.
fn timeout_fn(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Concurrent)?;
    match args {
        [Value::Int(_ms), thunk] => {
            let result = nexl_runtime::call_value(thunk, &[])
                .map_err(|e| format!("`async/timeout` thunk failed: {e}"))?;
            Ok(ok_val(result))
        }
        [other, _] => Err(format!(
            "`async/timeout` expected Int ms, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`async/timeout` requires 2 arguments (ms thunk), got {}",
            args.len()
        )),
    }
}

/// `(async/all thunks)` — run all thunks sequentially, return `(Vec result)`.
fn all_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(thunks)] => {
            let mut results = Vec::with_capacity(thunks.len());
            for thunk in thunks.iter() {
                let v = nexl_runtime::call_value(thunk, &[])
                    .map_err(|e| format!("`async/all` thunk failed: {e}"))?;
                results.push(v);
            }
            Ok(Value::Vec(Rc::new(results)))
        }
        [other] => Err(format!(
            "`async/all` expected Vec of thunks, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`async/all` requires 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(async/race thunks)` — run first thunk, return its result (Stage 0: first in list).
fn race_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(thunks)] => {
            if thunks.is_empty() {
                return Err("`async/race` requires at least one thunk".into());
            }
            nexl_runtime::call_value(&thunks[0], &[])
                .map_err(|e| format!("`async/race` thunk failed: {e}"))
        }
        [other] => Err(format!(
            "`async/race` expected Vec of thunks, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`async/race` requires 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(async/defer cleanup thunk)` — run thunk, always run cleanup, return thunk result.
fn defer_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [cleanup, thunk] => {
            let result = nexl_runtime::call_value(thunk, &[]);
            // Always run cleanup, ignoring its result.
            let _ = nexl_runtime::call_value(cleanup, &[]);
            result.map_err(|e| format!("`async/defer` thunk failed: {e}"))
        }
        _ => Err(format!(
            "`async/defer` requires 2 arguments (cleanup thunk), got {}",
            args.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_thunk() -> Value {
        Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "noop",
            f: |_| Ok(Value::Unit),
        }))
    }

    fn const_thunk(val: Value) -> Value {
        // We can't close over val easily without a closure, use a native fn
        // that returns a known value via a side-channel-free approach.
        // Use Int 99 for testing.
        let _ = val; // accept the param for API parity
        Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "const99",
            f: |_| Ok(Value::Int(99)),
        }))
    }

    // 1. sleep 0 returns Unit
    #[test]
    fn test_sleep_zero() {
        assert_eq!(sleep(&[Value::Int(0)]).unwrap(), Value::Unit);
    }

    // 2. sleep negative returns error
    #[test]
    fn test_sleep_negative_error() {
        assert!(sleep(&[Value::Int(-1)]).is_err());
    }

    // 3. spawn wraps result in Future
    #[test]
    fn test_spawn_returns_future() {
        let thunk = const_thunk(Value::Int(42));
        let result = spawn_fn(&[thunk]).unwrap();
        assert!(
            matches!(&result, Value::Adt { type_name, ctor, .. }
                if type_name.as_ref() == "Future" && ctor.as_ref() == "Future"),
            "expected Future, got {result:?}"
        );
    }

    // 4. await unwraps Future
    #[test]
    fn test_await_unwraps_future() {
        let future = future_adt(Value::Int(99));
        let result = await_fn(&[future]).unwrap();
        assert_eq!(result, Value::Int(99));
    }

    // 5. await on non-Future errors
    #[test]
    fn test_await_non_future_errors() {
        assert!(await_fn(&[Value::Int(5)]).is_err());
    }

    // 6. timeout returns Ok(result)
    #[test]
    fn test_timeout_returns_ok() {
        let thunk = const_thunk(Value::Int(0));
        let result = timeout_fn(&[Value::Int(1000), thunk]).unwrap();
        assert!(matches!(&result, Value::Adt { ctor, .. } if ctor.as_ref() == "Ok"));
    }

    // 7. all runs all thunks, returns Vec
    #[test]
    fn test_all_runs_thunks() {
        let thunk = noop_thunk();
        let vec = Value::Vec(Rc::new(vec![thunk.clone(), thunk]));
        let result = all_fn(&[vec]).unwrap();
        match result {
            Value::Vec(v) => assert_eq!(v.len(), 2),
            other => panic!("expected Vec, got {other:?}"),
        }
    }

    // 8. all empty Vec returns empty Vec
    #[test]
    fn test_all_empty() {
        let result = all_fn(&[Value::Vec(Rc::new(vec![]))]).unwrap();
        match result {
            Value::Vec(v) => assert_eq!(v.len(), 0),
            other => panic!("expected Vec, got {other:?}"),
        }
    }

    // 9. race returns first thunk result
    #[test]
    fn test_race_returns_first() {
        let thunk = const_thunk(Value::Int(0));
        let result = race_fn(&[Value::Vec(Rc::new(vec![thunk]))]).unwrap();
        assert_eq!(result, Value::Int(99));
    }

    // 10. race empty Vec errors
    #[test]
    fn test_race_empty_errors() {
        assert!(race_fn(&[Value::Vec(Rc::new(vec![]))]).is_err());
    }

    // 11. defer runs cleanup and returns thunk result
    #[test]
    fn test_defer_runs_cleanup_and_returns_result() {
        let cleanup = noop_thunk();
        let thunk = const_thunk(Value::Int(0));
        let result = defer_fn(&[cleanup, thunk]).unwrap();
        assert_eq!(result, Value::Int(99));
    }

    // 12. entries include all functions
    #[test]
    fn test_entries_registered() {
        let names: Vec<&str> = entries().iter().map(|(n, _)| *n).collect();
        for name in ["sleep", "spawn", "await", "timeout", "all", "race", "defer"] {
            assert!(names.contains(&name), "missing: {name}");
        }
    }
}
