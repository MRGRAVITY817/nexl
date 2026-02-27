//! `json` module — JSON parsing and serialization.
//!
//! Stage 0 implementation uses a simple recursive descent parser for JSON
//! and a recursive stringifier. No external dependencies.

use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `json` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("parse", parse as fn(&[Value]) -> Result<Value, String>),
        ("stringify", stringify),
    ]
}

fn one_arg<'a>(op: &str, args: &'a [Value]) -> Result<&'a Value, String> {
    match args {
        [a] => Ok(a),
        _ => Err(format!(
            "`json/{op}` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

fn expect_str<'a>(op: &str, v: &'a Value) -> Result<&'a Rc<str>, String> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(format!(
            "`json/{op}` expected Str, got {}",
            other.type_name()
        )),
    }
}

/// `(json/parse s)` — parse a JSON string into a Nexl Value.
///
/// Returns `(Result Value Str)`.
/// JSON → Nexl mapping:
/// - number → Int or Float
/// - string → Str
/// - boolean → Bool
/// - null → Unit
/// - array → Vec
/// - object → Map (keyword keys)
fn parse(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("parse", args)?;
    let s = expect_str("parse", v)?;
    match parse_json(s.as_ref()) {
        Ok(val) => Ok(Value::Adt {
            type_name: Rc::from("Result"),
            ctor: Rc::from("Ok"),
            fields: Rc::new(vec![val]),
        }),
        Err(e) => Ok(Value::Adt {
            type_name: Rc::from("Result"),
            ctor: Rc::from("Err"),
            fields: Rc::new(vec![Value::Str(Rc::from(e.as_str()))]),
        }),
    }
}

/// `(json/stringify v)` — convert a Nexl Value to a JSON string.
fn stringify(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("stringify", args)?;
    Ok(Value::Str(Rc::from(value_to_json(v).as_str())))
}

// ---------------------------------------------------------------------------
// Simple JSON parser
// ---------------------------------------------------------------------------

fn parse_json(input: &str) -> Result<Value, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty JSON input".into());
    }
    let (val, rest) = parse_value(trimmed)?;
    if !rest.trim().is_empty() {
        return Err(format!("trailing data after JSON value: {rest}"));
    }
    Ok(val)
}

fn parse_value(input: &str) -> Result<(Value, &str), String> {
    let input = input.trim_start();
    if input.is_empty() {
        return Err("unexpected end of JSON".into());
    }
    match input.as_bytes()[0] {
        b'"' => parse_string(input),
        b'{' => parse_object(input),
        b'[' => parse_array(input),
        b't' => parse_literal(input, "true", Value::Bool(true)),
        b'f' => parse_literal(input, "false", Value::Bool(false)),
        b'n' => parse_literal(input, "null", Value::Unit),
        b'-' | b'0'..=b'9' => parse_number(input),
        c => Err(format!("unexpected character in JSON: '{}'", c as char)),
    }
}

fn parse_literal<'a>(
    input: &'a str,
    expected: &str,
    value: Value,
) -> Result<(Value, &'a str), String> {
    if let Some(rest) = input.strip_prefix(expected) {
        Ok((value, rest))
    } else {
        Err(format!("expected `{expected}` in JSON"))
    }
}

fn parse_string(input: &str) -> Result<(Value, &str), String> {
    debug_assert!(input.starts_with('"'));
    let rest = &input[1..];
    let mut result = String::new();
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => {
                return Ok((Value::Str(Rc::from(result.as_str())), &rest[i + 1..]));
            }
            '\\' => {
                if let Some((_, escaped)) = chars.next() {
                    match escaped {
                        '"' => result.push('"'),
                        '\\' => result.push('\\'),
                        '/' => result.push('/'),
                        'n' => result.push('\n'),
                        't' => result.push('\t'),
                        'r' => result.push('\r'),
                        _ => {
                            result.push('\\');
                            result.push(escaped);
                        }
                    }
                } else {
                    return Err("unexpected end of JSON string".into());
                }
            }
            _ => result.push(c),
        }
    }
    Err("unterminated JSON string".into())
}

fn parse_number(input: &str) -> Result<(Value, &str), String> {
    let end = input
        .find(|c: char| {
            !c.is_ascii_digit() && c != '.' && c != '-' && c != '+' && c != 'e' && c != 'E'
        })
        .unwrap_or(input.len());
    let num_str = &input[..end];
    let rest = &input[end..];

    if num_str.contains('.') || num_str.contains('e') || num_str.contains('E') {
        let f: f64 = num_str
            .parse()
            .map_err(|_| format!("invalid JSON number: {num_str}"))?;
        Ok((Value::Float(f), rest))
    } else {
        let n: i64 = num_str
            .parse()
            .map_err(|_| format!("invalid JSON number: {num_str}"))?;
        Ok((Value::Int(n), rest))
    }
}

