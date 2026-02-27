//! Type representation for the Nexl compiler.

pub mod resource;
mod subst;
mod types;
pub mod unify;

pub use resource::{ResourceLifecycleError, ResourceState, ResourceTracker};
pub use subst::Subst;
pub use types::{
    Constructor, EffectDef, EffectOpDef, EffectRow, ProtocolDef, ProtocolOpDef, Scheme, Type,
    TypeDef, TypeVar, TypeVarSupply,
};
pub use unify::{TypeError, TypeErrorKind, unify};
