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
        NodeKind::List(items) => synth_list(items, env, state),
        _ => unimplemented!("synth: {:?}", node.kind),
    }
}

/// Dispatch on the head symbol of a list form.
fn synth_list(items: &[Node], env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    match head_sym(items) {
        Some("let") => synth_let(items, env, state),
        Some("do") => synth_do(items, env, state),
        _ => unimplemented!("synth_list: {:?}", items.first().map(|n| &n.kind)),
    }
}

/// Synthesize the type of a `(do e1 e2 ... eN)` form.
///
/// Evaluates each expression in order and returns the type of the last.
/// Prior expressions are synthesized for their side effects only (spec §4.8).
fn synth_do(items: &[Node], env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    // items[0] is the "do" symbol; the rest are the body expressions.
    let exprs = &items[1..];

    if exprs.is_empty() {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "do requires at least one expression".to_string(),
        }));
    }

    let mut last_ty = Type::Unit; // placeholder; overwritten by the loop
    for expr in exprs {
        last_ty = synth(expr, env, state)?;
    }
    Ok(last_ty)
}

/// Return the name string if the first item in `items` is an unqualified symbol.
fn head_sym(items: &[Node]) -> Option<&str> {
    match items.first() {
        Some(Node { kind: NodeKind::Atom(Atom::Symbol { ns: None, name }), .. }) => Some(name),
        _ => None,
    }
}

