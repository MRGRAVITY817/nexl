//! `time` module — time and duration functions.
//!
//! Stage 0 uses `std::time` for basic time operations.

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `time` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("now", now as fn(&[Value]) -> Result<Value, String>),
        ("millis", millis),
        ("monotonic", monotonic),
    ]
}

/// `(time/now)` — current time as Unix milliseconds (Int).
fn now(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Time)?;
    if !args.is_empty() {
        return Err(format!("`time/now` takes no arguments, got {}", args.len()));
    }
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Ok(Value::Int(ms))
}

/// `(time/monotonic)` — monotonic clock reading in nanoseconds (Int).
///
/// Uses `std::time::Instant` measured from process start. Guaranteed to be
/// non-decreasing. Useful for elapsed-time measurements.
fn monotonic(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`time/monotonic` takes no arguments, got {}", args.len()));
    }
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    let ns = start.elapsed().as_nanos() as i64;
    Ok(Value::Int(ns))
}

/// `(time/millis duration-int)` — identity; documents that the Int is in milliseconds.
fn millis(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(n)] => Ok(Value::Int(*n)),
        [other] => Err(format!(
            "`time/millis` expected Int, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`time/millis` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monotonic_returns_positive_int() {
        let result = monotonic(&[]).unwrap();
        match result {
            Value::Int(ns) => assert!(ns > 0, "monotonic() should return positive nanoseconds"),
            _ => panic!("expected Int"),
        }
    }

    #[test]
    fn test_monotonic_is_nondecreasing() {
        let t1 = match monotonic(&[]).unwrap() {
            Value::Int(n) => n,
            _ => panic!("expected Int"),
        };
        let t2 = match monotonic(&[]).unwrap() {
            Value::Int(n) => n,
            _ => panic!("expected Int"),
        };
        assert!(t2 >= t1, "monotonic clock must be non-decreasing: t1={t1} t2={t2}");
    }

    #[test]
    fn test_now_returns_int() {
        let result = now(&[]).unwrap();
        match result {
            Value::Int(ms) => assert!(ms > 0, "now() should return positive millis"),
            _ => panic!("expected Int"),
        }
    }

    #[test]
    fn test_millis_identity() {
        assert_eq!(millis(&[Value::Int(1000)]).unwrap(), Value::Int(1000));
    }
}
