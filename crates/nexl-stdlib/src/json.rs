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
        ("encode", encode),
        ("decode", decode),
        ("pretty", pretty),
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

/// `(json/encode v)` — M24 primary name for stringify.
fn encode(args: &[Value]) -> Result<Value, String> {
    stringify(args)
}

/// `(json/decode s)` — M24 primary name for parse, returns `(Result Value Str)`.
fn decode(args: &[Value]) -> Result<Value, String> {
    parse(args)
}

/// `(json/pretty v)` — pretty-print a Nexl value as indented JSON.
///
/// Uses 2-space indentation.
fn pretty(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("pretty", args)?;
    Ok(Value::Str(Rc::from(
        value_to_json_pretty(v, 2, 0).as_str(),
    )))
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
                        'b' => result.push('\x08'),
                        'f' => result.push('\x0C'),
                        'u' => {
                            let code = parse_hex4(&mut chars)?;
                            let ch = if (0xD800..=0xDBFF).contains(&code) {
                                // High surrogate — expect \uLOW next
                                let low = parse_surrogate_low(&mut chars)?;
                                let codepoint =
                                    0x10000 + ((code - 0xD800) as u32) * 0x400
                                        + (low - 0xDC00) as u32;
                                char::from_u32(codepoint)
                                    .ok_or_else(|| format!("invalid surrogate pair: {codepoint:#x}"))?
                            } else {
                                char::from_u32(code as u32)
                                    .ok_or_else(|| format!("invalid unicode code point: {code:#x}"))?
                            };
                            result.push(ch);
                        }
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

/// Parse exactly 4 hex digits from the char iterator, returning the u16 value.
fn parse_hex4(chars: &mut std::str::CharIndices<'_>) -> Result<u16, String> {
    let mut code: u32 = 0;
    for _ in 0..4 {
        let (_, h) = chars
            .next()
            .ok_or("unexpected end of unicode escape")?;
        let digit = h
            .to_digit(16)
            .ok_or_else(|| format!("non-hex digit in \\uXXXX escape: '{h}'"))?;
        code = code * 16 + digit;
    }
    Ok(code as u16)
}

/// After consuming a high surrogate, expect `\uDCXX` (low surrogate).
fn parse_surrogate_low(chars: &mut std::str::CharIndices<'_>) -> Result<u16, String> {
    let next = chars.next().map(|(_, c)| c);
    if next != Some('\\') {
        return Err("expected \\uDCxx low surrogate after high surrogate".into());
    }
    let next = chars.next().map(|(_, c)| c);
    if next != Some('u') {
        return Err("expected \\uDCxx low surrogate after high surrogate".into());
    }
    let low = parse_hex4(chars)?;
    if !(0xDC00..=0xDFFF).contains(&low) {
        return Err(format!("expected low surrogate (0xDC00–0xDFFF), got {low:#x}"));
    }
    Ok(low)
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
        return Ok((Value::Map(Rc::new(entries.into())), after));
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
            return Ok((Value::Map(Rc::new(entries.into())), after));
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

/// Recursively pretty-print a value with `indent_size`-space indentation.
/// `depth` is the current nesting depth.
fn value_to_json_pretty(v: &Value, indent_size: usize, depth: usize) -> String {
    let indent = " ".repeat(indent_size * depth);
    let child_indent = " ".repeat(indent_size * (depth + 1));
    match v {
        Value::Map(entries) if !entries.is_empty() => {
            let parts: Vec<String> = entries
                .iter()
                .map(|(k, v)| {
                    let key_str = match k {
                        Value::Keyword { name, .. } => name.to_string(),
                        Value::Str(s) => s.to_string(),
                        other => other.to_string(),
                    };
                    format!(
                        "{child_indent}\"{key_str}\": {}",
                        value_to_json_pretty(v, indent_size, depth + 1)
                    )
                })
                .collect();
            format!("{{\n{}\n{indent}}}", parts.join(",\n"))
        }
        Value::Vec(items) if !items.is_empty() => {
            let parts: Vec<String> = items
                .iter()
                .map(|item| {
                    format!(
                        "{child_indent}{}",
                        value_to_json_pretty(item, indent_size, depth + 1)
                    )
                })
                .collect();
            format!("[\n{}\n{indent}]", parts.join(",\n"))
        }
        other => value_to_json(other),
    }
}

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
        Value::Str(s) => {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            for ch in s.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    '\x08' => out.push_str("\\b"),
                    '\x0C' => out.push_str("\\f"),
                    c if (c as u32) < 0x20 => {
                        out.push_str(&format!("\\u{:04x}", c as u32));
                    }
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
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
            Value::Map(Rc::new(
                vec![(
                    Value::Keyword {
                        ns: None,
                        name: Rc::from("a"),
                    },
                    Value::Int(1),
                )]
                .into()
            ))
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
    fn test_stringify_escapes_newline() {
        // JSON requires \n in strings to be encoded as \\n
        let v = Value::Str(Rc::from("line1\nline2"));
        assert_eq!(value_to_json(&v), r#""line1\nline2""#);
    }

    #[test]
    fn test_stringify_escapes_tab() {
        let v = Value::Str(Rc::from("col1\tcol2"));
        assert_eq!(value_to_json(&v), r#""col1\tcol2""#);
    }

    #[test]
    fn test_stringify_escapes_control() {
        // Control char < 0x20 must be \uXXXX encoded
        let v = Value::Str(Rc::from("\x01"));
        assert_eq!(value_to_json(&v), r#""\u0001""#);
    }

    #[test]
    fn test_stringify_array() {
        let v = Value::Vec(Rc::new(vec![Value::Int(1), Value::Bool(true)]));
        assert_eq!(value_to_json(&v), "[1,true]");
    }

    #[test]
    fn test_parse_unicode_escape_ascii() {
        // JSON §7 — \uXXXX escape sequences: \u0041 = 'A'
        let result = parse_json(r#""\u0041""#).unwrap();
        assert_eq!(result, Value::Str(Rc::from("A")));
    }

    #[test]
    fn test_parse_unicode_escape_bmp() {
        // \u4e2d = '中' (CJK Unified Ideograph)
        let result = parse_json(r#""\u4e2d""#).unwrap();
        assert_eq!(result, Value::Str(Rc::from("中")));
    }

    #[test]
    fn test_parse_unicode_surrogate_pair() {
        // \uD83D\uDE00 = '😀' (emoji encoded as UTF-16 surrogate pair)
        let result = parse_json(r#""\uD83D\uDE00""#).unwrap();
        assert_eq!(result, Value::Str(Rc::from("😀")));
    }

    #[test]
    fn test_encode_alias() {
        // encode is the primary M24 API name for stringify
        let v = Value::Str(Rc::from("hello"));
        let enc = encode(&[v.clone()]).unwrap();
        let sfy = stringify(&[v]).unwrap();
        assert_eq!(enc, sfy);
    }

    #[test]
    fn test_decode_returns_ok() {
        // decode wraps parsed result in Ok(...)
        let result = decode(&[Value::Str(Rc::from("42"))]).unwrap();
        match result {
            Value::Adt { ctor, .. } => assert_eq!(ctor.as_ref(), "Ok"),
            other => panic!("expected Ok Adt, got {other}"),
        }
    }

    #[test]
    fn test_decode_returns_err() {
        // decode wraps parse error in Err(...)
        let result = decode(&[Value::Str(Rc::from("bad json @@@"))]).unwrap();
        match result {
            Value::Adt { ctor, .. } => assert_eq!(ctor.as_ref(), "Err"),
            other => panic!("expected Err Adt, got {other}"),
        }
    }

    #[test]
    fn test_pretty_object() {
        // json/pretty produces indented output (2-space indent)
        let entry = (
            Value::Keyword { ns: None, name: Rc::from("a") },
            Value::Int(1),
        );
        let v = Value::Map(Rc::new(vec![entry].into()));
        let pretty_str = value_to_json_pretty(&v, 2, 0);
        assert!(pretty_str.contains('\n'), "pretty should have newlines");
        assert!(pretty_str.contains("  "), "pretty should have indent");
        assert!(pretty_str.contains(r#""a""#), "pretty should have key");
        assert!(pretty_str.contains("1"), "pretty should have value");
    }

    #[test]
    fn test_pretty_array() {
        let v = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2)]));
        let pretty_str = value_to_json_pretty(&v, 2, 0);
        assert!(pretty_str.contains('\n'));
        assert!(pretty_str.contains("  1"));
    }

    #[test]
    fn test_pretty_empty_containers() {
        // Empty object and array stay compact
        let empty_obj = Value::Map(Rc::new(vec![].into()));
        let empty_arr = Value::Vec(Rc::new(vec![]));
        assert_eq!(value_to_json_pretty(&empty_obj, 2, 0), "{}");
        assert_eq!(value_to_json_pretty(&empty_arr, 2, 0), "[]");
    }

    #[test]
    fn test_pretty_nested() {
        // Nested object gets deeper indentation
        let inner = Value::Map(Rc::new(
            vec![(
                Value::Keyword { ns: None, name: Rc::from("x") },
                Value::Int(1),
            )]
            .into(),
        ));
        let outer = Value::Map(Rc::new(
            vec![(
                Value::Keyword { ns: None, name: Rc::from("inner") },
                inner,
            )]
            .into(),
        ));
        let s = value_to_json_pretty(&outer, 2, 0);
        // Should have at least 4-space indent for the inner key
        assert!(s.contains("    \"x\""), "got: {s}");
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
