//! `process` module — child process management.
//!
//! Stage 0: backed by `std::process::Command`. Spawned processes block until
//! completion in this implementation (true async process handles require a
//! runtime).
//!
//! Functions:
//! - `(process/run cmd)` — run command string, return `Output`
//! - `(process/run-with opts)` — run with `ProcessOpts` map
//! - `(process/spawn cmd)` — spawn and return handle (Stage 0: runs to completion, returns handle)
//! - `(process/wait handle)` — wait for handle to finish → `Output`
//! - `(process/kill handle)` — kill the process
//! - `(process/pid)` — PID of the current process
//!
//! `Output` record: `{:exit-code Int :stdout Str :stderr Str}`
//! `ProcessOpts`: `{:cmd Str :args (Vec Str) :cwd (Option Str) :env (Map Str Str) :stdin (Option Str)}`

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use nexl_runtime::value::NexlMap;
use nexl_runtime::Value;

use crate::StdlibEntry;

// ─── Process handle registry ──────────────────────────────────────────────────

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

struct ProcessEntry {
    /// Pre-computed output (Stage 0: run-to-completion on spawn).
    output: Option<(i32, String, String)>,
    /// True if killed.
    killed: bool,
}

thread_local! {
    static PROCESSES: RefCell<HashMap<u64, ProcessEntry>> = RefCell::new(HashMap::new());
}

fn handle_adt(id: u64) -> Value {
    Value::Adt {
        type_name: Rc::from("ProcessHandle"),
        ctor: Rc::from("ProcessHandle"),
        fields: Rc::new(vec![Value::Int(id as i64)]),
    }
}

fn extract_handle_id(v: &Value) -> Result<u64, String> {
    match v {
        Value::Adt { type_name, ctor, fields }
            if type_name.as_ref() == "ProcessHandle" && ctor.as_ref() == "ProcessHandle" =>
        {
            match fields.first() {
                Some(Value::Int(id)) => Ok(*id as u64),
                _ => Err("ProcessHandle has unexpected field".into()),
            }
        }
        other => Err(format!("expected ProcessHandle, got {}", other.type_name())),
    }
}

fn output_map(exit_code: i32, stdout: &str, stderr: &str) -> Value {
    let mut m = NexlMap::new();
    m = m.put(
        Value::Keyword { ns: None, name: Rc::from("exit-code") },
        Value::Int(exit_code as i64),
    );
    m = m.put(
        Value::Keyword { ns: None, name: Rc::from("stdout") },
        Value::Str(Rc::from(stdout)),
    );
    m = m.put(
        Value::Keyword { ns: None, name: Rc::from("stderr") },
        Value::Str(Rc::from(stderr)),
    );
    Value::Map(Rc::new(m))
}

fn ok_val(v: Value) -> Value {
    Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Ok"),
        fields: Rc::new(vec![v]),
    }
}

fn err_val(msg: &str) -> Value {
    Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Err"),
        fields: Rc::new(vec![Value::Str(Rc::from(msg))]),
    }
}

/// Run a shell command, returning `(exit_code, stdout, stderr)`.
fn run_shell(cmd: &str) -> (i32, String, String) {
    let result = std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output();
    match result {
        Ok(out) => (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ),
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

/// Run with full `ProcessOpts` map.
fn run_opts(opts: &NexlMap) -> (i32, String, String) {
    let kw = |name: &str| Value::Keyword { ns: None, name: Rc::from(name) };

    let cmd = match opts.get(&kw("cmd")) {
        Some(Value::Str(s)) => s.to_string(),
        _ => return (-1, String::new(), "`process/run-with` opts missing :cmd Str".into()),
    };

    let mut command = std::process::Command::new(&cmd);

    // :args — Vec of Str
    if let Some(Value::Vec(args_vec)) = opts.get(&kw("args")) {
        for arg in args_vec.iter() {
            if let Value::Str(s) = arg {
                command.arg(s.as_ref());
            }
        }
    }

    // :cwd — Option Str
    if let Some(Value::Adt { ctor, fields, .. }) = opts.get(&kw("cwd")) {
        if ctor.as_ref() == "Some" {
            if let Some(Value::Str(dir)) = fields.first() {
                command.current_dir(dir.as_ref());
            }
        }
    }

    // :env — Map Str Str
    if let Some(Value::Map(env_map)) = opts.get(&kw("env")) {
        for (k, v) in env_map.iter() {
            if let (Value::Str(key), Value::Str(val)) = (k, v) {
                command.env(key.as_ref(), val.as_ref());
            }
        }
    }

    // :stdin — Option Str
    let stdin_data = match opts.get(&kw("stdin")) {
        Some(Value::Adt { ctor, fields, .. }) if ctor.as_ref() == "Some" => {
            if let Some(Value::Str(s)) = fields.first() {
                Some(s.to_string())
            } else {
                None
            }
        }
        Some(Value::Str(s)) => Some(s.to_string()),
        _ => None,
    };

    if stdin_data.is_some() {
        command.stdin(std::process::Stdio::piped());
    }
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    match command.spawn() {
        Ok(mut child) => {
            if let Some(data) = stdin_data {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(data.as_bytes());
                }
            }
            match child.wait_with_output() {
                Ok(out) => (
                    out.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stdout).into_owned(),
                    String::from_utf8_lossy(&out.stderr).into_owned(),
                ),
                Err(e) => (-1, String::new(), e.to_string()),
            }
        }
        Err(e) => (-1, String::new(), e.to_string()),
    }
}

