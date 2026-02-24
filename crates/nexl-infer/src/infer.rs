//! Synthesis and checking modes for the bidirectional inference engine.

use nexl_ast::{Atom, FloatSuffix, IntSuffix, Node, NodeKind};
use nexl_types::{Scheme, Subst, Type, TypeError, TypeErrorKind, TypeVarSupply};

use crate::Env;

/// Mutable inference state shared across a whole inference session.
///
/// The [`TypeVarSupply`] is held here so that all scopes share the same
/// counter and generate globally-unique type variables.  The [`Subst`]
/// accumulates variable bindings discovered during unification.
///
/// `recur_types` holds the expected argument types for the innermost
/// `loop` form currently being inferred.  `synth_loop` saves and restores
/// this field so that nested loops work correctly.  It is `None` when
/// inference is not inside a loop.
#[derive(Debug)]
pub struct InferState {
    pub supply: TypeVarSupply,
    pub subst: Subst,
    /// The loop-variable types that a `recur` in the current loop must match.
    pub recur_types: Option<Vec<Type>>,
}

impl InferState {
    /// Create a fresh inference state with no bindings.
    pub fn new() -> Self {
        Self { supply: TypeVarSupply::new(), subst: Subst::empty(), recur_types: None }
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
        Some("if") => synth_if(items, env, state),
        Some("fn") => synth_fn(items, env, state),
        Some("loop") => synth_loop(items, env, state),
        Some("recur") => synth_recur(items, env, state),
        _ => synth_application(items, env, state),
    }
}

/// Synthesize the type of a `(loop [x0 v0 x1 v1 ...] body)` form.
///
/// Each binding's init expression is synthesized to determine the loop
/// variable's type.  The body is inferred in an environment extended with
/// those bindings.  `recur_types` is set in `state` so that any `recur`
/// form inside the body can check its arguments against them.
///
/// The return type of the loop is the return type of the body.  `recur`
/// itself has type `Never`, which is the bottom type and unifies with any
/// type (spec §5.3), so branches containing `recur` do not constrain the
/// overall loop return type.
fn synth_loop(items: &[Node], env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    // Structure: (loop <bindings-vec> <body>) — exactly 3 elements.
    if items.len() != 3 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "loop expects (loop [bindings...] body), got {} elements",
                items.len()
            ),
        }));
    }

    // items[1] must be a Vector of name/expr pairs.
    let bvec = match &items[1].kind {
        NodeKind::Vector(elems) => elems,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "loop bindings must be a vector".to_string(),
            }));
        }
    };

    if bvec.len() % 2 != 0 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "loop binding vector must have an even number of elements, got {}",
                bvec.len()
            ),
        }));
    }

    // Infer each init expression; collect loop var names and their types.
    let mut current_env = env.clone();
    let mut loop_var_types: Vec<Type> = Vec::new();

    for pair in bvec.chunks(2) {
        let name_node = &pair[0];
        let init_node = &pair[1];

        let name = match &name_node.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
            _ => {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: "loop binding name must be an unqualified symbol".to_string(),
                }));
            }
        };

        let ty = synth(init_node, &current_env, state)?;
        loop_var_types.push(ty.clone());
        current_env = current_env.extend(name, Scheme::mono(ty));
    }

    // Save the outer recur target, set ours, infer body, then restore.
    let saved_recur = state.recur_types.take();
    state.recur_types = Some(loop_var_types);
    let body_ty = synth(&items[2], &current_env, state);
    state.recur_types = saved_recur;

    body_ty
}

/// Synthesize the type of a `(recur arg0 arg1 ...)` form.
///
/// Checks that each argument type matches the corresponding loop variable
/// type set by the enclosing `loop`.  Returns `Type::Never` — `recur`
/// never produces a value; it transfers control back to the loop head.
fn synth_recur(items: &[Node], env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    // items[0] is "recur"; the rest are the new loop-variable values.
    let arg_nodes = &items[1..];

    let expected = match &state.recur_types {
        Some(ts) => ts.clone(),
        None => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "recur used outside of a loop form".to_string(),
            }));
        }
    };

    if arg_nodes.len() != expected.len() {
        return Err(TypeError::new(TypeErrorKind::ArityMismatch {
            expected: expected.len(),
            found: arg_nodes.len(),
        }));
    }

    for (arg_node, expected_ty) in arg_nodes.iter().zip(expected.iter()) {
        let arg_ty = synth(arg_node, env, state)?;
        nexl_types::unify(expected_ty, &arg_ty, &mut state.subst)?;
    }

    Ok(Type::Never)
}

