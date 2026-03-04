//! `map` module — map-specific operations.
//!
//! Provides constructors `of` and `from-entries` in Rust.
//! Other operations (transforms, folds, queries) are written in Nexl (`map_impl.nx`).

use std::rc::Rc;

use nexl_runtime::value::NexlMap;
use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `map` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("of", of as fn(&[Value]) -> Result<Value, String>),
        ("from-entries", from_entries),
    ]
}

/// `(map/of [k v]...)` — construct a Map from variadic `[key value]` pair arguments.
fn of(args: &[Value]) -> Result<Value, String> {
    let mut map = NexlMap::new();
    for (i, arg) in args.iter().enumerate() {
        match arg {
            Value::Vec(pair) if pair.len() == 2 => {
                map = map.put(pair[0].clone(), pair[1].clone());
            }
            _ => {
                return Err(format!(
                    "`map/of` argument {i} must be a [key value] pair, got {}",
                    arg.type_name()
                ))
            }
        }
    }
    Ok(Value::Map(Rc::new(map)))
}

/// `(map/from-entries entries)` — build a Map from a Vec of `[key value]` pairs.
fn from_entries(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(pairs)] => {
            let mut map = NexlMap::new();
            for (i, pair) in pairs.iter().enumerate() {
                match pair {
                    Value::Vec(kv) if kv.len() == 2 => {
                        map = map.put(kv[0].clone(), kv[1].clone());
                    }
                    _ => {
                        return Err(format!(
                            "`map/from-entries` element {i} must be a [key value] pair, got {}",
                            pair.type_name()
                        ))
                    }
                }
            }
            Ok(Value::Map(Rc::new(map)))
        }
        _ => Err(format!(
            "`map/from-entries` requires 1 argument (Vec of [key value] pairs), got {}",
            args.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kw(name: &str) -> Value {
        Value::Keyword {
            ns: None,
            name: Rc::from(name),
        }
    }

    #[test]
    fn test_of_basic() {
        let result = of(&[
            Value::Vec(Rc::new(vec![kw("a"), Value::Int(1)])),
            Value::Vec(Rc::new(vec![kw("b"), Value::Int(2)])),
        ])
        .unwrap();
        match result {
            Value::Map(m) => {
                assert_eq!(m.len(), 2);
                assert_eq!(m.get(&kw("a")), Some(&Value::Int(1)));
                assert_eq!(m.get(&kw("b")), Some(&Value::Int(2)));
            }
            _ => panic!("expected Map"),
        }
    }

    #[test]
    fn test_of_empty() {
        let result = of(&[]).unwrap();
        match result {
            Value::Map(m) => assert_eq!(m.len(), 0),
            _ => panic!("expected Map"),
        }
    }

    #[test]
    fn test_of_non_pair_err() {
        assert!(of(&[Value::Int(1)]).is_err());
    }

    #[test]
    fn test_from_entries_basic() {
        let pairs = Value::Vec(Rc::new(vec![
            Value::Vec(Rc::new(vec![kw("x"), Value::Int(10)])),
            Value::Vec(Rc::new(vec![kw("y"), Value::Int(20)])),
        ]));
        let result = from_entries(&[pairs]).unwrap();
        match result {
            Value::Map(m) => {
                assert_eq!(m.len(), 2);
                assert_eq!(m.get(&kw("x")), Some(&Value::Int(10)));
            }
            _ => panic!("expected Map"),
        }
    }

    #[test]
    fn test_from_entries_empty() {
        let pairs = Value::Vec(Rc::new(vec![]));
        let result = from_entries(&[pairs]).unwrap();
        match result {
            Value::Map(m) => assert_eq!(m.len(), 0),
            _ => panic!("expected Map"),
        }
    }

    #[test]
    fn test_from_entries_last_wins() {
        let pairs = Value::Vec(Rc::new(vec![
            Value::Vec(Rc::new(vec![kw("a"), Value::Int(1)])),
            Value::Vec(Rc::new(vec![kw("a"), Value::Int(2)])),
        ]));
        let result = from_entries(&[pairs]).unwrap();
        match result {
            Value::Map(m) => assert_eq!(m.get(&kw("a")), Some(&Value::Int(2))),
            _ => panic!("expected Map"),
        }
    }
}
