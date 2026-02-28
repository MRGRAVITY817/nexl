//! Wasmtime-based WASM execution for `nexl run --wasm`.
//!
//! [`WasmRunner`] compiles WASM bytes with a wasmtime [`Engine`] and
//! calls the module's entry point (`_start` or `main`), with full
//! WASI Preview 1 support via [`wasmtime_wasi::p1`].

use wasmtime::{Engine, Linker, Module, Store, Val};
use wasmtime_wasi::p1::{self, WasiP1Ctx};
use wasmtime_wasi::WasiCtxBuilder;

/// Executes compiled WASM modules via wasmtime.
pub struct WasmRunner {
    engine: Engine,
}

/// An error produced during WASM execution.
#[derive(Debug)]
pub struct WasmRunError(pub String);

impl std::fmt::Display for WasmRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Output captured from a WASM program's stdout and stderr.
///
/// Returned by [`WasmRunner::run_wasm_captured`]. Used in tests and tooling.
#[derive(Debug, Default)]
#[allow(dead_code)] // used in tests; will be consumed by capture-output tooling in M23+
pub struct CapturedOutput {
    /// Bytes written to stdout.
    pub stdout: Vec<u8>,
    /// Bytes written to stderr.
    pub stderr: Vec<u8>,
}

impl Default for WasmRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmRunner {
    /// Create a new runner with a default wasmtime engine configuration.
    pub fn new() -> Self {
        WasmRunner {
            engine: Engine::default(),
        }
    }

    /// Compile and execute WASM `bytes` with WASI Preview 1 support.
    ///
    /// `args` is the WASI argv passed to the program (index 0 is the program name).
    /// stdio is inherited from the host process.
    /// Returns `Err` if the entry point is missing or execution traps.
    pub fn run_wasm(&self, bytes: &[u8], args: &[&str]) -> Result<(), WasmRunError> {
        self.run_inner(bytes, args, None, None)
    }

    /// Compile and execute WASM `bytes`, capturing stdout and stderr.
    ///
    /// `args` is the argument list passed to the WASM program (WASI argv).
    /// Returns a [`CapturedOutput`] with the bytes written to stdout and stderr.
    #[allow(dead_code)] // used in tests; will be consumed by capture-output tooling in M23+
    pub fn run_wasm_captured(
        &self,
        bytes: &[u8],
        args: &[&str],
    ) -> Result<CapturedOutput, WasmRunError> {
        use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
        let stdout = MemoryOutputPipe::new(64 * 1024);
        let stderr = MemoryOutputPipe::new(64 * 1024);
        self.run_inner(bytes, args, Some(stdout.clone()), Some(stderr.clone()))?;
        Ok(CapturedOutput {
            stdout: stdout.contents().to_vec(),
            stderr: stderr.contents().to_vec(),
        })
    }

    fn run_inner(
        &self,
        bytes: &[u8],
        args: &[&str],
        stdout: Option<wasmtime_wasi::p2::pipe::MemoryOutputPipe>,
        stderr: Option<wasmtime_wasi::p2::pipe::MemoryOutputPipe>,
    ) -> Result<(), WasmRunError> {
        let module = Module::new(&self.engine, bytes)
            .map_err(|e| WasmRunError(format!("failed to compile module: {e}")))?;

        let mut builder = WasiCtxBuilder::new();
        builder.args(args);
        if let Some(pipe) = stdout {
            builder.stdout(pipe);
        } else {
            builder.inherit_stdout();
        }
        if let Some(pipe) = stderr {
            builder.stderr(pipe);
        } else {
            builder.inherit_stderr();
        }
        let wasi_ctx = builder.build_p1();

        let mut linker: Linker<WasiP1Ctx> = Linker::new(&self.engine);
        p1::add_to_linker_sync(&mut linker, |ctx| ctx)
            .map_err(|e| WasmRunError(format!("failed to set up WASI linker: {e}")))?;

        let mut store = Store::new(&self.engine, wasi_ctx);

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| WasmRunError(format!("failed to instantiate: {e}")))?;

        let func = instance
            .get_func(&mut store, "_start")
            .or_else(|| instance.get_func(&mut store, "main"))
            .ok_or_else(|| {
                WasmRunError("module has no `_start` or `main` export".to_string())
            })?;

