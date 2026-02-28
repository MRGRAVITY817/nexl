//! Wasmtime-based WASM execution for `nexl run --wasm`.
//!
//! [`WasmRunner`] compiles WASM bytes with a wasmtime [`Engine`] and
//! calls the module's entry point (`_start` or `main`).

use wasmtime::{Engine, Linker, Module, Store, Val};

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

    /// Compile and execute WASM `bytes`.
    ///
    /// Tries `_start` first (WASI convention), then falls back to `main`.
    /// Returns `Err` if neither export is found or if execution traps.
    pub fn run_wasm(&self, bytes: &[u8]) -> Result<(), WasmRunError> {
        let module = Module::new(&self.engine, bytes)
            .map_err(|e| WasmRunError(format!("failed to compile module: {e}")))?;

        let linker: Linker<()> = Linker::new(&self.engine);
        let mut store = Store::new(&self.engine, ());

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
        func.call(&mut store, &[], &mut results)
            .map_err(|e| WasmRunError(format!("runtime error: {e}")))?;

        Ok(())
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
        );
        assert!(result.is_ok(), "expected Ok from main-export module, got: {result:?}");
    }

    /// run_wasm returns Err when neither `_start` nor `main` is exported.
    #[test]
    fn test_run_missing_start_returns_err() {
        let runner = WasmRunner::new();
        let result = runner.run_wasm(b"(module)");
        assert!(result.is_err(), "expected Err for module with no entry point");
    }

    /// run_wasm succeeds on a minimal module with a no-op `_start` export.
    #[test]
    fn test_run_empty_start() {
        let runner = WasmRunner::new();
        let result = runner.run_wasm(
            br#"(module
                  (func $_start)
                  (export "_start" (func $_start)))"#,
        );
        assert!(result.is_ok(), "expected Ok, got: {result:?}");
    }
}