/// Synthesize the type of a function application `(callee arg0 arg1 ...)`.
///
/// The callee is synthesized first, then each argument.  A fresh return type
/// variable is introduced; the callee type is unified with
/// `(Fn [arg_types...] -> ret_var)`.  The resolved return type is returned.
///
/// This handles:
/// - Correct calls: argument types are unified against parameter types.
/// - Arity errors: caught by [`nexl_types::unify`] as `ArityMismatch`.
/// - Non-callable callee: unifying e.g. `Int` with `(Fn [...] -> t)` yields
///   `Mismatch`.
/// - Type variable callee: the unification binds the variable to the inferred
///   `Fn` shape.
fn synth_application(items: &[Node], env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    if items.is_empty() {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "empty application — a callee is required".to_string(),
        }));
    }

    let callee_node = &items[0];
    let arg_nodes = &items[1..];

    // Synthesize the callee type.
    let callee_ty = synth(callee_node, env, state)?;

    // Synthesize each argument type in order.
    let arg_types: Vec<Type> = arg_nodes
        .iter()
        .map(|a| synth(a, env, state))
        .collect::<Result<Vec<_>, _>>()?;

    // Introduce a fresh return type variable.
    let ret_var = state.fresh_var();

    // Unify the callee with the expected function shape.
    // Any arity or type mismatch surfaces here.
    let expected_fn = Type::Fn { params: arg_types, ret: Box::new(ret_var.clone()) };
    nexl_types::unify(&callee_ty, &expected_fn, &mut state.subst)?;

    Ok(state.subst.apply(&ret_var))
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

/// Synthesize the type of a `(fn [params...] body)` form.
///
/// Each parameter receives a fresh type variable.  The body is inferred in
/// an environment extended with those bindings.  Returns
/// `(Fn [param-types...] -> body-type)` with the substitution applied (spec §4.3).
fn synth_fn(items: &[Node], env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    // Structure: (fn <params-vec> <body>) — exactly 3 elements.
    if items.len() != 3 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "fn expects (fn [params...] body), got {} elements",
                items.len()
            ),
        }));
    }

    let param_nodes = match &items[1].kind {
        NodeKind::Vector(elems) => elems,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "fn parameter list must be a vector".to_string(),
            }));
        }
    };

    // Allocate a fresh type var for each parameter and extend the environment.
    let mut param_types: Vec<Type> = Vec::new();
    let mut body_env = env.clone();

    for param in param_nodes {
        let name = match &param.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
            _ => {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: "fn parameter names must be unqualified symbols".to_string(),
                }));
            }
        };
        let tv = state.fresh_var();
        param_types.push(tv.clone());
        body_env = body_env.extend(name, Scheme::mono(tv));
    }

    // Infer the body type in the extended environment.
    let ret_ty = synth(&items[2], &body_env, state)?;

    // Apply the accumulated substitution so any param vars that were unified
    // during body inference are resolved in the returned type.
    let param_types = param_types.iter().map(|t| state.subst.apply(t)).collect();
    let ret_ty = state.subst.apply(&ret_ty);

    Ok(Type::Fn { params: param_types, ret: Box::new(ret_ty) })
}

/// Synthesize the type of an `(if cond then else)` form.
///
/// The condition must be `Bool` (ADR-004 — no truthiness).
/// Both branches must unify to a common type (spec §4.5).
fn synth_if(items: &[Node], env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    // Structure: (if <cond> <then> <else>) — exactly 4 elements.
    if items.len() != 4 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "if expects (if cond then else), got {} elements",
                items.len()
            ),
        }));
    }

    let cond_node = &items[1];
    let then_node = &items[2];
    let else_node = &items[3];

    // Condition must be Bool (ADR-004).
    check(cond_node, &Type::Bool, env, state)?;

    // Synthesize then-branch; use it as the expected type for else.
    let then_ty = synth(then_node, env, state)?;
    nexl_types::unify(&then_ty, &synth(else_node, env, state)?, &mut state.subst)?;

    Ok(state.subst.apply(&then_ty))
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

        // Synthesize the binding expression in the current env, then
        // generalize: quantify type variables not constrained by the outer env.
        let ty = synth(expr_node, &current_env, state)?;
        let scheme = generalize(&ty, &current_env, state);
        current_env = current_env.extend(name, scheme);
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
// defn form
// ---------------------------------------------------------------------------

