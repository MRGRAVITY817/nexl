//! `env` module — environment variable and configuration access.
//!
//! Functions:
//! - `(env/get name)` → `(Option Str)` — get env var or `None`
//! - `(env/require name)` → `Str` — get env var, error if missing
//! - `(env/all)` → `Map` — all env vars as keyword-keyed Map
//! - `(env/load-dotenv path)` → `Unit` — load a `.env` file into process environment
//!
//! `.env` format: `KEY=value` lines, `#` comments, blank lines ignored.
//! Quoted values (`KEY="value with spaces"`) have quotes stripped.

use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `env` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("get", get_fn as fn(&[Value]) -> Result<Value, String>),
        ("require", require_fn),
        ("all", all_fn),
        ("load-dotenv", load_dotenv_fn),
    ]
}

/// Build a `Some(v)` Option ADT.
fn some_val(v: Value) -> Value {
    Value::Adt {
        type_name: Rc::from("Option"),
        ctor: Rc::from("Some"),
        fields: Rc::new(vec![v]),
    }
}

/// `None` Option ADT.
fn none_val() -> Value {
    Value::Adt {
        type_name: Rc::from("Option"),
        ctor: Rc::from("None"),
        fields: Rc::new(vec![]),
    }
}

/// Build a Keyword value with no namespace.
fn kw(name: &str) -> Value {
    Value::Keyword {
        ns: None,
        name: Rc::from(name),
    }
}

// ─── Stdlib functions ─────────────────────────────────────────────────────────

/// `(env/get name)` — look up an environment variable. Returns `(Option Str)`.
fn get_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(name)] => match std::env::var(name.as_ref()) {
            Ok(val) => Ok(some_val(Value::Str(Rc::from(val.as_str())))),
            Err(_) => Ok(none_val()),
        },
        [other] => Err(format!(
            "`env/get` expected Str name, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`env/get` requires 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(env/require name)` — get an env var, returning an error if missing.
fn require_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(name)] => std::env::var(name.as_ref())
            .map(|val| Value::Str(Rc::from(val.as_str())))
            .map_err(|_| {
                format!(
                    "required environment variable `{name}` is not set"
                )
            }),
        [other] => Err(format!(
            "`env/require` expected Str name, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`env/require` requires 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(env/all)` — return all environment variables as a Map of keyword → Str pairs.
fn all_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!(
            "`env/all` takes no arguments, got {}",
            args.len()
        ));
    }
    let entries: Vec<(Value, Value)> = std::env::vars()
        .map(|(k, v)| (kw(&k), Value::Str(Rc::from(v.as_str()))))
        .collect();
    Ok(Value::Map(Rc::new(entries.into())))
}

/// `(env/load-dotenv path)` — parse a `.env` file and set variables in the process environment.
///
/// Format: `KEY=value` lines. `#` starts a comment. Quoted values have quotes stripped.
fn load_dotenv_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Str(path)] => {
            let content = std::fs::read_to_string(path.as_ref())
                .map_err(|e| format!("`env/load-dotenv` failed to read `{path}`: {e}"))?;
            parse_and_set_dotenv(&content);
            Ok(Value::Unit)
        }
        [other] => Err(format!(
            "`env/load-dotenv` expected Str path, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`env/load-dotenv` requires 1 argument, got {}",
            args.len()
        )),
    }
}

