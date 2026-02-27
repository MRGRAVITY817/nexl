//! WASM code generation for the Nexl compiler.
//!
//! Walks a [`nexl_ir::Module`] (ANF IR) and emits a binary WebAssembly core
//! module using the [`wasm_encoder`] crate. Also provides WIT interface
//! generation for the WASM Component Model (spec §15.1).
//!
//! Pipeline position: nexl-ir → **nexl-wasm** → `.wasm` file

pub mod canonical_abi;
mod emit;
pub mod marshal;
pub mod wit;

pub use canonical_abi::{CanonicalAbiError, WasmValType, canonical_alignment, canonical_size, flatten_params, wasm_valtype};
pub use emit::{EmitError, Emitter};
pub use marshal::{MarshalError, MarshalStrategy, c_function_decl, c_type_name, marshal_strategy};
pub use wit::{WitError, WitResource, effect_to_wit_interface, fn_type_to_wit, generate_wit_interface, generate_wit_interface_full, generate_wit_resource, type_to_wit, wit_interface_to_effect};