/// Infer the type of a `(defn name [params...] body)` form.
///
/// Sugar for `(def name (fn [params...] body))`.  Returns the bound name,
/// the synthesized function type, and a new environment that extends `env`
/// with `name → Scheme::mono(fn-type)`.
pub fn infer_defn(
    node: &Node,
    env: &Env,
    state: &mut InferState,
) -> Result<(String, Type, Env), TypeError> {
    let items = match &node.kind {
        NodeKind::List(items) => items,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "defn must be a list".to_string(),
            }));
        }
    };

    // Structure: (defn <name> <params-vec> <body>) — exactly 4 elements.
    if items.len() != 4 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "defn expects (defn name [params...] body), got {} elements",
                items.len()
            ),
        }));
    }

    // items[1] must be an unqualified symbol (the function name).
    let name = match &items[1].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "defn function name must be an unqualified symbol".to_string(),
            }));
        }
    };

    // Desugar: build a synthetic `fn` items slice and reuse synth_fn.
    // items[2] = params vector, items[3] = body.
    let fn_head = Node::new(
        NodeKind::Atom(Atom::Symbol { ns: None, name: "fn".to_string() }),
        node.span,
    );
    let fn_items = [fn_head, items[2].clone(), items[3].clone()];
    let fn_ty = synth_fn(&fn_items, env, state)?;

    let new_env = env.extend(name.clone(), Scheme::mono(fn_ty.clone()));
    Ok((name, fn_ty, new_env))
}

// ---------------------------------------------------------------------------
// Generalization
// ---------------------------------------------------------------------------

