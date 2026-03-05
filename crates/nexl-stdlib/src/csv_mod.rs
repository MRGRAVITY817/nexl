//! `csv` module — CSV parsing and writing.
//!
//! Backed by the `csv` crate. Header-aware parsing returns keyword-keyed maps.

use std::rc::Rc;

use nexl_runtime::value::NexlMap;
use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `csv` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("parse", parse as fn(&[Value]) -> Result<Value, String>),
        ("parse-with-headers", parse_with_headers),
        ("encode", encode),
        ("encode-with-headers", encode_with_headers),
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

fn expect_str_ref<'a>(name: &str, v: &'a Value) -> Result<&'a str, String> {
    match v {
        Value::Str(s) => Ok(s.as_ref()),
        other => Err(format!("`csv/{name}` expected Str, got {other}")),
    }
}

fn expect_vec_ref<'a>(name: &str, v: &'a Value) -> Result<&'a [Value], String> {
    match v {
        Value::Vec(rows) => Ok(rows.as_ref()),
        other => Err(format!("`csv/{name}` expected Vec, got {other}")),
    }
}

fn expect_kw_str(name: &str, v: &Value) -> Result<String, String> {
    match v {
        Value::Keyword { name: kname, .. } => Ok(kname.to_string()),
        Value::Str(s) => Ok(s.to_string()),
        other => Err(format!("`csv/{name}` expected Keyword, got {other}")),
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// `(csv/parse str)` → `(Result (Vec (Vec Str)) Str)` — parse CSV to rows of cells.
fn parse(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let s = expect_str_ref("parse", v)?;
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(false)
                .from_reader(s.as_bytes());
            let mut rows: Vec<Value> = Vec::new();
            for result in reader.records() {
                match result {
                    Ok(record) => {
                        let cells: Vec<Value> = record.iter().map(str_val).collect();
                        rows.push(Value::Vec(Rc::new(cells)));
                    }
                    Err(e) => return Ok(err_val(&e.to_string())),
                }
            }
            Ok(ok(Value::Vec(Rc::new(rows))))
        }
        _ => Err(format!("`csv/parse` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(csv/parse-with-headers str)` → `(Result (Vec (Map Keyword Str)) Str)`.
///
/// The first row is treated as the header row; each subsequent row becomes a
/// map from `:header-name` keywords to cell strings.
fn parse_with_headers(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let s = expect_str_ref("parse-with-headers", v)?;
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(true)
                .from_reader(s.as_bytes());
            let headers: Vec<String> = match reader.headers() {
                Ok(h) => h.iter().map(|s| s.to_string()).collect(),
                Err(e) => return Ok(err_val(&e.to_string())),
            };
            let mut rows: Vec<Value> = Vec::new();
            for result in reader.records() {
                match result {
                    Ok(record) => {
                        let mut map = NexlMap::new();
                        for (header, cell) in headers.iter().zip(record.iter()) {
                            map = map.put(kw(header), str_val(cell));
                        }
                        rows.push(Value::Map(Rc::new(map)));
                    }
                    Err(e) => return Ok(err_val(&e.to_string())),
                }
            }
            Ok(ok(Value::Vec(Rc::new(rows))))
        }
        _ => Err(format!("`csv/parse-with-headers` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(csv/encode rows)` → `Str` — encode `(Vec (Vec Str))` to a CSV string.
fn encode(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let rows = expect_vec_ref("encode", v)?;
            let mut writer = csv::WriterBuilder::new().from_writer(vec![]);
            for row_val in rows {
                let cells = expect_vec_ref("encode", row_val)?;
                let strings: Result<Vec<&str>, String> = cells
                    .iter()
                    .map(|c| expect_str_ref("encode", c))
                    .collect();
                writer.write_record(strings?).map_err(|e| e.to_string())?;
            }
            let data = writer.into_inner().map_err(|e| e.to_string())?;
            Ok(str_val(String::from_utf8_lossy(&data)))
        }
        _ => Err(format!("`csv/encode` requires 1 argument (Vec), got {}", args.len())),
    }
}

/// `(csv/encode-with-headers headers rows)` → `Str`.
///
/// `headers` is `(Vec Keyword)`, `rows` is `(Vec (Map Keyword Str))`.
/// The header row is written first, then each row in header order.
fn encode_with_headers(args: &[Value]) -> Result<Value, String> {
    match args {
        [headers_val, rows_val] => {
            let headers_vec = expect_vec_ref("encode-with-headers", headers_val)?;
            let header_names: Result<Vec<String>, String> = headers_vec
                .iter()
                .map(|v| expect_kw_str("encode-with-headers", v))
                .collect();
            let header_names = header_names?;

            let rows = expect_vec_ref("encode-with-headers", rows_val)?;
            let mut writer = csv::WriterBuilder::new().from_writer(vec![]);

            // Write header row
            writer
                .write_record(header_names.iter().map(String::as_str))
                .map_err(|e| e.to_string())?;

            // Write data rows
            for row_val in rows {
                match row_val {
                    Value::Map(m) => {
                        let cells: Result<Vec<&str>, String> = header_names
                            .iter()
                            .map(|h| {
                                let k = kw(h);
                                match m.get(&k) {
                                    Some(Value::Str(s)) => Ok(s.as_ref()),
                                    Some(other) => Err(format!(
                                        "`csv/encode-with-headers` cell must be Str, got {other}"
                                    )),
                                    None => Ok(""),
                                }
                            })
                            .collect();
                        writer.write_record(cells?).map_err(|e| e.to_string())?;
                    }
                    other => {
                        return Err(format!(
                            "`csv/encode-with-headers` rows must be Maps, got {other}"
                        ))
                    }
                }
            }

            let data = writer.into_inner().map_err(|e| e.to_string())?;
            Ok(str_val(String::from_utf8_lossy(&data)))
        }
        _ => Err(format!(
            "`csv/encode-with-headers` requires 2 arguments (Vec Vec), got {}",
            args.len()
        )),
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
        let result = parse(&[s("a,b,c\n1,2,3\n")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Ok"));
        if let Value::Adt { fields, .. } = result {
            if let Value::Vec(rows) = &fields[0] {
                assert_eq!(rows.len(), 2);
            }
        }
    }

    #[test]
    fn test_parse_with_headers() {
        let result = parse_with_headers(&[s("name,age\nAlice,30\nBob,25\n")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Ok"));
        if let Value::Adt { fields, .. } = result {
            if let Value::Vec(rows) = &fields[0] {
                assert_eq!(rows.len(), 2);
                if let Value::Map(m) = &rows[0] {
                    assert_eq!(m.get(&kw("name")), Some(&s("Alice")));
                    assert_eq!(m.get(&kw("age")), Some(&s("30")));
                }
            }
        }
    }

    #[test]
    fn test_encode_basic() {
        let row1 = Value::Vec(Rc::new(vec![s("a"), s("b"), s("c")]));
        let row2 = Value::Vec(Rc::new(vec![s("1"), s("2"), s("3")]));
        let rows = Value::Vec(Rc::new(vec![row1, row2]));
        let result = encode(&[rows]).unwrap();
        assert!(matches!(result, Value::Str(_)));
        if let Value::Str(csv) = result {
            assert!(csv.contains("a,b,c"));
            assert!(csv.contains("1,2,3"));
        }
    }

    #[test]
    fn test_encode_with_headers_roundtrip() {
        // Build a row map
        let mut m = NexlMap::new();
        m = m.put(kw("name"), s("Alice"));
        m = m.put(kw("age"), s("30"));
        let rows = Value::Vec(Rc::new(vec![Value::Map(Rc::new(m))]));
        let headers = Value::Vec(Rc::new(vec![kw("name"), kw("age")]));

        let result = encode_with_headers(&[headers, rows]).unwrap();
        assert!(matches!(result, Value::Str(_)));
        if let Value::Str(csv) = result {
            assert!(csv.contains("name,age") || csv.contains("name") && csv.contains("age"));
            assert!(csv.contains("Alice"));
        }
    }
}
