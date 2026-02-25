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
        effects: EffectRow,
    },

    /// Applied algebraic data type: a named type constructor applied to zero
    /// or more type arguments.
    ///
    /// - `Color` → `Adt { name: "Color", args: [] }`
    /// - `(Option Int)` → `Adt { name: "Option", args: [Int] }`
    /// - `(Result Int Str)` → `Adt { name: "Result", args: [Int, Str] }`
    ///
    /// (spec §5.7)
    Adt {
        name: String,
        args: Vec<Type>,
    },

    /// Nominal record type with named fields (spec §5.7).
    Record {
        name: String,
        fields: Vec<(String, Type)>,
    },

    /// Tuple type (spec §5.3), 2–8 elements.
    Tuple(Vec<Type>),

    /// Persistent vector type: `(Vec a)` (spec §5.3).
    Vec(Box<Type>),

    /// Persistent map type: `(Map k v)` (spec §5.3).
    Map {
        key: Box<Type>,
        val: Box<Type>,
    },

    /// Persistent set type: `(Set a)` (spec §5.3).
    Set(Box<Type>),
}

/// A row of effects on a function type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRow {
    /// Named effects in the row, e.g. `["Console", "Net"]`.
    pub effects: Vec<String>,
    /// Optional row variable name (e.g. `r` in `[Console | r]`).
    pub tail: Option<String>,
}

impl EffectRow {
    /// Construct an effect row, sorting and deduplicating effects.
    pub fn new(mut effects: Vec<String>, tail: Option<String>) -> Self {
        effects.sort();
        effects.dedup();
        Self { effects, tail }
    }

    /// An empty effect row: `! []`.
    pub fn empty() -> Self {
        Self::new(Vec::new(), None)
    }

    /// Returns `true` when this row has no effects and no tail variable.
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty() && self.tail.is_none()
    }
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
            Type::Fn {
                params,
                ret,
                effects,
            } => {
                write!(f, "(Fn [")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, "] -> {ret}")?;
                if !effects.is_empty() {
                    write!(f, " ! [")?;
                    for (i, eff) in effects.effects.iter().enumerate() {
                        if i > 0 {
                            write!(f, " ")?;
                        }
                        write!(f, "{eff}")?;
                    }
                    if let Some(tail) = &effects.tail {
                        if !effects.effects.is_empty() {
                            write!(f, " ")?;
                        }
                        write!(f, "| {tail}")?;
                    }
                    write!(f, "]")?;
                }
                write!(f, ")")
            }
            Type::Adt { name, args } => {
                if args.is_empty() {
                    write!(f, "{name}")
                } else {
                    write!(f, "({name}")?;
                    for arg in args {
                        write!(f, " {arg}")?;
                    }
                    write!(f, ")")
                }
            }
            Type::Record { name, .. } => write!(f, "{name}"),
            Type::Tuple(items) => {
                write!(f, "(Tuple")?;
                for item in items {
                    write!(f, " {item}")?;
                }
                write!(f, ")")
            }
            Type::Vec(elem) => write!(f, "(Vec {elem})"),
            Type::Map { key, val } => write!(f, "(Map {key} {val})"),
            Type::Set(elem) => write!(f, "(Set {elem})"),
        }
    }
}

/// A single constructor in an ADT definition.
///
/// - Nullary: `Red` in `(deftype Color | Red | Green | Blue)` — `fields` is empty.
/// - N-ary: `Some` in `(deftype Option [a] | (Some a))` — `fields` has one entry.
///
/// Field types are expressed relative to the type definition's parameter list
/// and may contain `Type::Var` entries that correspond to the `TypeDef::params`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constructor {
    /// Constructor name (e.g. `"Red"`, `"Some"`, `"Ok"`).
    pub name: String,
    /// Positional field types.  Empty for nullary constructors.
    pub fields: Vec<Type>,
}

impl Constructor {
    /// Create a nullary constructor (no fields).
    pub fn nullary(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: vec![],
        }
    }

    /// Create an n-ary constructor with the given field types.
    pub fn nary(name: impl Into<String>, fields: Vec<Type>) -> Self {
        Self {
            name: name.into(),
            fields,
        }
    }
}

