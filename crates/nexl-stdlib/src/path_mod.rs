//! `path` module — cross-platform path manipulation.
//!
//! All functions are pure (no effects) — they operate on strings interpreted
//! as file-system paths. Backed by `std::path::Path`.
//!
//! Functions follow the subject-first `->` convention: the path is always the
//! first argument.

use std::path::{Path, MAIN_SEPARATOR};
use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `path` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("join", join as fn(&[Value]) -> Result<Value, String>),
        ("parent", parent),
        ("file-name", file_name),
        ("stem", stem),
        ("extension", extension),
        ("with-extension", with_extension),
        ("normalize", normalize),
        ("absolute?", absolute_pred),
        ("relative?", relative_pred),
        ("separator", separator),
        ("components", components),
        ("relative-to", relative_to),
        ("starts-with?", starts_with_pred),
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn str_val(s: impl AsRef<str>) -> Value {
    Value::Str(Rc::from(s.as_ref()))
}

fn some(v: Value) -> Value {
    Value::Adt {
        type_name: Rc::from("Option"),
        ctor: Rc::from("Some"),
        fields: Rc::new(vec![v]),
    }
}

fn none() -> Value {
    Value::Adt {
        type_name: Rc::from("Option"),
        ctor: Rc::from("None"),
        fields: Rc::new(vec![]),
    }
}

fn expect_str<'a>(name: &str, v: &'a Value) -> Result<&'a str, String> {
    match v {
        Value::Str(s) => Ok(s.as_ref()),
        other => Err(format!("`path/{name}` expected Str, got {other}")),
    }
}

