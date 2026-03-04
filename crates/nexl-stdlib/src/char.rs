//! `char` module — character classification and conversion.
//!
//! Provides: `alpha?`, `digit?`, `alphanumeric?`, `whitespace?`, `upper?`, `lower?`,
//! `ascii?`, `control?`, `punctuation?`, `to-upper`, `to-lower`, `to-int`, `from-int`, `to-str`.

use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `char` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("alpha?", alpha_pred as fn(&[Value]) -> Result<Value, String>),
        ("digit?", digit_pred),
        ("alphanumeric?", alphanumeric_pred),
        ("whitespace?", whitespace_pred),
        ("upper?", upper_pred),
        ("lower?", lower_pred),
        ("ascii?", ascii_pred),
        ("control?", control_pred),
        ("punctuation?", punctuation_pred),
        ("to-upper", to_upper),
        ("to-lower", to_lower),
        ("to-int", to_int),
        ("from-int", from_int),
        ("to-str", to_str),
    ]
}

fn require_char(name: &str, args: &[Value]) -> Result<char, String> {
    match args {
        [Value::Char(c)] => Ok(*c),
        [other] => Err(format!(
            "`char/{name}` requires a Char argument, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`char/{name}` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(char/alpha? c)` — true if `c` is an alphabetic character.
fn alpha_pred(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(require_char("alpha?", args)?.is_alphabetic()))
}

/// `(char/digit? c)` — true if `c` is a decimal digit (0–9).
fn digit_pred(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(require_char("digit?", args)?.is_ascii_digit()))
}

/// `(char/alphanumeric? c)` — true if `c` is alphabetic or a digit.
fn alphanumeric_pred(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(require_char("alphanumeric?", args)?.is_alphanumeric()))
}

/// `(char/whitespace? c)` — true if `c` is a whitespace character.
fn whitespace_pred(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(require_char("whitespace?", args)?.is_whitespace()))
}

/// `(char/upper? c)` — true if `c` is an uppercase letter.
fn upper_pred(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(require_char("upper?", args)?.is_uppercase()))
}

/// `(char/lower? c)` — true if `c` is a lowercase letter.
fn lower_pred(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(require_char("lower?", args)?.is_lowercase()))
}

/// `(char/ascii? c)` — true if `c` is in the ASCII range (0–127).
fn ascii_pred(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(require_char("ascii?", args)?.is_ascii()))
}

/// `(char/control? c)` — true if `c` is a control character.
fn control_pred(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(require_char("control?", args)?.is_control()))
}

/// `(char/punctuation? c)` — true if `c` is ASCII punctuation.
fn punctuation_pred(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Bool(require_char("punctuation?", args)?.is_ascii_punctuation()))
}

/// `(char/to-upper c)` — convert `c` to its uppercase equivalent.
fn to_upper(args: &[Value]) -> Result<Value, String> {
    let c = require_char("to-upper", args)?;
    // to_uppercase returns an iterator; take the first char (for ASCII, always one).
    let upper: char = c.to_uppercase().next().unwrap_or(c);
    Ok(Value::Char(upper))
}

/// `(char/to-lower c)` — convert `c` to its lowercase equivalent.
fn to_lower(args: &[Value]) -> Result<Value, String> {
    let c = require_char("to-lower", args)?;
    let lower: char = c.to_lowercase().next().unwrap_or(c);
    Ok(Value::Char(lower))
}

/// `(char/to-int c)` — return the Unicode codepoint of `c` as an Int.
fn to_int(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Int(require_char("to-int", args)? as i64))
}

/// `(char/from-int n)` — return `(Some Char)` for a valid codepoint `n`, or `None`.
fn from_int(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(n)] => {
            if *n < 0 || *n > u32::MAX as i64 {
                return Ok(Value::Adt {
                    type_name: Rc::from("Option"),
                    ctor: Rc::from("None"),
                    fields: Rc::new(vec![]),
                });
            }
            match char::from_u32(*n as u32) {
                Some(c) => Ok(Value::Adt {
                    type_name: Rc::from("Option"),
                    ctor: Rc::from("Some"),
                    fields: Rc::new(vec![Value::Char(c)]),
                }),
                None => Ok(Value::Adt {
                    type_name: Rc::from("Option"),
                    ctor: Rc::from("None"),
                    fields: Rc::new(vec![]),
                }),
            }
        }
        [other] => Err(format!(
            "`char/from-int` requires an Int argument, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`char/from-int` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(char/to-str c)` — convert `c` to a single-character Str.
fn to_str(args: &[Value]) -> Result<Value, String> {
    let c = require_char("to-str", args)?;
    Ok(Value::Str(Rc::from(c.to_string().as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alpha_pred() {
        assert_eq!(alpha_pred(&[Value::Char('a')]).unwrap(), Value::Bool(true));
        assert_eq!(alpha_pred(&[Value::Char('1')]).unwrap(), Value::Bool(false));
    }

    #[test]
    fn test_digit_pred() {
        assert_eq!(digit_pred(&[Value::Char('5')]).unwrap(), Value::Bool(true));
        assert_eq!(digit_pred(&[Value::Char('a')]).unwrap(), Value::Bool(false));
    }

    #[test]
    fn test_upper_lower() {
        assert_eq!(upper_pred(&[Value::Char('A')]).unwrap(), Value::Bool(true));
        assert_eq!(lower_pred(&[Value::Char('a')]).unwrap(), Value::Bool(true));
        assert_eq!(upper_pred(&[Value::Char('a')]).unwrap(), Value::Bool(false));
    }

    #[test]
    fn test_to_upper_lower() {
        assert_eq!(to_upper(&[Value::Char('a')]).unwrap(), Value::Char('A'));
        assert_eq!(to_lower(&[Value::Char('A')]).unwrap(), Value::Char('a'));
    }

    #[test]
    fn test_to_int() {
        assert_eq!(to_int(&[Value::Char('A')]).unwrap(), Value::Int(65));
    }

    #[test]
    fn test_from_int_valid() {
        let result = from_int(&[Value::Int(65)]).unwrap();
        assert_eq!(
            result,
            Value::Adt {
                type_name: Rc::from("Option"),
                ctor: Rc::from("Some"),
                fields: Rc::new(vec![Value::Char('A')]),
            }
        );
    }

    #[test]
    fn test_from_int_invalid() {
        let result = from_int(&[Value::Int(0xD800)]).unwrap(); // surrogate — invalid
        assert!(matches!(result, Value::Adt { ctor, .. } if ctor.as_ref() == "None"));
    }

    #[test]
    fn test_to_str() {
        assert_eq!(
            to_str(&[Value::Char('Z')]).unwrap(),
            Value::Str(Rc::from("Z"))
        );
    }

    #[test]
    fn test_whitespace() {
        assert_eq!(
            whitespace_pred(&[Value::Char(' ')]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            whitespace_pred(&[Value::Char('a')]).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn test_ascii() {
        assert_eq!(ascii_pred(&[Value::Char('~')]).unwrap(), Value::Bool(true));
        assert_eq!(ascii_pred(&[Value::Char('é')]).unwrap(), Value::Bool(false));
    }
}
