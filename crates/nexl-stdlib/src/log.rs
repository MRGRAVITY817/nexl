//! `log` module — structured JSON logging.
//!
//! Each log call emits a single-line JSON object to stderr:
//! ```json
//! {"level":"INFO","msg":"hello","request_id":"abc"}
//! ```
//!
//! Functions:
//! - `(log/debug msg)` — log at DEBUG level
//! - `(log/info msg)` — log at INFO level
//! - `(log/warn msg)` — log at WARN level
//! - `(log/error msg)` — log at ERROR level
//! - `(log/with ctx body)` — run body with ctx Map fields merged into all log calls
//! - `(log/set-level level)` — set minimum log level (filters lower-priority logs)
//!
//! Context fields accumulate on a thread-local stack, pushed on `log/with` and
//! popped after body returns. Levels: DEBUG=0, INFO=1, WARN=2, ERROR=3.

use std::cell::RefCell;
use std::collections::HashMap;

use nexl_runtime::Value;

use crate::StdlibEntry;

// ─── Thread-local state ───────────────────────────────────────────────────────

/// Log level values (higher = more important).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum Level {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "debug" => Some(Level::Debug),
            "info" => Some(Level::Info),
            "warn" => Some(Level::Warn),
            "error" => Some(Level::Error),
            _ => None,
        }
    }
}

thread_local! {
    /// Minimum log level. Defaults to DEBUG (allow all).
    static MIN_LEVEL: RefCell<Level> = const { RefCell::new(Level::Debug) };

    /// Context stack: each entry is a flat key→value map of extra log fields.
    static CONTEXT_STACK: RefCell<Vec<HashMap<String, String>>> = const { RefCell::new(Vec::new()) };
}

fn current_min_level() -> Level {
    MIN_LEVEL.with(|l| *l.borrow())
}

fn set_min_level(level: Level) {
    MIN_LEVEL.with(|l| *l.borrow_mut() = level);
}

/// Collect all current context fields as a flat key→value map.
fn collect_context() -> HashMap<String, String> {
    CONTEXT_STACK.with(|stack| {
        let mut merged = HashMap::new();
        for frame in stack.borrow().iter() {
            for (k, v) in frame {
                merged.insert(k.clone(), v.clone());
            }
        }
        merged
    })
}

fn push_context(ctx: HashMap<String, String>) {
    CONTEXT_STACK.with(|stack| stack.borrow_mut().push(ctx));
}

fn pop_context() {
    CONTEXT_STACK.with(|stack| {
        stack.borrow_mut().pop();
    });
}

// ─── Formatting ───────────────────────────────────────────────────────────────

/// Escape a string for inclusion in a JSON value (no surrounding quotes).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Format a single log line as a compact JSON object.
pub(crate) fn format_log_line(level: &str, msg: &str, context: &HashMap<String, String>) -> String {
    let mut line = String::from("{");
    line.push_str(&format!("\"level\":\"{}\"", json_escape(level)));
    line.push_str(&format!(",\"msg\":\"{}\"", json_escape(msg)));
    // Sort context keys for deterministic output in tests.
    let mut keys: Vec<&String> = context.keys().collect();
    keys.sort();
    for k in keys {
        let v = &context[k];
        line.push_str(&format!(",\"{}\":\"{}\"", json_escape(k), json_escape(v)));
    }
    line.push('}');
    line
}

// ─── Core log function ────────────────────────────────────────────────────────

fn log_at_level(level: Level, args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Console)?;
    if args.is_empty() {
        return Err(format!("`log/{}` requires at least 1 argument", level.as_str()));
    }
    if level < current_min_level() {
        // Filtered out — return Unit without writing.
        return Ok(Value::Unit);
    }
    let msg = match &args[0] {
        Value::Str(s) => s.to_string(),
        other => other.to_string(),
    };
    let ctx = collect_context();
    let line = format_log_line(level.as_str(), &msg, &ctx);
    eprintln!("{line}");
    Ok(Value::Unit)
}

// ─── Stdlib entries ───────────────────────────────────────────────────────────

/// Return all `log` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("debug", debug as fn(&[Value]) -> Result<Value, String>),
        ("info", info),
        ("warn", warn),
        ("error", error),
        ("with", with_fn),
        ("set-level", set_level_fn),
    ]
}

/// `(log/debug msg)` — log at DEBUG level.
fn debug(args: &[Value]) -> Result<Value, String> {
    log_at_level(Level::Debug, args)
}

/// `(log/info msg)` — log at INFO level.
fn info(args: &[Value]) -> Result<Value, String> {
    log_at_level(Level::Info, args)
}

/// `(log/warn msg)` — log at WARN level.
fn warn(args: &[Value]) -> Result<Value, String> {
    log_at_level(Level::Warn, args)
}

/// `(log/error msg)` — log at ERROR level.
fn error(args: &[Value]) -> Result<Value, String> {
    log_at_level(Level::Error, args)
}

