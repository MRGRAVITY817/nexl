//! `net` module — HTTP client and TCP stub functions.
//!
//! Provides `http/get` and `http/post` via a blocking HTTP/1.0 implementation
//! over `std::net::TcpStream` (plain HTTP only — no TLS).
//!
//! `http/serve` and `tcp/connect` are stubs that return an error in eval mode;
//! full support requires the WASM Component Model (`wasi:http` / `wasi:sockets`).

use std::io::{Read, Write};
use std::net::TcpStream;
use nexl_runtime::sandbox::{self, Capability};
use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `net` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("get", http_get_fn as fn(&[Value]) -> Result<Value, String>),
        ("post", http_post_fn as fn(&[Value]) -> Result<Value, String>),
        ("serve", http_serve_fn as fn(&[Value]) -> Result<Value, String>),
        ("tcp-connect", tcp_connect_fn as fn(&[Value]) -> Result<Value, String>),
    ]
}

// ─── URL parser ──────────────────────────────────────────────────────────────

/// Parse an `http://host[:port]/path` URL.
///
/// Returns `(host, port, path)`.  Only `http://` is supported; `https://` returns an error.
pub(crate) fn parse_http_url(url: &str) -> Result<(String, u16, String), String> {
    let url = url.trim();
    if !url.starts_with("http://") {
        return Err(format!(
            "`net/get` only supports http:// URLs (not https or other schemes); got: {url}"
        ));
    }
    let rest = &url["http://".len()..];

    // Split authority from path.
    let (authority, path) = match rest.find('/') {
        Some(slash) => (&rest[..slash], rest[slash..].to_string()),
        None => (rest, "/".to_string()),
    };

    // Split host and optional port.
    let (host, port) = match authority.rfind(':') {
        Some(colon) => {
            let h = &authority[..colon];
            let p = authority[colon + 1..]
                .parse::<u16>()
                .map_err(|_| format!("invalid port in URL: {url}"))?;
            (h.to_string(), p)
        }
        None => (authority.to_string(), 80u16),
    };

    if host.is_empty() {
        return Err(format!("empty host in URL: {url}"));
    }

    Ok((host, port, path))
}

// ─── Core HTTP helpers ────────────────────────────────────────────────────────

/// Strip HTTP/1.x response headers and return the body.
pub(crate) fn strip_headers(response: &str) -> &str {
    if let Some(pos) = response.find("\r\n\r\n") {
        return &response[pos + 4..];
    }
    if let Some(pos) = response.find("\n\n") {
        return &response[pos + 2..];
    }
    response
}

/// Perform a blocking HTTP/1.0 GET request.
pub(crate) fn http_get(url: &str) -> Result<String, String> {
    let (host, port, path) = parse_http_url(url)?;
    let addr = format!("{host}:{port}");
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| format!("`net/get` connect to {addr} failed: {e}"))?;

    let request = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("`net/get` send failed: {e}"))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| format!("`net/get` receive failed: {e}"))?;

    Ok(strip_headers(&response).to_string())
}

/// Perform a blocking HTTP/1.0 POST request with a text body.
pub(crate) fn http_post(url: &str, body: &str) -> Result<String, String> {
    let (host, port, path) = parse_http_url(url)?;
    let addr = format!("{host}:{port}");
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| format!("`net/post` connect to {addr} failed: {e}"))?;

    let request = format!(
        "POST {path} HTTP/1.0\r\nHost: {host}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("`net/post` send failed: {e}"))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| format!("`net/post` receive failed: {e}"))?;

    Ok(strip_headers(&response).to_string())
}

// ─── Stdlib function wrappers ─────────────────────────────────────────────────

/// `(net/get url)` — HTTP GET request, returns response body as Str.
fn http_get_fn(args: &[Value]) -> Result<Value, String> {
    sandbox::check(Capability::Net)?;
    match args {
        [Value::Str(url)] => {
            let body = http_get(url)?;
            Ok(Value::Str(std::rc::Rc::from(body.as_str())))
        }
        [other] => Err(format!(
            "`net/get` expected Str URL, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`net/get` requires exactly 1 argument (url), got {}",
            args.len()
        )),
    }
}

