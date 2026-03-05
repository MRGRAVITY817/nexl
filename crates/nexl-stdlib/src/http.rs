//! `http` module — HTTP client with Request/Response record types.
//!
//! Provides a higher-level HTTP interface on top of the `net` module's
//! TCP-based transport:
//!
//! - `(http/get url)` → `(Result Response Str)`
//! - `(http/post url body headers)` → `(Result Response Str)`
//! - `(http/response status body)` → `Response`
//! - `(http/serve handler port)` → stub (Component Model in WASM mode)
//! - `(http/status resp)` → `Int`
//! - `(http/body resp)` → `Str`
//! - `(http/headers resp)` → `Map`
//!
//! Response is a Map record: `{:status Int, :body Str, :headers Map}`.
//! Request is a Map record: `{:method Str, :url Str, :path Str, :headers Map, :body (Option Str)}`.

use std::rc::Rc;

use nexl_runtime::sandbox::{self, Capability};
use nexl_runtime::value::NexlMap;
use nexl_runtime::Value;

use crate::{net, StdlibEntry};

/// Return all `http` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("get", get_fn as fn(&[Value]) -> Result<Value, String>),
        ("post", post_fn),
        ("put", put_fn),
        ("patch", patch_fn),
        ("delete", delete_fn),
        ("head", head_fn),
        ("request", request_fn),
        ("response", response_fn),
        ("serve", serve_fn),
        ("status", status_fn),
        ("body", body_fn),
        ("headers", headers_fn),
        ("header", header_fn),
        ("ok?", ok_pred),
    ]
}

// ─── Response record helpers ─────────────────────────────────────────────────

/// Build a Response Map: `{:status status, :body body, :headers headers}`.
fn make_response(status: i64, body: &str, headers: Value) -> Value {
    Value::Map(Rc::new(
        vec![
            (kw("status"), Value::Int(status)),
            (kw("body"), Value::Str(Rc::from(body))),
            (kw("headers"), headers),
        ]
        .into(),
    ))
}

/// Build a Keyword value with no namespace.
fn kw(name: &str) -> Value {
    Value::Keyword {
        ns: None,
        name: Rc::from(name),
    }
}

/// Wrap a value in `Ok(...)`.
fn ok_val(v: Value) -> Value {
    Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Ok"),
        fields: Rc::new(vec![v]),
    }
}

/// Wrap a string in `Err(...)`.
fn err_val(msg: &str) -> Value {
    Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Err"),
        fields: Rc::new(vec![Value::Str(Rc::from(msg))]),
    }
}

/// Extract the value associated with a keyword key from a Map, if present.
fn map_get<'a>(map: &'a NexlMap, key: &str) -> Option<&'a Value> {
    map.iter().find_map(|(k, v)| match k {
        Value::Keyword { name, .. } if name.as_ref() == key => Some(v),
        _ => None,
    })
}

// ─── Stdlib functions ─────────────────────────────────────────────────────────

/// `(http/get url)` — HTTP GET. Returns `(Result Response Str)`.
fn get_fn(args: &[Value]) -> Result<Value, String> {
    sandbox::check(Capability::Net)?;
    match args {
        [Value::Str(url)] => {
            let empty_headers = Value::Map(Rc::new(vec![].into()));
            match net::http_get(url) {
                Ok(body) => Ok(ok_val(make_response(200, &body, empty_headers))),
                Err(e) => Ok(err_val(&e)),
            }
        }
        [other] => Err(format!(
            "`http/get` expected Str url, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`http/get` requires 1 argument (url), got {}",
            args.len()
        )),
    }
}

