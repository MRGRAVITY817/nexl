//! Type representation for the Nexl compiler.

mod subst;
mod types;
pub mod unify;

pub use subst::Subst;
pub use types::{Constructor, Scheme, Type, TypeDef, TypeVar, TypeVarSupply};
pub use unify::{TypeError, TypeErrorKind, unify};