/// `(net/post url body)` — HTTP POST request, returns response body as Str.
fn http_post_fn(args: &[Value]) -> Result<Value, String> {
    sandbox::check(Capability::Net)?;
    match args {
        [Value::Str(url), Value::Str(body)] => {
            let response = http_post(url, body)?;
            Ok(Value::Str(response.into()))
        }
        [_, _] => Err(format!(
            "`net/post` expected (Str Str), got ({}, {})",
            args[0].type_name(),
            args[1].type_name()
        )),
        _ => Err(format!(
            "`net/post` requires exactly 2 arguments (url body), got {}",
            args.len()
        )),
    }
}

/// `(net/serve handler port)` — stub: HTTP server requires WASM Component Model.
fn http_serve_fn(args: &[Value]) -> Result<Value, String> {
    let _ = args;
    Err("`net/serve` is not available in eval mode; use `nexl run --wasm` with wasi:http Component Model".to_string())
}

/// `(net/tcp-connect host port)` — stub: TCP connect requires WASM Component Model.
fn tcp_connect_fn(args: &[Value]) -> Result<Value, String> {
    let _ = args;
    Err("`net/tcp-connect` is not available in eval mode; use `nexl run --wasm` with wasi:sockets Component Model".to_string())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test 1 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_http_entries_registered() {
        let names: Vec<&str> = entries().iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"get"), "missing get");
        assert!(names.contains(&"post"), "missing post");
        assert!(names.contains(&"serve"), "missing serve");
        assert!(names.contains(&"tcp-connect"), "missing tcp-connect");
    }

    // ── Test 2 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_http_get_no_args() {
        let result = http_get_fn(&[]);
        assert!(
            result.is_err(),
            "`net/get` with no args should error"
        );
        let err = result.unwrap_err();
        assert!(err.contains("net/get"), "error should mention net/get: {err}");
    }

    // ── Test 3 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_http_get_wrong_type() {
        let result = http_get_fn(&[Value::Int(42)]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Str") || err.contains("Int"), "error: {err}");
    }

    // ── Test 4 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_http_get_https_url_rejected() {
        let result = parse_http_url("https://example.com/path");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("https"), "error should mention https: {err}");
    }

    // ── Test 5 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_http_get_empty_host_rejected() {
        let result = parse_http_url("http:///path");
        assert!(result.is_err(), "empty host should be rejected");
    }

    // ── Test 6 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_http_post_no_args() {
        let result = http_post_fn(&[]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("net/post"), "error should mention net/post: {err}");
    }

    // ── Test 7 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_http_post_one_arg() {
        let result = http_post_fn(&[Value::Str(std::rc::Rc::from("http://example.com"))]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("2"), "error should mention 2 arguments: {err}");
    }

    // ── Test 8 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_http_serve_stub_error() {
        let result = http_serve_fn(&[Value::Int(8080)]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("wasi:http") || err.contains("Component Model"),
            "error should mention Component Model: {err}"
        );
    }

    // ── Test 9 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_tcp_connect_stub_error() {
        let result = tcp_connect_fn(&[
            Value::Str(std::rc::Rc::from("localhost")),
            Value::Int(8080),
        ]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("wasi:sockets") || err.contains("Component Model"),
            "error should mention Component Model: {err}"
        );
    }

    // ── Test 10 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_http_url_with_port() {
        let (host, port, path) = parse_http_url("http://localhost:3000/api/v1").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 3000);
        assert_eq!(path, "/api/v1");
    }

    // ── Test 11 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_http_url_default_port() {
        let (host, port, path) = parse_http_url("http://example.com").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
        assert_eq!(path, "/");
    }

    // ── Test 12 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_http_url_with_path() {
        let (host, port, path) = parse_http_url("http://api.example.com/data/123").unwrap();
        assert_eq!(host, "api.example.com");
        assert_eq!(port, 80);
        assert_eq!(path, "/data/123");
    }

    // ── Test 13 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_strip_headers_crlf() {
        let response = "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\nhello world";
        assert_eq!(strip_headers(response), "hello world");
    }

    // ── Test 14 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_strip_headers_lf_only() {
        let response = "HTTP/1.0 200 OK\nContent-Length: 5\n\nhello";
        assert_eq!(strip_headers(response), "hello");
    }

    // ── Test 15 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_strip_headers_no_headers() {
        // No header separator → return entire string.
        let response = "just a plain string";
        assert_eq!(strip_headers(response), "just a plain string");
    }

    // ── Live HTTP integration test (ignored in CI) ───────────────────────────

    #[test]
    #[ignore = "requires network access"]
    fn test_http_get_live() {
        let body = http_get("http://httpbin.org/get").unwrap();
        assert!(body.contains("httpbin"), "expected httpbin response: {body}");
    }
}