/// `(http/post url body headers)` — HTTP POST. Returns `(Result Response Str)`.
///
/// `headers` should be a Map of String→String pairs (ignored in HTTP/1.0 transport).
fn post_fn(args: &[Value]) -> Result<Value, String> {
    sandbox::check(Capability::Net)?;
    match args {
        [Value::Str(url), Value::Str(body), Value::Map(_headers)] => {
            let empty_headers = Value::Map(Rc::new(vec![].into()));
            match net::http_post(url, body) {
                Ok(resp_body) => Ok(ok_val(make_response(200, &resp_body, empty_headers))),
                Err(e) => Ok(err_val(&e)),
            }
        }
        _ if args.len() != 3 => Err(format!(
            "`http/post` requires 3 arguments (url body headers), got {}",
            args.len()
        )),
        _ => Err(format!(
            "`http/post` expected (Str Str Map), got ({}, {}, {})",
            args[0].type_name(),
            args[1].type_name(),
            args[2].type_name()
        )),
    }
}

/// `(http/put url body headers)` — HTTP PUT. Returns `(Result Response Str)`.
fn put_fn(args: &[Value]) -> Result<Value, String> {
    sandbox::check(Capability::Net)?;
    match args {
        [Value::Str(url), Value::Str(req_body), Value::Map(_headers)] => {
            let empty_headers = Value::Map(Rc::new(vec![].into()));
            match net::http_request("PUT", url, req_body) {
                Ok(resp) => Ok(ok_val(make_response(200, &resp, empty_headers))),
                Err(e) => Ok(err_val(&e)),
            }
        }
        _ if args.len() != 3 => Err(format!(
            "`http/put` requires 3 arguments (url body headers), got {}",
            args.len()
        )),
        _ => Err("`http/put` expected (Str Str Map)".to_string()),
    }
}

/// `(http/patch url body headers)` — HTTP PATCH. Returns `(Result Response Str)`.
fn patch_fn(args: &[Value]) -> Result<Value, String> {
    sandbox::check(Capability::Net)?;
    match args {
        [Value::Str(url), Value::Str(req_body), Value::Map(_headers)] => {
            let empty_headers = Value::Map(Rc::new(vec![].into()));
            match net::http_request("PATCH", url, req_body) {
                Ok(resp) => Ok(ok_val(make_response(200, &resp, empty_headers))),
                Err(e) => Ok(err_val(&e)),
            }
        }
        _ if args.len() != 3 => Err(format!(
            "`http/patch` requires 3 arguments (url body headers), got {}",
            args.len()
        )),
        _ => Err("`http/patch` expected (Str Str Map)".to_string()),
    }
}

/// `(http/delete url)` — HTTP DELETE. Returns `(Result Response Str)`.
fn delete_fn(args: &[Value]) -> Result<Value, String> {
    sandbox::check(Capability::Net)?;
    match args {
        [Value::Str(url)] => {
            let empty_headers = Value::Map(Rc::new(vec![].into()));
            match net::http_request("DELETE", url, "") {
                Ok(resp) => Ok(ok_val(make_response(200, &resp, empty_headers))),
                Err(e) => Ok(err_val(&e)),
            }
        }
        _ => Err(format!("`http/delete` requires 1 argument (url), got {}", args.len())),
    }
}

/// `(http/head url)` — HTTP HEAD. Returns `(Result Response Str)`.
fn head_fn(args: &[Value]) -> Result<Value, String> {
    sandbox::check(Capability::Net)?;
    match args {
        [Value::Str(url)] => {
            let empty_headers = Value::Map(Rc::new(vec![].into()));
            match net::http_request("HEAD", url, "") {
                Ok(_resp) => Ok(ok_val(make_response(200, "", empty_headers))),
                Err(e) => Ok(err_val(&e)),
            }
        }
        _ => Err(format!("`http/head` requires 1 argument (url), got {}", args.len())),
    }
}

/// `(http/request req)` — generic HTTP request.
///
/// `req` is a Map: `{:method Str :url Str :headers Map :body Str}`.
/// Returns `(Result Response Str)`.
fn request_fn(args: &[Value]) -> Result<Value, String> {
    sandbox::check(Capability::Net)?;
    match args {
        [Value::Map(m)] => {
            let method = match map_get(m, "method") {
                Some(Value::Str(s)) => s.to_string(),
                _ => return Err("`http/request` requires :method Str in request map".to_string()),
            };
            let url_str = match map_get(m, "url") {
                Some(Value::Str(s)) => s.to_string(),
                _ => return Err("`http/request` requires :url Str in request map".to_string()),
            };
            let body_str = match map_get(m, "body") {
                Some(Value::Str(s)) => s.to_string(),
                None => String::new(),
                _ => return Err("`http/request` :body must be Str".to_string()),
            };
            let empty_headers = Value::Map(Rc::new(vec![].into()));
            match net::http_request(&method, &url_str, &body_str) {
                Ok(resp) => Ok(ok_val(make_response(200, &resp, empty_headers))),
                Err(e) => Ok(err_val(&e)),
            }
        }
        _ => Err(format!("`http/request` requires 1 argument (Map), got {}", args.len())),
    }
}

