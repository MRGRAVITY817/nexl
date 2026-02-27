//! `log` module — structured logging.
//!
//! Stage 0 logs directly to stderr. Will be refactored to use the `Log` effect.

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `log` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("debug", debug as fn(&[Value]) -> Result<Value, String>),
        ("info", info),
        ("warn", warn),
        ("error", error),
    ]
}

/// `(log/debug msg)` — log at debug level.
fn debug(args: &[Value]) -> Result<Value, String> {
    log_at_level("DEBUG", args)
}

/// `(log/info msg)` — log at info level.
fn info(args: &[Value]) -> Result<Value, String> {
    log_at_level("INFO", args)
}

/// `(log/warn msg)` — log at warn level.
fn warn(args: &[Value]) -> Result<Value, String> {
    log_at_level("WARN", args)
}

/// `(log/error msg)` — log at error level.
fn error(args: &[Value]) -> Result<Value, String> {
    log_at_level("ERROR", args)
}

fn log_at_level(level: &str, args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Console)?;
    if args.is_empty() {
        return Err(format!("`log/{level}` requires at least 1 argument"));
    }
    let msg = match &args[0] {
        Value::Str(s) => s.to_string(),
        other => other.to_string(),
    };
    eprintln!("[{level}] {msg}");
    Ok(Value::Unit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn test_log_returns_unit() {
        assert_eq!(debug(&[Value::Str(Rc::from("test"))]).unwrap(), Value::Unit);
        assert_eq!(info(&[Value::Str(Rc::from("test"))]).unwrap(), Value::Unit);
        assert_eq!(warn(&[Value::Str(Rc::from("test"))]).unwrap(), Value::Unit);
        assert_eq!(error(&[Value::Str(Rc::from("test"))]).unwrap(), Value::Unit);
    }
}