/// Parse `.env` content and set variables in the current process environment.
fn parse_and_set_dotenv(content: &str) {
    for line in content.lines() {
        let line = line.trim();
        // Skip blank lines and comments.
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim();
            let val = val.trim();
            // Strip surrounding quotes if present.
            let val = if (val.starts_with('"') && val.ends_with('"'))
                || (val.starts_with('\'') && val.ends_with('\''))
            {
                &val[1..val.len() - 1]
            } else {
                val
            };
            if !key.is_empty() {
                // SAFETY: single-threaded evaluator; caller owns the env.
                unsafe { std::env::set_var(key, val) };
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize env-mutating tests to avoid races.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    // ── Test 1 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_env_entries_registered() {
        let names: Vec<&str> = entries().iter().map(|(n, _)| *n).collect();
        for name in ["get", "require", "all", "load-dotenv"] {
            assert!(names.contains(&name), "missing entry: {name}");
        }
    }

    // ── Test 2 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_env_get_missing() {
        let result = get_fn(&[Value::Str(Rc::from("NEXL_STDLIB_TEST_MISSING_VAR_XYZ"))]).unwrap();
        match result {
            Value::Adt { ctor, .. } => assert_eq!(ctor.as_ref(), "None"),
            other => panic!("expected None, got {other}"),
        }
    }

    // ── Test 3 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_env_get_present() {
        let _lock = ENV_LOCK.lock().unwrap();
        // SAFETY: single-threaded test; ENV_LOCK ensures no concurrent mutation.
        unsafe { std::env::set_var("NEXL_STDLIB_TEST_VAR", "hello") };
        let result = get_fn(&[Value::Str(Rc::from("NEXL_STDLIB_TEST_VAR"))]).unwrap();
        unsafe { std::env::remove_var("NEXL_STDLIB_TEST_VAR") };
        match result {
            Value::Adt { ctor, fields, .. } if ctor.as_ref() == "Some" => {
                assert_eq!(fields[0], Value::Str(Rc::from("hello")));
            }
            other => panic!("expected Some(Str), got {other}"),
        }
    }

    // ── Test 4 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_env_require_missing() {
        let err = require_fn(&[Value::Str(Rc::from("NEXL_STDLIB_TEST_MISSING_VAR_ABC"))]).unwrap_err();
        assert!(err.contains("not set") || err.contains("NEXL_STDLIB_TEST_MISSING_VAR_ABC"), "{err}");
    }

    // ── Test 5 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_env_require_present() {
        let _lock = ENV_LOCK.lock().unwrap();
        // SAFETY: single-threaded test; ENV_LOCK ensures no concurrent mutation.
        unsafe { std::env::set_var("NEXL_STDLIB_TEST_REQ", "world") };
        let val = require_fn(&[Value::Str(Rc::from("NEXL_STDLIB_TEST_REQ"))]).unwrap();
        unsafe { std::env::remove_var("NEXL_STDLIB_TEST_REQ") };
        assert_eq!(val, Value::Str(Rc::from("world")));
    }

    // ── Test 6 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_env_all_returns_map() {
        let result = all_fn(&[]).unwrap();
        assert!(matches!(result, Value::Map(_)), "env/all should return a Map");
    }

    // ── Test 7 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_env_load_dotenv_missing_file() {
        let err = load_dotenv_fn(&[Value::Str(Rc::from(
            "/no/such/path/.env.nonexistent",
        ))]).unwrap_err();
        assert!(err.contains("load-dotenv"), "{err}");
    }

    // ── Test 8 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_env_load_dotenv_parses_file() {
        let _lock = ENV_LOCK.lock().unwrap();
        // Write a temp .env file and load it.
        let dir = std::env::temp_dir();
        let path = dir.join("nexl_test_dotenv.env");
        std::fs::write(&path, "NEXL_DOTENV_TEST_KEY=dotenv_value\n# comment\n\nKEY2=\"quoted\"\n").unwrap();
        load_dotenv_fn(&[Value::Str(Rc::from(path.to_str().unwrap()))]).unwrap();
        assert_eq!(std::env::var("NEXL_DOTENV_TEST_KEY").unwrap(), "dotenv_value");
        assert_eq!(std::env::var("KEY2").unwrap(), "quoted");
        // SAFETY: single-threaded test; ENV_LOCK ensures no concurrent mutation.
        unsafe {
            std::env::remove_var("NEXL_DOTENV_TEST_KEY");
            std::env::remove_var("KEY2");
        }
        std::fs::remove_file(&path).ok();
    }

    // ── Test 9 ──────────────────────────────────────────────────────────────

    #[test]
    fn test_env_get_wrong_arg() {
        let err = get_fn(&[Value::Int(42)]).unwrap_err();
        assert!(err.contains("Str"), "{err}");
    }

    // ── Test 10 ─────────────────────────────────────────────────────────────

    #[test]
    fn test_env_all_no_args() {
        let err = all_fn(&[Value::Int(1)]).unwrap_err();
        assert!(err.contains("no arguments"), "{err}");
    }
}
