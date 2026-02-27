//! `async` module — concurrency primitives re-export.
//!
//! Stage 0 provides stub implementations. Full concurrency support requires
//! the `Concurrent` effect and a task runtime.

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `async` module function entries.
///
/// Provides `sleep` as the only Stage 0 function.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("sleep", sleep as fn(&[Value]) -> Result<Value, String>),
    ]
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
        [other] => Err(format!("`async/sleep` expected Int, got {}", other.type_name())),
        _ => Err(format!(
            "`async/sleep` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sleep_zero() {
        assert_eq!(sleep(&[Value::Int(0)]).unwrap(), Value::Unit);
    }

    #[test]
    fn test_sleep_negative_error() {
        assert!(sleep(&[Value::Int(-1)]).is_err());
    }
}
