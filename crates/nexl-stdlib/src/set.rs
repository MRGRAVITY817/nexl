//! `set` module — set-specific operations.
//!
//! Provides constructors `of`, `from-vec`, and `to-vec` in Rust.
//! Transforms and folds are written in Nexl (`set_impl.nx`).

use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `set` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("of", of as fn(&[Value]) -> Result<Value, String>),
        ("from-vec", from_vec),
        ("to-vec", to_vec),
    ]
}

/// `(set/of elem...)` — construct a Set from variadic arguments (deduplicates).
fn of(args: &[Value]) -> Result<Value, String> {
    let mut items: Vec<Value> = Vec::new();
    for v in args {
        if !items.contains(v) {
            items.push(v.clone());
        }
    }
    Ok(Value::Set(Rc::new(items)))
}

/// `(set/from-vec xs)` — build a Set from a Vec (deduplicates, preserves first occurrence order).
fn from_vec(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v)] => {
            let mut items: Vec<Value> = Vec::new();
            for item in v.iter() {
                if !items.contains(item) {
                    items.push(item.clone());
                }
            }
            Ok(Value::Set(Rc::new(items)))
        }
        _ => Err(format!(
            "`set/from-vec` requires 1 argument (Vec), got {}",
            args.len()
        )),
    }
}

/// `(set/to-vec s)` — convert a Set to a Vec (in insertion order).
fn to_vec(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Set(items)] => Ok(Value::Vec(Rc::new(items.to_vec()))),
        _ => Err(format!(
            "`set/to-vec` requires 1 argument (Set), got {}",
            args.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_of_deduplicates() {
        let result = of(&[Value::Int(1), Value::Int(2), Value::Int(1), Value::Int(3)]).unwrap();
        match result {
            Value::Set(items) => assert_eq!(items.len(), 3),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn test_of_empty() {
        let result = of(&[]).unwrap();
        match result {
            Value::Set(items) => assert_eq!(items.len(), 0),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn test_from_vec_deduplicates() {
        let v = Value::Vec(Rc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(2),
            Value::Int(3),
        ]));
        let result = from_vec(&[v]).unwrap();
        match result {
            Value::Set(items) => assert_eq!(items.len(), 3),
            _ => panic!("expected Set"),
        }
    }

    #[test]
    fn test_to_vec() {
        let s = Value::Set(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        let result = to_vec(&[s]).unwrap();
        match result {
            Value::Vec(items) => assert_eq!(items.len(), 3),
            _ => panic!("expected Vec"),
        }
    }
}
