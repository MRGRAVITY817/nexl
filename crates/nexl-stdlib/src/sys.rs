//! `sys` module — system interface (args, env vars, exit).
//!
//! In Stage 0, these call Rust standard library functions directly.

use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `sys` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("args", sys_args as fn(&[Value]) -> Result<Value, String>),
        ("getenv", sys_getenv),
        ("exit", sys_exit),
    ]
}

/// `(sys/args)` — return command-line arguments after `nexl run <file>`.
/// Returns (Vec Str).
fn sys_args(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Console)?;
    if !args.is_empty() {
        return Err(format!(
            "`sys/args` requires 0 arguments, got {}",
            args.len()
        ));
    }
    let argv = nexl_runtime::sys::get_program_args();
    let values: Vec<Value> = argv.iter().map(|s| Value::Str(Rc::from(s.as_str()))).collect();
    Ok(Value::Vec(Rc::new(values)))
}

/// `(sys/getenv name)` — read environment variable. Returns (Option Str).
fn sys_getenv(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Console)?;
    let name = match args {
        [Value::Str(s)] => s,
        [other] => {
            return Err(format!(
                "`sys/getenv` expected Str, got {}",
                other.type_name()
            ))
        }
        _ => {
            return Err(format!(
                "`sys/getenv` requires exactly 1 argument, got {}",
                args.len()
            ))
        }
    };
    match std::env::var(name.as_ref()) {
        Ok(val) => Ok(Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("Some"),
            fields: Rc::new(vec![Value::Str(Rc::from(val.as_str()))]),
        }),
        Err(_) => Ok(Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("None"),
            fields: Rc::new(vec![]),
        }),
    }
}

/// `(sys/exit code)` — exit with status code.
fn sys_exit(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(code)] => std::process::exit(*code as i32),
        [other] => Err(format!(
            "`sys/exit` expected Int, got {}",
            other.type_name()
        )),
        _ => Err(format!(
            "`sys/exit` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_getenv_home() {
        // HOME should exist on macOS/Linux
        let result = sys_getenv(&[Value::Str(Rc::from("HOME"))]).unwrap();
        match result {
            Value::Adt { ctor, .. } => assert_eq!(ctor.as_ref(), "Some"),
            _ => panic!("expected Option.Some"),
        }
    }

    #[test]
    fn test_getenv_missing() {
        let result =
            sys_getenv(&[Value::Str(Rc::from("NEXL_DEFINITELY_DOES_NOT_EXIST_12345"))]).unwrap();
        match result {
            Value::Adt { ctor, .. } => assert_eq!(ctor.as_ref(), "None"),
            _ => panic!("expected Option.None"),
        }
    }

    #[test]
    fn test_args_returns_vec() {
        let result = sys_args(&[]).unwrap();
        match result {
            Value::Vec(_) => {}
            _ => panic!("expected Vec"),
        }
    }
}