/// Generalize `ty` relative to the outer environment `env`.
///
/// Applies `subst` to `ty`, then computes the set of type variables that are
/// free in the resolved type but NOT free in the outer environment.  These
/// "unconstrained" variables are universally quantified, producing a
/// polymorphic [`Scheme`].
///
/// Variables that are free in the environment are NOT generalized because
/// they are constrained by an outer context and must remain monomorphic.
fn generalize(ty: &Type, env: &Env, state: &InferState) -> Scheme {
    let ty = state.subst.apply(ty);
    let ty_free = ty.free_vars();
    let env_free = env.free_vars(&state.subst);
    let forall: std::collections::HashSet<_> =
        ty_free.difference(&env_free).copied().collect();
    Scheme { forall, body: ty }
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
    // Put `expected` first so Mismatch errors read "expected X, found Y".
    nexl_types::unify(expected, &actual, &mut state.subst)
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
    use super::{InferState, check, infer_def, infer_defn, synth};
    use crate::Env;

    /// Build `(defn name [param...] body)` as a List node.
    fn defn_node(name: &str, params: Vec<&str>, body: Node) -> Node {
        let head = sym_node("defn");
        let pvec = Node::new(NodeKind::Vector(params.iter().map(|p| sym_node(p)).collect()), syn_span());
        Node::new(NodeKind::List(vec![head, sym_node(name), pvec, body]), syn_span())
    }

    /// Build `(fn [param...] body)` as a List node.
    fn fn_node(params: Vec<&str>, body: Node) -> Node {
        let head = sym_node("fn");
        let pvec = Node::new(NodeKind::Vector(params.iter().map(|p| sym_node(p)).collect()), syn_span());
        Node::new(NodeKind::List(vec![head, pvec, body]), syn_span())
    }

    /// Build `(if cond then else)` as a List node.
    fn if_node(cond: Node, then: Node, else_: Node) -> Node {
        Node::new(NodeKind::List(vec![sym_node("if"), cond, then, else_]), syn_span())
    }

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
    // defn form tests
    // -----------------------------------------------------------------------

    // -- Test 1 (defn) --
    #[test]
    fn infer_defn_name_returned() {
        // (defn f [] 42) → name is "f"
        let (env, mut state) = empty();
        let node = defn_node("f", vec![], int_node(42));
        let (name, _ty, _env) = infer_defn(&node, &env, &mut state).unwrap();
        assert_eq!(name, "f");
    }

    // -- Test 2 (defn) --
    #[test]
    fn infer_defn_zero_params_type() {
        // (defn f [] 42) → type is (Fn [] -> Int)
        let (env, mut state) = empty();
        let node = defn_node("f", vec![], int_node(42));
        let (_name, ty, _env) = infer_defn(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Fn { params: vec![], ret: Box::new(Type::Int) });
    }

    // -- Test 3 (defn) --
    #[test]
    fn infer_defn_one_param_identity_type() {
        // (defn f [x] x) → (Fn [t?] -> t?) where param == ret var
        let (env, mut state) = empty();
        let node = defn_node("f", vec!["x"], sym_node("x"));
        let (_name, ty, _env) = infer_defn(&node, &env, &mut state).unwrap();
        match ty {
            Type::Fn { params, ret } => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0], *ret, "param and return type must be the same var");
            }
            other => panic!("expected Fn type, got {other:?}"),
        }
    }

    // -- Test 4 (defn) --
    #[test]
    fn infer_defn_extends_env() {
        // returned env has f → (Fn [] -> Int); original env unchanged
        let (env, mut state) = empty();
        let node = defn_node("f", vec![], int_node(42));
        let (_name, _ty, new_env) = infer_defn(&node, &env, &mut state).unwrap();
        assert!(env.lookup("f").is_none(), "original env must not have f");
        let scheme = new_env.lookup("f").expect("new env must have f");
        assert_eq!(scheme.body, Type::Fn { params: vec![], ret: Box::new(Type::Int) });
    }

    // -- Test 5 (defn) --
    #[test]
    fn infer_defn_body_error() {
        // (defn f [x] unknown) → UnboundVariable("unknown")
        let (env, mut state) = empty();
        let node = defn_node("f", vec!["x"], sym_node("unknown"));
        let err = infer_defn(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "expected UnboundVariable(unknown), got {err:?}"
        );
    }

    // -- Test 6 (defn) --
    #[test]
    fn infer_defn_malformed_wrong_arity() {
        // (defn f [x]) — missing body → MalformedForm
        let (env, mut state) = empty();
        let pvec = Node::new(NodeKind::Vector(vec![sym_node("x")]), syn_span());
        let node = Node::new(
            NodeKind::List(vec![sym_node("defn"), sym_node("f"), pvec]),
            syn_span(),
        );
        let err = infer_defn(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 7 (defn) --
    #[test]
    fn infer_defn_malformed_name_not_symbol() {
        // (defn 42 [x] body) → MalformedForm
        let (env, mut state) = empty();
        let pvec = Node::new(NodeKind::Vector(vec![sym_node("x")]), syn_span());
        let node = Node::new(
            NodeKind::List(vec![sym_node("defn"), int_node(42), pvec, int_node(99)]),
            syn_span(),
        );
        let err = infer_defn(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 8 (defn) --
    #[test]
    fn infer_defn_malformed_params_not_vector() {
        // (defn f 42 body) → MalformedForm (params must be a vector)
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::List(vec![sym_node("defn"), sym_node("f"), int_node(42), int_node(99)]),
            syn_span(),
        );
        let err = infer_defn(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // fn form tests
    // -----------------------------------------------------------------------

    // -- Test 1 (fn) --
    #[test]
    fn infer_fn_no_params() {
        // (fn [] 42) → (Fn [] -> Int)
        let (env, mut state) = empty();
        let node = fn_node(vec![], int_node(42));
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Fn { params: vec![], ret: Box::new(Type::Int) });
    }

    // -- Test 2 (fn) --
    #[test]
    fn infer_fn_body_is_constant() {
        // (fn [x] 42) → (Fn [t?] -> Int); param stays as a free var
        let (env, mut state) = empty();
        let node = fn_node(vec!["x"], int_node(42));
        let ty = synth(&node, &env, &mut state).unwrap();
        match ty {
            Type::Fn { params, ret } => {
                assert_eq!(params.len(), 1);
                assert!(matches!(params[0], Type::Var(_)), "param should be a free var");
                assert_eq!(*ret, Type::Int);
            }
            other => panic!("expected Fn type, got {other:?}"),
        }
    }

    // -- Test 3 (fn) --
    #[test]
    fn infer_fn_identity() {
        // (fn [x] x) → (Fn [t?] -> t?) where param and ret are the same var
        let (env, mut state) = empty();
        let node = fn_node(vec!["x"], sym_node("x"));
        let ty = synth(&node, &env, &mut state).unwrap();
        match ty {
            Type::Fn { params, ret } => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0], *ret, "param and return type must be the same var");
            }
            other => panic!("expected Fn type, got {other:?}"),
        }
    }

    // -- Test 4 (fn) --
    #[test]
    fn infer_fn_two_params_returns_first() {
        // (fn [x y] x) → (Fn [t0 t1] -> t0); two distinct vars, ret matches first
        let (env, mut state) = empty();
        let node = fn_node(vec!["x", "y"], sym_node("x"));
        let ty = synth(&node, &env, &mut state).unwrap();
        match ty {
            Type::Fn { params, ret } => {
                assert_eq!(params.len(), 2);
                assert_ne!(params[0], params[1], "params should have distinct type vars");
                assert_eq!(params[0], *ret, "return type should match first param");
            }
            other => panic!("expected Fn type, got {other:?}"),
        }
    }

    // -- Test 5 (fn) --
    #[test]
    fn infer_fn_body_error() {
        // (fn [x] unknown) → UnboundVariable("unknown")
        let (env, mut state) = empty();
        let node = fn_node(vec!["x"], sym_node("unknown"));
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "expected UnboundVariable(unknown), got {err:?}"
        );
    }

    // -- Test 6 (fn) --
    #[test]
    fn infer_fn_malformed_wrong_arity() {
        // (fn [x]) — missing body → MalformedForm
        let (env, mut state) = empty();
        let pvec = Node::new(NodeKind::Vector(vec![sym_node("x")]), syn_span());
        let node = Node::new(NodeKind::List(vec![sym_node("fn"), pvec]), syn_span());
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 7 (fn) --
    #[test]
    fn infer_fn_malformed_params_not_vector() {
        // (fn 42 body) — params not a Vector → MalformedForm
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::List(vec![sym_node("fn"), int_node(42), int_node(99)]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 8 (fn) --
    #[test]
    fn infer_fn_malformed_param_not_symbol() {
        // (fn [42] body) — param name not a symbol → MalformedForm
        let (env, mut state) = empty();
        let pvec = Node::new(NodeKind::Vector(vec![int_node(42)]), syn_span());
        let node = Node::new(
            NodeKind::List(vec![sym_node("fn"), pvec, int_node(99)]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // if form tests
    // -----------------------------------------------------------------------

    // -- Test 1 (if) --
    #[test]
    fn infer_if_both_branches_int() {
        // (if true 1 2) → Int
        let (env, mut state) = empty();
        let node = if_node(atom_node(Atom::Bool(true)), int_node(1), int_node(2));
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 2 (if) --
    #[test]
    fn infer_if_returns_bool_branch_type() {
        // (if false true false) → Bool
        let (env, mut state) = empty();
        let node = if_node(
            atom_node(Atom::Bool(false)),
            atom_node(Atom::Bool(true)),
            atom_node(Atom::Bool(false)),
        );
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Bool);
    }

    // -- Test 3 (if) --
    #[test]
    fn infer_if_non_bool_condition_error() {
        // (if 42 1 2) → Mismatch {expected: Bool, found: Int}  (ADR-004)
        let (env, mut state) = empty();
        let node = if_node(int_node(42), int_node(1), int_node(2));
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::Mismatch { ref expected, ref found }
                if *expected == Type::Bool && *found == Type::Int
            ),
            "expected Mismatch(Bool, Int), got {err:?}"
        );
    }

    // -- Test 4 (if) --
    #[test]
    fn infer_if_branch_type_mismatch() {
        // (if true 42 "hello") → Mismatch {expected: Int, found: Str}
        let (env, mut state) = empty();
        let node = if_node(
            atom_node(Atom::Bool(true)),
            int_node(42),
            atom_node(Atom::Str("hello".into())),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch, got {err:?}"
        );
    }

    // -- Test 5 (if) --
    #[test]
    fn infer_if_condition_error() {
        // (if unknown 1 2) → UnboundVariable("unknown")
        let (env, mut state) = empty();
        let node = if_node(sym_node("unknown"), int_node(1), int_node(2));
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "expected UnboundVariable(unknown), got {err:?}"
        );
    }

    // -- Test 6 (if) --
    #[test]
    fn infer_if_error_in_then() {
        // (if true unknown 2) → UnboundVariable("unknown")
        let (env, mut state) = empty();
        let node = if_node(atom_node(Atom::Bool(true)), sym_node("unknown"), int_node(2));
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "expected UnboundVariable(unknown), got {err:?}"
        );
    }

    // -- Test 7 (if) --
    #[test]
    fn infer_if_error_in_else() {
        // (if true 1 unknown) → UnboundVariable("unknown")
        let (env, mut state) = empty();
        let node = if_node(atom_node(Atom::Bool(true)), int_node(1), sym_node("unknown"));
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "expected UnboundVariable(unknown), got {err:?}"
        );
    }

    // -- Test 8 (if) --
    #[test]
    fn infer_if_malformed_missing_else() {
        // (if true 1) — no else branch → MalformedForm
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::List(vec![sym_node("if"), atom_node(Atom::Bool(true)), int_node(1)]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 9 (if) --
    #[test]
    fn infer_if_malformed_too_many_args() {
        // (if true 1 2 3) — extra element → MalformedForm
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::List(vec![
                sym_node("if"),
                atom_node(Atom::Bool(true)),
                int_node(1),
                int_node(2),
                int_node(3),
            ]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
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

    // -----------------------------------------------------------------------
    // let-generalization tests
    // -----------------------------------------------------------------------

    // -- Test 1 (let-gen) --
    #[test]
    fn let_gen_identity_at_int() {
        // (let [id (fn [x] x)] (id 42)) → Int
        let (env, mut state) = empty();
        let node = let_node(
            vec![(sym_node("id"), fn_node(vec!["x"], sym_node("x")))],
            app_node(sym_node("id"), vec![int_node(42)]),
        );
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 2 (let-gen) --
    #[test]
    fn let_gen_identity_at_bool() {
        // (let [id (fn [x] x)] (id true)) → Bool
        let (env, mut state) = empty();
        let node = let_node(
            vec![(sym_node("id"), fn_node(vec!["x"], sym_node("x")))],
            app_node(sym_node("id"), vec![atom_node(Atom::Bool(true))]),
        );
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Bool);
    }

    // -- Test 3 (let-gen) — the distinguishing test --
    #[test]
    fn let_gen_identity_used_twice_different_types() {
        // (let [id (fn [x] x)] (if (id true) (id 42) 0)) → Int
        //
        // Without generalization, (id true) binds t0→Bool, then (id 42)
        // tries t0→Int and gets a Mismatch.  With generalization, id gets
        // scheme ∀a. (Fn [a] -> a) and each call instantiates a fresh var.
        let (env, mut state) = empty();
        let id_fn = fn_node(vec!["x"], sym_node("x"));
        let body = if_node(
            app_node(sym_node("id"), vec![atom_node(Atom::Bool(true))]),
            app_node(sym_node("id"), vec![int_node(42)]),
            int_node(0),
        );
        let node = let_node(vec![(sym_node("id"), id_fn)], body);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 4 (let-gen) --
    #[test]
    fn let_gen_two_independent_poly_bindings() {
        // (let [f (fn [x] x) g (fn [y] y)] (if (f true) (g 42) 0)) → Int
        // Both f and g are independently generalized; each can be used
        // at different types without conflict.
        let (env, mut state) = empty();
        let node = let_node(
            vec![
                (sym_node("f"), fn_node(vec!["x"], sym_node("x"))),
                (sym_node("g"), fn_node(vec!["y"], sym_node("y"))),
            ],
            if_node(
                app_node(sym_node("f"), vec![atom_node(Atom::Bool(true))]),
                app_node(sym_node("g"), vec![int_node(42)]),
                int_node(0),
            ),
        );
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 5 (let-gen) --
    #[test]
    fn let_gen_sequential_poly_then_apply() {
        // (let [id (fn [x] x) y (id 42)] y) → Int
        // Second binding uses the generalized first binding.
        let (env, mut state) = empty();
        let node = let_node(
            vec![
                (sym_node("id"), fn_node(vec!["x"], sym_node("x"))),
                (sym_node("y"), app_node(sym_node("id"), vec![int_node(42)])),
            ],
            sym_node("y"),
        );
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 6 (let-gen) --
    #[test]
    fn let_gen_mono_literal_unchanged() {
        // (let [n 42] n) → Int — monomorphic literal, no free vars to generalize.
        let (env, mut state) = empty();
        let node = let_node(vec![(sym_node("n"), int_node(42))], sym_node("n"));
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -----------------------------------------------------------------------
    // loop / recur form tests
    // -----------------------------------------------------------------------

    /// Build `(loop [k0 v0 k1 v1 ...] body)` as a List node.
    fn loop_node(bindings: Vec<(Node, Node)>, body: Node) -> Node {
        let head = sym_node("loop");
        let bvec: Vec<Node> = bindings.into_iter().flat_map(|(k, v)| [k, v]).collect();
        let bvec_node = Node::new(NodeKind::Vector(bvec), syn_span());
        Node::new(NodeKind::List(vec![head, bvec_node, body]), syn_span())
    }

    /// Build `(recur arg0 arg1 ...)` as a List node.
    fn recur_node(args: Vec<Node>) -> Node {
        let mut items = vec![sym_node("recur")];
        items.extend(args);
        Node::new(NodeKind::List(items), syn_span())
    }

    // -- Test 9 (loop) --
    #[test]
    fn loop_malformed_odd_bindings() {
        // (loop [x] body) — odd binding vector → MalformedForm
        let (env, mut state) = empty();
        let bvec = Node::new(NodeKind::Vector(vec![sym_node("x")]), syn_span());
        let node = Node::new(
            NodeKind::List(vec![sym_node("loop"), bvec, int_node(42)]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 10 (loop) --
    #[test]
    fn loop_malformed_missing_body() {
        // (loop [x 1]) — missing body → MalformedForm
        let (env, mut state) = empty();
        let bvec = Node::new(
            NodeKind::Vector(vec![sym_node("x"), int_node(1)]),
            syn_span(),
        );
        let node = Node::new(
            NodeKind::List(vec![sym_node("loop"), bvec]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 8 (loop) --
    #[test]
    fn loop_recur_outside_loop() {
        // (recur 42) with no enclosing loop → MalformedForm
        let (env, mut state) = empty();
        let node = recur_node(vec![int_node(42)]);
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 7 (loop) --
    #[test]
    fn loop_multi_var_correct() {
        // (loop [x 0 b true] (if b x (recur 1 false))) → Int
        // Two loop vars: x : Int, b : Bool.
        // recur passes (1 : Int, false : Bool) — both match.
        // then branch returns x : Int; else branch returns Never.
        let (env, mut state) = empty();
        let body = if_node(
            sym_node("b"),
            sym_node("x"),
            recur_node(vec![int_node(1), atom_node(Atom::Bool(false))]),
        );
        let node = loop_node(
            vec![
                (sym_node("x"), int_node(0)),
                (sym_node("b"), atom_node(Atom::Bool(true))),
            ],
            body,
        );
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 5 (loop) --
    #[test]
    fn loop_recur_arity_too_many() {
        // (loop [i 0] (recur 1 2)) — 1 loop var but recur passes 2
        // → ArityMismatch { expected: 1, found: 2 }
        let (env, mut state) = empty();
        let node = loop_node(
            vec![(sym_node("i"), int_node(0))],
            recur_node(vec![int_node(1), int_node(2)]),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::ArityMismatch { expected: 1, found: 2 }),
            "expected ArityMismatch(1,2), got {err:?}"
        );
    }

    // -- Test 6 (loop) --
    #[test]
    fn loop_recur_arity_too_few() {
        // (loop [i 0] (recur)) — 1 loop var but recur passes 0
        // → ArityMismatch { expected: 1, found: 0 }
        let (env, mut state) = empty();
        let node = loop_node(
            vec![(sym_node("i"), int_node(0))],
            recur_node(vec![]),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::ArityMismatch { expected: 1, found: 0 }),
            "expected ArityMismatch(1,0), got {err:?}"
        );
    }

    // -- Test 4 (loop) --
    #[test]
    fn loop_recur_arg_type_mismatch() {
        // (loop [i 0] (recur true)) — loop var is Int, recur passes Bool
        // → Mismatch { expected: Int, found: Bool }
        let (env, mut state) = empty();
        let node = loop_node(
            vec![(sym_node("i"), int_node(0))],
            recur_node(vec![atom_node(Atom::Bool(true))]),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::Mismatch { ref expected, ref found }
                if *expected == Type::Int && *found == Type::Bool
            ),
            "expected Mismatch(Int, Bool), got {err:?}"
        );
    }

    // -- Test 3 (loop) --
    #[test]
    fn loop_recur_correct() {
        // (loop [i 0] (if true 99 (recur 1))) → Int
        // The `then` branch is Int; `(recur 1)` has type Never which unifies
        // with Int (spec §5.3 — Never is the bottom type).
        let (env, mut state) = empty();
        let body = if_node(
            atom_node(Atom::Bool(true)),
            int_node(99),
            recur_node(vec![int_node(1)]),
        );
        let node = loop_node(vec![(sym_node("i"), int_node(0))], body);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 2 (loop) --
    #[test]
    fn loop_empty_bindings() {
        // (loop [] 99) → Int  (no loop variables, body is a literal)
        let (env, mut state) = empty();
        let node = loop_node(vec![], int_node(99));
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 1 (loop) --
    #[test]
    fn loop_degenerate_returns_body() {
        // (loop [x 42] x) → Int  (loop var accessible in body, no recur)
        let (env, mut state) = empty();
        let node = loop_node(vec![(sym_node("x"), int_node(42))], sym_node("x"));
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -----------------------------------------------------------------------
    // Function application tests
    // -----------------------------------------------------------------------

    /// Build `(callee arg0 arg1 ...)` as a List node.
    fn app_node(callee: Node, args: Vec<Node>) -> Node {
        let mut items = vec![callee];
        items.extend(args);
        Node::new(NodeKind::List(items), syn_span())
    }

    // -- Test 10 (apply) --
    #[test]
    fn apply_callee_unbound() {
        // (unknown 1) → UnboundVariable("unknown")
        let (env, mut state) = empty();
        let node = app_node(sym_node("unknown"), vec![int_node(1)]);
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "expected UnboundVariable(unknown), got {err:?}"
        );
    }

    // -- Test 9 (apply) --
    #[test]
    fn apply_inline_lambda() {
        // ((fn [x] x) 42) → Int
        let (env, mut state) = empty();
        let lambda = fn_node(vec!["x"], sym_node("x"));
        let node = app_node(lambda, vec![int_node(42)]);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 8 (apply) --
    #[test]
    fn apply_polymorphic_identity() {
        // id : ∀a. (Fn [a] -> a); (id 42) → Int
        use std::collections::HashSet;
        let mut state = InferState::new();
        let t0 = state.supply.fresh(); // consume t0 for the scheme
        let scheme = nexl_types::Scheme {
            forall: [t0].into_iter().collect::<HashSet<_>>(),
            body: Type::Fn { params: vec![Type::Var(t0)], ret: Box::new(Type::Var(t0)) },
        };
        let env = Env::new().extend("id", scheme);
        let node = app_node(sym_node("id"), vec![int_node(42)]);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 7 (apply) --
    #[test]
    fn apply_not_a_function() {
        // (42 1) — Int is not callable → Mismatch
        let (env, mut state) = empty();
        let node = app_node(int_node(42), vec![int_node(1)]);
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch (Int is not a function), got {err:?}"
        );
    }

    // -- Test 6 (apply) --
    #[test]
    fn apply_arity_too_few() {
        // (f) where f : (Fn [Int] -> Bool) → ArityMismatch {expected: 1, found: 0}
        let f_ty = Type::Fn { params: vec![Type::Int], ret: Box::new(Type::Bool) };
        let env = Env::new().extend("f", Scheme::mono(f_ty));
        let mut state = InferState::new();
        let node = app_node(sym_node("f"), vec![]);
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::ArityMismatch { expected: 1, found: 0 }
            ),
            "expected ArityMismatch(1,0), got {err:?}"
        );
    }

    // -- Test 5 (apply) --
    #[test]
    fn apply_arity_too_many() {
        // (f 1 2) where f : (Fn [Int] -> Bool) → ArityMismatch {expected: 1, found: 2}
        let f_ty = Type::Fn { params: vec![Type::Int], ret: Box::new(Type::Bool) };
        let env = Env::new().extend("f", Scheme::mono(f_ty));
        let mut state = InferState::new();
        let node = app_node(sym_node("f"), vec![int_node(1), int_node(2)]);
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::ArityMismatch { expected: 1, found: 2 }
            ),
            "expected ArityMismatch(1,2), got {err:?}"
        );
    }

    // -- Test 4 (apply) --
    #[test]
    fn apply_arg_type_mismatch() {
        // (f true) where f : (Fn [Int] -> Bool) → Mismatch {expected: Int, found: Bool}
        let f_ty = Type::Fn { params: vec![Type::Int], ret: Box::new(Type::Bool) };
        let env = Env::new().extend("f", Scheme::mono(f_ty));
        let mut state = InferState::new();
        let node = app_node(sym_node("f"), vec![atom_node(Atom::Bool(true))]);
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::Mismatch { ref expected, ref found }
                if *expected == Type::Int && *found == Type::Bool
            ),
            "expected Mismatch(Int, Bool), got {err:?}"
        );
    }

    // -- Test 3 (apply) --
    #[test]
    fn apply_two_arg_fn() {
        // (f 42 "hello") where f : (Fn [Int Str] -> Float) → Float
        let f_ty = Type::Fn { params: vec![Type::Int, Type::Str], ret: Box::new(Type::Float) };
        let env = Env::new().extend("f", Scheme::mono(f_ty));
        let mut state = InferState::new();
        let node = app_node(sym_node("f"), vec![int_node(42), atom_node(Atom::Str("hello".into()))]);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Float);
    }

    // -- Test 2 (apply) --
    #[test]
    fn apply_one_arg_fn() {
        // (f 42) where f : (Fn [Int] -> Bool) → Bool
        let f_ty = Type::Fn { params: vec![Type::Int], ret: Box::new(Type::Bool) };
        let env = Env::new().extend("f", Scheme::mono(f_ty));
        let mut state = InferState::new();
        let node = app_node(sym_node("f"), vec![int_node(42)]);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Bool);
    }

    // -- Test 1 (apply) --
    #[test]
    fn apply_zero_arg_fn() {
        // (f) where f : (Fn [] -> Int) → Int
        let f_ty = Type::Fn { params: vec![], ret: Box::new(Type::Int) };
        let env = Env::new().extend("f", Scheme::mono(f_ty));
        let mut state = InferState::new();
        let node = app_node(sym_node("f"), vec![]);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
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