// ─── Entries ──────────────────────────────────────────────────────────────────

/// Return all `process` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("run", run_fn as fn(&[Value]) -> Result<Value, String>),
        ("run-with", run_with_fn),
        ("spawn", spawn_fn),
        ("wait", wait_fn),
        ("kill", kill_fn),
        ("pid", pid_fn),
    ]
}

// ─── Implementations ──────────────────────────────────────────────────────────

/// `(process/run cmd)` — run shell command, return `(Result Output Str)`.
fn run_fn(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Console)?;
    match args {
        [Value::Str(cmd)] => {
            let (code, stdout, stderr) = run_shell(cmd);
            Ok(ok_val(output_map(code, &stdout, &stderr)))
        }
        [other] => Err(format!("`process/run` expected Str, got {}", other.type_name())),
        _ => Err(format!("`process/run` requires 1 argument, got {}", args.len())),
    }
}

/// `(process/run-with opts)` — run with ProcessOpts map, return `(Result Output Str)`.
fn run_with_fn(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Console)?;
    match args {
        [Value::Map(opts)] => {
            let (code, stdout, stderr) = run_opts(opts);
            Ok(ok_val(output_map(code, &stdout, &stderr)))
        }
        [other] => Err(format!("`process/run-with` expected Map opts, got {}", other.type_name())),
        _ => Err(format!("`process/run-with` requires 1 argument, got {}", args.len())),
    }
}

/// `(process/spawn cmd)` — spawn command and return handle (Stage 0: runs to completion).
fn spawn_fn(args: &[Value]) -> Result<Value, String> {
    nexl_runtime::sandbox::check(nexl_runtime::sandbox::Capability::Console)?;
    match args {
        [Value::Str(cmd)] => {
            let (code, stdout, stderr) = run_shell(cmd);
            let id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
            PROCESSES.with(|p| {
                p.borrow_mut().insert(
                    id,
                    ProcessEntry {
                        output: Some((code, stdout, stderr)),
                        killed: false,
                    },
                );
            });
            Ok(handle_adt(id))
        }
        [other] => Err(format!("`process/spawn` expected Str, got {}", other.type_name())),
        _ => Err(format!("`process/spawn` requires 1 argument, got {}", args.len())),
    }
}

/// `(process/wait handle)` — wait for process, return `(Result Output Str)`.
fn wait_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [handle] => {
            let id = extract_handle_id(handle)?;
            let result = PROCESSES.with(|p| {
                let map = p.borrow();
                map.get(&id).map(|e| {
                    if e.killed {
                        Err("process was killed".to_string())
                    } else if let Some((code, ref out, ref err)) = e.output {
                        Ok((code, out.clone(), err.clone()))
                    } else {
                        Err("process output not available".to_string())
                    }
                })
            });
            match result {
                Some(Ok((code, stdout, stderr))) => Ok(ok_val(output_map(code, &stdout, &stderr))),
                Some(Err(e)) => Ok(err_val(&e)),
                None => Ok(err_val(&format!("process handle {id} not found"))),
            }
        }
        _ => Err(format!("`process/wait` requires 1 argument, got {}", args.len())),
    }
}

/// `(process/kill handle)` — kill the process (Stage 0: marks as killed).
fn kill_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [handle] => {
            let id = extract_handle_id(handle)?;
            PROCESSES.with(|p| {
                let mut map = p.borrow_mut();
                if let Some(entry) = map.get_mut(&id) {
                    entry.killed = true;
                }
            });
            Ok(Value::Unit)
        }
        _ => Err(format!("`process/kill` requires 1 argument, got {}", args.len())),
    }
}

