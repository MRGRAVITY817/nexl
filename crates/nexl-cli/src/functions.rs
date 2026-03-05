//! `nexl functions` — effect-sandboxed HTTP function host.
//!
//! Functions are `.nx` files that export a `handle` function:
//! ```nexl
//! (defn handle [req] {:status 200 :headers {} :body "Hello!"})
//! ```
//!
//! Registry is stored at `.nexl-functions/registry.json` in the CWD.
//! Invocation logs are appended to `.nexl-functions/logs/<name>.jsonl`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ─── Registry ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionEntry {
    pub name: String,
    pub file: PathBuf,
    pub capability: String,
    pub route: String,
    pub deployed_at: String,
}

fn registry_path() -> PathBuf {
    PathBuf::from(".nexl-functions/registry.json")
}

fn logs_dir() -> PathBuf {
    PathBuf::from(".nexl-functions/logs")
}

fn ensure_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(".nexl-functions/logs")
}

pub fn load_registry() -> Vec<FunctionEntry> {
    let path = registry_path();
    if !path.exists() {
        return vec![];
    }
    let data = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&data).unwrap_or_default()
}

pub fn save_registry(entries: &[FunctionEntry]) {
    let _ = ensure_dirs();
    let json = serde_json::to_string_pretty(entries).expect("serialize registry");
    let _ = std::fs::write(registry_path(), json);
}

fn iso_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    // Simple ISO 8601: YYYY-MM-DDTHH:MM:SSZ
    let s = secs;
    let sec = s % 60;
    let min = (s / 60) % 60;
    let hour = (s / 3600) % 24;
    let days = s / 86400;
    // Approximate date from Unix epoch (not accurate for DST/leap seconds but fine for logs)
    let year = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

// ─── CLI commands ─────────────────────────────────────────────────────────────

pub fn cmd_deploy(
    file: &Path,
    name: Option<&str>,
    capability: &str,
    route: Option<&str>,
) -> Result<(), String> {
    let abs_file = file
        .canonicalize()
        .map_err(|e| format!("cannot resolve {}: {e}", file.display()))?;

    let name = name
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            abs_file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("handler")
                .to_string()
        });

    if !["pure", "read-only", "full"].contains(&capability) {
        return Err(format!(
            "invalid capability `{capability}`; expected pure, read-only, or full"
        ));
    }

    let route = route
        .map(|r| r.to_string())
        .unwrap_or_else(|| format!("/{name}"));

    let mut registry = load_registry();
    // Replace if same name already exists.
    registry.retain(|e| e.name != name);
    registry.push(FunctionEntry {
        name: name.clone(),
        file: abs_file,
        capability: capability.to_string(),
        route: route.clone(),
        deployed_at: iso_now(),
    });
    save_registry(&registry);

    println!("✓ Deployed `{name}` → {route}  [{capability}]");
    Ok(())
}

pub fn cmd_list() {
    let registry = load_registry();
    if registry.is_empty() {
        println!("No functions deployed. Use `nexl functions deploy <file.nx>` to register one.");
        return;
    }
    println!("{:<20} {:<12} {:<30} {}", "NAME", "CAPABILITY", "ROUTE", "FILE");
    println!("{}", "-".repeat(80));
    for entry in &registry {
        println!(
            "{:<20} {:<12} {:<30} {}",
            entry.name,
            entry.capability,
            entry.route,
            entry.file.display()
        );
    }
}

pub fn cmd_logs(name: &str, n: usize) {
    let log_file = logs_dir().join(format!("{name}.jsonl"));
    if !log_file.exists() {
        println!("No logs for `{name}`.");
        return;
    }
    let data = std::fs::read_to_string(&log_file).unwrap_or_default();
    let lines: Vec<&str> = data.lines().collect();
    let start = if lines.len() > n { lines.len() - n } else { 0 };
    println!("Last {} invocations of `{name}`:", lines.len().min(n));
    for line in &lines[start..] {
        println!("{line}");
    }
}

pub fn cmd_invoke(
    name: &str,
    method: &str,
    path: Option<&str>,
    body: Option<&str>,
) -> Result<(), String> {
    let registry = load_registry();
    let entry = registry
        .iter()
        .find(|e| e.name == name)
        .ok_or_else(|| format!("function `{name}` not found; run `nexl functions list`"))?;

    let req_path = path.unwrap_or(&entry.route).to_string();
    let req_body = body.unwrap_or("").to_string();

    let result = invoke_function(entry, method, &req_path, "", &HashMap::new(), &req_body)?;
    println!("Status: {}", result.0);
    println!("Body: {}", result.2);
    Ok(())
}

// ─── HTTP server ──────────────────────────────────────────────────────────────

