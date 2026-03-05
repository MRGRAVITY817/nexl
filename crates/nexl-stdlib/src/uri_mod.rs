//! `uri` module — URI parsing and construction.
//!
//! Backed by the `url` crate. URI values are stored as `(Uri "string")` ADT
//! instances. Percent-encoding helpers operate directly on strings.

use std::rc::Rc;

use nexl_runtime::value::NexlMap;
use nexl_runtime::Value;
use url::Url;

use crate::StdlibEntry;

/// Return all `uri` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("parse", parse as fn(&[Value]) -> Result<Value, String>),
        ("to-str", to_str),
        ("scheme", scheme),
        ("host", host),
        ("port", port),
        ("path", path),
        ("query", query),
        ("query-params", query_params),
        ("fragment", fragment),
        ("encode", encode),
        ("decode", decode),
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
fn some(v: Value) -> Value { adt("Option", "Some", vec![v]) }
fn none() -> Value { adt("Option", "None", vec![]) }

fn str_val(s: impl AsRef<str>) -> Value {
    Value::Str(Rc::from(s.as_ref()))
}

/// Pack a parsed URL as a `(Uri "url-string")` ADT.
fn uri_val(u: &Url) -> Value {
    adt("Uri", "Uri", vec![str_val(u.as_str())])
}

/// Extract the URL string from a `(Uri "...")` ADT and parse it.
fn get_url(v: &Value) -> Result<Url, String> {
    match v {
        Value::Adt { ctor, fields, .. } if ctor.as_ref() == "Uri" => {
            match fields.first() {
                Some(Value::Str(s)) => {
                    Url::parse(s).map_err(|e| e.to_string())
                }
                _ => Err("`uri` functions require a Uri value".to_string()),
            }
        }
        _ => Err(format!("`uri` functions require a Uri value, got {v}")),
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// `(uri/parse str)` → `(Result Uri Str)` — parse a URI string.
fn parse(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => match Url::parse(s) {
            Ok(u) => Ok(ok(uri_val(&u))),
            Err(e) => Ok(err(&e.to_string())),
        },
        _ => Err(format!("`uri/parse` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(uri/to-str uri)` → `Str` — render a URI back to its string form.
fn to_str(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let u = get_url(v)?;
            Ok(str_val(u.as_str()))
        }
        _ => Err(format!("`uri/to-str` requires 1 argument (Uri), got {}", args.len())),
    }
}

/// `(uri/scheme uri)` → `(Some Str)` or `None` — extract the URI scheme.
fn scheme(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let u = get_url(v)?;
            Ok(some(str_val(u.scheme())))
        }
        _ => Err(format!("`uri/scheme` requires 1 argument (Uri), got {}", args.len())),
    }
}

/// `(uri/host uri)` → `(Some Str)` or `None` — extract the host.
fn host(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let u = get_url(v)?;
            match u.host_str() {
                Some(h) => Ok(some(str_val(h))),
                None => Ok(none()),
            }
        }
        _ => Err(format!("`uri/host` requires 1 argument (Uri), got {}", args.len())),
    }
}

/// `(uri/port uri)` → `(Some Int)` or `None` — extract the port.
fn port(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let u = get_url(v)?;
            match u.port() {
                Some(p) => Ok(some(Value::Int(p as i64))),
                None => Ok(none()),
            }
        }
        _ => Err(format!("`uri/port` requires 1 argument (Uri), got {}", args.len())),
    }
}

/// `(uri/path uri)` → `Str` — extract the path component.
fn path(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let u = get_url(v)?;
            Ok(str_val(u.path()))
        }
        _ => Err(format!("`uri/path` requires 1 argument (Uri), got {}", args.len())),
    }
}

/// `(uri/query uri)` → `(Some Str)` or `None` — extract the raw query string.
fn query(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let u = get_url(v)?;
            match u.query() {
                Some(q) => Ok(some(str_val(q))),
                None => Ok(none()),
            }
        }
        _ => Err(format!("`uri/query` requires 1 argument (Uri), got {}", args.len())),
    }
}

/// `(uri/query-params uri)` → `(Map Str Str)` — parse query parameters.
fn query_params(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let u = get_url(v)?;
            let mut map = NexlMap::new();
            for (key, val) in u.query_pairs() {
                map = map.put(str_val(key.as_ref()), str_val(val.as_ref()));
            }
            Ok(Value::Map(Rc::new(map)))
        }
        _ => Err(format!("`uri/query-params` requires 1 argument (Uri), got {}", args.len())),
    }
}