/// `(http/header resp name)` → `(Option Str)` — get a single header from a response.
fn header_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Map(m), Value::Str(name)] => {
            match map_get(m, "headers") {
                Some(Value::Map(hdrs)) => {
                    let kw_key = Value::Keyword { ns: None, name: Rc::clone(name) };
                    let str_key = Value::Str(Rc::clone(name));
                    let v = hdrs.get(&kw_key).or_else(|| hdrs.get(&str_key));
                    match v {
                        Some(val) => Ok(Value::Adt {
                            type_name: Rc::from("Option"),
                            ctor: Rc::from("Some"),
                            fields: Rc::new(vec![val.clone()]),
                        }),
                        None => Ok(Value::Adt {
                            type_name: Rc::from("Option"),
                            ctor: Rc::from("None"),
                            fields: Rc::new(vec![]),
                        }),
                    }
                }
                _ => Ok(Value::Adt {
                    type_name: Rc::from("Option"),
                    ctor: Rc::from("None"),
                    fields: Rc::new(vec![]),
                }),
            }
        }
        _ => Err(format!("`http/header` requires 2 arguments (Response Str), got {}", args.len())),
    }
}

/// `(http/ok? resp)` → `Bool` — true if status is 200-299.
fn ok_pred(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Map(m)] => {
            match map_get(m, "status") {
                Some(Value::Int(status)) => Ok(Value::Bool(*status >= 200 && *status <= 299)),
                _ => Ok(Value::Bool(false)),
            }
        }
        _ => Err(format!("`http/ok?` requires 1 argument (Response), got {}", args.len())),
    }
}

/// `(http/response status body)` — build a Response record.
fn response_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(status), Value::Str(body)] => {
            let empty_headers = Value::Map(Rc::new(vec![].into()));
            Ok(make_response(*status, body, empty_headers))
        }
        [s, _] if !matches!(s, Value::Int(_)) => Err(format!(
            "`http/response` expected Int status, got {}",
            s.type_name()
        )),
        [_, b] if !matches!(b, Value::Str(_)) => Err(format!(
            "`http/response` expected Str body, got {}",
            b.type_name()
        )),
        _ => Err(format!(
            "`http/response` requires 2 arguments (status body), got {}",
            args.len()
        )),
    }
}

/// `(http/serve handler port)` — stub: requires WASM Component Model.
fn serve_fn(args: &[Value]) -> Result<Value, String> {
    let _ = args;
    Err("`http/serve` is not available in eval mode; use `nexl run --wasm` with wasi:http Component Model".to_string())
}