        let result_count = func.ty(&store).results().len();
        let mut results = vec![Val::I64(0); result_count];
        match func.call(&mut store, &[], &mut results) {
            Ok(()) => Ok(()),
            Err(e) => {
                // proc_exit(0) is a clean exit — not an error.
                // proc_exit(n) for n != 0 is a non-zero exit — report the code.
                if let Some(exit) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                    if exit.0 == 0 {
                        return Ok(());
                    }
                    return Err(WasmRunError(format!(
                        "process exited with code {}",
                        exit.0
                    )));
                }
                Err(WasmRunError(format!("runtime error: {e}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WasmRunner::new() constructs without panicking (engine init).
    #[test]
    fn test_wasm_runner_new() {
        let _runner = WasmRunner::new();
    }

    /// run_wasm falls back to `main` when there is no `_start` export,
    /// even when `main` returns a value (the result is discarded).
    #[test]
    fn test_run_wasm_with_named_main_export() {
        let runner = WasmRunner::new();
        let result = runner.run_wasm(
            br#"(module
                  (func $main (result i64) i64.const 42)
                  (export "main" (func $main)))"#,
            &[],
        );
        assert!(result.is_ok(), "expected Ok from main-export module, got: {result:?}");
    }

    /// run_wasm returns Err when neither `_start` nor `main` is exported.
    #[test]
    fn test_run_missing_start_returns_err() {
        let runner = WasmRunner::new();
        let result = runner.run_wasm(b"(module)", &[]);
        assert!(result.is_err(), "expected Err for module with no entry point");
    }

    /// proc_exit(42) returns Err containing the exit code.
    #[test]
    fn test_wasi_proc_exit_nonzero() {
        let runner = WasmRunner::new();
        let result = runner.run_wasm(
            br#"(module
                  (import "wasi_snapshot_preview1" "proc_exit"
                    (func $proc_exit (param i32)))
                  (memory (export "memory") 1)
                  (func $_start
                    (call $proc_exit (i32.const 42)))
                  (export "_start" (func $_start)))"#,
            &[],
        );
        match result {
            Err(WasmRunError(msg)) => {
                assert!(
                    msg.contains("42"),
                    "error should mention exit code 42, got: {msg}"
                );
            }
            Ok(()) => panic!("proc_exit(42) should be Err, not Ok"),
        }
    }

    /// proc_exit(0) is treated as a clean exit — run_wasm returns Ok.
    #[test]
    fn test_wasi_proc_exit_zero() {
        let runner = WasmRunner::new();
        let result = runner.run_wasm(
            br#"(module
                  (import "wasi_snapshot_preview1" "proc_exit"
                    (func $proc_exit (param i32)))
                  (memory (export "memory") 1)
                  (func $_start
                    (call $proc_exit (i32.const 0)))
                  (export "_start" (func $_start)))"#,
            &[],
        );
        assert!(result.is_ok(), "proc_exit(0) should be Ok, got: {result:?}");
    }

    /// run_wasm_captured with args passes them as WASI argv; module writes
    /// the arg count as an ASCII digit to stdout.
    #[test]
    fn test_wasi_args() {
        let runner = WasmRunner::new();
        // Module calls args_sizes_get → argc in memory[0], writes "N\n" to stdout.
        // args_sizes_get(argc_out: i32, argv_buf_size_out: i32) -> i32
        let result = runner.run_wasm_captured(
            br#"(module
                  (import "wasi_snapshot_preview1" "args_sizes_get"
                    (func $args_sizes_get (param i32 i32) (result i32)))
                  (import "wasi_snapshot_preview1" "fd_write"
                    (func $fd_write (param i32 i32 i32 i32) (result i32)))
                  (memory (export "memory") 1)
                  (func $_start
                    ;; args_sizes_get(argc_out=0, argv_buf_size_out=4)
                    (drop (call $args_sizes_get (i32.const 0) (i32.const 4)))
                    ;; argc is at memory[0]; write it as ASCII digit + newline at offset 100
                    (i32.store8 (i32.const 100)
                      (i32.add (i32.load (i32.const 0)) (i32.const 48)))
                    (i32.store8 (i32.const 101) (i32.const 10))
                    ;; iov: buf=100, len=2 at offset 200
                    (i32.store (i32.const 200) (i32.const 100))
                    (i32.store (i32.const 204) (i32.const 2))
                    (drop (call $fd_write (i32.const 1) (i32.const 200) (i32.const 1) (i32.const 300))))
                  (export "_start" (func $_start)))"#,
            &["prog", "hello"],
        );
        let out = result.expect("run_wasm_captured should succeed");
        assert_eq!(out.stdout, b"2\n", "argc should be 2 for [\"prog\", \"hello\"]");
    }

