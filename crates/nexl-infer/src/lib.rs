//! Bidirectional type inference for Nexl.

mod env;
mod infer;

pub use env::Env;
pub use infer::{
    InferState, check, check_module_performs, infer_def, infer_defn, infer_defpattern, infer_impl,
    synth, validate_module_performs,
};
