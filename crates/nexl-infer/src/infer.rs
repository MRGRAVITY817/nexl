//! Synthesis and checking modes for the bidirectional inference engine.

use nexl_ast::{Atom, FloatSuffix, IntSuffix, Node, NodeKind};
use nexl_types::{Scheme, Subst, Type, TypeError, TypeErrorKind, TypeVarSupply};

use crate::Env;

/// Mutable inference state shared across a whole inference session.
///
/// The [`TypeVarSupply`] is held here so that all scopes share the same
/// counter and generate globally-unique type variables.  The [`Subst`]
/// accumulates variable bindings discovered during unification.
#[derive(Debug)]
pub struct InferState {
    pub supply: TypeVarSupply,
    pub subst: Subst,
}

impl InferState {
    /// Create a fresh inference state with no bindings.
    pub fn new() -> Self {
        Self { supply: TypeVarSupply::new(), subst: Subst::empty() }
    }

    /// Allocate a fresh unification variable and return it as a `Type`.
    pub fn fresh_var(&mut self) -> Type {
        Type::Var(self.supply.fresh())
    }
}

impl Default for InferState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Synthesize mode
// ---------------------------------------------------------------------------

/// Synthesize a type for `node` given typing environment `env`.
///
/// Returns the synthesized type, or a [`TypeError`] if synthesis fails.
/// New variable bindings produced by unification are recorded in `state`.
pub fn synth(node: &Node, env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    match &node.kind {
        NodeKind::Atom(atom) => synth_atom(atom, env, state),
        _ => unimplemented!("synth: {:?}", node.kind),
    }
}

/// Synthesize a type for a literal atom.
fn synth_atom(atom: &Atom, env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    match atom {
        Atom::Int { suffix: None, .. } => Ok(Type::Int),
        Atom::Int { suffix: Some(s), .. } => Ok(int_suffix_type(*s)),
        Atom::Float { suffix: None, .. } => Ok(Type::Float),
        Atom::Float { suffix: Some(s), .. } => Ok(float_suffix_type(*s)),
        Atom::Ratio { .. } => Ok(Type::Ratio),
        Atom::Bool(_) => Ok(Type::Bool),
        Atom::Char(_) => Ok(Type::Char),
        Atom::Str(_) => Ok(Type::Str),
        Atom::Keyword { .. } => Ok(Type::Keyword),
        Atom::Unit => Ok(Type::Unit),
        Atom::Symbol { ns: None, name } => synth_var(name, env, state),
        Atom::Symbol { ns: Some(_), name: _ } => {
            // Qualified symbols (module-prefixed) are not yet supported.
            unimplemented!("qualified symbol lookup")
        }
    }
}

/// Map an integer suffix to its fixed-width type.
fn int_suffix_type(s: IntSuffix) -> Type {
    match s {
        IntSuffix::I8 => Type::Int8,
        IntSuffix::I16 => Type::Int16,
        IntSuffix::I32 => Type::Int32,
        IntSuffix::I64 => Type::Int64,
        IntSuffix::U8 => Type::U8,
        IntSuffix::U16 => Type::U16,
        IntSuffix::U32 => Type::U32,
        IntSuffix::U64 => Type::U64,
    }
}

/// Map a float suffix to its fixed-width type.
fn float_suffix_type(s: FloatSuffix) -> Type {
    match s {
        FloatSuffix::F32 => Type::F32,
        FloatSuffix::F64 => Type::F64,
    }
}

/// Synthesize the type of a variable by looking it up in the environment.
fn synth_var(name: &str, env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    match env.lookup(name) {
        Some(scheme) => Ok(scheme.instantiate(&mut state.supply)),
        None => Err(TypeError::new(TypeErrorKind::UnboundVariable {
            name: name.to_string(),
        })),
    }
}

// ---------------------------------------------------------------------------
// Check mode
// ---------------------------------------------------------------------------

