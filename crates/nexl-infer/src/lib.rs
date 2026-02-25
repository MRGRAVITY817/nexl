//! Bidirectional type inference for Nexl.

mod env;
mod infer;

pub use env::Env;
pub use infer::{InferState, check, infer_def, infer_defn, synth, validate_module_performs};
