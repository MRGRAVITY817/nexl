//! Typing environment: maps variable names to polymorphic type schemes.

use std::collections::HashMap;

use nexl_types::Scheme;

/// The typing environment: maps names to polymorphic type schemes.
///
/// `Env` is designed to be cheap to extend — `extend` clones the map and
/// inserts a new binding, shadowing any prior binding with the same name.
/// The original `Env` is left unchanged.
#[derive(Debug, Clone, Default)]
pub struct Env {
    bindings: HashMap<String, Scheme>,
}

impl Env {
    /// An empty environment with no bindings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return a new environment that extends `self` with `name` bound to
    /// `scheme`.  Any previous binding of `name` is shadowed.
    pub fn extend(&self, name: impl Into<String>, scheme: Scheme) -> Self {
        let mut bindings = self.bindings.clone();
        bindings.insert(name.into(), scheme);
        Self { bindings }
    }

    /// Look up `name` in the environment.
    pub fn lookup(&self, name: &str) -> Option<&Scheme> {
        self.bindings.get(name)
    }
}

#[cfg(test)]
mod tests {
    use nexl_types::{Scheme, Type};

    use super::Env;

    // -- Test 1 --
    #[test]
    fn env_empty_lookup_is_none() {
        let env = Env::new();
        assert!(env.lookup("x").is_none());
    }

    // -- Test 2 --
    #[test]
    fn env_extend_lookup() {
        let env = Env::new().extend("x", Scheme::mono(Type::Int));
        assert_eq!(env.lookup("x").unwrap().body, Type::Int);
    }

    // -- Test 3 --
    #[test]
    fn env_extend_shadows() {
        let env = Env::new()
            .extend("x", Scheme::mono(Type::Int))
            .extend("x", Scheme::mono(Type::Bool));
        assert_eq!(env.lookup("x").unwrap().body, Type::Bool);
    }

    // -- Test 4 --
    #[test]
    fn env_original_unchanged_after_extend() {
        let base = Env::new().extend("x", Scheme::mono(Type::Int));
        let _child = base.extend("y", Scheme::mono(Type::Bool));
        // base must still have x:Int and no y
        assert_eq!(base.lookup("x").unwrap().body, Type::Int);
        assert!(base.lookup("y").is_none());
    }
}
