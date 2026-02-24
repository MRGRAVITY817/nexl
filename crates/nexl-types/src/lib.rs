//! Type representation for the Nexl compiler.

mod subst;
mod types;

pub use subst::Subst;
pub use types::{Scheme, Type, TypeVar, TypeVarSupply};
