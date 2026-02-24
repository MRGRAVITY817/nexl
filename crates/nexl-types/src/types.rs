//! Core type definitions for the Nexl type system.

use std::collections::HashSet;
use std::fmt;

/// A unique identifier for a unification (type) variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeVar(pub u32);

/// A Nexl type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    // -- Primitive types (spec §5.2) --
    Int,
    Float,
    Ratio,
    Bool,
    Char,
    Str,
    Keyword,
    Symbol,
    Unit,
    Never,

    // -- Fixed-width numeric types (spec §5.2) --
    Int8,
    Int16,
    Int32,
    Int64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,

    /// Unification variable.
    Var(TypeVar),

    /// Function type: `(Fn [params...] -> ret)`.
    Fn {
        params: Vec<Type>,
        ret: Box<Type>,
    },
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => write!(f, "Int"),
            Type::Float => write!(f, "Float"),
            Type::Ratio => write!(f, "Ratio"),
            Type::Bool => write!(f, "Bool"),
            Type::Char => write!(f, "Char"),
            Type::Str => write!(f, "Str"),
            Type::Keyword => write!(f, "Keyword"),
            Type::Symbol => write!(f, "Symbol"),
            Type::Unit => write!(f, "Unit"),
            Type::Never => write!(f, "Never"),
            Type::Int8 => write!(f, "Int8"),
            Type::Int16 => write!(f, "Int16"),
            Type::Int32 => write!(f, "Int32"),
            Type::Int64 => write!(f, "Int64"),
            Type::U8 => write!(f, "U8"),
            Type::U16 => write!(f, "U16"),
            Type::U32 => write!(f, "U32"),
            Type::U64 => write!(f, "U64"),
            Type::F32 => write!(f, "F32"),
            Type::F64 => write!(f, "F64"),
            Type::Var(TypeVar(id)) => write!(f, "t{id}"),
            Type::Fn { params, ret } => {
                write!(f, "(Fn [")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, "] -> {ret})")
            }
        }
    }
}

/// Monotonically increasing source of fresh [`TypeVar`]s.
#[derive(Debug)]
pub struct TypeVarSupply {
    next: u32,
}

impl TypeVarSupply {
    /// Create a supply starting at 0.
    pub fn new() -> Self {
        Self { next: 0 }
    }

    /// Produce the next fresh type variable.
    pub fn fresh(&mut self) -> TypeVar {
        let tv = TypeVar(self.next);
        self.next += 1;
        tv
    }
}

impl Default for TypeVarSupply {
    fn default() -> Self {
        Self::new()
    }
}

/// A polymorphic type scheme: `∀ vars. body`.
///
/// `forall` lists the type variables that are universally quantified.
/// Instantiation replaces each of them with a fresh unification variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheme {
    pub forall: HashSet<TypeVar>,
    pub body: Type,
}

impl Type {
    /// Collect all type variables that appear free in this type.
    pub fn free_vars(&self) -> HashSet<TypeVar> {
        let mut result = HashSet::new();
        self.collect_free_vars(&mut result);
        result
    }

    fn collect_free_vars(&self, result: &mut HashSet<TypeVar>) {
        match self {
            Type::Var(tv) => { result.insert(*tv); }
            Type::Fn { params, ret } => {
                for p in params { p.collect_free_vars(result); }
                ret.collect_free_vars(result);
            }
            _ => {}
        }
    }
}

impl Scheme {
    /// Collect all type variables that appear free in this scheme
    /// (i.e., free in the body but not universally quantified).
    pub fn free_vars(&self) -> HashSet<TypeVar> {
        let mut vars = self.body.free_vars();
        for tv in &self.forall {
            vars.remove(tv);
        }
        vars
    }

    /// Create a monomorphic scheme (no quantified variables).
    pub fn mono(ty: Type) -> Self {
        Self {
            forall: HashSet::new(),
            body: ty,
        }
    }

