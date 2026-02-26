//! `conv` module — numeric conversion functions.
//!
//! Provides: `->int`, `->float`, `->f32`, `->str`, widening (total) and
//! narrowing (Option) conversions.
//!
//! In the Stage 0 bootstrap compiler we only have `Int` (i64) and `Float` (f64),
//! so fixed-width conversions (`->int8`, `->u8`, etc.) are deferred.

use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `conv` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("->int", to_int as fn(&[Value]) -> Result<Value, String>),
        ("->float", to_float),
        ("->str", to_str),
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn one_arg<'a>(op: &str, args: &'a [Value]) -> Result<&'a Value, String> {
    match args {
        [a] => Ok(a),
        _ => Err(format!("`conv/{op}` requires exactly 1 argument, got {}", args.len())),
    }
}

fn option_some(v: Value) -> Value {
    Value::Adt {
        type_name: Rc::from("Option"),
        ctor: Rc::from("Some"),
        fields: Rc::new(vec![v]),
    }
}

fn option_none() -> Value {
    Value::Adt {
        type_name: Rc::from("Option"),
        ctor: Rc::from("None"),
        fields: Rc::new(vec![]),
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// `(conv/->int x)` — convert to Int.
///
/// - Float → Int: truncates. Returns `(Some Int)` if value fits i64, else `None`.
/// - Str → (Option Int): parses decimal integer string.
/// - Bool → Int: true=1, false=0.
/// - Int → Int: identity.
fn to_int(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("->int", args)?;
    match v {
        Value::Int(n) => Ok(option_some(Value::Int(*n))),
        Value::Float(f) => {
            if f.is_finite() && *f >= i64::MIN as f64 && *f <= i64::MAX as f64 {
                Ok(option_some(Value::Int(*f as i64)))
            } else {
                Ok(option_none())
            }
        }
        Value::Bool(b) => Ok(option_some(Value::Int(if *b { 1 } else { 0 }))),
        Value::Str(s) => match s.parse::<i64>() {
            Ok(n) => Ok(option_some(Value::Int(n))),
            Err(_) => Ok(option_none()),
        },
        other => Err(format!(
            "`conv/->int` cannot convert {} to Int",
            other.type_name()
        )),
    }
}

/// `(conv/->float x)` — convert to Float.
///
/// - Int → Float: widening (always succeeds).
/// - Str → (Option Float): parses float string.
/// - Bool → Float: true=1.0, false=0.0.
/// - Float → Float: identity.
fn to_float(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("->float", args)?;
    match v {
        Value::Float(f) => Ok(option_some(Value::Float(*f))),
        Value::Int(n) => Ok(option_some(Value::Float(*n as f64))),
        Value::Bool(b) => Ok(option_some(Value::Float(if *b { 1.0 } else { 0.0 }))),
        Value::Str(s) => match s.parse::<f64>() {
            Ok(f) => Ok(option_some(Value::Float(f))),
            Err(_) => Ok(option_none()),
        },
        other => Err(format!(
            "`conv/->float` cannot convert {} to Float",
            other.type_name()
        )),
    }
}

/// `(conv/->str x)` — convert any value to its string representation.
fn to_str(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("->str", args)?;
    match v {
        Value::Str(s) => Ok(Value::Str(s.clone())),
        other => Ok(Value::Str(Rc::from(other.to_string().as_str()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_int_from_int() {
        assert_eq!(
            to_int(&[Value::Int(42)]).unwrap(),
            option_some(Value::Int(42))
        );
    }

    #[test]
    fn test_to_int_from_float() {
        assert_eq!(
            to_int(&[Value::Float(3.7)]).unwrap(),
            option_some(Value::Int(3))
        );
    }

    #[test]
    fn test_to_int_from_float_nan() {
        assert_eq!(to_int(&[Value::Float(f64::NAN)]).unwrap(), option_none());
    }

    #[test]
    fn test_to_int_from_bool() {
        assert_eq!(
            to_int(&[Value::Bool(true)]).unwrap(),
            option_some(Value::Int(1))
        );
        assert_eq!(
            to_int(&[Value::Bool(false)]).unwrap(),
            option_some(Value::Int(0))
        );
    }

    #[test]
    fn test_to_int_from_str() {
        assert_eq!(
            to_int(&[Value::Str(Rc::from("42"))]).unwrap(),
            option_some(Value::Int(42))
        );
        assert_eq!(
            to_int(&[Value::Str(Rc::from("abc"))]).unwrap(),
            option_none()
        );
    }

    #[test]
    fn test_to_float_from_int() {
        assert_eq!(
            to_float(&[Value::Int(42)]).unwrap(),
            option_some(Value::Float(42.0))
        );
    }

    #[test]
    fn test_to_float_from_str() {
        assert_eq!(
            to_float(&[Value::Str(Rc::from("2.5"))]).unwrap(),
            option_some(Value::Float(2.5))
        );
    }

    #[test]
    fn test_to_str_from_int() {
        assert_eq!(
            to_str(&[Value::Int(42)]).unwrap(),
            Value::Str(Rc::from("42"))
        );
    }

    #[test]
    fn test_to_str_from_bool() {
        assert_eq!(
            to_str(&[Value::Bool(true)]).unwrap(),
            Value::Str(Rc::from("true"))
        );
    }

    #[test]
    fn test_to_str_identity() {
        assert_eq!(
            to_str(&[Value::Str(Rc::from("hello"))]).unwrap(),
            Value::Str(Rc::from("hello"))
        );
    }
}