    /// run_wasm_captured captures bytes written to stdout via fd_write.
    #[test]
    fn test_wasi_stdout_capture() {
        let runner = WasmRunner::new();
        // Module writes "hello\n" to fd 1 (stdout) via WASI fd_write.
        // Memory layout: bytes at 0..5, iov at 8 (buf=0, len=6).
        let result = runner.run_wasm_captured(
            br#"(module
                  (import "wasi_snapshot_preview1" "fd_write"
                    (func $fd_write (param i32 i32 i32 i32) (result i32)))
                  (memory (export "memory") 1)
                  (data (i32.const 0) "hello\n")
                  (data (i32.const 8) "\00\00\00\00\06\00\00\00")
                  (func $_start
                    (drop (call $fd_write
                      (i32.const 1)
                      (i32.const 8)
                      (i32.const 1)
                      (i32.const 16))))
                  (export "_start" (func $_start)))"#,
            &[],
        );
        let out = result.expect("run_wasm_captured should succeed");
        assert_eq!(out.stdout, b"hello\n", "stdout should contain 'hello\\n'");
    }

    /// A module that imports wasi_snapshot_preview1 functions instantiates
    /// without errors, proving the WASI linker is set up correctly.
    #[test]
    fn test_wasi_module_instantiates() {
        let runner = WasmRunner::new();
        let result = runner.run_wasm(
            br#"(module
                  (import "wasi_snapshot_preview1" "fd_write"
                    (func $fd_write (param i32 i32 i32 i32) (result i32)))
                  (memory (export "memory") 1)
                  (func $_start)
                  (export "_start" (func $_start)))"#,
            &[],
        );
        assert!(result.is_ok(), "WASI module should instantiate cleanly, got: {result:?}");
    }

    /// clock_time_get for realtime clock (id=0) returns errno 0 — clock is linked.
    #[test]
    fn test_wasi_clock_realtime() {
        let runner = WasmRunner::new();
        // Module calls clock_time_get(0, 1_000_000, time_ptr=0), converts the
        // returned errno to an ASCII digit, and writes it + newline to stdout.
        let result = runner.run_wasm_captured(
            br#"(module
                  (import "wasi_snapshot_preview1" "clock_time_get"
                    (func $clock_time_get (param i32 i64 i32) (result i32)))
                  (import "wasi_snapshot_preview1" "fd_write"
                    (func $fd_write (param i32 i32 i32 i32) (result i32)))
                  (memory (export "memory") 1)
                  (func $_start (local $errno i32)
                    (local.set $errno
                      (call $clock_time_get (i32.const 0) (i64.const 1000000) (i32.const 0)))
                    (i32.store8 (i32.const 100)
                      (i32.add (local.get $errno) (i32.const 48)))
                    (i32.store8 (i32.const 101) (i32.const 10))
                    (i32.store (i32.const 200) (i32.const 100))
                    (i32.store (i32.const 204) (i32.const 2))
                    (drop (call $fd_write
                      (i32.const 1) (i32.const 200) (i32.const 1) (i32.const 300))))
                  (export "_start" (func $_start)))
            "#,
            &[],
        );
        let out = result.expect("clock_time_get module should run without error");
        assert_eq!(out.stdout, b"0\n", "clock_time_get(realtime) should return errno 0");
    }

    /// clock_time_get for monotonic clock (id=1) returns errno 0 — clock is linked.
    #[test]
    fn test_wasi_clock_monotonic() {
        let runner = WasmRunner::new();
        let result = runner.run_wasm_captured(
            br#"(module
                  (import "wasi_snapshot_preview1" "clock_time_get"
                    (func $clock_time_get (param i32 i64 i32) (result i32)))
                  (import "wasi_snapshot_preview1" "fd_write"
                    (func $fd_write (param i32 i32 i32 i32) (result i32)))
                  (memory (export "memory") 1)
                  (func $_start (local $errno i32)
                    (local.set $errno
                      (call $clock_time_get (i32.const 1) (i64.const 1000000) (i32.const 0)))
                    (i32.store8 (i32.const 100)
                      (i32.add (local.get $errno) (i32.const 48)))
                    (i32.store8 (i32.const 101) (i32.const 10))
                    (i32.store (i32.const 200) (i32.const 100))
                    (i32.store (i32.const 204) (i32.const 2))
                    (drop (call $fd_write
                      (i32.const 1) (i32.const 200) (i32.const 1) (i32.const 300))))
                  (export "_start" (func $_start)))
            "#,
            &[],
        );
        let out = result.expect("clock_time_get module should run without error");
        assert_eq!(
            out.stdout, b"0\n",
            "clock_time_get(monotonic) should return errno 0"
        );
    }

    /// random_get fills a buffer with random bytes and returns errno 0 — random is linked.
    #[test]
    fn test_wasi_random_get() {
        let runner = WasmRunner::new();
        // Module calls random_get(buf=0, len=8), writes errno as ASCII digit to stdout.
        let result = runner.run_wasm_captured(
            br#"(module
                  (import "wasi_snapshot_preview1" "random_get"
                    (func $random_get (param i32 i32) (result i32)))
                  (import "wasi_snapshot_preview1" "fd_write"
                    (func $fd_write (param i32 i32 i32 i32) (result i32)))
                  (memory (export "memory") 1)
                  (func $_start (local $errno i32)
                    (local.set $errno (call $random_get (i32.const 0) (i32.const 8)))
                    (i32.store8 (i32.const 100)
                      (i32.add (local.get $errno) (i32.const 48)))
                    (i32.store8 (i32.const 101) (i32.const 10))
                    (i32.store (i32.const 200) (i32.const 100))
                    (i32.store (i32.const 204) (i32.const 2))
                    (drop (call $fd_write
                      (i32.const 1) (i32.const 200) (i32.const 1) (i32.const 300))))
                  (export "_start" (func $_start)))
            "#,
            &[],
        );
        let out = result.expect("random_get module should run without error");
        assert_eq!(out.stdout, b"0\n", "random_get should return errno 0");
    }

    /// run_wasm succeeds on a minimal module with a no-op `_start` export.
    #[test]
    fn test_run_empty_start() {
        let runner = WasmRunner::new();
        let result = runner.run_wasm(
            br#"(module
                  (func $_start)
                  (export "_start" (func $_start)))"#,
            &[],
        );
        assert!(result.is_ok(), "expected Ok, got: {result:?}");
    }
}