/// Check that `node` has type `expected`.
///
/// Synthesizes the node's type, then unifies the result with `expected`.
/// Any new variable bindings are recorded in `state.subst`.
pub fn check(
    node: &Node,
    expected: &Type,
    env: &Env,
    state: &mut InferState,
) -> Result<(), TypeError> {
    let actual = synth(node, env, state)?;
    nexl_types::unify(&actual, expected, &mut state.subst)
}

// ---------------------------------------------------------------------------
// def form
// ---------------------------------------------------------------------------

/// Infer the type of a `(def name expr)` form.
///
/// Returns the bound name, the synthesized type of `expr`, and a new
/// environment that extends `env` with `name → Scheme::mono(type)`.
pub fn infer_def(
    node: &Node,
    env: &Env,
    state: &mut InferState,
) -> Result<(String, Type, Env), TypeError> {
    let (name, body) = parse_def(node)?;
    let ty = synth(body, env, state)?;
    let new_env = env.extend(name.clone(), Scheme::mono(ty.clone()));
    Ok((name, ty, new_env))
}

/// Parse `(def name expr)` and return the binding name and body node.
fn parse_def(node: &Node) -> Result<(String, &Node), TypeError> {
    let items = match &node.kind {
        NodeKind::List(items) => items,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "def must be a list".to_string(),
            }));
        }
    };

    if items.len() != 3 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!("def expects (def name expr), got {} elements", items.len()),
        }));
    }

    // items[0] must be Symbol("def")
    let is_def_head = matches!(
        &items[0].kind,
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "def"
    );
    if !is_def_head {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "first element of def form must be the symbol `def`".to_string(),
        }));
    }

    // items[1] must be an unqualified symbol (the binding name)
    let binding_name = match &items[1].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "def binding name must be an unqualified symbol".to_string(),
            }));
        }
    };

    Ok((binding_name, &items[2]))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use nexl_ast::{Atom, FloatSuffix, IntSuffix, Node, Span};
    use nexl_types::{Scheme, Type, TypeErrorKind};

    use nexl_ast::NodeKind;
    use super::{InferState, check, infer_def, synth};
    use crate::Env;

    fn syn_span() -> Span {
        Span::synthetic()
    }

    fn atom_node(atom: Atom) -> Node {
        Node::atom(atom, syn_span())
    }

    fn int_node(value: i128) -> Node {
        atom_node(Atom::Int { value, suffix: None })
    }

    fn int_node_suf(value: i128, suffix: IntSuffix) -> Node {
        atom_node(Atom::Int { value, suffix: Some(suffix) })
    }

    fn float_node(value: f64) -> Node {
        atom_node(Atom::Float { value, suffix: None })
    }

    fn float_node_suf(value: f64, suffix: FloatSuffix) -> Node {
        atom_node(Atom::Float { value, suffix: Some(suffix) })
    }

    fn sym_node(name: &str) -> Node {
        atom_node(Atom::Symbol { ns: None, name: name.to_string() })
    }

    /// Build `(def name expr)` as a List node.
    fn def_node(name: &str, expr: Node) -> Node {
        let head = sym_node("def");
        let binding = sym_node(name);
        Node::new(NodeKind::List(vec![head, binding, expr]), syn_span())
    }

    fn empty() -> (Env, InferState) {
        (Env::new(), InferState::new())
    }

    // -- Test 5 --
    #[test]
    fn synth_int_no_suffix() {
        let (env, mut state) = empty();
        assert_eq!(synth(&int_node(42), &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 6 --
    #[test]
    fn synth_float_no_suffix() {
        let (env, mut state) = empty();
        assert_eq!(synth(&float_node(3.14), &env, &mut state).unwrap(), Type::Float);
    }

    // -- Test 7 --
    #[test]
    fn synth_ratio() {
        let (env, mut state) = empty();
        let node = atom_node(Atom::Ratio { numer: 1, denom: 3 });
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Ratio);
    }

    // -- Test 8 --
    #[test]
    fn synth_bool() {
        let (env, mut state) = empty();
        assert_eq!(synth(&atom_node(Atom::Bool(true)), &env, &mut state).unwrap(), Type::Bool);
    }

    // -- Test 9 --
    #[test]
    fn synth_char() {
        let (env, mut state) = empty();
        assert_eq!(synth(&atom_node(Atom::Char('a')), &env, &mut state).unwrap(), Type::Char);
    }

    // -- Test 10 --
    #[test]
    fn synth_str() {
        let (env, mut state) = empty();
        let node = atom_node(Atom::Str("hello".into()));
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Str);
    }

    // -- Test 11 --
    #[test]
    fn synth_keyword() {
        let (env, mut state) = empty();
        let node = atom_node(Atom::Keyword { ns: None, name: "ok".into() });
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Keyword);
    }

    // -- Test 12 --
    #[test]
    fn synth_unit() {
        let (env, mut state) = empty();
        assert_eq!(synth(&atom_node(Atom::Unit), &env, &mut state).unwrap(), Type::Unit);
    }

    // -- Test 13 --
    #[test]
    fn synth_int_i8_suffix() {
        let (env, mut state) = empty();
        assert_eq!(synth(&int_node_suf(1, IntSuffix::I8), &env, &mut state).unwrap(), Type::Int8);
    }

    // -- Test 14 --
    #[test]
    fn synth_int_i16_suffix() {
        let (env, mut state) = empty();
        assert_eq!(
            synth(&int_node_suf(1, IntSuffix::I16), &env, &mut state).unwrap(),
            Type::Int16
        );
    }

    // -- Test 15 --
    #[test]
    fn synth_int_i32_suffix() {
        let (env, mut state) = empty();
        assert_eq!(
            synth(&int_node_suf(1, IntSuffix::I32), &env, &mut state).unwrap(),
            Type::Int32
        );
    }

    // -- Test 16 --
    #[test]
    fn synth_int_i64_suffix() {
        let (env, mut state) = empty();
        assert_eq!(
            synth(&int_node_suf(1, IntSuffix::I64), &env, &mut state).unwrap(),
            Type::Int64
        );
    }

    // -- Test 17 --
    #[test]
    fn synth_int_u8_suffix() {
        let (env, mut state) = empty();
        assert_eq!(
            synth(&int_node_suf(255, IntSuffix::U8), &env, &mut state).unwrap(),
            Type::U8
        );
    }

    // -- Test 18 --
    #[test]
    fn synth_int_u16_suffix() {
        let (env, mut state) = empty();
        assert_eq!(
            synth(&int_node_suf(1, IntSuffix::U16), &env, &mut state).unwrap(),
            Type::U16
        );
    }

    // -- Test 19 --
    #[test]
    fn synth_int_u32_suffix() {
        let (env, mut state) = empty();
        assert_eq!(
            synth(&int_node_suf(1, IntSuffix::U32), &env, &mut state).unwrap(),
            Type::U32
        );
    }

    // -- Test 20 --
    #[test]
    fn synth_int_u64_suffix() {
        let (env, mut state) = empty();
        assert_eq!(
            synth(&int_node_suf(1, IntSuffix::U64), &env, &mut state).unwrap(),
            Type::U64
        );
    }

    // -- Test 21 --
    #[test]
    fn synth_float_f32_suffix() {
        let (env, mut state) = empty();
        assert_eq!(
            synth(&float_node_suf(1.0, FloatSuffix::F32), &env, &mut state).unwrap(),
            Type::F32
        );
    }

    // -- Test 22 --
    #[test]
    fn synth_float_f64_suffix() {
        let (env, mut state) = empty();
        assert_eq!(
            synth(&float_node_suf(1.0, FloatSuffix::F64), &env, &mut state).unwrap(),
            Type::F64
        );
    }

    // -- Test 23 --
    #[test]
    fn synth_var_monomorphic() {
        let env = Env::new().extend("x", Scheme::mono(Type::Int));
        let mut state = InferState::new();
        assert_eq!(synth(&sym_node("x"), &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 24 --
    #[test]
    fn synth_var_polymorphic() {
        // ∀t. (Fn [t] → t) — the identity scheme.
        //
        // IMPORTANT: consume `t0` from *state.supply* so that when
        // `instantiate` asks for the next fresh var it gets TypeVar(1),
        // not TypeVar(0) again (which would create a self-referential
        // substitution t0→Var(t0) and infinite recursion).
        use std::collections::HashSet;
        let mut state = InferState::new();
        let t0 = state.supply.fresh(); // TypeVar(0) — now consumed
        let scheme = nexl_types::Scheme {
            forall: [t0].into_iter().collect::<HashSet<_>>(),
            body: Type::Fn { params: vec![Type::Var(t0)], ret: Box::new(Type::Var(t0)) },
        };
        let env = Env::new().extend("id", scheme);
        // instantiate will call state.supply.fresh() → TypeVar(1)
        let ty = synth(&sym_node("id"), &env, &mut state).unwrap();
        match ty {
            Type::Fn { params, ret } => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0], *ret, "param and ret must be the same fresh var");
                assert_ne!(params[0], Type::Var(t0), "must be a fresh var, not the original t0");
            }
            other => panic!("expected Fn type, got {other:?}"),
        }
    }

    // -- Test 25 --
    #[test]
    fn synth_var_unknown() {
        let (env, mut state) = empty();
        let err = synth(&sym_node("y"), &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "y"),
            "expected UnboundVariable(y), got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Check mode tests
    // -----------------------------------------------------------------------

    // -- Test 1 (check) --
    #[test]
    fn check_lit_matches_expected() {
        let (env, mut state) = empty();
        assert!(check(&int_node(42), &Type::Int, &env, &mut state).is_ok());
    }

    // -- Test 2 (check) --
    #[test]
    fn check_lit_wrong_type() {
        let (env, mut state) = empty();
        let err = check(&int_node(42), &Type::Bool, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch, got {err:?}"
        );
    }

    // -- Test 3 (check) --
    #[test]
    fn check_unifies_type_var() {
        use nexl_types::TypeVar;
        let (env, mut state) = empty();
        // t0 is unbound; checking 42 against t0 should bind t0 → Int
        let t0 = Type::Var(TypeVar(0));
        check(&int_node(42), &t0, &env, &mut state).unwrap();
        assert_eq!(state.subst.apply(&t0), Type::Int);
    }

    // -----------------------------------------------------------------------
    // def form tests
    // -----------------------------------------------------------------------

    // -- Test 4 (def) --
    #[test]
    fn infer_def_name_returned() {
        let (env, mut state) = empty();
        let node = def_node("x", int_node(42));
        let (name, _ty, _new_env) = infer_def(&node, &env, &mut state).unwrap();
        assert_eq!(name, "x");
    }

    // -- Test 5 (def) --
    #[test]
    fn infer_def_int_type() {
        let (env, mut state) = empty();
        let node = def_node("x", int_node(42));
        let (_name, ty, _new_env) = infer_def(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int);
    }

    // -- Test 6 (def) --
    #[test]
    fn infer_def_bool_type() {
        let (env, mut state) = empty();
        let node = def_node("flag", atom_node(Atom::Bool(true)));
        let (_name, ty, _new_env) = infer_def(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Bool);
    }

    // -- Test 7 (def) --
    #[test]
    fn infer_def_extends_env() {
        let (env, mut state) = empty();
        let node = def_node("x", int_node(42));
        let (_name, _ty, new_env) = infer_def(&node, &env, &mut state).unwrap();
        // original env is unchanged
        assert!(env.lookup("x").is_none());
        // new env has x → Int
        let scheme = new_env.lookup("x").expect("x should be in new env");
        assert_eq!(scheme.body, Type::Int);
    }

    // -- Test 8 (def) --
    #[test]
    fn infer_def_body_error() {
        let (env, mut state) = empty();
        // (def x unknown) — 'unknown' is not in env
        let node = def_node("x", sym_node("unknown"));
        let err = infer_def(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "expected UnboundVariable(unknown), got {err:?}"
        );
    }
}
