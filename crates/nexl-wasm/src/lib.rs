//! WASM code generation for the Nexl compiler.
//!
//! Walks a [`nexl_ir::Module`] (ANF IR) and emits a binary WebAssembly core
//! module using the [`wasm_encoder`] crate. Also provides WIT interface
//! generation for the WASM Component Model (spec §15.1).
//!
//! Pipeline position: nexl-ir → **nexl-wasm** → `.wasm` file

pub mod canonical_abi;
pub mod composition;
pub mod effect_wasi;
mod emit;
pub mod gc_emit;
pub mod marshal;
pub mod wit;
pub mod wit_export;
pub mod wit_import;
pub mod wasi3;

pub use canonical_abi::{
    CanonicalAbiError, WasmValType, canonical_alignment, canonical_size, flatten_params,
    wasm_valtype,
};
pub use emit::{EmitError, Emitter};
pub use marshal::{MarshalError, MarshalStrategy, c_function_decl, c_type_name, marshal_strategy};
pub use wit::{
    WitError, WitResource, effect_to_wit_interface, fn_type_to_wit, generate_wit_interface,
    generate_wit_interface_full, generate_wit_resource, type_to_wit, wit_interface_to_effect,
};
pub use effect_wasi::{
    CapabilityViolation, EffectWasiEntry, EFFECT_WASI_MAP, effect_to_wasi_interface,
    effects_to_wasm_imports, validate_wasi_capabilities, wasi_interface_effects,
};
pub use wit_export::{
    WitExportFn, WitImportIface, WitModuleExport, generate_wit_world, nexl_type_to_wit,
    record_type_def, variant_type_def,
};
pub use wit_import::{
    WitFn, WitImportBinding, WitImportError, WitInterface, WitParam, WitResourceDef, WitType,
    parse_wit_interface, wit_interface_to_bindings, wit_type_to_nexl,
};