/// Synthesize the type of a `(let [x e1 y e2 ...] body)` form.
///
/// Bindings are evaluated sequentially; each binding is in scope for
/// subsequent bindings and for the body (spec §4.4).
fn synth_let(items: &[Node], env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    // Structure: (let <bindings-vec> <body>)
    if items.len() != 3 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "let expects (let [bindings...] body), got {} elements",
                items.len()
            ),
        }));
    }

    // items[1] must be a Vector of name/expr pairs.
    let bvec = match &items[1].kind {
        NodeKind::Vector(elems) => elems,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "let bindings must be a vector".to_string(),
            }));
        }
    };

    if bvec.len() % 2 != 0 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "let binding vector must have an even number of elements, got {}",
                bvec.len()
            ),
        }));
    }

    // Process each name/expr pair sequentially, extending the env.
    let mut current_env = env.clone();
    for pair in bvec.chunks(2) {
        let name_node = &pair[0];
        let expr_node = &pair[1];

        // Binding name must be an unqualified symbol.
        let name = match &name_node.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
            _ => {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: "let binding name must be an unqualified symbol".to_string(),
                }));
            }
        };

        // Synthesize the binding expression in the current env.
        let ty = synth(expr_node, &current_env, state)?;
        current_env = current_env.extend(name, Scheme::mono(ty));
    }

    // Synthesize the body in the fully-extended env.
    synth(&items[2], &current_env, state)
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

    /// Build `(do e1 e2 ...)` as a List node.
    fn do_node(exprs: Vec<Node>) -> Node {
        let mut items = vec![sym_node("do")];
        items.extend(exprs);
        Node::new(NodeKind::List(items), syn_span())
    }

    /// Build `(let [k0 v0 k1 v1 ...] body)` as a List node.
    fn let_node(bindings: Vec<(Node, Node)>, body: Node) -> Node {
        let head = sym_node("let");
        let bvec: Vec<Node> = bindings.into_iter().flat_map(|(k, v)| [k, v]).collect();
        let bvec_node = Node::new(NodeKind::Vector(bvec), syn_span());
        Node::new(NodeKind::List(vec![head, bvec_node, body]), syn_span())
    }

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

    // -----------------------------------------------------------------------
    // do form tests
    // -----------------------------------------------------------------------

    // -- Test 1 (do) --
    #[test]
    fn infer_do_single_expr() {
        // (do 42) → Int
        let (env, mut state) = empty();
        let node = do_node(vec![int_node(42)]);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 2 (do) --
    #[test]
    fn infer_do_returns_last_of_two() {
        // (do true 42) → Int  (Bool from first is discarded)
        let (env, mut state) = empty();
        let node = do_node(vec![atom_node(Atom::Bool(true)), int_node(42)]);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 3 (do) --
    #[test]
    fn infer_do_returns_last_of_three() {
        // (do true 3.14 "hello") → Str
        let (env, mut state) = empty();
        let node = do_node(vec![
            atom_node(Atom::Bool(true)),
            float_node(3.14),
            atom_node(Atom::Str("hello".into())),
        ]);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Str);
    }

    // -- Test 4 (do) --
    #[test]
    fn infer_do_error_in_early_expr() {
        // (do unknown 42) → UnboundVariable("unknown")
        let (env, mut state) = empty();
        let node = do_node(vec![sym_node("unknown"), int_node(42)]);
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "expected UnboundVariable(unknown), got {err:?}"
        );
    }

    // -- Test 5 (do) --
    #[test]
    fn infer_do_error_in_last_expr() {
        // (do 42 unknown) → UnboundVariable("unknown")
        let (env, mut state) = empty();
        let node = do_node(vec![int_node(42), sym_node("unknown")]);
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "expected UnboundVariable(unknown), got {err:?}"
        );
    }

    // -- Test 6 (do) --
    #[test]
    fn infer_do_malformed_empty() {
        // (do) → MalformedForm — no expressions to return
        let (env, mut state) = empty();
        let node = do_node(vec![]);
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // let form tests
    // -----------------------------------------------------------------------

    // -- Test 1 (let) --
    #[test]
    fn infer_let_empty_bindings() {
        // (let [] 42) → Int
        let (env, mut state) = empty();
        let node = let_node(vec![], int_node(42));
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 2 (let) --
    #[test]
    fn infer_let_single_binding() {
        // (let [x 42] x) → Int
        let (env, mut state) = empty();
        let node = let_node(vec![(sym_node("x"), int_node(42))], sym_node("x"));
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 3 (let) --
    #[test]
    fn infer_let_binding_bool_type() {
        // (let [x true] x) → Bool
        let (env, mut state) = empty();
        let node = let_node(
            vec![(sym_node("x"), atom_node(Atom::Bool(true)))],
            sym_node("x"),
        );
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Bool);
    }

    // -- Test 4 (let) --
    #[test]
    fn infer_let_sequential_bindings_in_scope() {
        // (let [x 42 y x] y) → Int
        // 'y' is bound to 'x', which is bound to 42 : Int
        let (env, mut state) = empty();
        let node = let_node(
            vec![(sym_node("x"), int_node(42)), (sym_node("y"), sym_node("x"))],
            sym_node("y"),
        );
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 5 (let) --
    #[test]
    fn infer_let_shadows_outer_env() {
        // outer: x : Bool; (let [x 42] x) → Int (inner shadows outer)
        let env = Env::new().extend("x", Scheme::mono(Type::Bool));
        let mut state = InferState::new();
        let node = let_node(vec![(sym_node("x"), int_node(42))], sym_node("x"));
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 6 (let) --
    #[test]
    fn infer_let_body_error() {
        // (let [x 42] unknown) → UnboundVariable("unknown")
        let (env, mut state) = empty();
        let node = let_node(vec![(sym_node("x"), int_node(42))], sym_node("unknown"));
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "expected UnboundVariable(unknown), got {err:?}"
        );
    }

    // -- Test 7 (let) --
    #[test]
    fn infer_let_binding_expr_error() {
        // (let [x unknown] x) → UnboundVariable("unknown") from the binding expr
        let (env, mut state) = empty();
        let node = let_node(vec![(sym_node("x"), sym_node("unknown"))], sym_node("x"));
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "expected UnboundVariable(unknown), got {err:?}"
        );
    }

    // -- Test 8 (let) --
    #[test]
    fn infer_let_malformed_not_list() {
        // passing an atom to synth with a List dispatch → only reachable via
        // direct synth_let call; test via a malformed node with wrong element count.
        // Use a let list with only 1 element: (let)
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::List(vec![sym_node("let")]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 9 (let) --
    #[test]
    fn infer_let_malformed_wrong_arity() {
        // (let [x 1]) — missing body → MalformedForm
        let (env, mut state) = empty();
        let bvec = Node::new(
            NodeKind::Vector(vec![sym_node("x"), int_node(1)]),
            syn_span(),
        );
        let node = Node::new(
            NodeKind::List(vec![sym_node("let"), bvec]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 10 (let) --
    #[test]
    fn infer_let_malformed_bindings_not_vector() {
        // (let 42 body) — bindings not a Vector → MalformedForm
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::List(vec![sym_node("let"), int_node(42), int_node(99)]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 11 (let) --
    #[test]
    fn infer_let_malformed_odd_bindings() {
        // (let [x] body) — odd binding vector → MalformedForm
        let (env, mut state) = empty();
        let bvec = Node::new(NodeKind::Vector(vec![sym_node("x")]), syn_span());
        let node = Node::new(
            NodeKind::List(vec![sym_node("let"), bvec, int_node(42)]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 12 (let) --
    #[test]
    fn infer_let_binding_name_not_symbol() {
        // (let [42 x] body) — binding name is not a symbol → MalformedForm
        let (env, mut state) = empty();
        let bvec = Node::new(
            NodeKind::Vector(vec![int_node(42), int_node(99)]),
            syn_span(),
        );
        let node = Node::new(
            NodeKind::List(vec![sym_node("let"), bvec, int_node(42)]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }
}
