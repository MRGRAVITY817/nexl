//! Typing environment: maps variable names to polymorphic type schemes.

use std::collections::{HashMap, HashSet};

use nexl_types::{Constructor, Scheme, Subst, Type, TypeDef, TypeVar};

/// A record type definition: name, type parameters, and fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordDef {
    /// Record type name (e.g. `"Point"`).
    pub name: String,
    /// Ordered list of universally-quantified type parameters.
    pub params: Vec<TypeVar>,
    /// Named fields and their types.
    pub fields: Vec<(String, Type)>,
}

/// A constructor definition, paired with its parent type name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CtorDef {
    /// The parent type name (e.g. `"Option"`).
    pub type_name: String,
    /// The constructor itself.
    pub ctor: Constructor,
}

/// The typing environment: maps names to polymorphic type schemes.
///
/// `Env` is designed to be cheap to extend — `extend` clones the map and
/// inserts a new binding, shadowing any prior binding with the same name.
/// The original `Env` is left unchanged.
#[derive(Debug, Clone, Default)]
pub struct Env {
    bindings: HashMap<String, Scheme>,
    type_defs: HashMap<String, TypeDef>,
    record_defs: HashMap<String, RecordDef>,
    constructors: HashMap<String, CtorDef>,
    module_bindings: HashMap<String, HashMap<String, Scheme>>,
}

impl Env {
    /// A base environment with built-in ADTs.
    pub fn new() -> Self {
        Self::default().with_builtins()
    }

    fn with_builtins(self) -> Self {
        let option_t0 = TypeVar(0);
        let option = TypeDef {
            name: "Option".to_string(),
            params: vec![option_t0],
            constructors: vec![
                Constructor::nullary("None"),
                Constructor::nary("Some", vec![Type::Var(option_t0)]),
            ],
        };
        let result_t0 = TypeVar(0);
        let result_t1 = TypeVar(1);
        let result = TypeDef {
            name: "Result".to_string(),
            params: vec![result_t0, result_t1],
            constructors: vec![
                Constructor::nary("Ok", vec![Type::Var(result_t0)]),
                Constructor::nary("Err", vec![Type::Var(result_t1)]),
            ],
        };
        self.extend_type_def(option).extend_type_def(result)
    }

    /// Return a new environment that extends `self` with `name` bound to
    /// `scheme`.  Any previous binding of `name` is shadowed.
    pub fn extend(&self, name: impl Into<String>, scheme: Scheme) -> Self {
        let mut bindings = self.bindings.clone();
        bindings.insert(name.into(), scheme);
        Self {
            bindings,
            type_defs: self.type_defs.clone(),
            record_defs: self.record_defs.clone(),
            constructors: self.constructors.clone(),
            module_bindings: self.module_bindings.clone(),
        }
    }

    /// Look up `name` in the environment.
    pub fn lookup(&self, name: &str) -> Option<&Scheme> {
        self.bindings.get(name)
    }

    /// Extend the environment with an imported module alias.
    pub fn extend_module(
        &self,
        alias: impl Into<String>,
        exports: HashMap<String, Scheme>,
    ) -> Self {
        let mut module_bindings = self.module_bindings.clone();
        module_bindings.insert(alias.into(), exports);
        Self {
            bindings: self.bindings.clone(),
            type_defs: self.type_defs.clone(),
            record_defs: self.record_defs.clone(),
            constructors: self.constructors.clone(),
            module_bindings,
        }
    }

    /// Look up a qualified name `alias/name` in imported module bindings.
    pub fn lookup_qualified(&self, alias: &str, name: &str) -> Option<&Scheme> {
        self.module_bindings
            .get(alias)
            .and_then(|exports| exports.get(name))
    }

    /// Return a new environment with `typedef` added, and its constructors registered.
    pub fn extend_type_def(&self, typedef: TypeDef) -> Self {
        let mut bindings = self.bindings.clone();
        let mut type_defs = self.type_defs.clone();
        let mut constructors = self.constructors.clone();
        let type_name = typedef.name.clone();
        let adt_args: Vec<Type> = typedef.params.iter().map(|tv| Type::Var(*tv)).collect();
        let mut forall = HashSet::new();
        for tv in &typedef.params {
            forall.insert(*tv);
        }
        for ctor in &typedef.constructors {
            constructors.insert(
                ctor.name.clone(),
                CtorDef {
                    type_name: type_name.clone(),
                    ctor: ctor.clone(),
                },
            );
            let body = if ctor.fields.is_empty() {
                Type::Adt {
                    name: type_name.clone(),
                    args: adt_args.clone(),
                }
            } else {
                Type::Fn {
                    params: ctor.fields.clone(),
                    ret: Box::new(Type::Adt {
                        name: type_name.clone(),
                        args: adt_args.clone(),
                    }),
                }
            };
            bindings.insert(
                ctor.name.clone(),
                Scheme {
                    forall: forall.clone(),
                    body,
                },
            );
        }
        type_defs.insert(type_name, typedef);
        Self {
            bindings,
            type_defs,
            record_defs: self.record_defs.clone(),
            constructors,
            module_bindings: self.module_bindings.clone(),
        }
    }

    /// Return a new environment with `record` added.
    pub fn extend_record_def(&self, record: RecordDef) -> Self {
        let mut record_defs = self.record_defs.clone();
        let mut bindings = self.bindings.clone();
        let mut forall = HashSet::new();
        for tv in &record.params {
            forall.insert(*tv);
        }
        let record_ty = Type::Record {
            name: record.name.clone(),
            fields: record.fields.clone(),
        };
        bindings.insert(
            record.name.clone(),
            Scheme {
                forall,
                body: Type::Fn {
                    params: vec![record_ty.clone()],
                    ret: Box::new(record_ty),
                },
            },
        );
        record_defs.insert(record.name.clone(), record);
        Self {
            bindings,
            type_defs: self.type_defs.clone(),
            record_defs,
            constructors: self.constructors.clone(),
            module_bindings: self.module_bindings.clone(),
        }
    }

