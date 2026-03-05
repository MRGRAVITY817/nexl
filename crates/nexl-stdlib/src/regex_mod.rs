//! `regex` module — regex operations backed by the `regex` crate.
//!
//! Regex values are stored as `(Regex "pattern")` ADT instances so that
//! compiled patterns can be identified by type. All functions recompile the
//! pattern on each call — acceptable overhead for the Stage 0 evaluator.
//!
//! Match maps have shape `{:start Int :end Int :text Str}`.

use std::rc::Rc;

use nexl_runtime::value::NexlMap;
use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `regex` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("new", new as fn(&[Value]) -> Result<Value, String>),
        ("matches?", matches_pred),
        ("find", find),
        ("find-all", find_all),
        ("replace", replace),
        ("replace-first", replace_first),
        ("split", split),
        ("captures", captures),
        ("escape", escape),
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

fn ok(v: Value) -> Value {
    adt("Result", "Ok", vec![v])
}

fn err(msg: &str) -> Value {
    adt("Result", "Err", vec![Value::Str(Rc::from(msg))])
}

fn some(v: Value) -> Value {
    adt("Option", "Some", vec![v])
}

fn none() -> Value {
    adt("Option", "None", vec![])
}

fn kw(name: &str) -> Value {
    Value::Keyword { ns: None, name: Rc::from(name) }
}

/// Build a `{:start N :end N :text "..."}` map from a regex Match.
fn match_map(start: usize, end: usize, text: &str) -> Value {
    let map = NexlMap::new()
        .put(kw("start"), Value::Int(start as i64))
        .put(kw("end"), Value::Int(end as i64))
        .put(kw("text"), Value::Str(Rc::from(text)));
    Value::Map(Rc::new(map))
}

/// Extract the pattern string from a `(Regex "pattern")` ADT value.
fn get_pattern(rx: &Value) -> Result<String, String> {
    match rx {
        Value::Adt { ctor, fields, .. } if ctor.as_ref() == "Regex" => {
            match fields.first() {
                Some(Value::Str(p)) => Ok(p.to_string()),
                _ => Err("`regex` functions require a Regex value".to_string()),
            }
        }
        _ => Err(format!("`regex` functions require a Regex value, got {rx}")),
    }
}

