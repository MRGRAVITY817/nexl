//! `uuid` module — UUID generation and parsing.
//!
//! Backed by the `uuid` crate with v4 (random) and v7 (time-ordered) variants.
//! Uuid values are stored as hyphenated lowercase strings.

use std::rc::Rc;

use uuid::Uuid;
use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `uuid` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("v4", v4 as fn(&[Value]) -> Result<Value, String>),
        ("v7", v7),
        ("parse", parse),
        ("to-str", to_str),
        ("nil", nil),
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn adt(type_name: &str, ctor: &str, fields: Vec<Value>) -> Value {
    Value::Adt {
        type_name: Rc::from(type_name),
        ctor: Rc::from(ctor),
        fields: Rc::new(fields),
    }
}

fn ok(v: Value) -> Value { adt("Result", "Ok", vec![v]) }
fn err(msg: &str) -> Value { adt("Result", "Err", vec![Value::Str(Rc::from(msg))]) }

/// Pack a UUID as a `(Uuid "hyphenated-string")` ADT.
fn uuid_val(u: Uuid) -> Value {
    adt("Uuid", "Uuid", vec![Value::Str(Rc::from(u.hyphenated().to_string().as_str()))])
}

/// Extract UUID string from a `(Uuid "...")` ADT.
fn get_uuid_str(v: &Value) -> Result<String, String> {
    match v {
        Value::Adt { ctor, fields, .. } if ctor.as_ref() == "Uuid" => {
            match fields.first() {
                Some(Value::Str(s)) => Ok(s.to_string()),
                _ => Err("`uuid` functions require a Uuid value".to_string()),
            }
        }
        _ => Err(format!("`uuid` functions require a Uuid value, got {v}")),
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// `(uuid/v4)` → `(Uuid "...")` — generate a random UUID v4.
fn v4(args: &[Value]) -> Result<Value, String> {
    match args {
        [] => Ok(uuid_val(Uuid::new_v4())),
        _ => Err(format!("`uuid/v4` requires 0 arguments, got {}", args.len())),
    }
}

/// `(uuid/v7)` → `(Uuid "...")` — generate a time-ordered UUID v7.
fn v7(args: &[Value]) -> Result<Value, String> {
    match args {
        [] => {
            let ts = uuid::Timestamp::now(uuid::NoContext);
            Ok(uuid_val(Uuid::new_v7(ts)))
        }
        _ => Err(format!("`uuid/v7` requires 0 arguments, got {}", args.len())),
    }
}

/// `(uuid/parse str)` → `(Result (Uuid "...") Str)` — parse a UUID string.
fn parse(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => match Uuid::parse_str(s) {
            Ok(u) => Ok(ok(uuid_val(u))),
            Err(e) => Ok(err(&e.to_string())),
        },
        _ => Err(format!("`uuid/parse` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(uuid/to-str uuid)` → `Str` — render UUID as hyphenated lowercase string.
fn to_str(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let s = get_uuid_str(v)?;
            Ok(Value::Str(Rc::from(s.as_str())))
        }
        _ => Err(format!("`uuid/to-str` requires 1 argument (Uuid), got {}", args.len())),
    }
}

/// `(uuid/nil)` → `(Uuid "00000000-0000-0000-0000-000000000000")` — the nil UUID.
fn nil(args: &[Value]) -> Result<Value, String> {
    match args {
        [] => Ok(uuid_val(Uuid::nil())),
        _ => Err(format!("`uuid/nil` requires 0 arguments, got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v4_is_uuid() {
        let result = v4(&[]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Uuid"));
    }

    #[test]
    fn test_v4_unique() {
        let a = v4(&[]).unwrap();
        let b = v4(&[]).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn test_v7_is_uuid() {
        let result = v7(&[]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Uuid"));
    }

    #[test]
    fn test_v7_has_uuid_format() {
        // v7 UUIDs should be valid 36-char hyphenated UUIDs
        let result = to_str(&[v7(&[]).unwrap()]).unwrap();
        if let Value::Str(s) = result {
            assert_eq!(s.len(), 36);
            assert_eq!(s.chars().filter(|&c| c == '-').count(), 4);
        }
    }

    #[test]
    fn test_parse_valid() {
        let result = parse(&[Value::Str(Rc::from("550e8400-e29b-41d4-a716-446655440000"))]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Ok"));
    }

    #[test]
    fn test_parse_invalid() {
        let result = parse(&[Value::Str(Rc::from("not-a-uuid"))]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Err"));
    }

    #[test]
    fn test_to_str() {
        let u = v4(&[]).unwrap();
        let s = to_str(&[u]).unwrap();
        assert!(matches!(s, Value::Str(_)));
        if let Value::Str(text) = s {
            assert_eq!(text.len(), 36);
            assert!(text.contains('-'));
        }
    }

    #[test]
    fn test_nil() {
        let result = nil(&[]).unwrap();
        let s = to_str(&[result]).unwrap();
        assert_eq!(s, Value::Str(Rc::from("00000000-0000-0000-0000-000000000000")));
    }

    #[test]
    fn test_roundtrip() {
        let u = v4(&[]).unwrap();
        let s = to_str(&[u]).unwrap();
        let parsed = parse(&[s]).unwrap();
        assert!(matches!(parsed, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Ok"));
    }
}
