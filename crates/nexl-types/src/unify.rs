//! Robinson unification for Nexl types.

use std::fmt;

use nexl_ast::Span;

use crate::{Subst, Type, TypeVar};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// The reason a unification failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeErrorKind {
    /// Two concrete types could not be unified.
    Mismatch { expected: Type, found: Type },
    /// The occurs check failed: a type variable appears within the type it
    /// would be bound to, which would create an infinite type.
    InfiniteType { var: TypeVar, ty: Type },
    /// Two function types have different numbers of parameters.
    ArityMismatch { expected: usize, found: usize },
    /// A name was used but is not bound in the typing environment.
    UnboundVariable { name: String },
    /// A syntactic form was structurally invalid (wrong head, wrong arity, etc.).
    MalformedForm { description: String },
}

/// A type error produced during unification.
///
/// The `span` field is left as `None` by the unifier itself; the inference
/// engine (nexl-infer) fills it in when it has source-location context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeError {
    pub kind: TypeErrorKind,
    pub span: Option<Span>,
}

impl TypeError {
    /// Create an error without a source span.
    pub fn new(kind: TypeErrorKind) -> Self {
        Self { kind, span: None }
    }

    /// Attach a source span to this error.
    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            TypeErrorKind::Mismatch { expected, found } => {
                write!(f, "expected {expected}, found {found}")
            }
            TypeErrorKind::InfiniteType { var, ty } => {
                write!(f, "infinite type: {var} = {ty}", var = Type::Var(*var))
            }
            TypeErrorKind::ArityMismatch { expected, found } => {
                write!(
                    f,
                    "function arity mismatch: expected {expected} parameter(s), found {found}"
                )
            }
            TypeErrorKind::UnboundVariable { name } => {
                write!(f, "unbound variable: {name}")
            }
            TypeErrorKind::MalformedForm { description } => {
                write!(f, "malformed form: {description}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Alias normalisation
// ---------------------------------------------------------------------------

/// Normalise aliases: Int64 → Int, F64 → Float (spec §5.2).
fn normalize(ty: Type) -> Type {
    match ty {
        Type::Int64 => Type::Int,
        Type::F64 => Type::Float,
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Occurs check
// ---------------------------------------------------------------------------

/// Returns `true` if `tv` appears free anywhere in `ty`.
///
/// Called on types that have *already* been walked through the substitution,
/// so there is no need to apply the substitution again.
fn occurs_in(tv: TypeVar, ty: &Type) -> bool {
    match ty {
        Type::Var(v) => *v == tv,
        Type::Fn { params, ret } => {
            params.iter().any(|p| occurs_in(tv, p)) || occurs_in(tv, ret)
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Unification
// ---------------------------------------------------------------------------

/// Unify two types, extending `subst` with any new variable bindings.
///
/// Returns `Ok(())` on success.  On failure the substitution is left in an
/// **unspecified** state; callers should not rely on its contents after an
/// error.
pub fn unify(a: &Type, b: &Type, subst: &mut Subst) -> Result<(), TypeError> {
    let a = normalize(subst.apply(a));
    let b = normalize(subst.apply(b));

    // Reflexivity short-circuit — avoids spurious occurs-check failure for t0 = t0.
    if a == b {
        return Ok(());
    }

    match (&a, &b) {
        // Identical concrete types — always succeed.
        (Type::Int, Type::Int)
        | (Type::Float, Type::Float)
        | (Type::Ratio, Type::Ratio)
        | (Type::Bool, Type::Bool)
        | (Type::Char, Type::Char)
        | (Type::Str, Type::Str)
        | (Type::Keyword, Type::Keyword)
        | (Type::Symbol, Type::Symbol)
        | (Type::Unit, Type::Unit)
        | (Type::Never, Type::Never)
        | (Type::Int8, Type::Int8)
        | (Type::Int16, Type::Int16)
        | (Type::Int32, Type::Int32)
        | (Type::Int64, Type::Int64)
        | (Type::U8, Type::U8)
        | (Type::U16, Type::U16)
        | (Type::U32, Type::U32)
        | (Type::U64, Type::U64)
        | (Type::F32, Type::F32)
        | (Type::F64, Type::F64) => Ok(()),

        // Variable on the left.
        (Type::Var(tv), _) => {
            let tv = *tv;
            if occurs_in(tv, &b) {
                return Err(TypeError::new(TypeErrorKind::InfiniteType {
                    var: tv,
                    ty: b.clone(),
                }));
            }
            subst.insert(tv, b.clone());
            Ok(())
        }

        // Variable on the right.
        (_, Type::Var(tv)) => {
            let tv = *tv;
            if occurs_in(tv, &a) {
                return Err(TypeError::new(TypeErrorKind::InfiniteType {
                    var: tv,
                    ty: a.clone(),
                }));
            }
            subst.insert(tv, a.clone());
            Ok(())
        }

        // Two function types.
        (
            Type::Fn {
                params: pa,
                ret: ra,
            },
            Type::Fn {
                params: pb,
                ret: rb,
            },
        ) => {
            if pa.len() != pb.len() {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: pa.len(),
                    found: pb.len(),
                }));
            }
            // Clone to avoid borrow issues while mutating `subst`.
            let pa = pa.clone();
            let pb = pb.clone();
            let ra = (**ra).clone();
            let rb = (**rb).clone();
            for (p_a, p_b) in pa.iter().zip(pb.iter()) {
                unify(p_a, p_b, subst)?;
            }
            unify(&ra, &rb, subst)
        }

        // Mismatch — two distinct concrete types.
        _ => Err(TypeError::new(TypeErrorKind::Mismatch {
            expected: a.clone(),
            found: b.clone(),
        })),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{TypeError, TypeErrorKind, unify};
    use crate::{Subst, Type, TypeVar};

    fn tv(n: u32) -> Type {
        Type::Var(TypeVar(n))
    }

    fn fn1(param: Type, ret: Type) -> Type {
        Type::Fn { params: vec![param], ret: Box::new(ret) }
    }

    fn fn2(p1: Type, p2: Type, ret: Type) -> Type {
        Type::Fn { params: vec![p1, p2], ret: Box::new(ret) }
    }

    // -- Test 1 --
    #[test]
    fn unify_identical_primitives() {
        let mut s = Subst::empty();
        assert!(unify(&Type::Int, &Type::Int, &mut s).is_ok());
        assert!(unify(&Type::Bool, &Type::Bool, &mut s).is_ok());
        assert!(unify(&Type::Str, &Type::Str, &mut s).is_ok());
        assert!(unify(&Type::Unit, &Type::Unit, &mut s).is_ok());
        assert!(unify(&Type::Never, &Type::Never, &mut s).is_ok());
    }

    // -- Test 2 --
    #[test]
    fn unify_different_primitives() {
        let mut s = Subst::empty();
        let err = unify(&Type::Int, &Type::Float, &mut s).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch, got {err:?}"
        );
    }

    // -- Test 3 --
    #[test]
    fn unify_var_with_primitive() {
        let mut s = Subst::empty();
        // t0 = Int  →  s should bind t0 → Int
        unify(&tv(0), &Type::Int, &mut s).unwrap();
        assert_eq!(s.apply(&tv(0)), Type::Int);
    }

    // -- Test 4 --
    #[test]
    fn unify_primitive_with_var() {
        let mut s = Subst::empty();
        // Int = t0  →  symmetric
        unify(&Type::Int, &tv(0), &mut s).unwrap();
        assert_eq!(s.apply(&tv(0)), Type::Int);
    }

    // -- Test 5 --
    #[test]
    fn unify_two_vars() {
        let mut s = Subst::empty();
        // t0 = t1  →  one binds to the other
        unify(&tv(0), &tv(1), &mut s).unwrap();
        // Applying s to t0 and t1 should produce the same type.
        assert_eq!(s.apply(&tv(0)), s.apply(&tv(1)));
    }

    // -- Test 6 --
    #[test]
    fn unify_var_with_itself() {
        let mut s = Subst::empty();
        // t0 = t0  →  no error, no new binding added
        unify(&tv(0), &tv(0), &mut s).unwrap();
        // t0 still unresolved (no binding)
        assert_eq!(s.apply(&tv(0)), tv(0));
    }

    // -- Test 7 --
    #[test]
    fn unify_fn_matching() {
        let mut s = Subst::empty();
        let a = fn1(Type::Int, Type::Bool);
        let b = fn1(Type::Int, Type::Bool);
        unify(&a, &b, &mut s).unwrap();
    }

    // -- Test 8 --
    #[test]
    fn unify_fn_param_mismatch() {
        let mut s = Subst::empty();
        let a = fn1(Type::Int, Type::Bool);
        let b = fn1(Type::Str, Type::Bool);
        let err = unify(&a, &b, &mut s).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch, got {err:?}"
        );
    }

    // -- Test 9 --
    #[test]
    fn unify_fn_ret_mismatch() {
        let mut s = Subst::empty();
        let a = fn1(Type::Int, Type::Bool);
        let b = fn1(Type::Int, Type::Str);
        let err = unify(&a, &b, &mut s).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch, got {err:?}"
        );
    }

    // -- Test 10 --
    #[test]
    fn unify_fn_arity_mismatch() {
        let mut s = Subst::empty();
        let a = fn1(Type::Int, Type::Bool);
        let b = fn2(Type::Int, Type::Str, Type::Bool);
        let err = unify(&a, &b, &mut s).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::ArityMismatch { expected: 1, found: 2 }
            ),
            "expected ArityMismatch(1,2), got {err:?}"
        );
    }

    // -- Test 11 --
    #[test]
    fn occurs_check_direct() {
        let mut s = Subst::empty();
        // t0 = (Fn [t0] -> Int)  →  infinite type
        let ty = fn1(tv(0), Type::Int);
        let err = unify(&tv(0), &ty, &mut s).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::InfiniteType { var: TypeVar(0), .. }),
            "expected InfiniteType(t0), got {err:?}"
        );
    }

    // -- Test 12 --
    #[test]
    fn occurs_check_nested() {
        let mut s = Subst::empty();
        // t0 = (Fn [(Fn [t0] -> Int)] -> Int)  →  infinite type (nested)
        let inner = fn1(tv(0), Type::Int);
        let outer = fn1(inner, Type::Int);
        let err = unify(&tv(0), &outer, &mut s).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::InfiniteType { var: TypeVar(0), .. }),
            "expected InfiniteType(t0), got {err:?}"
        );
    }

    // -- Test 13 --
    #[test]
    fn unify_through_subst() {
        let mut s = Subst::empty();
        // Pre-bind t0 → Int, then unify t1 = t0 → t1 should resolve to Int.
        s.insert(TypeVar(0), Type::Int);
        unify(&tv(1), &tv(0), &mut s).unwrap();
        assert_eq!(s.apply(&tv(1)), Type::Int);
    }

    // -- Test 14 --
    #[test]
    fn unify_int64_int_alias() {
        let mut s = Subst::empty();
        // Int64 is an alias for Int (spec §5.2)
        assert!(unify(&Type::Int64, &Type::Int, &mut s).is_ok());
        assert!(unify(&Type::Int, &Type::Int64, &mut s).is_ok());
        assert!(unify(&Type::Int64, &Type::Int64, &mut s).is_ok());
    }

    // -- Test 15 --
    #[test]
    fn unify_f64_float_alias() {
        let mut s = Subst::empty();
        // F64 is an alias for Float (spec §5.2)
        assert!(unify(&Type::F64, &Type::Float, &mut s).is_ok());
        assert!(unify(&Type::Float, &Type::F64, &mut s).is_ok());
        assert!(unify(&Type::F64, &Type::F64, &mut s).is_ok());
    }

    // -- Test 16 --
    #[test]
    fn unify_fixed_width_no_alias() {
        // Only Int64↔Int and F64↔Float are aliases; other fixed-width types are distinct.
        let mut s = Subst::empty();
        assert!(unify(&Type::Int8, &Type::Int16, &mut s).is_err());
        assert!(unify(&Type::Int32, &Type::Int, &mut s).is_err());
        assert!(unify(&Type::U8, &Type::U16, &mut s).is_err());
        assert!(unify(&Type::F32, &Type::Float, &mut s).is_err());
    }

    // -- Test 17 --
    #[test]
    fn unify_var_with_fn() {
        let mut s = Subst::empty();
        // t0 = (Fn [Int] -> Bool)
        let fn_ty = fn1(Type::Int, Type::Bool);
        unify(&tv(0), &fn_ty, &mut s).unwrap();
        assert_eq!(s.apply(&tv(0)), fn_ty);
    }

    // -- Test 18 --
    #[test]
    fn unify_nested_fn() {
        // (Fn [t0] -> t0) = (Fn [Int] -> Int)  →  t0 = Int
        let mut s = Subst::empty();
        let a = fn1(tv(0), tv(0));
        let b = fn1(Type::Int, Type::Int);
        unify(&a, &b, &mut s).unwrap();
        assert_eq!(s.apply(&tv(0)), Type::Int);
    }

    // -- Test 19 --
    #[test]
    fn type_error_mismatch_display() {
        let err = TypeError::new(TypeErrorKind::Mismatch {
            expected: Type::Int,
            found: Type::Float,
        });
        let msg = err.to_string();
        assert!(msg.contains("expected"), "missing 'expected' in '{msg}'");
        assert!(msg.contains("Int"), "missing 'Int' in '{msg}'");
        assert!(msg.contains("Float"), "missing 'Float' in '{msg}'");
    }

    // -- Test 20 --
    #[test]
    fn type_error_infinite_type_display() {
        let err = TypeError::new(TypeErrorKind::InfiniteType {
            var: TypeVar(0),
            ty: fn1(tv(0), Type::Int),
        });
        let msg = err.to_string();
        assert!(msg.contains("infinite") || msg.contains("t0"), "uninformative: '{msg}'");
    }

    // -- Test 21 --
    #[test]
    fn type_error_arity_display() {
        let err = TypeError::new(TypeErrorKind::ArityMismatch { expected: 2, found: 3 });
        let msg = err.to_string();
        assert!(msg.contains('2'), "missing '2' in '{msg}'");
        assert!(msg.contains('3'), "missing '3' in '{msg}'");
    }
}