fn parse_array(input: &str) -> Result<(Value, &str), String> {
    debug_assert!(input.starts_with('['));
    let mut rest = input[1..].trim_start();
    let mut items = Vec::new();

    if let Some(after) = rest.strip_prefix(']') {
        return Ok((Value::Vec(Rc::new(items)), after));
    }

    loop {
        let (val, after) = parse_value(rest)?;
        items.push(val);
        rest = after.trim_start();
        if let Some(after) = rest.strip_prefix(']') {
            return Ok((Value::Vec(Rc::new(items)), after));
        }
        if let Some(after) = rest.strip_prefix(',') {
            rest = after.trim_start();
        } else {
            return Err("expected ',' or ']' in JSON array".into());
        }
    }
}

fn parse_object(input: &str) -> Result<(Value, &str), String> {
    debug_assert!(input.starts_with('{'));
    let mut rest = input[1..].trim_start();
    let mut entries = Vec::new();

    if let Some(after) = rest.strip_prefix('}') {
        return Ok((Value::Map(Rc::new(entries)), after));
    }

    loop {
        let (key_val, after_key) = parse_string(rest.trim_start())?;
        let Value::Str(key_str) = key_val else {
            return Err("JSON object key must be a string".into());
        };
        let after_colon = after_key.trim_start();
        let Some(after_colon) = after_colon.strip_prefix(':') else {
            return Err("expected ':' in JSON object".into());
        };

        let (val, after_val) = parse_value(after_colon)?;
        let key = Value::Keyword {
            ns: None,
            name: key_str,
        };
        entries.push((key, val));

        rest = after_val.trim_start();
        if let Some(after) = rest.strip_prefix('}') {
            return Ok((Value::Map(Rc::new(entries)), after));
        }
        if let Some(after) = rest.strip_prefix(',') {
            rest = after.trim_start();
        } else {
            return Err("expected ',' or '}' in JSON object".into());
        }
    }
}

// ---------------------------------------------------------------------------
// JSON stringifier
// ---------------------------------------------------------------------------

fn value_to_json(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(f) => {
            if f.is_infinite() || f.is_nan() {
                "null".to_string()
            } else {
                f.to_string()
            }
        }
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::Unit => "null".to_string(),
        Value::Vec(items) => {
            let parts: Vec<String> = items.iter().map(value_to_json).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Map(entries) => {
            let parts: Vec<String> = entries
                .iter()
                .map(|(k, v)| {
                    let key_str = match k {
                        Value::Keyword { name, .. } => name.to_string(),
                        Value::Str(s) => s.to_string(),
                        other => other.to_string(),
                    };
                    format!("\"{}\":{}", key_str, value_to_json(v))
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        _ => "null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_int() {
        let result = parse_json("42").unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn test_parse_float() {
        let result = parse_json("2.5").unwrap();
        assert_eq!(result, Value::Float(2.5));
    }

    #[test]
    fn test_parse_string() {
        let result = parse_json(r#""hello""#).unwrap();
        assert_eq!(result, Value::Str(Rc::from("hello")));
    }

    #[test]
    fn test_parse_bool() {
        assert_eq!(parse_json("true").unwrap(), Value::Bool(true));
        assert_eq!(parse_json("false").unwrap(), Value::Bool(false));
    }

    #[test]
    fn test_parse_null() {
        assert_eq!(parse_json("null").unwrap(), Value::Unit);
    }

    #[test]
    fn test_parse_array() {
        let result = parse_json("[1, 2, 3]").unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]))
        );
    }

    #[test]
    fn test_parse_object() {
        let result = parse_json(r#"{"a": 1}"#).unwrap();
        assert_eq!(
            result,
            Value::Map(Rc::new(vec![(
                Value::Keyword {
                    ns: None,
                    name: Rc::from("a"),
                },
                Value::Int(1),
            )]))
        );
    }

    #[test]
    fn test_stringify_int() {
        assert_eq!(value_to_json(&Value::Int(42)), "42");
    }

    #[test]
    fn test_stringify_string() {
        assert_eq!(value_to_json(&Value::Str(Rc::from("hi"))), "\"hi\"");
    }

    #[test]
    fn test_stringify_array() {
        let v = Value::Vec(Rc::new(vec![Value::Int(1), Value::Bool(true)]));
        assert_eq!(value_to_json(&v), "[1,true]");
    }

    #[test]
    fn test_roundtrip() {
        let json = r#"{"name":"nexl","version":1,"active":true}"#;
        let parsed = parse_json(json).unwrap();
        let back = value_to_json(&parsed);
        // Re-parse to verify structural equality
        let reparsed = parse_json(&back).unwrap();
        assert_eq!(parsed, reparsed);
    }
}
