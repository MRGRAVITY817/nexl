//! `toml` module — TOML parsing and writing.
//!
//! Backed by the `toml` crate. Nexl values map to TOML as follows:
//! - `Map` → TOML table (keys are stringified)
//! - `Vec` → TOML array
//! - `Str`, `Int`, `Float`, `Bool` → corresponding TOML scalars

use std::rc::Rc;

use nexl_runtime::value::NexlMap;
use nexl_runtime::Value;
use toml::Value as TomlValue;

use crate::StdlibEntry;

/// Return all `toml` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("parse", parse as fn(&[Value]) -> Result<Value, String>),
        ("encode", encode),
        ("pretty", pretty),
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
fn err_val(msg: &str) -> Value { adt("Result", "Err", vec![Value::Str(Rc::from(msg))]) }

fn kw(name: &str) -> Value {
    Value::Keyword { ns: None, name: Rc::from(name) }
}

fn str_val(s: impl AsRef<str>) -> Value {
    Value::Str(Rc::from(s.as_ref()))
}

/// Convert a TOML `Value` to a Nexl `Value`.
fn toml_to_nexl(v: &TomlValue) -> Value {
    match v {
        TomlValue::String(s) => str_val(s),
        TomlValue::Integer(n) => Value::Int(*n),
        TomlValue::Float(f) => Value::Float(*f),
        TomlValue::Boolean(b) => Value::Bool(*b),
        TomlValue::Array(arr) => {
            let items: Vec<Value> = arr.iter().map(toml_to_nexl).collect();
            Value::Vec(Rc::new(items))
        }
        TomlValue::Table(table) => {
            let mut map = NexlMap::new();
            for (k, v) in table {
                map = map.put(kw(k), toml_to_nexl(v));
            }
            Value::Map(Rc::new(map))
        }
        TomlValue::Datetime(dt) => str_val(dt.to_string()),
    }
}

/// Convert a Nexl `Value` to a TOML `Value`.
fn nexl_to_toml(v: &Value) -> Result<TomlValue, String> {
    match v {
        Value::Str(s) => Ok(TomlValue::String(s.to_string())),
        Value::Int(n) => Ok(TomlValue::Integer(*n)),
        Value::Float(f) => Ok(TomlValue::Float(*f)),
        Value::Bool(b) => Ok(TomlValue::Boolean(*b)),
        Value::Vec(items) => {
            let arr: Result<Vec<TomlValue>, String> =
                items.iter().map(nexl_to_toml).collect();
            Ok(TomlValue::Array(arr?))
        }
        Value::Map(m) => {
            let mut table = toml::map::Map::new();
            for (k, v) in m.iter() {
                let key = match k {
                    Value::Keyword { name, .. } => name.to_string(),
                    Value::Str(s) => s.to_string(),
                    other => return Err(format!(
                        "`toml/encode` map keys must be Keyword or Str, got {other}"
                    )),
                };
                table.insert(key, nexl_to_toml(v)?);
            }
            Ok(TomlValue::Table(table))
        }
        other => Err(format!("`toml/encode` cannot convert {other} to TOML")),
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// `(toml/parse str)` → `(Result Any Str)` — parse a TOML string to Nexl values.
fn parse(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => match s.parse::<TomlValue>() {
            Ok(v) => Ok(ok(toml_to_nexl(&v))),
            Err(e) => Ok(err_val(&e.to_string())),
        },
        _ => Err(format!("`toml/parse` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(toml/encode value)` → `(Result Str Str)` — encode Nexl values to TOML.
fn encode(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => match nexl_to_toml(v) {
            Ok(tv) => match toml::to_string(&tv) {
                Ok(s) => Ok(ok(str_val(&s))),
                Err(e) => Ok(err_val(&e.to_string())),
            },
            Err(e) => Ok(err_val(&e)),
        },
        _ => Err(format!("`toml/encode` requires 1 argument, got {}", args.len())),
    }
}

/// `(toml/pretty value)` → `(Result Str Str)` — encode Nexl values to formatted TOML.
fn pretty(args: &[Value]) -> Result<Value, String> {
    // `toml` crate doesn't differentiate compact/pretty for tables,
    // but `toml::to_string_pretty` provides nicer formatting.
    match args {
        [v] => match nexl_to_toml(v) {
            Ok(tv) => match toml::to_string_pretty(&tv) {
                Ok(s) => Ok(ok(str_val(&s))),
                Err(e) => Ok(err_val(&e.to_string())),
            },
            Err(e) => Ok(err_val(&e)),
        },
        _ => Err(format!("`toml/pretty` requires 1 argument, got {}", args.len())),
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
    fn test_parse_basic() {
        let input = "[package]\nname = \"nexl\"\nversion = \"1.0\"\n";
        let result = parse(&[s(input)]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Ok"));
        if let Value::Adt { fields, .. } = result {
            assert!(matches!(fields[0], Value::Map(_)));
        }
    }

    #[test]
    fn test_parse_invalid() {
        let result = parse(&[s("not = valid = toml")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Err"));
    }

    #[test]
    fn test_parse_types() {
        let input = "str = \"hello\"\nnum = 42\nfloat = 3.14\nbool = true\n";
        let result = parse(&[s(input)]).unwrap();
        if let Value::Adt { fields, .. } = result {
            if let Value::Map(m) = &fields[0] {
                assert_eq!(m.get(&kw("str")), Some(&s("hello")));
                assert_eq!(m.get(&kw("num")), Some(&Value::Int(42)));
                assert_eq!(m.get(&kw("bool")), Some(&Value::Bool(true)));
            }
        }
    }

    #[test]
    fn test_encode_map() {
        let mut m = NexlMap::new();
        m = m.put(kw("name"), s("nexl"));
        let result = encode(&[Value::Map(Rc::new(m))]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Ok"));
        if let Value::Adt { fields, .. } = result {
            if let Value::Str(text) = &fields[0] {
                assert!(text.contains("name"));
                assert!(text.contains("nexl"));
            }
        }
    }

    #[test]
    fn test_roundtrip() {
        let input = "name = \"nexl\"\nversion = 1\n";
        let parsed = parse(&[s(input)]).unwrap();
        if let Value::Adt { fields, .. } = parsed {
            let encoded = encode(&[fields[0].clone()]).unwrap();
            if let Value::Adt { fields: ef, .. } = encoded {
                if let Value::Str(text) = &ef[0] {
                    assert!(text.contains("nexl"));
                }
            }
        }
    }
}
