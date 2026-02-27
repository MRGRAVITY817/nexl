//! `io` module — convenience wrappers around FileSystem/Console effects.
//!
//! In Stage 0, these functions perform actual I/O directly (not via effects).
//! They will be refactored to use the effect system in a future milestone.

use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `io` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        (
            "println",
            io_println as fn(&[Value]) -> Result<Value, String>,
        ),
        ("print", io_print),
        ("read-line", read_line),
        ("read-file", read_file),
        ("write-file", write_file),
        ("path-join", path_join),
        ("file-exists?", file_exists),
        ("read-dir", read_dir),
        ("delete-file", delete_file),
    ]
}

fn one_arg<'a>(op: &str, args: &'a [Value]) -> Result<&'a Value, String> {
    match args {
        [a] => Ok(a),
        _ => Err(format!(
            "`io/{op}` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

fn two_args<'a>(op: &str, args: &'a [Value]) -> Result<(&'a Value, &'a Value), String> {
    match args {
        [a, b] => Ok((a, b)),
        _ => Err(format!(
            "`io/{op}` requires exactly 2 arguments, got {}",
            args.len()
        )),
    }
}

fn expect_str<'a>(op: &str, v: &'a Value) -> Result<&'a Rc<str>, String> {
    match v {
        Value::Str(s) => Ok(s),
        other => Err(format!("`io/{op}` expected Str, got {}", other.type_name())),
    }
}

/// `(io/println s)` — print string with newline.
fn io_println(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Console)?;
    let v = one_arg("println", args)?;
    match v {
        Value::Str(s) => println!("{s}"),
        other => println!("{other}"),
    }
    Ok(Value::Unit)
}

/// `(io/print s)` — print string without newline.
fn io_print(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Console)?;
    let v = one_arg("print", args)?;
    match v {
        Value::Str(s) => print!("{s}"),
        other => print!("{other}"),
    }
    Ok(Value::Unit)
}

/// `(io/read-file path)` — read file contents as Str. Returns (Result Str Str).
fn read_file(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::FileSystem)?;
    let v = one_arg("read-file", args)?;
    let path = expect_str("read-file", v)?;
    match std::fs::read_to_string(path.as_ref()) {
        Ok(contents) => Ok(Value::Adt {
            type_name: Rc::from("Result"),
            ctor: Rc::from("Ok"),
            fields: Rc::new(vec![Value::Str(Rc::from(contents.as_str()))]),
        }),
        Err(e) => Ok(Value::Adt {
            type_name: Rc::from("Result"),
            ctor: Rc::from("Err"),
            fields: Rc::new(vec![Value::Str(Rc::from(e.to_string().as_str()))]),
        }),
    }
}

/// `(io/write-file path content)` — write string to file. Returns (Result Unit Str).
fn write_file(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::FileSystem)?;
    let (path, content) = two_args("write-file", args)?;
    let path = expect_str("write-file", path)?;
    let content = expect_str("write-file", content)?;
    match std::fs::write(path.as_ref(), content.as_ref()) {
        Ok(()) => Ok(Value::Adt {
            type_name: Rc::from("Result"),
            ctor: Rc::from("Ok"),
            fields: Rc::new(vec![Value::Unit]),
        }),
        Err(e) => Ok(Value::Adt {
            type_name: Rc::from("Result"),
            ctor: Rc::from("Err"),
            fields: Rc::new(vec![Value::Str(Rc::from(e.to_string().as_str()))]),
        }),
    }
}

/// `(io/read-line)` — read one line from stdin. Returns (Result Str Str).
fn read_line(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Console)?;
    if !args.is_empty() {
        return Err(format!(
            "`io/read-line` requires 0 arguments, got {}",
            args.len()
        ));
    }
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => Ok(result_err("eof")),
        Ok(_) => {
            // Strip trailing newline
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Ok(result_ok(Value::Str(Rc::from(line.as_str()))))
        }
        Err(e) => Ok(result_err(&e.to_string())),
    }
}

/// `(io/file-exists? path)` — check if path exists. Returns Bool.
fn file_exists(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::FileSystem)?;
    let v = one_arg("file-exists?", args)?;
    let path = expect_str("file-exists?", v)?;
    Ok(Value::Bool(std::path::Path::new(path.as_ref()).exists()))
}

/// `(io/read-dir path)` — list directory contents. Returns (Result (Vec Str) Str).
fn read_dir(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::FileSystem)?;
    let v = one_arg("read-dir", args)?;
    let path = expect_str("read-dir", v)?;
    match std::fs::read_dir(path.as_ref()) {
        Ok(entries) => {
            let mut names = Vec::new();
            for entry in entries {
                match entry {
                    Ok(e) => names.push(Value::Str(Rc::from(
                        e.file_name().to_string_lossy().as_ref(),
                    ))),
                    Err(e) => return Ok(result_err(&e.to_string())),
                }
            }
            Ok(result_ok(Value::Vec(Rc::new(names))))
        }
        Err(e) => Ok(result_err(&e.to_string())),
    }
}

/// `(io/delete-file path)` — delete a file. Returns (Result Unit Str).
fn delete_file(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::FileSystem)?;
    let v = one_arg("delete-file", args)?;
    let path = expect_str("delete-file", v)?;
    match std::fs::remove_file(path.as_ref()) {
        Ok(()) => Ok(result_ok(Value::Unit)),
        Err(e) => Ok(result_err(&e.to_string())),
    }
}

/// Helper: create (Ok value) ADT.
fn result_ok(value: Value) -> Value {
    Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Ok"),
        fields: Rc::new(vec![value]),
    }
}

/// Helper: create (Err msg) ADT.
fn result_err(msg: &str) -> Value {
    Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Err"),
        fields: Rc::new(vec![Value::Str(Rc::from(msg))]),
    }
}

/// `(io/path-join parts...)` — join path components.
fn path_join(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("`io/path-join` requires at least 1 argument".into());
    }
    let mut path = std::path::PathBuf::new();
    for arg in args {
        let s = expect_str("path-join", arg)?;
        path.push(s.as_ref());
    }
    Ok(Value::Str(Rc::from(path.to_string_lossy().as_ref())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_join() {
        let result = path_join(&[
            Value::Str(Rc::from("a")),
            Value::Str(Rc::from("b")),
            Value::Str(Rc::from("c.txt")),
        ])
        .unwrap();
        let Value::Str(s) = result else {
            panic!("expected Str");
        };
        assert!(s.contains("a") && s.contains("b") && s.contains("c.txt"));
    }
}
