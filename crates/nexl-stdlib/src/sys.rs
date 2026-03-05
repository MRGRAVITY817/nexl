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
        ("os", sys_os),
        ("arch", sys_arch),
        ("cpu-count", sys_cpu_count),
        ("cwd", sys_cwd),
        ("home-dir", sys_home_dir),
        ("exe-path", sys_exe_path),
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
    let values: Vec<Value> = argv
        .iter()
        .map(|s| Value::Str(Rc::from(s.as_str())))
        .collect();
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
            ));
        }
        _ => {
            return Err(format!(
                "`sys/getenv` requires exactly 1 argument, got {}",
                args.len()
            ));
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

/// `(sys/os)` — return the operating system name string.
fn sys_os(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`sys/os` takes 0 arguments, got {}", args.len()));
    }
    Ok(Value::Str(Rc::from(std::env::consts::OS)))
}

/// `(sys/arch)` — return the CPU architecture string.
fn sys_arch(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`sys/arch` takes 0 arguments, got {}", args.len()));
    }
    Ok(Value::Str(Rc::from(std::env::consts::ARCH)))
}

/// `(sys/cpu-count)` — return the number of available CPUs as Int.
fn sys_cpu_count(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`sys/cpu-count` takes 0 arguments, got {}", args.len()));
    }
    Ok(Value::Int(num_cpus() as i64))
}

fn num_cpus() -> usize {
    // std::thread::available_parallelism() is stable since Rust 1.59
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// `(sys/cwd)` — return the current working directory path as Str.
fn sys_cwd(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::FileSystem)?;
    if !args.is_empty() {
        return Err(format!("`sys/cwd` takes 0 arguments, got {}", args.len()));
    }
    let cwd = std::env::current_dir()
        .map_err(|e| format!("`sys/cwd` failed: {e}"))?;
    Ok(Value::Str(Rc::from(cwd.to_string_lossy().as_ref())))
}

/// `(sys/home-dir)` — return the user home directory as `(Option Str)`.
fn sys_home_dir(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`sys/home-dir` takes 0 arguments, got {}", args.len()));
    }
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok());
    Ok(match home {
        Some(h) => Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("Some"),
            fields: Rc::new(vec![Value::Str(Rc::from(h.as_str()))]),
        },
        None => Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("None"),
            fields: Rc::new(vec![]),
        },
    })
}

/// `(sys/exe-path)` — return the path to the current executable as `(Option Str)`.
fn sys_exe_path(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`sys/exe-path` takes 0 arguments, got {}", args.len()));
    }
    Ok(match std::env::current_exe().ok() {
        Some(path) => Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("Some"),
            fields: Rc::new(vec![Value::Str(Rc::from(path.to_string_lossy().as_ref()))]),
        },
        None => Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("None"),
            fields: Rc::new(vec![]),
        },
    })
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

    #[test]
    fn test_os_returns_non_empty_str() {
        match sys_os(&[]).unwrap() {
            Value::Str(s) => assert!(!s.is_empty(), "os should not be empty"),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn test_arch_returns_non_empty_str() {
        match sys_arch(&[]).unwrap() {
            Value::Str(s) => assert!(!s.is_empty(), "arch should not be empty"),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn test_cpu_count_positive() {
        match sys_cpu_count(&[]).unwrap() {
            Value::Int(n) => assert!(n >= 1, "cpu-count should be at least 1"),
            other => panic!("expected Int, got {other:?}"),
        }
    }

    #[test]
    fn test_cwd_returns_str() {
        match sys_cwd(&[]).unwrap() {
            Value::Str(s) => assert!(!s.is_empty(), "cwd should not be empty"),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn test_home_dir_returns_option() {
        match sys_home_dir(&[]).unwrap() {
            Value::Adt { type_name, .. } => assert_eq!(type_name.as_ref(), "Option"),
            other => panic!("expected Option, got {other:?}"),
        }
    }

    #[test]
    fn test_exe_path_returns_option() {
        match sys_exe_path(&[]).unwrap() {
            Value::Adt { type_name, .. } => assert_eq!(type_name.as_ref(), "Option"),
            other => panic!("expected Option, got {other:?}"),
        }
    }

    #[test]
    fn test_entries_registered() {
        let names: Vec<&str> = entries().iter().map(|(n, _)| *n).collect();
        for name in ["args", "getenv", "exit", "os", "arch", "cpu-count", "cwd", "home-dir", "exe-path"] {
            assert!(names.contains(&name), "missing: {name}");
        }
    }
}
