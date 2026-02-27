//! Optimization passes for the ANF IR.
//!
//! Each pass takes a [`Module`](crate::Module) and returns a transformed `Module`.
//! Passes are designed to be composed in sequence.

pub mod const_fold;
pub mod dce;
pub mod escape;
pub mod inline;
pub mod reuse;
pub mod specialize;

use crate::Module;

/// Run all optimization passes in the standard order.
///
/// Order: inline → constant fold → dead code elimination.
pub fn optimize(module: &Module) -> Module {
    let m = inline::inline_calls(module);
    let m = const_fold::fold_constants(&m);
    dce::eliminate_dead_code(&m)
}