/// Compile the pattern from a Regex ADT value.
fn compile(rx: &Value) -> Result<regex::Regex, String> {
    let pattern = get_pattern(rx)?;
    regex::Regex::new(&pattern).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// `(regex/new pattern)` → `(Ok (Regex pattern))` or `(Err msg)`.
///
/// Compiles the pattern to validate it; stores the pattern string in the ADT.
fn new(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(pattern)] => {
            match regex::Regex::new(pattern) {
                Ok(_) => {
                    let rx = adt("Regex", "Regex", vec![Value::Str(Rc::clone(pattern))]);
                    Ok(ok(rx))
                }
                Err(e) => Ok(err(&e.to_string())),
            }
        }
        _ => Err(format!("`regex/new` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(regex/matches? rx str)` → `Bool` — true if the string matches the regex.
fn matches_pred(args: &[Value]) -> Result<Value, String> {
    match args {
        [rx, Value::Str(s)] => {
            let re = compile(rx)?;
            Ok(Value::Bool(re.is_match(s)))
        }
        _ => Err(format!("`regex/matches?` requires 2 arguments (Regex Str), got {}", args.len())),
    }
}

/// `(regex/find rx str)` → `(Some {:start N :end N :text "..."})` or `None`.
fn find(args: &[Value]) -> Result<Value, String> {
    match args {
        [rx, Value::Str(s)] => {
            let re = compile(rx)?;
            match re.find(s) {
                Some(m) => Ok(some(match_map(m.start(), m.end(), m.as_str()))),
                None => Ok(none()),
            }
        }
        _ => Err(format!("`regex/find` requires 2 arguments (Regex Str), got {}", args.len())),
    }
}

/// `(regex/find-all rx str)` → `Vec` of match maps.
fn find_all(args: &[Value]) -> Result<Value, String> {
    match args {
        [rx, Value::Str(s)] => {
            let re = compile(rx)?;
            let results: Vec<Value> = re
                .find_iter(s)
                .map(|m| match_map(m.start(), m.end(), m.as_str()))
                .collect();
            Ok(Value::Vec(Rc::new(results)))
        }
        _ => Err(format!("`regex/find-all` requires 2 arguments (Regex Str), got {}", args.len())),
    }
}

/// `(regex/replace rx str replacement)` → `Str` with all matches replaced.
fn replace(args: &[Value]) -> Result<Value, String> {
    match args {
        [rx, Value::Str(s), Value::Str(repl)] => {
            let re = compile(rx)?;
            Ok(Value::Str(Rc::from(re.replace_all(s, repl.as_ref()).as_ref())))
        }
        _ => Err(format!("`regex/replace` requires 3 arguments (Regex Str Str), got {}", args.len())),
    }
}

/// `(regex/replace-first rx str replacement)` → `Str` with first match replaced.
fn replace_first(args: &[Value]) -> Result<Value, String> {
    match args {
        [rx, Value::Str(s), Value::Str(repl)] => {
            let re = compile(rx)?;
            Ok(Value::Str(Rc::from(re.replace(s, repl.as_ref()).as_ref())))
        }
        _ => Err(format!(
            "`regex/replace-first` requires 3 arguments (Regex Str Str), got {}",
            args.len()
        )),
    }
}

/// `(regex/split rx str)` → `Vec` of strings between matches.
fn split(args: &[Value]) -> Result<Value, String> {
    match args {
        [rx, Value::Str(s)] => {
            let re = compile(rx)?;
            let parts: Vec<Value> = re.split(s).map(|p| Value::Str(Rc::from(p))).collect();
            Ok(Value::Vec(Rc::new(parts)))
        }
        _ => Err(format!("`regex/split` requires 2 arguments (Regex Str), got {}", args.len())),
    }
}

/// `(regex/captures rx str)` → `(Some [full-match g1 g2 ...])` or `None`.
///
/// Each element in the vec is `(Some "text")` for a matched group or `None`
/// for an unmatched optional group.
fn captures(args: &[Value]) -> Result<Value, String> {
    match args {
        [rx, Value::Str(s)] => {
            let re = compile(rx)?;
            match re.captures(s) {
                None => Ok(none()),
                Some(caps) => {
                    let groups: Vec<Value> = caps
                        .iter()
                        .map(|m| match m {
                            Some(g) => some(Value::Str(Rc::from(g.as_str()))),
                            None => none(),
                        })
                        .collect();
                    Ok(some(Value::Vec(Rc::new(groups))))
                }
            }
        }
        _ => Err(format!("`regex/captures` requires 2 arguments (Regex Str), got {}", args.len())),
    }
}

/// `(regex/escape str)` → `Str` with all regex metacharacters escaped.
fn escape(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => Ok(Value::Str(Rc::from(regex::escape(s).as_str()))),
        _ => Err(format!("`regex/escape` requires 1 argument (Str), got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn rx(pattern: &str) -> Value {
        adt("Regex", "Regex", vec![Value::Str(Rc::from(pattern))])
    }

    #[test]
    fn test_new_valid() {
        let result = new(&[Value::Str(Rc::from(r"\d+"))]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Ok"));
    }

    #[test]
    fn test_new_invalid() {
        let result = new(&[Value::Str(Rc::from("[invalid"))]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Err"));
    }

    #[test]
    fn test_matches_true() {
        assert_eq!(
            matches_pred(&[rx(r"\d+"), Value::Str(Rc::from("abc123"))]).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn test_matches_false() {
        assert_eq!(
            matches_pred(&[rx(r"^\d+$"), Value::Str(Rc::from("abc"))]).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn test_find_some() {
        let result = find(&[rx(r"\d+"), Value::Str(Rc::from("abc123def"))]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Some"));
    }

    #[test]
    fn test_find_none() {
        let result = find(&[rx(r"\d+"), Value::Str(Rc::from("abcdef"))]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "None"));
    }

    #[test]
    fn test_find_all_count() {
        let result = find_all(&[rx(r"\d+"), Value::Str(Rc::from("1 22 333"))]).unwrap();
        assert_eq!(result, Value::Vec(Rc::new(vec![
            match_map(0, 1, "1"),
            match_map(2, 4, "22"),
            match_map(5, 8, "333"),
        ])));
    }

    #[test]
    fn test_replace() {
        let result = replace(&[
            rx(r"\d+"),
            Value::Str(Rc::from("a1b2c3")),
            Value::Str(Rc::from("N")),
        ]).unwrap();
        assert_eq!(result, Value::Str(Rc::from("aNbNcN")));
    }

    #[test]
    fn test_replace_first() {
        let result = replace_first(&[
            rx(r"\d+"),
            Value::Str(Rc::from("a1b2c3")),
            Value::Str(Rc::from("N")),
        ]).unwrap();
        assert_eq!(result, Value::Str(Rc::from("aNb2c3")));
    }

    #[test]
    fn test_split() {
        let result = split(&[rx(","), Value::Str(Rc::from("a,b,c"))]).unwrap();
        assert_eq!(result, Value::Vec(Rc::new(vec![
            Value::Str(Rc::from("a")),
            Value::Str(Rc::from("b")),
            Value::Str(Rc::from("c")),
        ])));
    }

    #[test]
    fn test_escape() {
        let result = escape(&[Value::Str(Rc::from("1+1=2"))]).unwrap();
        assert_eq!(result, Value::Str(Rc::from(r"1\+1=2")));
    }

    #[test]
    fn test_captures() {
        let result = captures(&[rx(r"(\d+)-(\d+)"), Value::Str(Rc::from("2024-01"))]).unwrap();
        // Some([Some("2024-01"), Some("2024"), Some("01")])
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Some"));
    }
}