/// Resolve `.` and `..` components without touching the filesystem.
///
/// Algorithm: iterate components, pushing to a stack; `..` pops the last
/// non-root entry; `.` is skipped.
fn resolve_dots(path: &str) -> String {
    let p = Path::new(path);
    let is_absolute = p.is_absolute();
    let mut parts: Vec<String> = Vec::new();

    for comp in p.components() {
        use std::path::Component;
        match comp {
            Component::RootDir | Component::Prefix(_) => {
                parts.push(comp.as_os_str().to_string_lossy().into_owned());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.last().map(|s| s != "/" && s != "\\").unwrap_or(false) {
                    parts.pop();
                }
            }
            Component::Normal(s) => {
                parts.push(s.to_string_lossy().into_owned());
            }
        }
    }

    if is_absolute {
        let sep = MAIN_SEPARATOR.to_string();
        // parts[0] is already the root prefix or "/"
        if parts.len() == 1 {
            parts[0].clone()
        } else {
            let mut result = parts[0].clone();
            if !result.ends_with(MAIN_SEPARATOR) {
                result.push(MAIN_SEPARATOR);
            }
            result + &parts[1..].join(&sep)
        }
    } else {
        parts.join(&MAIN_SEPARATOR.to_string())
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// `(path/join component ...)` → `Str` — join path components with the OS separator.
fn join(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("`path/join` requires at least 1 argument".to_string());
    }
    let mut result = std::path::PathBuf::new();
    for (i, arg) in args.iter().enumerate() {
        let s = expect_str(&format!("join[{i}]"), arg)?;
        result.push(s);
    }
    Ok(str_val(result.to_string_lossy()))
}

/// `(path/parent path)` → `(Some Str)` or `None` — parent directory.
fn parent(args: &[Value]) -> Result<Value, String> {
    match args {
        [p] => {
            let s = expect_str("parent", p)?;
            match Path::new(s).parent() {
                Some(parent) if parent != Path::new("") => {
                    Ok(some(str_val(parent.to_string_lossy())))
                }
                Some(_) | None => Ok(none()),
            }
        }
        _ => Err(format!("`path/parent` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(path/file-name path)` → `(Some Str)` or `None` — file name including extension.
fn file_name(args: &[Value]) -> Result<Value, String> {
    match args {
        [p] => {
            let s = expect_str("file-name", p)?;
            match Path::new(s).file_name() {
                Some(name) => Ok(some(str_val(name.to_string_lossy()))),
                None => Ok(none()),
            }
        }
        _ => Err(format!("`path/file-name` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(path/stem path)` → `(Some Str)` or `None` — file name without extension.
fn stem(args: &[Value]) -> Result<Value, String> {
    match args {
        [p] => {
            let s = expect_str("stem", p)?;
            match Path::new(s).file_stem() {
                Some(stem) => Ok(some(str_val(stem.to_string_lossy()))),
                None => Ok(none()),
            }
        }
        _ => Err(format!("`path/stem` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(path/extension path)` → `(Some Str)` or `None` — extension without the dot.
fn extension(args: &[Value]) -> Result<Value, String> {
    match args {
        [p] => {
            let s = expect_str("extension", p)?;
            match Path::new(s).extension() {
                Some(ext) => Ok(some(str_val(ext.to_string_lossy()))),
                None => Ok(none()),
            }
        }
        _ => Err(format!("`path/extension` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(path/with-extension path ext)` → `Str` — replace (or add) the extension.
///
/// Pass an empty string `""` to strip the extension.
fn with_extension(args: &[Value]) -> Result<Value, String> {
    match args {
        [p, ext] => {
            let s = expect_str("with-extension", p)?;
            let e = expect_str("with-extension", ext)?;
            let new_path = Path::new(s).with_extension(e);
            Ok(str_val(new_path.to_string_lossy()))
        }
        _ => Err(format!("`path/with-extension` requires 2 arguments (Str Str), got {}", args.len())),
    }
}

/// `(path/normalize path)` → `Str` — resolve `.` and `..` without I/O.
fn normalize(args: &[Value]) -> Result<Value, String> {
    match args {
        [p] => {
            let s = expect_str("normalize", p)?;
            Ok(str_val(resolve_dots(s)))
        }
        _ => Err(format!("`path/normalize` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(path/absolute? path)` → `Bool` — is this an absolute path?
fn absolute_pred(args: &[Value]) -> Result<Value, String> {
    match args {
        [p] => {
            let s = expect_str("absolute?", p)?;
            Ok(Value::Bool(Path::new(s).is_absolute()))
        }
        _ => Err(format!("`path/absolute?` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(path/relative? path)` → `Bool` — is this a relative path?
fn relative_pred(args: &[Value]) -> Result<Value, String> {
    match args {
        [p] => {
            let s = expect_str("relative?", p)?;
            Ok(Value::Bool(Path::new(s).is_relative()))
        }
        _ => Err(format!("`path/relative?` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(path/separator)` → `Str` — the OS path separator (`/` or `\`).
fn separator(args: &[Value]) -> Result<Value, String> {
    match args {
        [] => Ok(str_val(MAIN_SEPARATOR.to_string())),
        _ => Err(format!("`path/separator` requires 0 arguments, got {}", args.len())),
    }
}

/// `(path/components path)` → `Vec` of `Str` — split into path components.
fn components(args: &[Value]) -> Result<Value, String> {
    match args {
        [p] => {
            let s = expect_str("components", p)?;
            let parts: Vec<Value> = Path::new(s)
                .components()
                .map(|c| str_val(c.as_os_str().to_string_lossy()))
                .collect();
            Ok(Value::Vec(Rc::new(parts)))
        }
        _ => Err(format!("`path/components` requires 1 argument (Str), got {}", args.len())),
    }
}

/// `(path/relative-to base path)` → `(Some Str)` or `None` — make `path` relative to `base`.
fn relative_to(args: &[Value]) -> Result<Value, String> {
    match args {
        [base, p] => {
            let base_s = expect_str("relative-to", base)?;
            let p_s = expect_str("relative-to", p)?;
            match Path::new(p_s).strip_prefix(Path::new(base_s)) {
                Ok(rel) => Ok(some(str_val(rel.to_string_lossy()))),
                Err(_) => Ok(none()),
            }
        }
        _ => Err(format!("`path/relative-to` requires 2 arguments (Str Str), got {}", args.len())),
    }
}

/// `(path/starts-with? path prefix)` → `Bool` — does the path start with `prefix`?
fn starts_with_pred(args: &[Value]) -> Result<Value, String> {
    match args {
        [p, prefix] => {
            let p_s = expect_str("starts-with?", p)?;
            let prefix_s = expect_str("starts-with?", prefix)?;
            Ok(Value::Bool(Path::new(p_s).starts_with(Path::new(prefix_s))))
        }
        _ => Err(format!("`path/starts-with?` requires 2 arguments (Str Str), got {}", args.len())),
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
    fn test_join_two() {
        let result = join(&[s("foo"), s("bar")]).unwrap();
        let expected = format!("foo{MAIN_SEPARATOR}bar");
        assert_eq!(result, str_val(&expected));
    }

    #[test]
    fn test_join_three() {
        let result = join(&[s("a"), s("b"), s("c")]).unwrap();
        let sep = MAIN_SEPARATOR.to_string();
        assert_eq!(result, str_val(&format!("a{sep}b{sep}c")));
    }

    #[test]
    fn test_parent_some() {
        let result = parent(&[s("foo/bar/baz.txt")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Some"));
    }

    #[test]
    fn test_parent_none_at_root() {
        // A bare filename has no parent
        let result = parent(&[s("file.txt")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "None"));
    }

    #[test]
    fn test_file_name_some() {
        let result = file_name(&[s("foo/bar.txt")]).unwrap();
        if let Value::Adt { ctor, fields, .. } = result {
            assert_eq!(ctor.as_ref(), "Some");
            assert_eq!(fields[0], str_val("bar.txt"));
        } else {
            panic!("expected Some");
        }
    }

    #[test]
    fn test_stem() {
        let result = stem(&[s("foo/bar.txt")]).unwrap();
        if let Value::Adt { ctor, fields, .. } = result {
            assert_eq!(ctor.as_ref(), "Some");
            assert_eq!(fields[0], str_val("bar"));
        } else {
            panic!("expected Some");
        }
    }

    #[test]
    fn test_extension_some() {
        let result = extension(&[s("file.rs")]).unwrap();
        if let Value::Adt { ctor, fields, .. } = result {
            assert_eq!(ctor.as_ref(), "Some");
            assert_eq!(fields[0], str_val("rs"));
        } else {
            panic!("expected Some");
        }
    }

    #[test]
    fn test_extension_none() {
        let result = extension(&[s("Makefile")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "None"));
    }

    #[test]
    fn test_with_extension() {
        let result = with_extension(&[s("foo/bar.txt"), s("rs")]).unwrap();
        let expected = Path::new("foo/bar.txt").with_extension("rs");
        assert_eq!(result, str_val(expected.to_string_lossy()));
    }

    #[test]
    fn test_with_extension_strip() {
        let result = with_extension(&[s("foo/bar.txt"), s("")]).unwrap();
        let expected = Path::new("foo/bar.txt").with_extension("");
        assert_eq!(result, str_val(expected.to_string_lossy()));
    }

    #[test]
    fn test_normalize_dots() {
        let result = normalize(&[s("foo/./bar/../baz")]).unwrap();
        assert_eq!(result, str_val("foo/baz"));
    }

    #[test]
    fn test_absolute_pred() {
        assert_eq!(absolute_pred(&[s("/etc/hosts")]).unwrap(), Value::Bool(true));
        assert_eq!(absolute_pred(&[s("relative/path")]).unwrap(), Value::Bool(false));
    }

    #[test]
    fn test_relative_pred() {
        assert_eq!(relative_pred(&[s("foo/bar")]).unwrap(), Value::Bool(true));
        assert_eq!(relative_pred(&[s("/foo/bar")]).unwrap(), Value::Bool(false));
    }

    #[test]
    fn test_separator() {
        let result = separator(&[]).unwrap();
        assert_eq!(result, str_val(MAIN_SEPARATOR.to_string()));
    }

    #[test]
    fn test_components() {
        let result = components(&[s("foo/bar/baz")]).unwrap();
        assert!(matches!(result, Value::Vec(_)));
    }

    #[test]
    fn test_relative_to_some() {
        let result = relative_to(&[s("/usr"), s("/usr/local/bin")]).unwrap();
        if let Value::Adt { ctor, fields, .. } = result {
            assert_eq!(ctor.as_ref(), "Some");
            assert_eq!(fields[0], str_val("local/bin"));
        } else {
            panic!("expected Some");
        }
    }

    #[test]
    fn test_relative_to_none() {
        let result = relative_to(&[s("/usr"), s("/etc/hosts")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "None"));
    }

    #[test]
    fn test_starts_with_pred() {
        assert_eq!(
            starts_with_pred(&[s("/usr/local/bin"), s("/usr/local")]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            starts_with_pred(&[s("/usr/local/bin"), s("/etc")]).unwrap(),
            Value::Bool(false)
        );
    }
}
