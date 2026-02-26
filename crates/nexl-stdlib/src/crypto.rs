//! `crypto` module — cryptographic functions.
//!
//! Stage 0 provides basic hashing via Rust's standard library.
//! SHA-256 and SHA-3 require external crates (deferred).
//! For now we provide a simple hash function using Rust's DefaultHasher.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `crypto` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("hash", hash_fn as fn(&[Value]) -> Result<Value, String>),
        ("constant-time=", constant_time_eq),
    ]
}

fn one_arg<'a>(op: &str, args: &'a [Value]) -> Result<&'a Value, String> {
    match args {
        [a] => Ok(a),
        _ => Err(format!("`crypto/{op}` requires exactly 1 argument, got {}", args.len())),
    }
}

fn two_args<'a>(op: &str, args: &'a [Value]) -> Result<(&'a Value, &'a Value), String> {
    match args {
        [a, b] => Ok((a, b)),
        _ => Err(format!("`crypto/{op}` requires exactly 2 arguments, got {}", args.len())),
    }
}

fn expect_str<'a>(op: &str, v: &'a Value) -> Result<&'a Rc<str>, String> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(format!("`crypto/{op}` expected Str, got {}", other.type_name())),
    }
}

/// `(crypto/hash s)` — compute a hash of a string (returns Int).
///
/// Uses Rust's DefaultHasher. NOT cryptographically secure.
/// SHA-256/SHA-3 to be added when external crates are included.
fn hash_fn(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("hash", args)?;
    let s = expect_str("hash", v)?;
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    Ok(Value::Int(hasher.finish() as i64))
}

/// `(crypto/constant-time= a b)` — constant-time string comparison.
fn constant_time_eq(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("constant-time=", args)?;
    let a = expect_str("constant-time=", a)?;
    let b = expect_str("constant-time=", b)?;

    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();

    if a_bytes.len() != b_bytes.len() {
        return Ok(Value::Bool(false));
    }

    let mut diff: u8 = 0;
    for (x, y) in a_bytes.iter().zip(b_bytes.iter()) {
        diff |= x ^ y;
    }
    Ok(Value::Bool(diff == 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_deterministic() {
        let h1 = hash_fn(&[Value::Str(Rc::from("hello"))]).unwrap();
        let h2 = hash_fn(&[Value::Str(Rc::from("hello"))]).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_different_inputs() {
        let h1 = hash_fn(&[Value::Str(Rc::from("hello"))]).unwrap();
        let h2 = hash_fn(&[Value::Str(Rc::from("world"))]).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_constant_time_eq_same() {
        assert_eq!(
            constant_time_eq(&[
                Value::Str(Rc::from("secret")),
                Value::Str(Rc::from("secret")),
            ]).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn test_constant_time_eq_different() {
        assert_eq!(
            constant_time_eq(&[
                Value::Str(Rc::from("secret")),
                Value::Str(Rc::from("other!")),
            ]).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn test_constant_time_eq_different_lengths() {
        assert_eq!(
            constant_time_eq(&[
                Value::Str(Rc::from("short")),
                Value::Str(Rc::from("longer")),
            ]).unwrap(),
            Value::Bool(false)
        );
    }
}