pub fn cmd_serve(port: u16) -> Result<(), String> {
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr)
        .map_err(|e| format!("cannot bind to {addr}: {e}"))?;

    println!("nexl functions server listening on http://127.0.0.1:{port}");
    println!("Press Ctrl+C to stop.");
    list_routes();

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(e) = handle_connection(stream) {
                    eprintln!("connection error: {e}");
                }
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
    Ok(())
}

fn list_routes() {
    let registry = load_registry();
    if registry.is_empty() {
        println!("  (no functions deployed)");
    } else {
        for entry in &registry {
            println!("  {} -> {} [{}]", entry.route, entry.name, entry.capability);
        }
    }
}

// ─── HTTP parsing ─────────────────────────────────────────────────────────────

struct HttpRequest {
    method: String,
    path: String,
    query: String,
    headers: HashMap<String, String>,
    body: String,
}

fn parse_request(stream: &TcpStream) -> Option<HttpRequest> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let request_line = request_line.trim();

    // Parse: METHOD SP PATH HTTP/1.1
    let mut parts = request_line.splitn(3, ' ');
    let method = parts.next()?.to_string();
    let path_query = parts.next()?.to_string();

    let (path, query) = if let Some(idx) = path_query.find('?') {
        (path_query[..idx].to_string(), path_query[idx + 1..].to_string())
    } else {
        (path_query, String::new())
    };

    // Parse headers
    let mut headers: HashMap<String, String> = HashMap::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end_matches("\r\n").trim_end_matches('\n');
        if line.is_empty() {
            break;
        }
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_lowercase();
            let value = line[colon + 1..].trim().to_string();
            if key == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            headers.insert(key, value);
        }
    }

    // Read body
    let body = if content_length > 0 {
        let mut buf = vec![0u8; content_length.min(1024 * 1024)];
        reader.read_exact(&mut buf).ok()?;
        String::from_utf8_lossy(&buf).into_owned()
    } else {
        String::new()
    };

    Some(HttpRequest { method, path, query, headers, body })
}

