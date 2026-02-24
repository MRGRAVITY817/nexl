//! Bidirectional type inference for Nexl.

mod env;
mod infer;

pub use env::Env;
pub use infer::{InferState, synth};
