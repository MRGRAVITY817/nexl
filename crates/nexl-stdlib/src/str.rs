//! `str` module — string manipulation functions.
//!
//! Provides: `split`, `join`, `trim`, `trim-start`, `trim-end`, `upper`, `lower`,
//! `starts-with?`, `ends-with?`, `contains?`, `replace`, `index-of`, `format`,
//! `blank?`, `chars`, `graphemes`.

use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `str` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("split", split as fn(&[Value]) -> Result<Value, String>),
        ("join", join),
        ("trim", trim),
        ("trim-start", trim_start),
        ("trim-end", trim_end),
        ("upper", upper),
        ("lower", lower),
        ("starts-with?", starts_with),
        ("ends-with?", ends_with),
        ("contains?", contains),
        ("replace", replace),
        ("index-of", index_of),
        ("blank?", blank),
        ("chars", chars),
        ("graphemes", graphemes),
        ("format", str_format),
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn expect_str<'a>(op: &str, v: &'a Value) -> Result<&'a Rc<str>, String> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(format!("`{op}` expected Str, got {}", other.type_name())),
    }
}

fn one_arg<'a>(op: &str, args: &'a [Value]) -> Result<&'a Value, String> {
    match args {
        [a] => Ok(a),
        _ => Err(format!(
            "`{op}` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

fn two_args<'a>(op: &str, args: &'a [Value]) -> Result<(&'a Value, &'a Value), String> {
    match args {
        [a, b] => Ok((a, b)),
        _ => Err(format!(
            "`{op}` requires exactly 2 arguments, got {}",
            args.len()
        )),
    }
}

fn three_args<'a>(
    op: &str,
    args: &'a [Value],
) -> Result<(&'a Value, &'a Value, &'a Value), String> {
    match args {
        [a, b, c] => Ok((a, b, c)),
        _ => Err(format!(
            "`{op}` requires exactly 3 arguments, got {}",
            args.len()
        )),
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// `(str/split s sep)` — split string by separator, return Vec of Str.
fn split(args: &[Value]) -> Result<Value, String> {
    let (s, sep) = two_args("str/split", args)?;
    let s = expect_str("str/split", s)?;
    let sep = expect_str("str/split", sep)?;
    let parts: Vec<Value> = s
        .split(sep.as_ref())
        .map(|part| Value::Str(Rc::from(part)))
        .collect();
    Ok(Value::Vec(Rc::new(parts)))
}

/// `(str/join sep parts)` — join a Vec of Str with separator.
fn join(args: &[Value]) -> Result<Value, String> {
    let (sep, coll) = two_args("str/join", args)?;
    let sep = expect_str("str/join", sep)?;
    let Value::Vec(items) = coll else {
        return Err(format!(
            "`str/join` second argument must be Vec, got {}",
            coll.type_name()
        ));
    };
    let mut parts = Vec::with_capacity(items.len());
    for item in items.iter() {
        let s = expect_str("str/join", item)?;
        parts.push(s.as_ref().to_string());
    }
    Ok(Value::Str(Rc::from(parts.join(sep.as_ref()).as_str())))
}

/// `(str/trim s)` — remove leading and trailing whitespace.
fn trim(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("str/trim", args)?;
    let s = expect_str("str/trim", v)?;
    Ok(Value::Str(Rc::from(s.trim())))
}

/// `(str/trim-start s)` — remove leading whitespace.
fn trim_start(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("str/trim-start", args)?;
    let s = expect_str("str/trim-start", v)?;
    Ok(Value::Str(Rc::from(s.trim_start())))
}

/// `(str/trim-end s)` — remove trailing whitespace.
fn trim_end(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("str/trim-end", args)?;
    let s = expect_str("str/trim-end", v)?;
    Ok(Value::Str(Rc::from(s.trim_end())))
}

/// `(str/upper s)` — convert to uppercase.
fn upper(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("str/upper", args)?;
    let s = expect_str("str/upper", v)?;
    Ok(Value::Str(Rc::from(s.to_uppercase().as_str())))
}

/// `(str/lower s)` — convert to lowercase.
fn lower(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("str/lower", args)?;
    let s = expect_str("str/lower", v)?;
    Ok(Value::Str(Rc::from(s.to_lowercase().as_str())))
}

/// `(str/starts-with? s prefix)` — check if string starts with prefix.
fn starts_with(args: &[Value]) -> Result<Value, String> {
    let (s, prefix) = two_args("str/starts-with?", args)?;
    let s = expect_str("str/starts-with?", s)?;
    let prefix = expect_str("str/starts-with?", prefix)?;
    Ok(Value::Bool(s.starts_with(prefix.as_ref())))
}

/// `(str/ends-with? s suffix)` — check if string ends with suffix.
fn ends_with(args: &[Value]) -> Result<Value, String> {
    let (s, suffix) = two_args("str/ends-with?", args)?;
    let s = expect_str("str/ends-with?", s)?;
    let suffix = expect_str("str/ends-with?", suffix)?;
    Ok(Value::Bool(s.ends_with(suffix.as_ref())))
}

/// `(str/contains? s substr)` — check if string contains substring.
fn contains(args: &[Value]) -> Result<Value, String> {
    let (s, substr) = two_args("str/contains?", args)?;
    let s = expect_str("str/contains?", s)?;
    let substr = expect_str("str/contains?", substr)?;
    Ok(Value::Bool(s.contains(substr.as_ref())))
}

/// `(str/replace s from to)` — replace all occurrences of `from` with `to`.
fn replace(args: &[Value]) -> Result<Value, String> {
    let (s, from, to) = three_args("str/replace", args)?;
    let s = expect_str("str/replace", s)?;
    let from = expect_str("str/replace", from)?;
    let to = expect_str("str/replace", to)?;
    Ok(Value::Str(Rc::from(
        s.replace(from.as_ref(), to.as_ref()).as_str(),
    )))
}

/// `(str/index-of s substr)` — return (Some Int) of first occurrence, or None.
fn index_of(args: &[Value]) -> Result<Value, String> {
    let (s, substr) = two_args("str/index-of", args)?;
    let s = expect_str("str/index-of", s)?;
    let substr = expect_str("str/index-of", substr)?;
    match s.find(substr.as_ref()) {
        Some(idx) => Ok(Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("Some"),
            fields: Rc::new(vec![Value::Int(idx as i64)]),
        }),
        None => Ok(Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("None"),
            fields: Rc::new(vec![]),
        }),
    }
}

/// `(str/blank? s)` — true if empty or only whitespace.
fn blank(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("str/blank?", args)?;
    let s = expect_str("str/blank?", v)?;
    Ok(Value::Bool(s.trim().is_empty()))
}

/// `(str/chars s)` — return Vec of Char (Unicode scalar values).
fn chars(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("str/chars", args)?;
    let s = expect_str("str/chars", v)?;
    let char_values: Vec<Value> = s.chars().map(Value::Char).collect();
    Ok(Value::Vec(Rc::new(char_values)))
}

/// `(str/graphemes s)` — return Vec of Str (grapheme clusters).
///
/// Uses Unicode scalar values as an approximation (full grapheme segmentation
/// requires the `unicode-segmentation` crate; deferred for Stage 0).
fn graphemes(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("str/graphemes", args)?;
    let s = expect_str("str/graphemes", v)?;
    // Approximate: each char as a separate string.
    // Full grapheme support would require unicode-segmentation.
    let grapheme_values: Vec<Value> = s
        .chars()
        .map(|c| Value::Str(Rc::from(c.to_string().as_str())))
        .collect();
    Ok(Value::Vec(Rc::new(grapheme_values)))
}

/// `(str/format template args...)` — positional `{}` placeholder formatting.
fn str_format(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("`str/format` requires at least 1 argument (the template)".into());
    }
    let template = expect_str("str/format", &args[0])?;
    let mut result = String::new();
    let mut arg_idx = 1;
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' && chars.peek() == Some(&'}') {
            chars.next(); // consume '}'
            if arg_idx >= args.len() {
                return Err(format!(
                    "`str/format` has more placeholders than arguments (expected arg #{})",
                    arg_idx
                ));
            }
            result.push_str(&display_value(&args[arg_idx]));
            arg_idx += 1;
        } else {
            result.push(ch);
        }
    }
    Ok(Value::Str(Rc::from(result.as_str())))
}