fn write_response(
    mut stream: TcpStream,
    status: u16,
    headers: &HashMap<String, String>,
    body: &str,
) {
    let reason = http_reason(status);
    let mut response = format!("HTTP/1.1 {status} {reason}\r\n");
    response.push_str(&format!("Content-Length: {}\r\n", body.len()));
    response.push_str("Connection: close\r\n");
    for (k, v) in headers {
        response.push_str(&format!("{k}: {v}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(body);
    let _ = stream.write_all(response.as_bytes());
}

fn http_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

fn handle_connection(stream: TcpStream) -> Result<(), String> {
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    let stream_clone = stream.try_clone().map_err(|e| e.to_string())?;

    let Some(req) = parse_request(&stream) else {
        write_response(stream, 400, &HashMap::new(), "Bad Request");
        return Ok(());
    };

    eprintln!("{} {}", req.method, req.path);

    // Dashboard at GET /
    if req.path == "/" && req.method == "GET" {
        let body = render_dashboard();
        let mut h = HashMap::new();
        h.insert("Content-Type".into(), "text/html; charset=utf-8".into());
        write_response(stream, 200, &h, &body);
        return Ok(());
    }

    // Route to function
    let registry = load_registry();
    let entry = registry.iter().find(|e| route_matches(&e.route, &req.path));

    match entry {
        Some(entry) => {
            let start = Instant::now();
            match invoke_function(entry, &req.method, &req.path, &req.query, &req.headers, &req.body) {
                Ok((status, resp_headers, body)) => {
                    let elapsed = start.elapsed().as_millis();
                    log_invocation(&entry.name, &req.method, &req.path, status, elapsed);
                    eprintln!("  → {status} ({elapsed}ms)  [{}]", entry.name);
                    let mut h: HashMap<String, String> = HashMap::new();
                    for (k, v) in &resp_headers {
                        h.insert(k.clone(), v.clone());
                    }
                    write_response(stream, status, &h, &body);
                }
                Err(e) => {
                    eprintln!("  → 500 eval error: {e}");
                    log_invocation(&entry.name, &req.method, &req.path, 500, 0);
                    write_response(stream, 500, &HashMap::new(), &format!("eval error: {e}"));
                }
            }
        }
        None => {
            let body = format!("Not Found: {}\n\nDeployed routes:\n{}", req.path, route_list());
            write_response(stream, 404, &HashMap::new(), &body);
        }
    }

    drop(peer);
    drop(stream_clone);
    Ok(())
}

fn route_matches(pattern: &str, path: &str) -> bool {
    // Simple prefix matching; `:param` segments match any single path segment.
    let p_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let r_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if p_parts.len() != r_parts.len() {
        return false;
    }
    p_parts.iter().zip(r_parts.iter()).all(|(p, r)| {
        p.starts_with(':') || *p == *r
    })
}

fn route_list() -> String {
    let registry = load_registry();
    registry.iter()
        .map(|e| format!("  {} [{}]", e.route, e.capability))
        .collect::<Vec<_>>()
        .join("\n")
}

// ─── Function invocation ──────────────────────────────────────────────────────

/// Invoke a Nexl function, returning `(status, headers, body)`.
pub fn invoke_function(
    entry: &FunctionEntry,
    method: &str,
    path: &str,
    query: &str,
    req_headers: &HashMap<String, String>,
    body: &str,
) -> Result<(u16, HashMap<String, String>, String), String> {
    let source = std::fs::read_to_string(&entry.file)
        .map_err(|e| format!("cannot read {}: {e}", entry.file.display()))?;

    // Apply sandbox capability policy for this function's declared level.
    let policy = capability_set(&entry.capability);
    nexl_runtime::sandbox::set_policy(policy);
    let env = nexl_eval::stdlib::standard_env();

    // Parse + macro-expand the source.
    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC)
        .map_err(|_| format!("parse error in {}", entry.file.display()))?;

    let (expanded, prelude_forms) =
        crate::macro_expand(&nodes).map_err(|e| format!("macro expand failed: {e}"))?;

    for node in &prelude_forms {
        let _ = nexl_eval::eval::eval(node, &env);
    }
    for node in &expanded {
        nexl_eval::eval::eval(node, &env)
            .map_err(|e| format!("eval error in {}: {e}", entry.file.display()))?;
    }

    // Look up `handle` in the env.
    let handle_fn = env
        .get("handle")
        .ok_or_else(|| format!("`handle` is not defined in {}", entry.file.display()))?;

    // Build the request map and call.
    let req_value = build_request_value(method, path, query, req_headers, body);
    let result = nexl_runtime::call_value(&handle_fn, &[req_value])
        .map_err(|e| format!("`handle` call failed: {e}"))?;

    // Parse the response map.
    parse_response_value(result)
}

fn capability_set(cap: &str) -> nexl_runtime::sandbox::SandboxPolicy {
    use nexl_runtime::sandbox::{Capability, SandboxPolicy};
    match cap {
        "pure" => SandboxPolicy::sandbox(std::collections::HashSet::new()),
        "read-only" => {
            let mut set = std::collections::HashSet::new();
            set.insert(Capability::Console);
            set.insert(Capability::FileSystem);
            SandboxPolicy::sandbox(set)
        }
        _ => SandboxPolicy::unrestricted(),
    }
}

fn build_request_value(
    method: &str,
    path: &str,
    query: &str,
    headers: &HashMap<String, String>,
    body: &str,
) -> nexl_runtime::Value {
    use nexl_runtime::value::NexlMap;
    use nexl_runtime::Value;
    let kw = |name: &str| Value::Keyword { ns: None, name: Rc::from(name) };

    // Build headers map
    let mut hdrs = NexlMap::new();
    for (k, v) in headers {
        hdrs = hdrs.put(kw(k), Value::Str(Rc::from(v.as_str())));
    }

    let mut m = NexlMap::new();
    m = m.put(kw("method"), Value::Str(Rc::from(method)));
    m = m.put(kw("path"), Value::Str(Rc::from(path)));
    m = m.put(kw("query"), Value::Str(Rc::from(query)));
    m = m.put(kw("headers"), Value::Map(Rc::new(hdrs)));
    m = m.put(kw("body"), Value::Str(Rc::from(body)));
    Value::Map(Rc::new(m))
}

fn parse_response_value(
    val: nexl_runtime::Value,
) -> Result<(u16, HashMap<String, String>, String), String> {
    use nexl_runtime::Value;
    match val {
        Value::Map(m) => {
            let kw = |name: &str| Value::Keyword { ns: None, name: Rc::from(name) };

            let status = match m.get(&kw("status")) {
                Some(Value::Int(n)) => *n as u16,
                _ => 200,
            };

            let body = match m.get(&kw("body")) {
                Some(Value::Str(s)) => s.to_string(),
                Some(other) => other.to_string(),
                None => String::new(),
            };

            let mut headers = HashMap::new();
            if let Some(Value::Map(h)) = m.get(&kw("headers")) {
                for (k, v) in h.iter() {
                    let key = match k {
                        Value::Keyword { name, .. } => name.to_string(),
                        Value::Str(s) => s.to_string(),
                        _ => continue,
                    };
                    let val = match v {
                        Value::Str(s) => s.to_string(),
                        _ => v.to_string(),
                    };
                    headers.insert(key, val);
                }
            }

            Ok((status, headers, body))
        }
        Value::Str(s) => Ok((200, HashMap::new(), s.to_string())),
        Value::Int(n) => Ok((n as u16, HashMap::new(), String::new())),
        other => Err(format!(
            "`handle` must return a Map response, got {}",
            other.type_name()
        )),
    }
}

// ─── Logging ──────────────────────────────────────────────────────────────────

fn log_invocation(name: &str, method: &str, path: &str, status: u16, duration_ms: u128) {
    let _ = ensure_dirs();
    let log_file = logs_dir().join(format!("{name}.jsonl"));
    let entry = format!(
        "{{\"ts\":\"{}\",\"method\":\"{method}\",\"path\":\"{path}\",\"status\":{status},\"duration_ms\":{duration_ms}}}\n",
        iso_now()
    );
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .and_then(|mut f| f.write_all(entry.as_bytes()));
}

// ─── Dashboard ────────────────────────────────────────────────────────────────

fn render_dashboard() -> String {
    let registry = load_registry();
    let mut html = String::from(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">
<title>nexl functions</title>
<style>
body{font-family:monospace;background:#0f0f0f;color:#e0e0e0;margin:40px}
h1{color:#7ec8e3}
table{border-collapse:collapse;width:100%}
th,td{padding:8px 12px;text-align:left;border-bottom:1px solid #333}
th{color:#7ec8e3}
.pure{color:#a8d8a8}.read-only{color:#f0e68c}.full{color:#e07070}
a{color:#7ec8e3}
</style></head><body>
<h1>nexl functions</h1>
"#,
    );

    if registry.is_empty() {
        html.push_str("<p>No functions deployed. Use <code>nexl functions deploy &lt;file.nx&gt;</code> to get started.</p>");
    } else {
        html.push_str("<table><tr><th>Name</th><th>Route</th><th>Capability</th><th>Deployed</th></tr>");
        for entry in &registry {
            let cap_class = entry.capability.replace(' ', "-");
            html.push_str(&format!(
                "<tr><td>{}</td><td><a href=\"{}\">{}</a></td><td class=\"{cap_class}\">{}</td><td>{}</td></tr>",
                entry.name,
                entry.route,
                entry.route,
                entry.capability,
                entry.deployed_at,
            ));
        }
        html.push_str("</table>");
    }

    html.push_str("</body></html>");
    html
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_matches_exact() {
        assert!(route_matches("/api/hello", "/api/hello"));
        assert!(!route_matches("/api/hello", "/api/world"));
    }

    #[test]
    fn test_route_matches_param() {
        assert!(route_matches("/users/:id", "/users/42"));
        assert!(!route_matches("/users/:id", "/users/42/profile"));
    }

    #[test]
    fn test_route_matches_root() {
        assert!(route_matches("/hello", "/hello"));
        assert!(!route_matches("/hello", "/world"));
    }

    #[test]
    fn test_iso_now_format() {
        let ts = iso_now();
        assert_eq!(ts.len(), 20, "should be YYYY-MM-DDTHH:MM:SSZ: {ts}");
        assert!(ts.ends_with('Z'), "should end with Z: {ts}");
    }

    #[test]
    fn test_parse_response_map() {
        use nexl_runtime::value::NexlMap;
        use nexl_runtime::Value;
        let kw = |name: &str| Value::Keyword { ns: None, name: Rc::from(name) };
        let mut m = NexlMap::new();
        m = m.put(kw("status"), Value::Int(201));
        m = m.put(kw("body"), Value::Str(Rc::from("created")));
        m = m.put(kw("headers"), Value::Map(Rc::new(NexlMap::new())));
        let (status, _, body) = parse_response_value(Value::Map(Rc::new(m))).unwrap();
        assert_eq!(status, 201);
        assert_eq!(body, "created");
    }

    #[test]
    fn test_parse_response_str() {
        use nexl_runtime::Value;
        let (status, _, body) =
            parse_response_value(Value::Str(Rc::from("hello"))).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, "hello");
    }

    #[test]
    fn test_capability_set_pure() {
        let policy = capability_set("pure");
        use nexl_runtime::sandbox::Capability;
        assert!(policy.check(Capability::Net).is_err());
        assert!(policy.check(Capability::FileSystem).is_err());
    }

    #[test]
    fn test_capability_set_full() {
        let policy = capability_set("full");
        use nexl_runtime::sandbox::Capability;
        assert!(policy.check(Capability::Net).is_ok());
        assert!(policy.check(Capability::FileSystem).is_ok());
    }
}