    /// Instantiate this scheme by replacing each quantified variable with a
    /// fresh unification variable from `supply`.
    pub fn instantiate(&self, supply: &mut TypeVarSupply) -> Type {
        use crate::Subst;

        if self.forall.is_empty() {
            return self.body.clone();
        }

        let mut subst = Subst::empty();
        for &tv in &self.forall {
            let fresh = supply.fresh();
            subst.insert(tv, Type::Var(fresh));
        }
        subst.apply(&self.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Subst;

    // -- Test 1 --
    #[test]
    fn type_primitive_int_display() {
        assert_eq!(Type::Int.to_string(), "Int");
    }

    // -- Test 2 --
    #[test]
    fn type_primitive_all_display() {
        assert_eq!(Type::Float.to_string(), "Float");
        assert_eq!(Type::Ratio.to_string(), "Ratio");
        assert_eq!(Type::Bool.to_string(), "Bool");
        assert_eq!(Type::Char.to_string(), "Char");
        assert_eq!(Type::Str.to_string(), "Str");
        assert_eq!(Type::Keyword.to_string(), "Keyword");
        assert_eq!(Type::Symbol.to_string(), "Symbol");
        assert_eq!(Type::Unit.to_string(), "Unit");
        assert_eq!(Type::Never.to_string(), "Never");
    }

    // -- Test 3 --
    #[test]
    fn type_fixed_width_display() {
        assert_eq!(Type::Int8.to_string(), "Int8");
        assert_eq!(Type::Int16.to_string(), "Int16");
        assert_eq!(Type::Int32.to_string(), "Int32");
        assert_eq!(Type::Int64.to_string(), "Int64");
        assert_eq!(Type::U8.to_string(), "U8");
        assert_eq!(Type::U16.to_string(), "U16");
        assert_eq!(Type::U32.to_string(), "U32");
        assert_eq!(Type::U64.to_string(), "U64");
        assert_eq!(Type::F32.to_string(), "F32");
        assert_eq!(Type::F64.to_string(), "F64");
    }

    // -- Test 4 --
    #[test]
    fn type_int64_is_int_alias() {
        // Spec §5.2: "Int64 is an alias for Int … F64 is an alias for Float"
        // For M2 we represent them as distinct variants but document the alias.
        // The type checker will treat Int64 == Int and F64 == Float during unification.
        // For now just verify they are distinguishable at the representation level
        // and that the aliases are accounted for in display.
        assert_eq!(Type::Int64.to_string(), "Int64");
        assert_eq!(Type::F64.to_string(), "F64");
    }

    // -- Test 5 --
    #[test]
    fn type_var_display() {
        assert_eq!(Type::Var(TypeVar(0)).to_string(), "t0");
        assert_eq!(Type::Var(TypeVar(1)).to_string(), "t1");
        assert_eq!(Type::Var(TypeVar(42)).to_string(), "t42");
    }

    // -- Test 6 --
    #[test]
    fn type_fn_display_no_params() {
        let ty = Type::Fn {
            params: vec![],
            ret: Box::new(Type::Int),
        };
        assert_eq!(ty.to_string(), "(Fn [] -> Int)");
    }

    // -- Test 7 --
    #[test]
    fn type_fn_display_two_params() {
        let ty = Type::Fn {
            params: vec![Type::Int, Type::Str],
            ret: Box::new(Type::Bool),
        };
        assert_eq!(ty.to_string(), "(Fn [Int Str] -> Bool)");
    }

    // -- Test 8 --
    #[test]
    fn type_fn_display_nested() {
        let inner = Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Int),
        };
        let outer = Type::Fn {
            params: vec![inner],
            ret: Box::new(Type::Int),
        };
        assert_eq!(outer.to_string(), "(Fn [(Fn [Int] -> Int)] -> Int)");
    }

    // -- Test 9 --
    #[test]
    fn type_equality_same_primitives() {
        assert_eq!(Type::Int, Type::Int);
        assert_eq!(Type::Float, Type::Float);
        assert_eq!(Type::Unit, Type::Unit);
    }

    // -- Test 10 --
    #[test]
    fn type_equality_different_primitives() {
        assert_ne!(Type::Int, Type::Float);
        assert_ne!(Type::Bool, Type::Str);
        assert_ne!(Type::Unit, Type::Never);
    }

