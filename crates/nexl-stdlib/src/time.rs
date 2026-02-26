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
    ]
}

/// `(time/now)` — current time as Unix milliseconds (Int).
fn now(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`time/now` takes no arguments, got {}", args.len()));
    }
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Ok(Value::Int(ms))
}

/// `(time/millis duration-int)` — identity; documents that the Int is in milliseconds.
fn millis(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(n)] => Ok(Value::Int(*n)),
        [other] => Err(format!("`time/millis` expected Int, got {}", other.type_name())),
        _ => Err(format!("`time/millis` requires exactly 1 argument, got {}", args.len())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