    /// Look up a type definition by name.
    pub fn lookup_type_def(&self, name: &str) -> Option<&TypeDef> {
        self.type_defs.get(name)
    }

    /// Look up a record definition by name.
    pub fn lookup_record_def(&self, name: &str) -> Option<&RecordDef> {
        self.record_defs.get(name)
    }

    /// Look up a constructor definition by constructor name.
    pub fn lookup_ctor(&self, name: &str) -> Option<&CtorDef> {
        self.constructors.get(name)
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
        for exports in self.module_bindings.values() {
            for scheme in exports.values() {
                let applied_body = subst.apply(&scheme.body);
                for tv in applied_body.free_vars() {
                    if !scheme.forall.contains(&tv) {
                        result.insert(tv);
                    }
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use nexl_types::{Constructor, Scheme, Type, TypeDef, TypeVar};

    use super::{Env, RecordDef};

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
    fn env_new_includes_option_and_result() {
        let env = Env::new();
        assert!(
            env.lookup_type_def("Option").is_some(),
            "Option type should be built-in"
        );
        assert!(
            env.lookup_type_def("Result").is_some(),
            "Result type should be built-in"
        );
        assert!(
            env.lookup_ctor("None").is_some(),
            "None ctor should be built-in"
        );
        assert!(
            env.lookup_ctor("Some").is_some(),
            "Some ctor should be built-in"
        );
        assert!(
            env.lookup_ctor("Ok").is_some(),
            "Ok ctor should be built-in"
        );
        assert!(
            env.lookup_ctor("Err").is_some(),
            "Err ctor should be built-in"
        );
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
        assert!(
            env.free_vars(&Subst::empty()).is_empty(),
            "quantified var must not be free"
        );
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

    // -- Test 9 --
    #[test]
    fn env_extend_type_def_registers_constructors() {
        let t0 = TypeVar(0);
        let td = TypeDef {
            name: "Option".to_string(),
            params: vec![t0],
            constructors: vec![
                Constructor::nullary("None"),
                Constructor::nary("Some", vec![Type::Var(t0)]),
            ],
        };
        let env = Env::new().extend_type_def(td.clone());
        assert_eq!(env.lookup_type_def("Option"), Some(&td));
        let some = env
            .lookup_ctor("Some")
            .expect("Some constructor should be registered");
        assert_eq!(some.type_name, "Option");
        assert_eq!(some.ctor, Constructor::nary("Some", vec![Type::Var(t0)]));
    }

    // -- Test 10 --
    #[test]
    fn env_extend_record_def_registers_record() {
        let rec = RecordDef {
            name: "Point".to_string(),
            params: vec![],
            fields: vec![("x".to_string(), Type::Float)],
        };
        let env = Env::new().extend_record_def(rec.clone());
        assert_eq!(env.lookup_record_def("Point"), Some(&rec));
    }

    // -- Test 11 --
    #[test]
    fn env_extend_type_def_registers_nullary_constructor_scheme() {
        let t0 = TypeVar(0);
        let td = TypeDef {
            name: "Option".to_string(),
            params: vec![t0],
            constructors: vec![
                Constructor::nullary("None"),
                Constructor::nary("Some", vec![Type::Var(t0)]),
            ],
        };
        let env = Env::new().extend_type_def(td);
        let scheme = env.lookup("None").expect("None should be bound in env");
        assert!(
            scheme.forall.contains(&t0),
            "None should quantify its type param"
        );
        assert_eq!(
            scheme.body,
            Type::Adt {
                name: "Option".to_string(),
                args: vec![Type::Var(t0)]
            }
        );
    }

    // -- Test 12 --
    #[test]
    fn env_extend_type_def_registers_nary_constructor_scheme() {
        let t0 = TypeVar(0);
        let td = TypeDef {
            name: "Option".to_string(),
            params: vec![t0],
            constructors: vec![Constructor::nary("Some", vec![Type::Var(t0)])],
        };
        let env = Env::new().extend_type_def(td);
        let scheme = env.lookup("Some").expect("Some should be bound in env");
        assert!(
            scheme.forall.contains(&t0),
            "Some should quantify its type param"
        );
        assert_eq!(
            scheme.body,
            Type::Fn {
                params: vec![Type::Var(t0)],
                ret: Box::new(Type::Adt {
                    name: "Option".to_string(),
                    args: vec![Type::Var(t0)],
                }),
            }
        );
    }

    // -- Test 13 --
    #[test]
    fn env_extend_record_def_registers_constructor_scheme() {
        let rec = RecordDef {
            name: "Point".to_string(),
            params: vec![],
            fields: vec![
                ("x".to_string(), Type::Float),
                ("y".to_string(), Type::Float),
            ],
        };
        let env = Env::new().extend_record_def(rec);
        let scheme = env.lookup("Point").expect("Point should be bound in env");
        assert!(
            scheme.forall.is_empty(),
            "record constructor should be monomorphic here"
        );
        let record_ty = Type::Record {
            name: "Point".to_string(),
            fields: vec![
                ("x".to_string(), Type::Float),
                ("y".to_string(), Type::Float),
            ],
        };
        assert_eq!(
            scheme.body,
            Type::Fn {
                params: vec![record_ty.clone()],
                ret: Box::new(record_ty),
            }
        );
    }
}