/// `(log/with ctx body)` — run body with ctx fields merged into all log calls.
///
/// `ctx` is a Map of keyword/string keys to string values.
/// `body` is a zero-argument callable; the return value is propagated.
fn with_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Map(ctx_map), body] => {
            let mut ctx: HashMap<String, String> = HashMap::new();
            for (k, v) in ctx_map.iter() {
                let key = match k {
                    Value::Keyword { name, .. } => name.to_string(),
                    Value::Str(s) => s.to_string(),
                    other => other.to_string(),
                };
                let val = match v {
                    Value::Str(s) => s.to_string(),
                    other => other.to_string(),
                };
                ctx.insert(key, val);
            }
            push_context(ctx);
            let result = nexl_runtime::call_value(body, &[]);
            pop_context();
            result.map_err(|e| format!("log/with body failed: {e}"))
        }
        _ if args.len() != 2 => Err(format!(
            "`log/with` requires 2 arguments (ctx body), got {}",
            args.len()
        )),
        _ => Err(format!(
            "`log/with` expected (Map Fn), got ({}, {})",
            args[0].type_name(),
            args[1].type_name()
        )),
    }
}

/// `(log/set-level level)` — set the minimum log level ("debug", "info", "warn", "error").
fn set_level_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(level_str)] => {
            let level = Level::from_str(level_str)
                .ok_or_else(|| format!("unknown log level `{level_str}`; use debug|info|warn|error"))?;
            set_min_level(level);
            Ok(Value::Unit)
        }
        [Value::Keyword { name, .. }] => {
            let level = Level::from_str(name)
                .ok_or_else(|| format!("unknown log level `{name}`; use debug|info|warn|error"))?;
            set_min_level(level);
            Ok(Value::Unit)
        }
        [other] => Err(format!(
            "`log/set-level` expected Str or Keyword level, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`log/set-level` requires 1 argument, got {}",
            args.len()
        )),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use super::*;

    // ── Existing test (backward compat) ──────────────────────────────────────

    #[test]
    fn test_log_returns_unit() {
        // Reset level to DEBUG to ensure all levels are active.
        set_min_level(Level::Debug);
        assert_eq!(debug(&[Value::Str(Rc::from("test"))]).unwrap(), Value::Unit);
        assert_eq!(info(&[Value::Str(Rc::from("test"))]).unwrap(), Value::Unit);
        assert_eq!(warn(&[Value::Str(Rc::from("test"))]).unwrap(), Value::Unit);
        assert_eq!(error(&[Value::Str(Rc::from("test"))]).unwrap(), Value::Unit);
    }

    // ── Test 2 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_log_entries_registered() {
        let names: Vec<&str> = entries().iter().map(|(n, _)| *n).collect();
        for name in ["debug", "info", "warn", "error", "with", "set-level"] {
            assert!(names.contains(&name), "missing entry: {name}");
        }
    }

    // ── Test 3 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_log_format_is_valid_json() {
        let ctx = HashMap::new();
        let line = format_log_line("info", "hello", &ctx);
        assert!(line.starts_with('{'), "should be JSON object: {line}");
        assert!(line.ends_with('}'), "should end with }}: {line}");
    }

    // ── Test 4 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_log_format_has_level_field() {
        let ctx = HashMap::new();
        let line = format_log_line("warn", "test", &ctx);
        assert!(line.contains(r#""level":"warn""#), "should have level field: {line}");
    }

    // ── Test 5 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_log_format_has_msg_field() {
        let ctx = HashMap::new();
        let line = format_log_line("info", "my message", &ctx);
        assert!(line.contains(r#""msg":"my message""#), "should have msg field: {line}");
    }

    // ── Test 6 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_log_format_includes_context() {
        let mut ctx = HashMap::new();
        ctx.insert("request_id".to_string(), "abc123".to_string());
        let line = format_log_line("info", "ok", &ctx);
        assert!(line.contains(r#""request_id":"abc123""#), "should include context: {line}");
    }

    // ── Test 7 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_log_set_level_filters_debug() {
        set_min_level(Level::Warn);
        // DEBUG should be filtered (returns Unit silently, does not error).
        let result = debug(&[Value::Str(Rc::from("filtered"))]).unwrap();
        assert_eq!(result, Value::Unit, "filtered log should return Unit");
        // Reset.
        set_min_level(Level::Debug);
    }

    // ── Test 8 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_log_set_level_accepts_keyword() {
        let result = set_level_fn(&[Value::Keyword {
            ns: None,
            name: Rc::from("error"),
        }])
        .unwrap();
        assert_eq!(result, Value::Unit);
        set_min_level(Level::Debug); // reset
    }

    // ── Test 9 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_log_set_level_unknown_rejects() {
        let err = set_level_fn(&[Value::Str(Rc::from("verbose"))]).unwrap_err();
        assert!(err.contains("verbose") || err.contains("unknown"), "{err}");
    }

    // ── Test 10 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_log_with_runs_body() {
        let ctx_map = Value::Map(Rc::new(vec![].into()));
        let thunk = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "noop",
            f: |_| Ok(Value::Int(42)),
        }));
        let result = with_fn(&[ctx_map, thunk]).unwrap();
        assert_eq!(result, Value::Int(42), "log/with should return body's value");
    }

    // ── Test 11 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_log_with_context_visible_to_format() {
        // Push a context frame and verify it appears in collect_context().
        let mut ctx = HashMap::new();
        ctx.insert("trace_id".to_string(), "xyz".to_string());
        push_context(ctx);
        let collected = collect_context();
        pop_context();
        assert_eq!(collected.get("trace_id").map(String::as_str), Some("xyz"));
    }

    // ── Test 12 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_json_escape_special_chars() {
        assert_eq!(json_escape("say \"hi\""), r#"say \"hi\""#);
        assert_eq!(json_escape("line\nnext"), r#"line\nnext"#);
    }
}
