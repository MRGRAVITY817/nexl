//! WASM code generation for the Nexl compiler.
//!
//! Walks a [`nexl_ir::Module`] (ANF IR) and emits a binary WebAssembly core
//! module using the [`wasm_encoder`] crate.
//!
//! Pipeline position: nexl-ir → **nexl-wasm** → `.wasm` file

mod emit;

pub use emit::{EmitError, Emitter};