/// `(uri/fragment uri)` → `(Some Str)` or `None` — extract the fragment.
fn fragment(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let u = get_url(v)?;
            match u.fragment() {
                Some(f) => Ok(some(str_val(f))),
                None => Ok(none()),
            }
        }
        _ => Err(format!("`uri/fragment` requires 1 argument (Uri), got {}", args.len())),
    }
}

/// `(uri/encode str)` → `Str` — percent-encode a string for use in a URI.
fn encode(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => {
            let encoded = url::form_urlencoded::byte_serialize(s.as_bytes())
                .collect::<String>();
            Ok(str_val(&encoded))
        }
        _ => Err(format!("`uri/encode` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(uri/decode str)` → `(Result Str Str)` — percent-decode a URI-encoded string.
fn decode(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(s)] => {
            let bytes = percent_decode(s.as_bytes());
            match String::from_utf8(bytes) {
                Ok(text) => Ok(ok(str_val(&text))),
                Err(e) => Ok(err(&e.to_string())),
            }
        }
        _ => Err(format!("`uri/decode` requires 1 argument (Str), got {}", args.len())),
    }
}

/// Decode percent-encoded bytes (`%XX`) in a byte slice.
fn percent_decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' && i + 2 < input.len() {
            if let (Some(hi), Some(lo)) = (
                from_hex(input[i + 1]),
                from_hex(input[i + 2]),
            ) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        } else if input[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(input[i]);
        i += 1;
    }
    out
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Value { Value::Str(Rc::from(text)) }

    fn parse_ok(url: &str) -> Value {
        let result = parse(&[s(url)]).unwrap();
        if let Value::Adt { ref ctor, ref fields, .. } = result {
            assert_eq!(ctor.as_ref(), "Ok");
            fields[0].clone()
        } else {
            panic!("expected Ok");
        }
    }

    #[test]
    fn test_parse_valid() {
        let result = parse(&[s("https://example.com/path?q=1#sec")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Ok"));
    }

    #[test]
    fn test_parse_invalid() {
        let result = parse(&[s("not a uri")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Err"));
    }

    #[test]
    fn test_scheme() {
        let uri = parse_ok("https://example.com");
        let result = scheme(&[uri]).unwrap();
        if let Value::Adt { ctor, fields, .. } = result {
            assert_eq!(ctor.as_ref(), "Some");
            assert_eq!(fields[0], s("https"));
        }
    }

    #[test]
    fn test_host() {
        let uri = parse_ok("https://example.com/foo");
        let result = host(&[uri]).unwrap();
        if let Value::Adt { ctor, fields, .. } = result {
            assert_eq!(ctor.as_ref(), "Some");
            assert_eq!(fields[0], s("example.com"));
        }
    }

    #[test]
    fn test_port_explicit() {
        let uri = parse_ok("https://example.com:8080/");
        let result = port(&[uri]).unwrap();
        if let Value::Adt { ctor, fields, .. } = result {
            assert_eq!(ctor.as_ref(), "Some");
            assert_eq!(fields[0], Value::Int(8080));
        }
    }

    #[test]
    fn test_port_none() {
        let uri = parse_ok("https://example.com/");
        let result = port(&[uri]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "None"));
    }

    #[test]
    fn test_path_fn() {
        let uri = parse_ok("https://example.com/foo/bar");
        let result = path(&[uri]).unwrap();
        assert_eq!(result, s("/foo/bar"));
    }

    #[test]
    fn test_query() {
        let uri = parse_ok("https://example.com/?q=nexl&page=1");
        let result = query(&[uri]).unwrap();
        if let Value::Adt { ctor, .. } = result {
            assert_eq!(ctor.as_ref(), "Some");
        }
    }

    #[test]
    fn test_query_params() {
        let uri = parse_ok("https://example.com/?a=1&b=2");
        let result = query_params(&[uri]).unwrap();
        assert!(matches!(result, Value::Map(_)));
    }

    #[test]
    fn test_fragment() {
        let uri = parse_ok("https://example.com/page#section");
        let result = fragment(&[uri]).unwrap();
        if let Value::Adt { ctor, fields, .. } = result {
            assert_eq!(ctor.as_ref(), "Some");
            assert_eq!(fields[0], s("section"));
        }
    }

    #[test]
    fn test_encode() {
        let result = encode(&[s("hello world")]).unwrap();
        assert!(matches!(result, Value::Str(_)));
        if let Value::Str(s) = result {
            assert!(s.contains('+') || s.contains("%20"));
        }
    }

    #[test]
    fn test_decode() {
        let result = decode(&[s("hello%20world")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Ok"));
        if let Value::Adt { fields, .. } = result {
            assert_eq!(fields[0], s("hello world"));
        }
    }
}
