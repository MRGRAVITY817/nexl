//! `base64` module — Base64 encoding and decoding.
//!
//! Standard and URL-safe (no-padding) variants. Backed by the `base64` crate.

use std::rc::Rc;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `base64` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("encode", encode as fn(&[Value]) -> Result<Value, String>),
        ("decode", decode),
        ("encode-url", encode_url),
        ("decode-url", decode_url),
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

fn expect_str<'a>(name: &str, v: &'a Value) -> Result<&'a str, String> {
    match v {
        Value::Str(s) => Ok(s.as_ref()),
        other => Err(format!("`base64/{name}` expected Str, got {other}")),
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// `(base64/encode str)` → `Str` — standard base64 encode.
fn encode(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let s = expect_str("encode", v)?;
            Ok(Value::Str(Rc::from(STANDARD.encode(s.as_bytes()).as_str())))
        }
        _ => Err(format!("`base64/encode` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(base64/decode str)` → `(Result Str Str)` — standard base64 decode.
fn decode(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let s = expect_str("decode", v)?;
            match STANDARD.decode(s) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => Ok(ok(Value::Str(Rc::from(text.as_str())))),
                    Err(e) => Ok(err(&e.to_string())),
                },
                Err(e) => Ok(err(&e.to_string())),
            }
        }
        _ => Err(format!("`base64/decode` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(base64/encode-url str)` → `Str` — URL-safe base64 encode (no padding).
fn encode_url(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let s = expect_str("encode-url", v)?;
            Ok(Value::Str(Rc::from(URL_SAFE_NO_PAD.encode(s.as_bytes()).as_str())))
        }
        _ => Err(format!("`base64/encode-url` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(base64/decode-url str)` → `(Result Str Str)` — URL-safe base64 decode.
fn decode_url(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let s = expect_str("decode-url", v)?;
            match URL_SAFE_NO_PAD.decode(s) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => Ok(ok(Value::Str(Rc::from(text.as_str())))),
                    Err(e) => Ok(err(&e.to_string())),
                },
                Err(e) => Ok(err(&e.to_string())),
            }
        }
        _ => Err(format!("`base64/decode-url` requires 1 argument (Str), got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Value { Value::Str(Rc::from(text)) }

    #[test]
    fn test_encode_basic() {
        let result = encode(&[s("hello")]).unwrap();
        assert_eq!(result, Value::Str(Rc::from("aGVsbG8=")));
    }

    #[test]
    fn test_decode_basic() {
        let result = decode(&[s("aGVsbG8=")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Ok"));
        if let Value::Adt { fields, .. } = result {
            assert_eq!(fields[0], Value::Str(Rc::from("hello")));
        }
    }

    #[test]
    fn test_decode_invalid() {
        let result = decode(&[s("not!valid!base64!!!")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Err"));
    }

    #[test]
    fn test_encode_url_no_padding() {
        // URL-safe encoded strings should not contain '='
        let result = encode_url(&[s("hello world")]).unwrap();
        assert!(matches!(result, Value::Str(_)));
        if let Value::Str(s) = result {
            assert!(!s.contains('='));
            assert!(!s.contains('+'));
            assert!(!s.contains('/'));
        }
    }

    #[test]
    fn test_roundtrip() {
        let input = "Hello, Nexl! \u{1F980}"; // 🦀
        let encoded = encode(&[s(input)]).unwrap();
        let decoded = decode(&[encoded]).unwrap();
        if let Value::Adt { fields, .. } = decoded {
            assert_eq!(fields[0], Value::Str(Rc::from(input)));
        }
    }

    #[test]
    fn test_roundtrip_url() {
        let input = "hello world foo bar";
        let encoded = encode_url(&[s(input)]).unwrap();
        let decoded = decode_url(&[encoded]).unwrap();
        if let Value::Adt { fields, .. } = decoded {
            assert_eq!(fields[0], Value::Str(Rc::from(input)));
        }
    }
}
