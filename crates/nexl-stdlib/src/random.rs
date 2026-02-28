//! `random` module — cryptographically secure random number generation.
//!
//! Stage 0 uses `getrandom` for crypto-secure bytes.

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `random` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![("bytes", bytes as fn(&[Value]) -> Result<Value, String>)]
}

/// `(random/bytes n)` — return a Vec of `n` random bytes (each `Int` 0–255).
///
/// Uses the OS cryptographically secure random source (via `getrandom`).
fn bytes(args: &[Value]) -> Result<Value, String> {
    let n = match args {
        [Value::Int(n)] => *n,
        [other] => {
            return Err(format!(
                "`random/bytes` expected Int, got {}",
                other.type_name()
            ))
        }
        _ => {
            return Err(format!(
                "`random/bytes` requires exactly 1 argument, got {}",
                args.len()
            ))
        }
    };
    if n < 0 {
        return Err(format!("`random/bytes` count must be non-negative, got {n}"));
    }
    let mut buf = vec![0u8; n as usize];
    getrandom::getrandom(&mut buf)
        .map_err(|e| format!("`random/bytes` OS error: {e}"))?;
    let values: Vec<Value> = buf.into_iter().map(|b| Value::Int(b as i64)).collect();
    Ok(Value::Vec(std::rc::Rc::new(values)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_bytes_correct_length() {
        let result = bytes(&[Value::Int(16)]).unwrap();
        match result {
            Value::Vec(v) => assert_eq!(v.len(), 16, "bytes(16) should return 16 elements"),
            _ => panic!("expected Vec"),
        }
    }

    #[test]
    fn test_random_bytes_values_in_range() {
        let result = bytes(&[Value::Int(32)]).unwrap();
        match result {
            Value::Vec(v) => {
                for elem in v.iter() {
                    match elem {
                        Value::Int(b) => {
                            assert!(
                                *b >= 0 && *b <= 255,
                                "each byte must be in 0..=255, got {b}"
                            );
                        }
                        _ => panic!("expected Int element, got {elem:?}"),
                    }
                }
            }
            _ => panic!("expected Vec"),
        }
    }

    #[test]
    fn test_random_bytes_zero_length() {
        let result = bytes(&[Value::Int(0)]).unwrap();
        match result {
            Value::Vec(v) => assert!(v.is_empty(), "bytes(0) should return empty Vec"),
            _ => panic!("expected Vec"),
        }
    }
}
