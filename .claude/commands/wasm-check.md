---
model: sonnet
---

Compile a Nexl program to WASM and validate the output.

## Arguments
$ARGUMENTS — A .nx file path to compile, or "all" to check all examples.

## Instructions

1. If "all", glob for `examples/*.nx` and `tests/e2e/fixtures/*.nx`. Otherwise use the given file.
2. For each file:
   a. Compile to WASM: `cargo run -- compile --target wasm <file>`
   b. If compilation succeeds, validate the .wasm output:
      - Check it exists and is non-empty
      - Run `wasm-tools validate <output.wasm>` if wasm-tools is available
      - Run with wasmtime if available: `wasmtime <output.wasm>`
   c. Report: file name, compile status, validation status, runtime status
3. Summarize: N files compiled, N validated, N ran successfully, N errors.
4. If wasm-tools or wasmtime are not installed, note this and skip those steps.
5. Do NOT fix compilation errors automatically. Report them for the user.