    // -- Test 11 --
    #[test]
    fn type_equality_fn() {
        let a = Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Bool),
        };
        let b = Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Bool),
        };
        let c = Type::Fn {
            params: vec![Type::Str],
            ret: Box::new(Type::Bool),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // -- Test 12 --
    #[test]
    fn type_equality_var_by_id() {
        assert_eq!(Type::Var(TypeVar(0)), Type::Var(TypeVar(0)));
        assert_ne!(Type::Var(TypeVar(0)), Type::Var(TypeVar(1)));
    }

    // -- Test 13 --
    #[test]
    fn typevar_supply_generates_unique() {
        let mut supply = TypeVarSupply::new();
        let a = supply.fresh();
        let b = supply.fresh();
        let c = supply.fresh();
        assert_eq!(a, TypeVar(0));
        assert_eq!(b, TypeVar(1));
        assert_eq!(c, TypeVar(2));
        assert_ne!(a, b);
    }

    // -- Test 14 --
    #[test]
    fn subst_empty_is_identity() {
        let s = Subst::empty();
        assert_eq!(s.apply(&Type::Int), Type::Int);
        assert_eq!(s.apply(&Type::Var(TypeVar(0))), Type::Var(TypeVar(0)));
        let fn_ty = Type::Fn {
            params: vec![Type::Bool],
            ret: Box::new(Type::Str),
        };
        assert_eq!(s.apply(&fn_ty), fn_ty);
    }

    // -- Test 15 --
    #[test]
    fn subst_replaces_matching_var() {
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Int);
        assert_eq!(s.apply(&Type::Var(TypeVar(0))), Type::Int);
    }

    // -- Test 16 --
    #[test]
    fn subst_ignores_non_matching_var() {
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Int);
        assert_eq!(s.apply(&Type::Var(TypeVar(1))), Type::Var(TypeVar(1)));
    }

    // -- Test 17 --
    #[test]
    fn subst_recurses_into_fn() {
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Int);
        let input = Type::Fn {
            params: vec![Type::Var(TypeVar(0))],
            ret: Box::new(Type::Var(TypeVar(0))),
        };
        let expected = Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Int),
        };
        assert_eq!(s.apply(&input), expected);
    }

    // -- Test 18 --
    #[test]
    fn subst_leaves_primitives_alone() {
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Int);
        assert_eq!(s.apply(&Type::Bool), Type::Bool);
        assert_eq!(s.apply(&Type::Float), Type::Float);
        assert_eq!(s.apply(&Type::Never), Type::Never);
    }

    // -- Test 19 --
    #[test]
    fn subst_compose_chains() {
        // s1: t0 → t1,  s2: t1 → Int
        // compose(s1, s2) should give t0 → Int, t1 → Int
        let mut s1 = Subst::empty();
        s1.insert(TypeVar(0), Type::Var(TypeVar(1)));
        let mut s2 = Subst::empty();
        s2.insert(TypeVar(1), Type::Int);

        s1.compose(&s2);

        assert_eq!(s1.apply(&Type::Var(TypeVar(0))), Type::Int);
        assert_eq!(s1.apply(&Type::Var(TypeVar(1))), Type::Int);
    }

    // -- Test 20 --
    #[test]
    fn scheme_instantiate_fresh_vars() {
        let mut supply = TypeVarSupply::new();
        let t0 = supply.fresh(); // TypeVar(0) — used in the scheme
        let scheme = Scheme {
            forall: [t0].into_iter().collect(),
            body: Type::Fn {
                params: vec![Type::Var(t0)],
                ret: Box::new(Type::Var(t0)),
            },
        };
        let instantiated = scheme.instantiate(&mut supply);
        // supply.fresh() inside instantiate should produce TypeVar(1)
        let expected = Type::Fn {
            params: vec![Type::Var(TypeVar(1))],
            ret: Box::new(Type::Var(TypeVar(1))),
        };
        assert_eq!(instantiated, expected);
    }

    // -- Test 22 --
    #[test]
    fn type_free_vars_primitive_is_empty() {
        assert!(Type::Int.free_vars().is_empty());
        assert!(Type::Bool.free_vars().is_empty());
        assert!(Type::Never.free_vars().is_empty());
    }

    // -- Test 23 --
    #[test]
    fn type_free_vars_var_is_singleton() {
        let vars = Type::Var(TypeVar(0)).free_vars();
        assert_eq!(vars.len(), 1);
        assert!(vars.contains(&TypeVar(0)));
    }

    // -- Test 24 --
    #[test]
    fn type_free_vars_fn_collects_all() {
        // (Fn [t0] -> t1) has free vars {t0, t1}
        let ty = Type::Fn {
            params: vec![Type::Var(TypeVar(0))],
            ret: Box::new(Type::Var(TypeVar(1))),
        };
        let vars = ty.free_vars();
        assert_eq!(vars.len(), 2);
        assert!(vars.contains(&TypeVar(0)));
        assert!(vars.contains(&TypeVar(1)));
    }

    // -- Test 25 --
    #[test]
    fn scheme_free_vars_excludes_quantified() {
        // ∀t0. (Fn [t0] -> t1) — t0 is quantified, t1 is free
        let t0 = TypeVar(0);
        let t1 = TypeVar(1);
        let scheme = Scheme {
            forall: [t0].into_iter().collect(),
            body: Type::Fn {
                params: vec![Type::Var(t0)],
                ret: Box::new(Type::Var(t1)),
            },
        };
        let free = scheme.free_vars();
        assert!(!free.contains(&t0), "t0 is quantified, not free");
        assert!(free.contains(&t1), "t1 is free");
    }

    // -- Test 21 --
    #[test]
    fn scheme_monomorphic_no_change() {
        let mut supply = TypeVarSupply::new();
        let scheme = Scheme::mono(Type::Int);
        assert_eq!(scheme.instantiate(&mut supply), Type::Int);
        // supply should not have been consumed
        assert_eq!(supply.fresh(), TypeVar(0));
    }
}
