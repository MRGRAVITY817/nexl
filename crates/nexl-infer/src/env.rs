//! Typing environment: maps variable names to polymorphic type schemes.

use std::collections::{HashMap, HashSet};

use nexl_types::{Scheme, Subst, TypeVar};

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

    /// Collect all type variables that are free in this environment after
    /// applying `subst`.
    ///
    /// A variable is "free in the environment" if it appears in some scheme's
    /// body and is not quantified by that scheme's `forall`.  Such variables
    /// must not be generalized by a `let` binding because they are constrained
    /// by an outer context.
    pub fn free_vars(&self, subst: &Subst) -> HashSet<TypeVar> {
        let mut result = HashSet::new();
        for scheme in self.bindings.values() {
            let applied_body = subst.apply(&scheme.body);
            for tv in applied_body.free_vars() {
                if !scheme.forall.contains(&tv) {
                    result.insert(tv);
                }
            }
        }
        result
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

    // -- Test 5 --
    #[test]
    fn env_free_vars_empty_env() {
        use nexl_types::Subst;
        let env = Env::new();
        assert!(env.free_vars(&Subst::empty()).is_empty());
    }

    // -- Test 6 --
    #[test]
    fn env_free_vars_mono_concrete_is_empty() {
        use nexl_types::Subst;
        // x : Int — Int has no free vars
        let env = Env::new().extend("x", Scheme::mono(Type::Int));
        assert!(env.free_vars(&Subst::empty()).is_empty());
    }

    // -- Test 7 --
    #[test]
    fn env_free_vars_mono_var_is_reported() {
        use nexl_types::{Subst, TypeVar};
        // x : t0 (unresolved type var) — t0 is free in the env
        let t0 = TypeVar(0);
        let env = Env::new().extend("x", Scheme::mono(Type::Var(t0)));
        let free = env.free_vars(&Subst::empty());
        assert!(free.contains(&t0), "t0 must be free in the env");
    }

    // -- Test 8 --
    #[test]
    fn env_free_vars_quantified_not_reported() {
        use nexl_types::{Subst, TypeVar};
        // ∀t0. (Fn [t0] -> t0) — t0 is quantified, not free
        let t0 = TypeVar(0);
        let scheme = Scheme {
            forall: [t0].into_iter().collect(),
            body: Type::Fn {
                params: vec![Type::Var(t0)],
                ret: Box::new(Type::Var(t0)),
            },
        };
        let env = Env::new().extend("id", scheme);
        assert!(env.free_vars(&Subst::empty()).is_empty(), "quantified var must not be free");
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