/// A type definition: name, type parameters, and a list of constructors.
///
/// Represents the result of parsing a `deftype` form (spec §5.7):
/// - `(deftype Color | Red | Green | Blue)` — three nullary constructors, no params.
/// - `(deftype Option [a] | None | (Some a))` — one param `a`, two constructors.
/// - `(deftype Result [a e] | (Ok a) | (Err e))` — two params, two constructors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDef {
    /// The declared type name (e.g. `"Color"`, `"Option"`).
    pub name: String,
    /// Ordered list of universally-quantified type parameters.
    /// Each entry is a `TypeVar` that may appear in constructor field types.
    pub params: Vec<TypeVar>,
    /// The constructors, in declaration order.
    pub constructors: Vec<Constructor>,
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
            Type::Var(tv) => {
                result.insert(*tv);
            }
            Type::Fn { params, ret, .. } => {
                for p in params {
                    p.collect_free_vars(result);
                }
                ret.collect_free_vars(result);
            }
            Type::Adt { args, .. } => {
                for arg in args {
                    arg.collect_free_vars(result);
                }
            }
            Type::Record { fields, .. } => {
                for (_, field_ty) in fields {
                    field_ty.collect_free_vars(result);
                }
            }
            Type::Tuple(items) => {
                for item in items {
                    item.collect_free_vars(result);
                }
            }
            Type::Vec(elem) => elem.collect_free_vars(result),
            Type::Map { key, val } => {
                key.collect_free_vars(result);
                val.collect_free_vars(result);
            }
            Type::Set(elem) => elem.collect_free_vars(result),
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
            let mut fresh = supply.fresh();
            while fresh == tv {
                fresh = supply.fresh();
            }
            subst.insert(tv, Type::Var(fresh));
        }
        subst.apply(&self.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Subst;

    // -----------------------------------------------------------------------
    // ADT type tests
    // -----------------------------------------------------------------------

    // -- ADT Test 1 --
    #[test]
    fn adt_construction_no_args() {
        let ty = Type::Adt {
            name: "Color".to_string(),
            args: vec![],
        };
        assert_eq!(
            ty,
            Type::Adt {
                name: "Color".to_string(),
                args: vec![]
            }
        );
    }

    // -- ADT Test 2 --
    #[test]
    fn adt_construction_with_args() {
        let ty = Type::Adt {
            name: "Option".to_string(),
            args: vec![Type::Int],
        };
        assert_eq!(
            ty,
            Type::Adt {
                name: "Option".to_string(),
                args: vec![Type::Int]
            }
        );
    }

    // -- ADT Test 3 --
    #[test]
    fn adt_display_no_args() {
        let ty = Type::Adt {
            name: "Color".to_string(),
            args: vec![],
        };
        assert_eq!(ty.to_string(), "Color");
    }

    // -- ADT Test 4 --
    #[test]
    fn adt_display_one_arg() {
        let ty = Type::Adt {
            name: "Option".to_string(),
            args: vec![Type::Int],
        };
        assert_eq!(ty.to_string(), "(Option Int)");
    }

    // -- ADT Test 5 --
    #[test]
    fn adt_display_two_args() {
        let ty = Type::Adt {
            name: "Result".to_string(),
            args: vec![Type::Int, Type::Str],
        };
        assert_eq!(ty.to_string(), "(Result Int Str)");
    }

    // -- ADT Test 6 --
    #[test]
    fn adt_display_nested() {
        let inner = Type::Adt {
            name: "Option".to_string(),
            args: vec![Type::Int],
        };
        let outer = Type::Adt {
            name: "Option".to_string(),
            args: vec![inner],
        };
        assert_eq!(outer.to_string(), "(Option (Option Int))");
    }

    // -- ADT Test 7 --
    #[test]
    fn adt_equality_same() {
        let a = Type::Adt {
            name: "Color".to_string(),
            args: vec![],
        };
        let b = Type::Adt {
            name: "Color".to_string(),
            args: vec![],
        };
        assert_eq!(a, b);
    }

    // -- ADT Test 8 --
    #[test]
    fn adt_equality_different_name() {
        let a = Type::Adt {
            name: "Color".to_string(),
            args: vec![],
        };
        let b = Type::Adt {
            name: "Shape".to_string(),
            args: vec![],
        };
        assert_ne!(a, b);
    }

    // -- ADT Test 9 --
    #[test]
    fn adt_equality_different_args() {
        let a = Type::Adt {
            name: "Option".to_string(),
            args: vec![Type::Int],
        };
        let b = Type::Adt {
            name: "Option".to_string(),
            args: vec![Type::Str],
        };
        assert_ne!(a, b);
    }

    // -- ADT Test 10 --
    #[test]
    fn adt_free_vars_concrete() {
        // (Option Int) has no free type variables.
        let ty = Type::Adt {
            name: "Option".to_string(),
            args: vec![Type::Int],
        };
        assert!(ty.free_vars().is_empty());
    }

    // -- ADT Test 11 --
    #[test]
    fn adt_free_vars_with_var() {
        // (Option t0) has free var {t0}.
        let ty = Type::Adt {
            name: "Option".to_string(),
            args: vec![Type::Var(TypeVar(0))],
        };
        let fvs = ty.free_vars();
        assert_eq!(fvs.len(), 1);
        assert!(fvs.contains(&TypeVar(0)));
    }

    // -- ADT Test 12 --
    #[test]
    fn adt_free_vars_multiple() {
        // (Result t0 t1) has free vars {t0, t1}.
        let ty = Type::Adt {
            name: "Result".to_string(),
            args: vec![Type::Var(TypeVar(0)), Type::Var(TypeVar(1))],
        };
        let fvs = ty.free_vars();
        assert_eq!(fvs.len(), 2);
        assert!(fvs.contains(&TypeVar(0)));
        assert!(fvs.contains(&TypeVar(1)));
    }

    // -- ADT Test 13 --
    #[test]
    fn subst_apply_adt() {
        // subst {t0→Int} applied to (Option t0) → (Option Int)
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Int);
        let ty = Type::Adt {
            name: "Option".to_string(),
            args: vec![Type::Var(TypeVar(0))],
        };
        let result = s.apply(&ty);
        assert_eq!(
            result,
            Type::Adt {
                name: "Option".to_string(),
                args: vec![Type::Int]
            }
        );
    }

    // -- ADT Test 14 --
    #[test]
    fn subst_apply_adt_no_match() {
        // subst {t0→Int} leaves (Option t1) unchanged.
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Int);
        let ty = Type::Adt {
            name: "Option".to_string(),
            args: vec![Type::Var(TypeVar(1))],
        };
        let result = s.apply(&ty);
        assert_eq!(
            result,
            Type::Adt {
                name: "Option".to_string(),
                args: vec![Type::Var(TypeVar(1))]
            }
        );
    }

    // -----------------------------------------------------------------------
    // TypeDef and Constructor tests
    // -----------------------------------------------------------------------

    // -- ADT Test 21 --
    #[test]
    fn typedef_simple_enum() {
        // (deftype Color | Red | Green | Blue)
        let td = TypeDef {
            name: "Color".to_string(),
            params: vec![],
            constructors: vec![
                Constructor::nullary("Red"),
                Constructor::nullary("Green"),
                Constructor::nullary("Blue"),
            ],
        };
        assert_eq!(td.name, "Color");
        assert!(td.params.is_empty());
        assert_eq!(td.constructors.len(), 3);
        assert_eq!(td.constructors[0].name, "Red");
        assert_eq!(td.constructors[1].name, "Green");
        assert_eq!(td.constructors[2].name, "Blue");
    }

    // -- ADT Test 22 --
    #[test]
    fn typedef_parameterized() {
        // (deftype Option [a] | None | (Some a))
        // params: [t0], constructors: [None(nullary), Some(fields: [t0])]
        let t0 = TypeVar(0);
        let td = TypeDef {
            name: "Option".to_string(),
            params: vec![t0],
            constructors: vec![
                Constructor::nullary("None"),
                Constructor::nary("Some", vec![Type::Var(t0)]),
            ],
        };
        assert_eq!(td.name, "Option");
        assert_eq!(td.params, vec![t0]);
        assert_eq!(td.constructors.len(), 2);
        assert_eq!(td.constructors[0].name, "None");
        assert!(td.constructors[0].fields.is_empty());
        assert_eq!(td.constructors[1].name, "Some");
        assert_eq!(td.constructors[1].fields, vec![Type::Var(t0)]);
    }

    // -- ADT Test 23 --
    #[test]
    fn constructor_nullary() {
        let c = Constructor::nullary("Red");
        assert_eq!(c.name, "Red");
        assert!(c.fields.is_empty());
    }

    // -- ADT Test 24 --
    #[test]
    fn constructor_with_field() {
        let t0 = TypeVar(0);
        let c = Constructor::nary("Some", vec![Type::Var(t0)]);
        assert_eq!(c.name, "Some");
        assert_eq!(c.fields, vec![Type::Var(t0)]);
    }

    // -----------------------------------------------------------------------
    // Collection type tests (M4)
    // -----------------------------------------------------------------------

    // -- Collection Test 1 --
    #[test]
    fn vec_type_display() {
        let ty = Type::Vec(Box::new(Type::Int));
        assert_eq!(ty.to_string(), "(Vec Int)");
    }

    // -- Collection Test 2 --
    #[test]
    fn vec_type_display_nested() {
        let ty = Type::Vec(Box::new(Type::Vec(Box::new(Type::Int))));
        assert_eq!(ty.to_string(), "(Vec (Vec Int))");
    }

    // -- Collection Test 3 --
    #[test]
    fn map_type_display() {
        let ty = Type::Map {
            key: Box::new(Type::Str),
            val: Box::new(Type::Int),
        };
        assert_eq!(ty.to_string(), "(Map Str Int)");
    }

    // -- Collection Test 4 --
    #[test]
    fn set_type_display() {
        let ty = Type::Set(Box::new(Type::Int));
        assert_eq!(ty.to_string(), "(Set Int)");
    }

    // -- Collection Test 5 --
    #[test]
    fn vec_type_equality() {
        assert_eq!(
            Type::Vec(Box::new(Type::Int)),
            Type::Vec(Box::new(Type::Int))
        );
        assert_ne!(
            Type::Vec(Box::new(Type::Int)),
            Type::Vec(Box::new(Type::Str))
        );
    }

    // -- Collection Test 6 --
    #[test]
    fn map_type_equality() {
        let a = Type::Map {
            key: Box::new(Type::Str),
            val: Box::new(Type::Int),
        };
        let b = Type::Map {
            key: Box::new(Type::Str),
            val: Box::new(Type::Int),
        };
        let c = Type::Map {
            key: Box::new(Type::Str),
            val: Box::new(Type::Bool),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // -- Collection Test 7 --
    #[test]
    fn set_type_equality() {
        assert_eq!(
            Type::Set(Box::new(Type::Int)),
            Type::Set(Box::new(Type::Int))
        );
        assert_ne!(
            Type::Set(Box::new(Type::Int)),
            Type::Set(Box::new(Type::Bool))
        );
    }

    // -- Collection Test 8 --
    #[test]
    fn vec_type_free_vars() {
        let ty = Type::Vec(Box::new(Type::Var(TypeVar(0))));
        let fvs = ty.free_vars();
        assert_eq!(fvs.len(), 1);
        assert!(fvs.contains(&TypeVar(0)));
    }

    // -- Collection Test 9 --
    #[test]
    fn map_type_free_vars() {
        let ty = Type::Map {
            key: Box::new(Type::Var(TypeVar(0))),
            val: Box::new(Type::Var(TypeVar(1))),
        };
        let fvs = ty.free_vars();
        assert_eq!(fvs.len(), 2);
        assert!(fvs.contains(&TypeVar(0)));
        assert!(fvs.contains(&TypeVar(1)));
    }

    // -- Collection Test 10 --
    #[test]
    fn set_type_free_vars() {
        let ty = Type::Set(Box::new(Type::Var(TypeVar(0))));
        let fvs = ty.free_vars();
        assert_eq!(fvs.len(), 1);
        assert!(fvs.contains(&TypeVar(0)));
    }

    // -- Collection Test 11 --
    #[test]
    fn vec_type_free_vars_concrete() {
        let ty = Type::Vec(Box::new(Type::Int));
        assert!(ty.free_vars().is_empty());
    }

    // -- Collection Test 12 --
    #[test]
    fn subst_apply_vec() {
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Int);
        let ty = Type::Vec(Box::new(Type::Var(TypeVar(0))));
        assert_eq!(s.apply(&ty), Type::Vec(Box::new(Type::Int)));
    }

    // -- Collection Test 13 --
    #[test]
    fn subst_apply_map() {
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Str);
        s.insert(TypeVar(1), Type::Int);
        let ty = Type::Map {
            key: Box::new(Type::Var(TypeVar(0))),
            val: Box::new(Type::Var(TypeVar(1))),
        };
        assert_eq!(
            s.apply(&ty),
            Type::Map {
                key: Box::new(Type::Str),
                val: Box::new(Type::Int),
            }
        );
    }

    // -- Collection Test 14 --
    #[test]
    fn subst_apply_set() {
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Bool);
        let ty = Type::Set(Box::new(Type::Var(TypeVar(0))));
        assert_eq!(s.apply(&ty), Type::Set(Box::new(Type::Bool)));
    }

    // -- Collection Test 15 --
    #[test]
    fn subst_apply_vec_no_match() {
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Int);
        let ty = Type::Vec(Box::new(Type::Var(TypeVar(1))));
        assert_eq!(s.apply(&ty), Type::Vec(Box::new(Type::Var(TypeVar(1)))));
    }

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

    // -- Record Test 1 --
    #[test]
    fn record_display_nominal() {
        let ty = Type::Record {
            name: "Point".to_string(),
            fields: vec![
                ("x".to_string(), Type::Float),
                ("y".to_string(), Type::Float),
            ],
        };
        assert_eq!(ty.to_string(), "Point");
    }

    // -- Record Test 2 --
    #[test]
    fn record_free_vars_from_fields() {
        let ty = Type::Record {
            name: "Pair".to_string(),
            fields: vec![
                ("fst".to_string(), Type::Var(TypeVar(0))),
                ("snd".to_string(), Type::Var(TypeVar(1))),
            ],
        };
        let vars = ty.free_vars();
        assert_eq!(vars.len(), 2);
        assert!(vars.contains(&TypeVar(0)));
        assert!(vars.contains(&TypeVar(1)));
    }

    // -- Record Test 3 --
    #[test]
    fn record_subst_apply_fields() {
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Int);
        let ty = Type::Record {
            name: "Point".to_string(),
            fields: vec![
                ("x".to_string(), Type::Var(TypeVar(0))),
                ("y".to_string(), Type::Float),
            ],
        };
        let applied = s.apply(&ty);
        assert_eq!(
            applied,
            Type::Record {
                name: "Point".to_string(),
                fields: vec![("x".to_string(), Type::Int), ("y".to_string(), Type::Float),],
            }
        );
    }

    // -- Record Test 4 --
    #[test]
    fn record_subst_apply_nested_tuple_field() {
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Int);
        s.insert(TypeVar(1), Type::Bool);
        let ty = Type::Record {
            name: "PairBox".to_string(),
            fields: vec![(
                "pair".to_string(),
                Type::Tuple(vec![Type::Var(TypeVar(0)), Type::Var(TypeVar(1))]),
            )],
        };
        let applied = s.apply(&ty);
        assert_eq!(
            applied,
            Type::Record {
                name: "PairBox".to_string(),
                fields: vec![("pair".to_string(), Type::Tuple(vec![Type::Int, Type::Bool]),)],
            }
        );
    }

    // -- Tuple Test 1 --
    #[test]
    fn tuple_display_two_three() {
        let two = Type::Tuple(vec![Type::Int, Type::Str]);
        let three = Type::Tuple(vec![Type::Int, Type::Str, Type::Bool]);
        assert_eq!(two.to_string(), "(Tuple Int Str)");
        assert_eq!(three.to_string(), "(Tuple Int Str Bool)");
    }

    // -- Tuple Test 1b --
    #[test]
    fn tuple_display_eight() {
        let eight = Type::Tuple(vec![
            Type::Int,
            Type::Bool,
            Type::Str,
            Type::Float,
            Type::Unit,
            Type::Never,
            Type::Keyword,
            Type::Symbol,
        ]);
        assert_eq!(
            eight.to_string(),
            "(Tuple Int Bool Str Float Unit Never Keyword Symbol)"
        );
    }

    // -- Tuple Test 2 --
    #[test]
    fn tuple_free_vars_from_elems() {
        let ty = Type::Tuple(vec![Type::Var(TypeVar(0)), Type::Var(TypeVar(1))]);
        let vars = ty.free_vars();
        assert_eq!(vars.len(), 2);
        assert!(vars.contains(&TypeVar(0)));
        assert!(vars.contains(&TypeVar(1)));
    }

    // -- Tuple Test 2b --
    #[test]
    fn tuple_free_vars_nested_record_fields() {
        let ty = Type::Tuple(vec![
            Type::Record {
                name: "Box".to_string(),
                fields: vec![("inner".to_string(), Type::Var(TypeVar(0)))],
            },
            Type::Var(TypeVar(1)),
        ]);
        let vars = ty.free_vars();
        assert_eq!(vars.len(), 2);
        assert!(vars.contains(&TypeVar(0)));
        assert!(vars.contains(&TypeVar(1)));
    }

    // -- Tuple Test 3 --
    #[test]
    fn tuple_subst_apply_elems() {
        let mut s = Subst::empty();
        s.insert(TypeVar(0), Type::Int);
        let ty = Type::Tuple(vec![Type::Var(TypeVar(0)), Type::Bool]);
        let applied = s.apply(&ty);
        assert_eq!(applied, Type::Tuple(vec![Type::Int, Type::Bool]));
    }

    // -- Test 6 --
    #[test]
    fn type_fn_display_no_params() {
        let ty = Type::Fn {
            params: vec![],
            ret: Box::new(Type::Int),
            effects: EffectRow::empty(),
        };
        assert_eq!(ty.to_string(), "(Fn [] -> Int)");
    }

    // -- Test 7 --
    #[test]
    fn type_fn_display_two_params() {
        let ty = Type::Fn {
            params: vec![Type::Int, Type::Str],
            ret: Box::new(Type::Bool),
            effects: EffectRow::empty(),
        };
        assert_eq!(ty.to_string(), "(Fn [Int Str] -> Bool)");
    }

    // -- Test 8 --
    #[test]
    fn type_fn_display_nested() {
        let inner = Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Int),
            effects: EffectRow::empty(),
        };
        let outer = Type::Fn {
            params: vec![inner],
            ret: Box::new(Type::Int),
            effects: EffectRow::empty(),
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
            effects: EffectRow::empty(),
        };
        let b = Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Bool),
            effects: EffectRow::empty(),
        };
        let c = Type::Fn {
            params: vec![Type::Str],
            ret: Box::new(Type::Bool),
            effects: EffectRow::empty(),
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
            effects: EffectRow::empty(),
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
            effects: EffectRow::empty(),
        };
        let expected = Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Int),
            effects: EffectRow::empty(),
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
                effects: EffectRow::empty(),
            },
        };
        let instantiated = scheme.instantiate(&mut supply);
        // supply.fresh() inside instantiate should produce TypeVar(1)
        let expected = Type::Fn {
            params: vec![Type::Var(TypeVar(1))],
            ret: Box::new(Type::Var(TypeVar(1))),
            effects: EffectRow::empty(),
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
            effects: EffectRow::empty(),
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
                effects: EffectRow::empty(),
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