/// `(process/pid)` — return the PID of the current process as Int.
fn pid_fn(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`process/pid` takes 0 arguments, got {}", args.len()));
    }
    Ok(Value::Int(std::process::id() as i64))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 1. run echo returns exit code 0
    #[test]
    fn test_run_echo() {
        let result = run_fn(&[Value::Str(Rc::from("echo hello"))]).unwrap();
        match &result {
            Value::Adt { ctor, fields, .. } => {
                assert_eq!(ctor.as_ref(), "Ok");
                match &fields[0] {
                    Value::Map(m) => {
                        let code = m.get(&Value::Keyword { ns: None, name: Rc::from("exit-code") });
                        assert_eq!(code, Some(&Value::Int(0)));
                    }
                    other => panic!("expected Map output, got {other:?}"),
                }
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    // 2. run captures stdout
    #[test]
    fn test_run_captures_stdout() {
        let result = run_fn(&[Value::Str(Rc::from("echo nexl"))]).unwrap();
        match result {
            Value::Adt { ctor, fields, .. } if ctor.as_ref() == "Ok" => {
                if let Value::Map(m) = &fields[0] {
                    let stdout = m.get(&Value::Keyword { ns: None, name: Rc::from("stdout") });
                    match stdout {
                        Some(Value::Str(s)) => assert!(s.contains("nexl"), "stdout: {s}"),
                        other => panic!("expected Str stdout, got {other:?}"),
                    }
                }
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    // 3. spawn returns handle, wait returns output
    #[test]
    fn test_spawn_wait() {
        let handle = spawn_fn(&[Value::Str(Rc::from("echo hello"))]).unwrap();
        assert!(matches!(&handle, Value::Adt { type_name, .. } if type_name.as_ref() == "ProcessHandle"));
        let output = wait_fn(&[handle]).unwrap();
        assert!(matches!(&output, Value::Adt { ctor, .. } if ctor.as_ref() == "Ok"));
    }

    // 4. kill marks process as killed; wait returns Err
    #[test]
    fn test_kill_wait_returns_err() {
        let handle = spawn_fn(&[Value::Str(Rc::from("echo x"))]).unwrap();
        kill_fn(&[handle.clone()]).unwrap();
        let result = wait_fn(&[handle]).unwrap();
        assert!(matches!(&result, Value::Adt { ctor, .. } if ctor.as_ref() == "Err"));
    }

    // 5. pid returns positive Int
    #[test]
    fn test_pid_positive() {
        match pid_fn(&[]).unwrap() {
            Value::Int(n) => assert!(n > 0, "PID should be positive"),
            other => panic!("expected Int PID, got {other:?}"),
        }
    }

    // 6. run non-zero exit code
    #[test]
    fn test_run_nonzero_exit() {
        let result = run_fn(&[Value::Str(Rc::from("exit 42"))]).unwrap();
        match result {
            Value::Adt { ctor, fields, .. } if ctor.as_ref() == "Ok" => {
                if let Value::Map(m) = &fields[0] {
                    let code = m.get(&Value::Keyword { ns: None, name: Rc::from("exit-code") });
                    assert_eq!(code, Some(&Value::Int(42)));
                }
            }
            other => panic!("expected Ok output, got {other:?}"),
        }
    }

    // 7. run-with with cmd and args
    #[test]
    fn test_run_with_cmd_args() {
        let mut m = NexlMap::new();
        m = m.put(
            Value::Keyword { ns: None, name: Rc::from("cmd") },
            Value::Str(Rc::from("echo")),
        );
        m = m.put(
            Value::Keyword { ns: None, name: Rc::from("args") },
            Value::Vec(Rc::new(vec![Value::Str(Rc::from("hello"))])),
        );
        let result = run_with_fn(&[Value::Map(Rc::new(m))]).unwrap();
        assert!(matches!(&result, Value::Adt { ctor, .. } if ctor.as_ref() == "Ok"));
    }

    // 8. entries registered
    #[test]
    fn test_entries_registered() {
        let names: Vec<&str> = entries().iter().map(|(n, _)| *n).collect();
        for name in ["run", "run-with", "spawn", "wait", "kill", "pid"] {
            assert!(names.contains(&name), "missing: {name}");
        }
    }
}