/// `(http/status resp)` — extract the `:status` field from a Response.
fn status_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Map(m)] => map_get(m, "status")
            .cloned()
            .ok_or_else(|| "`http/status` response has no :status field".to_string()),
        [other] => Err(format!(
            "`http/status` expected Response Map, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`http/status` requires 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(http/body resp)` — extract the `:body` field from a Response.
fn body_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Map(m)] => map_get(m, "body")
            .cloned()
            .ok_or_else(|| "`http/body` response has no :body field".to_string()),
        [other] => Err(format!(
            "`http/body` expected Response Map, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`http/body` requires 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(http/headers resp)` — extract the `:headers` field from a Response.
fn headers_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Map(m)] => map_get(m, "headers")
            .cloned()
            .ok_or_else(|| "`http/headers` response has no :headers field".to_string()),
        [other] => Err(format!(
            "`http/headers` expected Response Map, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`http/headers` requires 1 argument, got {}",
            args.len()
        )),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn resp_200() -> Value {
        make_response(200, "hello", Value::Map(Rc::new(vec![].into())))
    }

    // ── Test 1 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_http_entries_registered() {
        let names: Vec<&str> = entries().iter().map(|(n, _)| *n).collect();
        for name in ["get", "post", "response", "serve", "status", "body", "headers"] {
            assert!(names.contains(&name), "missing entry: {name}");
        }
    }

    // ── Test 2 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_response_constructor() {
        let r = response_fn(&[Value::Int(200), Value::Str(Rc::from("ok"))]).unwrap();
        assert!(matches!(r, Value::Map(_)), "response should be a Map");
    }

    // ── Test 3 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_response_status_key() {
        let r = response_fn(&[Value::Int(404), Value::Str(Rc::from("not found"))]).unwrap();
        let status = status_fn(&[r]).unwrap();
        assert_eq!(status, Value::Int(404));
    }

    // ── Test 4 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_response_body_key() {
        let r = response_fn(&[Value::Int(200), Value::Str(Rc::from("world"))]).unwrap();
        let body = body_fn(&[r]).unwrap();
        assert_eq!(body, Value::Str(Rc::from("world")));
    }

    // ── Test 5 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_response_default_headers() {
        let r = response_fn(&[Value::Int(200), Value::Str(Rc::from(""))]).unwrap();
        let hdrs = headers_fn(&[r]).unwrap();
        assert!(matches!(hdrs, Value::Map(_)), "headers should be a Map");
    }

    // ── Test 6 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_status_extractor() {
        let status = status_fn(&[resp_200()]).unwrap();
        assert_eq!(status, Value::Int(200));
    }

    // ── Test 7 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_body_extractor() {
        let body = body_fn(&[resp_200()]).unwrap();
        assert_eq!(body, Value::Str(Rc::from("hello")));
    }

    // ── Test 8 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_headers_extractor() {
        let hdrs = headers_fn(&[resp_200()]).unwrap();
        assert!(matches!(hdrs, Value::Map(_)));
    }

    // ── Test 9 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_response_wrong_status_type() {
        let err = response_fn(&[Value::Str(Rc::from("200")), Value::Str(Rc::from("ok"))])
            .unwrap_err();
        assert!(err.contains("Int"), "error should mention Int: {err}");
    }

    // ── Test 10 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_response_wrong_arg_count() {
        let err = response_fn(&[]).unwrap_err();
        assert!(err.contains("2"), "error should mention 2 args: {err}");
    }

    // ── Test 11 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_get_bad_arg_type() {
        let err = get_fn(&[Value::Int(42)]).unwrap_err();
        assert!(err.contains("Str") || err.contains("url"), "error: {err}");
    }

    // ── Test 12 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_post_bad_arg_count() {
        let err = post_fn(&[Value::Str(Rc::from("http://example.com"))]).unwrap_err();
        assert!(err.contains("3"), "error should mention 3 args: {err}");
    }

    // ── Test 13 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_serve_stub_error() {
        let err = serve_fn(&[]).unwrap_err();
        assert!(
            err.contains("Component Model") || err.contains("wasi:http"),
            "error should mention Component Model: {err}"
        );
    }

    // ── Test 14 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_status_wrong_arg_type() {
        let err = status_fn(&[Value::Int(42)]).unwrap_err();
        assert!(err.contains("Map"), "error: {err}");
    }

    // ── Test 15 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_get_returns_ok_result_on_success() {
        // In unrestricted sandbox, a get to a bad URL should still return Err Result
        // (wrapped in Ok variant from Result) — tests the Result wrapping shape.
        let result = get_fn(&[Value::Str(Rc::from("http://localhost:1/"))]);
        // Either Ok(Err(...)) for connection refused, or Err for wrong type
        match result {
            Ok(Value::Adt { type_name, .. }) => {
                assert_eq!(type_name.as_ref(), "Result");
            }
            Ok(other) => panic!("expected Result Adt, got {other}"),
            Err(msg) => panic!("unexpected Err: {msg}"),
        }
    }
}
