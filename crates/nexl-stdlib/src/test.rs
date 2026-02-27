//! `test` module — testing framework.
//!
//! Stage 0 provides `is` (assertion) and `assert-eq` as native functions.
//! `deftest`, `check` (property-based), and generators are deferred to when
//! the macro system can support them.

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `test` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("is", is as fn(&[Value]) -> Result<Value, String>),
        ("assert-eq", assert_eq_fn),
    ]
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
}