/// Format a value for `str/format` display.
fn display_value(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Char(c) => c.to_string(),
        Value::Unit => "()".to_string(),
        other => format!("{other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split() {
        let result = split(&[Value::Str(Rc::from("a,b,c")), Value::Str(Rc::from(","))]).unwrap();
        let expected = Value::Vec(Rc::new(vec![
            Value::Str(Rc::from("a")),
            Value::Str(Rc::from("b")),
            Value::Str(Rc::from("c")),
        ]));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_join() {
        let result = join(&[
            Value::Str(Rc::from(", ")),
            Value::Vec(Rc::new(vec![
                Value::Str(Rc::from("a")),
                Value::Str(Rc::from("b")),
                Value::Str(Rc::from("c")),
            ])),
        ])
        .unwrap();
        assert_eq!(result, Value::Str(Rc::from("a, b, c")));
    }

    #[test]
    fn test_trim() {
        assert_eq!(
            trim(&[Value::Str(Rc::from("  hello  "))]).unwrap(),
            Value::Str(Rc::from("hello"))
        );
    }

    #[test]
    fn test_trim_start() {
        assert_eq!(
            trim_start(&[Value::Str(Rc::from("  hello  "))]).unwrap(),
            Value::Str(Rc::from("hello  "))
        );
    }

    #[test]
    fn test_trim_end() {
        assert_eq!(
            trim_end(&[Value::Str(Rc::from("  hello  "))]).unwrap(),
            Value::Str(Rc::from("  hello"))
        );
    }

    #[test]
    fn test_upper() {
        assert_eq!(
            upper(&[Value::Str(Rc::from("hello"))]).unwrap(),
            Value::Str(Rc::from("HELLO"))
        );
    }

    #[test]
    fn test_lower() {
        assert_eq!(
            lower(&[Value::Str(Rc::from("HELLO"))]).unwrap(),
            Value::Str(Rc::from("hello"))
        );
    }

    #[test]
    fn test_starts_with() {
        assert_eq!(
            starts_with(&[Value::Str(Rc::from("hello")), Value::Str(Rc::from("hel"))]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            starts_with(&[Value::Str(Rc::from("hello")), Value::Str(Rc::from("xyz"))]).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn test_ends_with() {
        assert_eq!(
            ends_with(&[Value::Str(Rc::from("hello")), Value::Str(Rc::from("llo"))]).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn test_contains() {
        assert_eq!(
            contains(&[
                Value::Str(Rc::from("hello world")),
                Value::Str(Rc::from("world"))
            ])
            .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            contains(&[Value::Str(Rc::from("hello")), Value::Str(Rc::from("xyz"))]).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn test_replace() {
        assert_eq!(
            replace(&[
                Value::Str(Rc::from("hello world")),
                Value::Str(Rc::from("world")),
                Value::Str(Rc::from("nexl")),
            ])
            .unwrap(),
            Value::Str(Rc::from("hello nexl"))
        );
    }

    #[test]
    fn test_index_of_found() {
        let result =
            index_of(&[Value::Str(Rc::from("hello")), Value::Str(Rc::from("ell"))]).unwrap();
        assert_eq!(
            result,
            Value::Adt {
                type_name: Rc::from("Option"),
                ctor: Rc::from("Some"),
                fields: Rc::new(vec![Value::Int(1)]),
            }
        );
    }

    #[test]
    fn test_index_of_not_found() {
        let result =
            index_of(&[Value::Str(Rc::from("hello")), Value::Str(Rc::from("xyz"))]).unwrap();
        assert_eq!(
            result,
            Value::Adt {
                type_name: Rc::from("Option"),
                ctor: Rc::from("None"),
                fields: Rc::new(vec![]),
            }
        );
    }

    #[test]
    fn test_blank_true() {
        assert_eq!(
            blank(&[Value::Str(Rc::from(""))]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            blank(&[Value::Str(Rc::from("  "))]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            blank(&[Value::Str(Rc::from(" \t\n "))]).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn test_blank_false() {
        assert_eq!(
            blank(&[Value::Str(Rc::from("a"))]).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn test_chars() {
        let result = chars(&[Value::Str(Rc::from("abc"))]).unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![
                Value::Char('a'),
                Value::Char('b'),
                Value::Char('c'),
            ]))
        );
    }

    #[test]
    fn test_graphemes() {
        let result = graphemes(&[Value::Str(Rc::from("hi"))]).unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![
                Value::Str(Rc::from("h")),
                Value::Str(Rc::from("i")),
            ]))
        );
    }

    #[test]
    fn test_format_basic() {
        let result = str_format(&[
            Value::Str(Rc::from("Hello, {}!")),
            Value::Str(Rc::from("world")),
        ])
        .unwrap();
        assert_eq!(result, Value::Str(Rc::from("Hello, world!")));
    }

    #[test]
    fn test_format_multiple_args() {
        let result = str_format(&[
            Value::Str(Rc::from("{} + {} = {}")),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ])
        .unwrap();
        assert_eq!(result, Value::Str(Rc::from("1 + 2 = 3")));
    }

    #[test]
    fn test_format_no_placeholders() {
        let result = str_format(&[Value::Str(Rc::from("hello"))]).unwrap();
        assert_eq!(result, Value::Str(Rc::from("hello")));
    }

    #[test]
    fn test_format_too_few_args() {
        let result = str_format(&[Value::Str(Rc::from("{} and {}"))]);
        assert!(result.is_err());
    }
}
