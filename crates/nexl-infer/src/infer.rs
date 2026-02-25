//! Synthesis and checking modes for the bidirectional inference engine.

#![allow(clippy::result_large_err)]

use std::collections::{HashMap, HashSet};

use nexl_ast::{Atom, FloatSuffix, IntSuffix, Node, NodeKind, Pattern, parse_pattern};
use nexl_types::{
    Constructor, EffectRow, Scheme, Subst, Type, TypeDef, TypeError, TypeErrorKind, TypeVar,
    TypeVarSupply,
};

use crate::Env;
use crate::env::RecordDef;

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
    /// Non-fatal errors accumulated during inference (Principle 6: don't stop at first).
    ///
    /// Sequential forms (`do`, `let` bindings, function arguments) push errors here
    /// instead of short-circuiting, so the caller sees all type errors at once.
    pub errors: Vec<TypeError>,
    /// Non-fatal warnings accumulated during inference.
    pub warnings: Vec<TypeError>,
}

impl InferState {
    /// Create a fresh inference state with no bindings.
    pub fn new() -> Self {
        Self {
            supply: TypeVarSupply::new(),
            subst: Subst::empty(),
            recur_types: None,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Allocate a fresh unification variable and return it as a `Type`.
    pub fn fresh_var(&mut self) -> Type {
        Type::Var(self.supply.fresh())
    }

    /// Push a non-fatal error into the accumulated error list.
    pub fn push_error(&mut self, e: TypeError) {
        self.errors.push(e);
    }

    /// Push a non-fatal warning into the accumulated warning list.
    pub fn push_warning(&mut self, w: TypeError) {
        self.warnings.push(w);
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
    let result = match &node.kind {
        NodeKind::Atom(atom) => synth_atom(atom, env, state),
        NodeKind::List(items) => {
            if matches!(head_sym(items), Some("match")) {
                synth_match(node, env, state)
            } else {
                synth_list(items, env, state)
            }
        }
        NodeKind::Vector(items) => synth_vec_literal(items, env, state),
        NodeKind::Map(entries) => synth_map_literal(entries, env, state),
        NodeKind::Set(items) => synth_set_literal(items, env, state),
        _ => unimplemented!("synth: {:?}", node.kind),
    };
    // Attach this node's span to any error that doesn't already carry one.
    // The innermost span wins: if an error already has a span from a deeper
    // call, we don't overwrite it.
    result.map_err(|e| {
        if e.span.is_none() && !node.span.is_synthetic() {
            e.with_span(node.span)
        } else {
            e
        }
    })
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
        Some("each") => synth_each(items, env, state),
        Some("times") => synth_times(items, env, state),
        Some("for") => synth_for(items, env, state),
        Some("for!") => synth_for(items, env, state),
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

fn synth_each(items: &[Node], env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    if items.len() < 3 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "each expects (each [binding coll] body...), got {} elements",
                items.len()
            ),
        }));
    }

    let bvec = match &items[1].kind {
        NodeKind::Vector(items) => items,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "each bindings must be a vector".to_string(),
            }));
        }
    };

    if bvec.len() != 2 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "each expects a binding vector of length 2, got {}",
                bvec.len()
            ),
        }));
    }

    let name = match &bvec[0].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "each binding name must be an unqualified symbol".to_string(),
            }));
        }
    };

    let coll_ty = synth(&bvec[1], env, state)?;
    let elem_ty = infer_each_elem_type(&coll_ty, state)?;
    let body_env = env.extend(name, Scheme::mono(elem_ty));

    let exprs = &items[2..];
    if exprs.is_empty() {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "each requires at least one body expression".to_string(),
        }));
    }

    for expr in exprs {
        if let Err(e) = synth(expr, &body_env, state) {
            state.push_error(e);
        }
    }
    Ok(Type::Unit)
}

fn synth_times(items: &[Node], env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    if items.len() < 3 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "times expects (times [binding n] body...), got {} elements",
                items.len()
            ),
        }));
    }

    let bvec = match &items[1].kind {
        NodeKind::Vector(items) => items,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "times bindings must be a vector".to_string(),
            }));
        }
    };

    if bvec.len() != 2 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "times expects a binding vector of length 2, got {}",
                bvec.len()
            ),
        }));
    }

    let name = match &bvec[0].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "times binding name must be an unqualified symbol".to_string(),
            }));
        }
    };

    let count_ty = synth(&bvec[1], env, state)?;
    nexl_types::unify(&Type::Int, &count_ty, &mut state.subst)?;

    let body_env = env.extend(name, Scheme::mono(Type::Int));
    let exprs = &items[2..];
    if exprs.is_empty() {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "times requires at least one body expression".to_string(),
        }));
    }

    for expr in exprs {
        if let Err(e) = synth(expr, &body_env, state) {
            state.push_error(e);
        }
    }
    Ok(Type::Unit)
}

fn synth_for(items: &[Node], env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    if items.len() < 3 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "for expects (for [bindings...] body...), got {} elements",
                items.len()
            ),
        }));
    }

    let bvec = match &items[1].kind {
        NodeKind::Vector(items) => items,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "for bindings must be a vector".to_string(),
            }));
        }
    };

    let mut current_env = env.clone();
    let mut i = 0;
    while i < bvec.len() {
        match &bvec[i].kind {
            NodeKind::Atom(Atom::Keyword { ns: None, name }) if name == "when" => {
                let cond_node = bvec.get(i + 1).ok_or_else(|| {
                    TypeError::new(TypeErrorKind::MalformedForm {
                        description: "for :when requires a condition expression".to_string(),
                    })
                })?;
                let cond_ty = synth(cond_node, &current_env, state)?;
                nexl_types::unify(&Type::Bool, &cond_ty, &mut state.subst)?;
                i += 2;
            }
            NodeKind::Atom(Atom::Keyword { ns: None, name }) if name == "while" => {
                let cond_node = bvec.get(i + 1).ok_or_else(|| {
                    TypeError::new(TypeErrorKind::MalformedForm {
                        description: "for :while requires a condition expression".to_string(),
                    })
                })?;
                let cond_ty = synth(cond_node, &current_env, state)?;
                nexl_types::unify(&Type::Bool, &cond_ty, &mut state.subst)?;
                i += 2;
            }
            NodeKind::Atom(Atom::Keyword { ns: None, name }) if name == "let" => {
                let binding_node = bvec.get(i + 1).ok_or_else(|| {
                    TypeError::new(TypeErrorKind::MalformedForm {
                        description: "for :let requires a binding vector".to_string(),
                    })
                })?;
                let binding_vec = match &binding_node.kind {
                    NodeKind::Vector(items) => items,
                    _ => {
                        return Err(TypeError::new(TypeErrorKind::MalformedForm {
                            description: "for :let expects a vector of bindings".to_string(),
                        }));
                    }
                };
                current_env = extend_for_let_bindings(&current_env, binding_vec, state)?;
                i += 2;
            }
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => {
                let coll_node = bvec.get(i + 1).ok_or_else(|| {
                    TypeError::new(TypeErrorKind::MalformedForm {
                        description: "for binding is missing its collection expression".to_string(),
                    })
                })?;
                let coll_ty = synth(coll_node, &current_env, state)?;
                let elem_ty = infer_each_elem_type(&coll_ty, state)?;
                current_env = current_env.extend(name.clone(), Scheme::mono(elem_ty));
                i += 2;
            }
            _ => {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: "for bindings must be symbol/keyword clauses".to_string(),
                }));
            }
        }
    }

    let body_exprs = &items[2..];
    if body_exprs.is_empty() {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "for requires at least one body expression".to_string(),
        }));
    }

    let mut body_ty = Type::Unit;
    for expr in body_exprs {
        match synth(expr, &current_env, state) {
            Ok(ty) => body_ty = ty,
            Err(e) => {
                state.push_error(e);
                body_ty = state.fresh_var();
            }
        }
    }

    Ok(Type::Vec(Box::new(state.subst.apply(&body_ty))))
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

    if let Some(name) = head_sym(items)
        && env.lookup_record_def(name).is_some()
    {
        return synth_record_constructor(name, &items[1..], env, state);
    }
    if let Some(field) = head_keyword(items) {
        return synth_keyword_access(field, &items[1..], env, state);
    }
    if let Some(name) = head_sym(items)
        && env.lookup(name).is_none()
        && let Some(ty) = synth_collection_op(name, &items[1..], env, state)?
    {
        return Ok(ty);
    }

    let callee_node = &items[0];
    let arg_nodes = &items[1..];

    // Synthesize the callee type.
    let callee_ty = synth(callee_node, env, state)?;

    // Synthesize each argument type in order, collecting errors rather than
    // stopping at the first bad argument (Principle 6).
    let mut arg_types: Vec<Type> = Vec::with_capacity(arg_nodes.len());
    for arg in arg_nodes {
        match synth(arg, env, state) {
            Ok(ty) => arg_types.push(ty),
            Err(e) => {
                state.push_error(e);
                arg_types.push(state.fresh_var());
            }
        }
    }

    // Introduce a fresh return type variable.
    let ret_var = state.fresh_var();

    // Unify the callee with the expected function shape.
    // Any arity or type mismatch surfaces here.
    let expected_fn = Type::Fn {
        params: arg_types.clone(),
        ret: Box::new(ret_var.clone()),
        effects: EffectRow::empty(),
    };
    nexl_types::unify(&callee_ty, &expected_fn, &mut state.subst)
        .map_err(|e| arithmetic_help(e, head_sym(items), &arg_types))?;

    Ok(state.subst.apply(&ret_var))
}

fn synth_collection_op(
    name: &str,
    arg_nodes: &[Node],
    env: &Env,
    state: &mut InferState,
) -> Result<Option<Type>, TypeError> {
    let arg_types = synth_arg_types(arg_nodes, env, state);
    let ty = match name {
        "get" => {
            if arg_types.len() != 2 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 2,
                    found: arg_types.len(),
                }));
            }
            let coll_ty = &arg_types[0];
            let key_ty = &arg_types[1];
            Some(infer_get(coll_ty, key_ty, state)?)
        }
        "put" => {
            if arg_types.len() != 3 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 3,
                    found: arg_types.len(),
                }));
            }
            let coll_ty = &arg_types[0];
            let key_ty = &arg_types[1];
            let val_ty = &arg_types[2];
            Some(infer_put(coll_ty, key_ty, val_ty, state)?)
        }
        "append" => {
            if arg_types.len() != 2 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 2,
                    found: arg_types.len(),
                }));
            }
            let elem_var = state.fresh_var();
            let vec_ty = Type::Vec(Box::new(elem_var.clone()));
            nexl_types::unify(&vec_ty, &arg_types[0], &mut state.subst)?;
            nexl_types::unify(&elem_var, &arg_types[1], &mut state.subst)?;
            Some(Type::Vec(Box::new(state.subst.apply(&elem_var))))
        }
        "first" => {
            if arg_types.len() != 1 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 1,
                    found: arg_types.len(),
                }));
            }
            let elem_var = state.fresh_var();
            let vec_ty = Type::Vec(Box::new(elem_var.clone()));
            nexl_types::unify(&vec_ty, &arg_types[0], &mut state.subst)?;
            Some(option_type(state.subst.apply(&elem_var)))
        }
        "rest" => {
            if arg_types.len() != 1 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 1,
                    found: arg_types.len(),
                }));
            }
            let elem_var = state.fresh_var();
            let vec_ty = Type::Vec(Box::new(elem_var.clone()));
            nexl_types::unify(&vec_ty, &arg_types[0], &mut state.subst)?;
            Some(Type::Vec(Box::new(state.subst.apply(&elem_var))))
        }
        "last" => {
            if arg_types.len() != 1 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 1,
                    found: arg_types.len(),
                }));
            }
            let elem_var = state.fresh_var();
            let vec_ty = Type::Vec(Box::new(elem_var.clone()));
            nexl_types::unify(&vec_ty, &arg_types[0], &mut state.subst)?;
            Some(option_type(state.subst.apply(&elem_var)))
        }
        "slice" => {
            if arg_types.len() != 3 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 3,
                    found: arg_types.len(),
                }));
            }
            let elem_var = state.fresh_var();
            let vec_ty = Type::Vec(Box::new(elem_var.clone()));
            nexl_types::unify(&vec_ty, &arg_types[0], &mut state.subst)?;
            nexl_types::unify(&Type::Int, &arg_types[1], &mut state.subst)?;
            nexl_types::unify(&Type::Int, &arg_types[2], &mut state.subst)?;
            Some(Type::Vec(Box::new(state.subst.apply(&elem_var))))
        }
        "remove" => {
            if arg_types.len() != 2 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 2,
                    found: arg_types.len(),
                }));
            }
            let coll_ty = state.subst.apply(&arg_types[0]);
            let key_ty = &arg_types[1];
            match coll_ty {
                Type::Map { key, val } => {
                    nexl_types::unify(&key, key_ty, &mut state.subst)?;
                    Some(Type::Map {
                        key: Box::new(state.subst.apply(&key)),
                        val: Box::new(state.subst.apply(&val)),
                    })
                }
                Type::Set(elem) => {
                    nexl_types::unify(&elem, key_ty, &mut state.subst)?;
                    Some(Type::Set(Box::new(state.subst.apply(&elem))))
                }
                other => {
                    return Err(TypeError::new(TypeErrorKind::Mismatch {
                        expected: Type::Map {
                            key: Box::new(state.fresh_var()),
                            val: Box::new(state.fresh_var()),
                        },
                        found: other,
                    }));
                }
            }
        }
        "keys" => {
            if arg_types.len() != 1 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 1,
                    found: arg_types.len(),
                }));
            }
            let key_var = state.fresh_var();
            let val_var = state.fresh_var();
            let map_ty = Type::Map {
                key: Box::new(key_var.clone()),
                val: Box::new(val_var),
            };
            nexl_types::unify(&map_ty, &arg_types[0], &mut state.subst)?;
            Some(Type::Vec(Box::new(state.subst.apply(&key_var))))
        }
        "vals" => {
            if arg_types.len() != 1 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 1,
                    found: arg_types.len(),
                }));
            }
            let key_var = state.fresh_var();
            let val_var = state.fresh_var();
            let map_ty = Type::Map {
                key: Box::new(key_var),
                val: Box::new(val_var.clone()),
            };
            nexl_types::unify(&map_ty, &arg_types[0], &mut state.subst)?;
            Some(Type::Vec(Box::new(state.subst.apply(&val_var))))
        }
        "entries" => {
            if arg_types.len() != 1 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 1,
                    found: arg_types.len(),
                }));
            }
            let key_var = state.fresh_var();
            let val_var = state.fresh_var();
            let map_ty = Type::Map {
                key: Box::new(key_var.clone()),
                val: Box::new(val_var.clone()),
            };
            nexl_types::unify(&map_ty, &arg_types[0], &mut state.subst)?;
            Some(Type::Vec(Box::new(Type::Tuple(vec![
                state.subst.apply(&key_var),
                state.subst.apply(&val_var),
            ]))))
        }
        "contains?" => {
            if arg_types.len() != 2 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 2,
                    found: arg_types.len(),
                }));
            }
            let coll_ty = state.subst.apply(&arg_types[0]);
            let key_ty = &arg_types[1];
            match coll_ty {
                Type::Map { key, .. } => {
                    nexl_types::unify(&key, key_ty, &mut state.subst)?;
                    Some(Type::Bool)
                }
                Type::Set(elem) => {
                    nexl_types::unify(&elem, key_ty, &mut state.subst)?;
                    Some(Type::Bool)
                }
                other => {
                    return Err(TypeError::new(TypeErrorKind::Mismatch {
                        expected: Type::Map {
                            key: Box::new(state.fresh_var()),
                            val: Box::new(state.fresh_var()),
                        },
                        found: other,
                    }));
                }
            }
        }
        "count" => {
            if arg_types.len() != 1 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 1,
                    found: arg_types.len(),
                }));
            }
            let coll_ty = state.subst.apply(&arg_types[0]);
            match coll_ty {
                Type::Str | Type::Vec(_) | Type::Map { .. } | Type::Set(_) | Type::Var(_) => {
                    Some(Type::Int)
                }
                other => {
                    return Err(TypeError::new(TypeErrorKind::Mismatch {
                        expected: Type::Vec(Box::new(state.fresh_var())),
                        found: other,
                    }));
                }
            }
        }
        "add" => {
            if arg_types.len() != 2 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 2,
                    found: arg_types.len(),
                }));
            }
            let elem_var = state.fresh_var();
            let set_ty = Type::Set(Box::new(elem_var.clone()));
            nexl_types::unify(&set_ty, &arg_types[0], &mut state.subst)?;
            nexl_types::unify(&elem_var, &arg_types[1], &mut state.subst)?;
            Some(Type::Set(Box::new(state.subst.apply(&elem_var))))
        }
        "union" | "intersection" | "difference" => {
            if arg_types.len() != 2 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 2,
                    found: arg_types.len(),
                }));
            }
            let elem_var = state.fresh_var();
            let set_ty = Type::Set(Box::new(elem_var.clone()));
            nexl_types::unify(&set_ty, &arg_types[0], &mut state.subst)?;
            nexl_types::unify(&set_ty, &arg_types[1], &mut state.subst)?;
            Some(Type::Set(Box::new(state.subst.apply(&elem_var))))
        }
        "map" => {
            if arg_types.len() != 2 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 2,
                    found: arg_types.len(),
                }));
            }
            Some(infer_map_op(&arg_types[0], &arg_types[1], state)?)
        }
        "filter" => {
            if arg_types.len() != 2 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 2,
                    found: arg_types.len(),
                }));
            }
            Some(infer_filter_op(&arg_types[0], &arg_types[1], state)?)
        }
        "reduce" => {
            if arg_types.len() != 3 {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 3,
                    found: arg_types.len(),
                }));
            }
            Some(infer_reduce_op(
                &arg_types[0],
                &arg_types[1],
                &arg_types[2],
                state,
            )?)
        }
        _ => None,
    };
    Ok(ty)
}

fn synth_arg_types(arg_nodes: &[Node], env: &Env, state: &mut InferState) -> Vec<Type> {
    let mut arg_types = Vec::with_capacity(arg_nodes.len());
    for arg in arg_nodes {
        match synth(arg, env, state) {
            Ok(ty) => arg_types.push(ty),
            Err(e) => {
                state.push_error(e);
                arg_types.push(state.fresh_var());
            }
        }
    }
    arg_types
}

fn option_type(inner: Type) -> Type {
    Type::Adt {
        name: "Option".to_string(),
        args: vec![inner],
    }
}

fn infer_get(coll_ty: &Type, key_ty: &Type, state: &mut InferState) -> Result<Type, TypeError> {
    let resolved = state.subst.apply(coll_ty);
    match resolved {
        Type::Vec(elem) => {
            nexl_types::unify(&Type::Int, key_ty, &mut state.subst)?;
            Ok(option_type(state.subst.apply(&elem)))
        }
        Type::Map { key, val } => {
            nexl_types::unify(&key, key_ty, &mut state.subst)?;
            Ok(option_type(state.subst.apply(&val)))
        }
        Type::Var(_) => {
            let snapshot = state.subst.clone();
            if nexl_types::unify(&Type::Int, key_ty, &mut state.subst).is_ok() {
                let elem_var = state.fresh_var();
                let vec_ty = Type::Vec(Box::new(elem_var.clone()));
                nexl_types::unify(&vec_ty, coll_ty, &mut state.subst)?;
                return Ok(option_type(state.subst.apply(&elem_var)));
            }
            state.subst = snapshot;
            let key_var = state.fresh_var();
            let val_var = state.fresh_var();
            let map_ty = Type::Map {
                key: Box::new(key_var.clone()),
                val: Box::new(val_var.clone()),
            };
            nexl_types::unify(&map_ty, coll_ty, &mut state.subst)?;
            nexl_types::unify(&key_var, key_ty, &mut state.subst)?;
            Ok(option_type(state.subst.apply(&val_var)))
        }
        other => Err(TypeError::new(TypeErrorKind::Mismatch {
            expected: Type::Vec(Box::new(state.fresh_var())),
            found: other,
        })),
    }
}

fn infer_put(
    coll_ty: &Type,
    key_ty: &Type,
    val_ty: &Type,
    state: &mut InferState,
) -> Result<Type, TypeError> {
    let resolved = state.subst.apply(coll_ty);
    match resolved {
        Type::Vec(elem) => {
            nexl_types::unify(&Type::Int, key_ty, &mut state.subst)?;
            nexl_types::unify(&elem, val_ty, &mut state.subst)?;
            Ok(Type::Vec(Box::new(state.subst.apply(&elem))))
        }
        Type::Map { key, val } => {
            nexl_types::unify(&key, key_ty, &mut state.subst)?;
            nexl_types::unify(&val, val_ty, &mut state.subst)?;
            Ok(Type::Map {
                key: Box::new(state.subst.apply(&key)),
                val: Box::new(state.subst.apply(&val)),
            })
        }
        Type::Var(_) => {
            let snapshot = state.subst.clone();
            if nexl_types::unify(&Type::Int, key_ty, &mut state.subst).is_ok() {
                let elem_var = state.fresh_var();
                let vec_ty = Type::Vec(Box::new(elem_var.clone()));
                nexl_types::unify(&vec_ty, coll_ty, &mut state.subst)?;
                nexl_types::unify(&elem_var, val_ty, &mut state.subst)?;
                return Ok(Type::Vec(Box::new(state.subst.apply(&elem_var))));
            }
            state.subst = snapshot;
            let key_var = state.fresh_var();
            let val_var = state.fresh_var();
            let map_ty = Type::Map {
                key: Box::new(key_var.clone()),
                val: Box::new(val_var.clone()),
            };
            nexl_types::unify(&map_ty, coll_ty, &mut state.subst)?;
            nexl_types::unify(&key_var, key_ty, &mut state.subst)?;
            nexl_types::unify(&val_var, val_ty, &mut state.subst)?;
            Ok(Type::Map {
                key: Box::new(state.subst.apply(&key_var)),
                val: Box::new(state.subst.apply(&val_var)),
            })
        }
        other => Err(TypeError::new(TypeErrorKind::Mismatch {
            expected: Type::Vec(Box::new(state.fresh_var())),
            found: other,
        })),
    }
}

fn infer_map_op(fn_ty: &Type, coll_ty: &Type, state: &mut InferState) -> Result<Type, TypeError> {
    let elem_var = state.fresh_var();
    let out_var = state.fresh_var();
    let expected_fn = Type::Fn {
        params: vec![elem_var.clone()],
        ret: Box::new(out_var.clone()),
        effects: EffectRow::empty(),
    };
    nexl_types::unify(fn_ty, &expected_fn, &mut state.subst)?;

    let resolved = state.subst.apply(coll_ty);
    match resolved {
        Type::Vec(elem) => {
            nexl_types::unify(&elem, &elem_var, &mut state.subst)?;
            Ok(Type::Vec(Box::new(state.subst.apply(&out_var))))
        }
        Type::Set(elem) => {
            nexl_types::unify(&elem, &elem_var, &mut state.subst)?;
            Ok(Type::Set(Box::new(state.subst.apply(&out_var))))
        }
        Type::Map { key, val } => {
            nexl_types::unify(&val, &elem_var, &mut state.subst)?;
            Ok(Type::Map {
                key: Box::new(state.subst.apply(&key)),
                val: Box::new(state.subst.apply(&out_var)),
            })
        }
        Type::Adt { name, args } if name == "Option" && args.len() == 1 => {
            nexl_types::unify(&args[0], &elem_var, &mut state.subst)?;
            Ok(option_type(state.subst.apply(&out_var)))
        }
        other => Err(TypeError::new(TypeErrorKind::Mismatch {
            expected: Type::Vec(Box::new(state.fresh_var())),
            found: other,
        })),
    }
}

fn infer_filter_op(
    fn_ty: &Type,
    coll_ty: &Type,
    state: &mut InferState,
) -> Result<Type, TypeError> {
    let elem_var = state.fresh_var();
    let expected_fn = Type::Fn {
        params: vec![elem_var.clone()],
        ret: Box::new(Type::Bool),
        effects: EffectRow::empty(),
    };
    nexl_types::unify(fn_ty, &expected_fn, &mut state.subst)?;

    let resolved = state.subst.apply(coll_ty);
    match resolved {
        Type::Vec(elem) => {
            nexl_types::unify(&elem, &elem_var, &mut state.subst)?;
            Ok(Type::Vec(Box::new(state.subst.apply(&elem))))
        }
        Type::Set(elem) => {
            nexl_types::unify(&elem, &elem_var, &mut state.subst)?;
            Ok(Type::Set(Box::new(state.subst.apply(&elem))))
        }
        Type::Map { key, val } => {
            nexl_types::unify(&val, &elem_var, &mut state.subst)?;
            Ok(Type::Map {
                key: Box::new(state.subst.apply(&key)),
                val: Box::new(state.subst.apply(&val)),
            })
        }
        Type::Adt { name, args } if name == "Option" && args.len() == 1 => {
            nexl_types::unify(&args[0], &elem_var, &mut state.subst)?;
            Ok(option_type(state.subst.apply(&elem_var)))
        }
        other => Err(TypeError::new(TypeErrorKind::Mismatch {
            expected: Type::Vec(Box::new(state.fresh_var())),
            found: other,
        })),
    }
}

fn infer_reduce_op(
    fn_ty: &Type,
    init_ty: &Type,
    coll_ty: &Type,
    state: &mut InferState,
) -> Result<Type, TypeError> {
    let acc_var = state.fresh_var();
    let elem_var = state.fresh_var();
    let expected_fn = Type::Fn {
        params: vec![acc_var.clone(), elem_var.clone()],
        ret: Box::new(acc_var.clone()),
        effects: EffectRow::empty(),
    };
    nexl_types::unify(fn_ty, &expected_fn, &mut state.subst)?;
    nexl_types::unify(init_ty, &acc_var, &mut state.subst)?;

    let resolved = state.subst.apply(coll_ty);
    match resolved {
        Type::Vec(elem) => {
            nexl_types::unify(&elem, &elem_var, &mut state.subst)?;
            Ok(state.subst.apply(&acc_var))
        }
        Type::Set(elem) => {
            nexl_types::unify(&elem, &elem_var, &mut state.subst)?;
            Ok(state.subst.apply(&acc_var))
        }
        Type::Map { val, .. } => {
            nexl_types::unify(&val, &elem_var, &mut state.subst)?;
            Ok(state.subst.apply(&acc_var))
        }
        Type::Adt { name, args } if name == "Option" && args.len() == 1 => {
            nexl_types::unify(&args[0], &elem_var, &mut state.subst)?;
            Ok(state.subst.apply(&acc_var))
        }
        other => Err(TypeError::new(TypeErrorKind::Mismatch {
            expected: Type::Vec(Box::new(state.fresh_var())),
            found: other,
        })),
    }
}

fn infer_each_elem_type(coll_ty: &Type, state: &mut InferState) -> Result<Type, TypeError> {
    let resolved = state.subst.apply(coll_ty);
    match resolved {
        Type::Vec(elem) => Ok(state.subst.apply(&elem)),
        Type::Set(elem) => Ok(state.subst.apply(&elem)),
        Type::Map { val, .. } => Ok(state.subst.apply(&val)),
        Type::Adt { name, args } if name == "Option" && args.len() == 1 => {
            Ok(state.subst.apply(&args[0]))
        }
        other => Err(TypeError::new(TypeErrorKind::Mismatch {
            expected: Type::Vec(Box::new(state.fresh_var())),
            found: other,
        })),
    }
}

fn extend_for_let_bindings(
    env: &Env,
    bindings: &[Node],
    state: &mut InferState,
) -> Result<Env, TypeError> {
    if !bindings.len().is_multiple_of(2) {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "for :let bindings must have an even number of forms".to_string(),
        }));
    }

    let mut current_env = env.clone();
    let mut i = 0;
    while i < bindings.len() {
        if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &bindings[i].kind
            && name == "mut"
        {
            i += 1;
        }

        let name_node = bindings.get(i).ok_or_else(|| {
            TypeError::new(TypeErrorKind::MalformedForm {
                description: "for :let binding is missing a name".to_string(),
            })
        })?;
        let name = match &name_node.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
            _ => {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: "for :let binding name must be a symbol".to_string(),
                }));
            }
        };
        i += 1;

        let value_node = bindings.get(i).ok_or_else(|| {
            TypeError::new(TypeErrorKind::MalformedForm {
                description: "for :let binding is missing its expression".to_string(),
            })
        })?;
        i += 1;

        let ty = match synth(value_node, &current_env, state) {
            Ok(ty) => ty,
            Err(e) => {
                state.push_error(e);
                state.fresh_var()
            }
        };
        let scheme = generalize(&ty, &current_env, state);
        current_env = current_env.extend(name, scheme);
    }

    Ok(current_env)
}

fn synth_record_constructor(
    name: &str,
    arg_nodes: &[Node],
    env: &Env,
    state: &mut InferState,
) -> Result<Type, TypeError> {
    if arg_nodes.len() != 1 {
        return Err(TypeError::new(TypeErrorKind::ArityMismatch {
            expected: 1,
            found: arg_nodes.len(),
        }));
    }

    let scheme = match env.lookup(name) {
        Some(scheme) => scheme,
        None => {
            return Err(TypeError::new(TypeErrorKind::UnboundVariable {
                name: name.to_string(),
            }));
        }
    };
    let ctor_ty = scheme.instantiate(&mut state.supply);
    let (param_ty, ret_ty) = match ctor_ty {
        Type::Fn { params, ret, .. } if params.len() == 1 => (params[0].clone(), *ret),
        other => {
            return Err(TypeError::new(TypeErrorKind::Mismatch {
                expected: Type::Fn {
                    params: vec![Type::Record {
                        name: name.to_string(),
                        fields: vec![],
                    }],
                    ret: Box::new(Type::Record {
                        name: name.to_string(),
                        fields: vec![],
                    }),
                    effects: EffectRow::empty(),
                },
                found: other,
            }));
        }
    };

    check_record_literal(&arg_nodes[0], &param_ty, env, state)?;
    Ok(state.subst.apply(&ret_ty))
}

fn check_record_literal(
    node: &Node,
    expected: &Type,
    env: &Env,
    state: &mut InferState,
) -> Result<(), TypeError> {
    let (record_name, record_fields) = match expected {
        Type::Record { name, fields } => (name, fields),
        _ => {
            return Err(TypeError::new(TypeErrorKind::Mismatch {
                expected: expected.clone(),
                found: Type::Keyword,
            }));
        }
    };

    let entries = match &node.kind {
        NodeKind::Map(entries) => entries,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: format!(
                    "record constructor expects a map literal for `{record_name}`"
                ),
            }));
        }
    };

    let mut provided: HashMap<String, &Node> = HashMap::new();
    for (key_node, val_node) in entries {
        let key = match &key_node.kind {
            NodeKind::Atom(Atom::Keyword { ns: None, name }) => name.clone(),
            _ => {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: "record literal keys must be unqualified keywords".to_string(),
                }));
            }
        };
        provided.insert(key, val_node);
    }

    for (field_name, field_ty) in record_fields {
        let value_node = match provided.remove(field_name) {
            Some(node) => node,
            None => {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: format!("record literal missing field `:{field_name}`"),
                }));
            }
        };
        check(value_node, field_ty, env, state)?;
    }

    if !provided.is_empty() {
        let extra = provided.keys().next().cloned().unwrap_or_default();
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!("record literal has unknown field `:{extra}`"),
        }));
    }

    Ok(())
}

fn head_keyword(items: &[Node]) -> Option<&str> {
    match items.first() {
        Some(Node {
            kind: NodeKind::Atom(Atom::Keyword { ns: None, name }),
            ..
        }) => Some(name),
        _ => None,
    }
}

fn synth_keyword_access(
    field: &str,
    arg_nodes: &[Node],
    env: &Env,
    state: &mut InferState,
) -> Result<Type, TypeError> {
    if arg_nodes.len() != 1 {
        return Err(TypeError::new(TypeErrorKind::ArityMismatch {
            expected: 1,
            found: arg_nodes.len(),
        }));
    }

    let arg_ty = synth(&arg_nodes[0], env, state)?;
    let arg_ty = state.subst.apply(&arg_ty);
    match arg_ty {
        Type::Record { fields, .. } => {
            for (name, ty) in fields {
                if name == field {
                    return Ok(ty);
                }
            }
            Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: format!("record has no field `:{field}`"),
            }))
        }
        other => Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!("keyword access expects a record, got {other}"),
        })),
    }
}

/// If `err` is a type mismatch from a known arithmetic operator applied to
/// mixed Int-like and Float-like operands, attach an ADR-006 help suggestion.
fn arithmetic_help(
    err: nexl_types::TypeError,
    callee: Option<&str>,
    args: &[Type],
) -> nexl_types::TypeError {
    if err.help.is_some() {
        return err;
    }
    let verb = match callee {
        Some("+") => "add",
        Some("-") => "subtract",
        Some("*") => "multiply",
        Some("/") => "divide",
        _ => return err,
    };
    let int_ty = args.iter().find(|t| is_int_like(t));
    let float_ty = args.iter().find(|t| is_float_like(t));
    if let (Some(int_ty), Some(float_ty)) = (int_ty, float_ty) {
        let help = format!(
            "cannot {verb} {int_ty} and {float_ty}; use (->float n) to convert the Int to Float"
        );
        err.with_help(help)
    } else {
        err
    }
}

/// Returns `true` if `ty` is an integer-family type.
fn is_int_like(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::Int64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
    )
}

/// Returns `true` if `ty` is a floating-point-family type.
fn is_float_like(ty: &Type) -> bool {
    matches!(ty, Type::Float | Type::F32 | Type::F64)
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
        match synth(expr, env, state) {
            Ok(ty) => last_ty = ty,
            // On failure, record the error and continue with a fresh type variable
            // so subsequent expressions are still checked (Principle 6).
            Err(e) => {
                state.push_error(e);
                last_ty = state.fresh_var();
            }
        }
    }
    Ok(last_ty)
}

/// Return `true` if `node` is the bare colon annotation separator `Symbol(":")`.
fn is_colon_node(node: &Node) -> bool {
    matches!(
        &node.kind,
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == ":"
    )
}

/// Return the name string if the first item in `items` is an unqualified symbol.
fn head_sym(items: &[Node]) -> Option<&str> {
    match items.first() {
        Some(Node {
            kind: NodeKind::Atom(Atom::Symbol { ns: None, name }),
            ..
        }) => Some(name),
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

    Ok(Type::Fn {
        params: param_types,
        ret: Box::new(ret_ty),
        effects: EffectRow::empty(),
    })
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

    // Parse the binding vector.  Each binding is either:
    //   name expr           (2 elements — no annotation)
    //   name : Type expr    (4 elements — with annotation)
    //   pattern expr        (2 elements — destructuring pattern)
    // Bindings of all kinds may be mixed freely.
    let mut current_env = env.clone();
    let mut i = 0;
    while i < bvec.len() {
        let binding_node = &bvec[i];

        // Name binding with optional annotation.
        if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &binding_node.kind {
            if bvec.get(i + 1).is_some_and(is_colon_node) {
                i += 1; // consume name
                i += 1; // consume ":"
                if i >= bvec.len() {
                    return Err(TypeError::new(TypeErrorKind::MalformedForm {
                        description: "expected type after `:` in let annotation".to_string(),
                    }));
                }
                let ann = parse_type_expr(&bvec[i])?;
                i += 1; // consume type
                if i >= bvec.len() {
                    return Err(TypeError::new(TypeErrorKind::MalformedForm {
                        description: "let binding is missing its init expression".to_string(),
                    }));
                }
                let expr_node = &bvec[i];
                i += 1;
                let ty = match synth(expr_node, &current_env, state) {
                    Ok(t) => t,
                    Err(e) => {
                        state.push_error(e);
                        state.fresh_var()
                    }
                };
                nexl_types::unify(&ann, &ty, &mut state.subst)?;
                let scheme = generalize(&ty, &current_env, state);
                current_env = current_env.extend(name.clone(), scheme);
                continue;
            }

            // If the symbol is not a constructor name, treat it as a simple binding.
            if current_env.lookup_ctor(name).is_none() {
                i += 1; // consume name
                if i >= bvec.len() {
                    return Err(TypeError::new(TypeErrorKind::MalformedForm {
                        description: "let binding is missing its init expression".to_string(),
                    }));
                }
                let expr_node = &bvec[i];
                i += 1;
                let ty = match synth(expr_node, &current_env, state) {
                    Ok(t) => t,
                    Err(e) => {
                        state.push_error(e);
                        state.fresh_var()
                    }
                };
                let scheme = generalize(&ty, &current_env, state);
                current_env = current_env.extend(name.clone(), scheme);
                continue;
            }
        }

        // Pattern binding (destructuring).
        let pattern = parse_pattern(binding_node).map_err(|e| {
            TypeError::new(TypeErrorKind::MalformedForm {
                description: format!("invalid let pattern: {}", e.description),
            })
        })?;
        i += 1; // consume pattern
        if i >= bvec.len() {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "let binding is missing its init expression".to_string(),
            }));
        }
        let expr_node = &bvec[i];
        i += 1;
        let ty_result = match (&pattern, &expr_node.kind) {
            (Pattern::Tuple(_), NodeKind::Vector(items)) => {
                synth_tuple_literal(items, &current_env, state)
            }
            _ => synth(expr_node, &current_env, state),
        };
        let ty = match ty_result {
            Ok(t) => t,
            Err(e) => {
                state.push_error(e);
                state.fresh_var()
            }
        };
        let resolved = state.subst.apply(&ty);
        let pattern_env = check_pattern(&pattern, &resolved, &current_env, state)?;
        check_exhaustive_let_pattern(&pattern, &resolved, &current_env, expr_node)?;
        current_env = pattern_env;
    }

    // Synthesize the body in the fully-extended env.
    synth(&items[2], &current_env, state)
}

/// Synthesize a type for a literal atom.
fn synth_atom(atom: &Atom, env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    match atom {
        Atom::Int { suffix: None, .. } => Ok(Type::Int),
        Atom::Int {
            suffix: Some(s), ..
        } => Ok(int_suffix_type(*s)),
        Atom::Float { suffix: None, .. } => Ok(Type::Float),
        Atom::Float {
            suffix: Some(s), ..
        } => Ok(float_suffix_type(*s)),
        Atom::Ratio { .. } => Ok(Type::Ratio),
        Atom::Bool(_) => Ok(Type::Bool),
        Atom::Char(_) => Ok(Type::Char),
        Atom::Str(_) => Ok(Type::Str),
        Atom::Keyword { .. } => Ok(Type::Keyword),
        Atom::Unit => Ok(Type::Unit),
        Atom::Symbol { ns: None, name } => synth_var(name, env, state),
        Atom::Symbol {
            ns: Some(alias),
            name,
        } => synth_qualified_var(alias, name, env, state),
    }
}

/// Synthesize a type for a vector literal `[a b ...]` (homogeneous elements).
fn synth_vec_literal(
    items: &[Node],
    env: &Env,
    state: &mut InferState,
) -> Result<Type, TypeError> {
    if items.is_empty() {
        return Ok(Type::Vec(Box::new(state.fresh_var())));
    }

    let mut elem_ty = match synth(&items[0], env, state) {
        Ok(ty) => ty,
        Err(e) => {
            state.push_error(e);
            state.fresh_var()
        }
    };

    for item in &items[1..] {
        let ty = match synth(item, env, state) {
            Ok(ty) => ty,
            Err(e) => {
                state.push_error(e);
                state.fresh_var()
            }
        };
        nexl_types::unify(&elem_ty, &ty, &mut state.subst)?;
        elem_ty = state.subst.apply(&elem_ty);
    }

    Ok(Type::Vec(Box::new(state.subst.apply(&elem_ty))))
}

/// Synthesize a type for a tuple literal `[a b ...]` (2–8 elements).
fn synth_tuple_literal(
    items: &[Node],
    env: &Env,
    state: &mut InferState,
) -> Result<Type, TypeError> {
    if items.len() < 2 || items.len() > 8 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!("tuple literal expects 2-8 elements, got {}", items.len()),
        }));
    }
    let mut elem_types = Vec::with_capacity(items.len());
    for item in items {
        match synth(item, env, state) {
            Ok(ty) => elem_types.push(ty),
            Err(e) => {
                state.push_error(e);
                elem_types.push(state.fresh_var());
            }
        }
    }
    Ok(Type::Tuple(elem_types))
}

/// Synthesize a type for a map literal `{:k v ...}` (homogeneous keys/values).
fn synth_map_literal(
    entries: &[(Node, Node)],
    env: &Env,
    state: &mut InferState,
) -> Result<Type, TypeError> {
    if entries.is_empty() {
        return Ok(Type::Map {
            key: Box::new(state.fresh_var()),
            val: Box::new(state.fresh_var()),
        });
    }

    let (first_key, first_val) = &entries[0];
    let mut key_ty = match synth(first_key, env, state) {
        Ok(ty) => ty,
        Err(e) => {
            state.push_error(e);
            state.fresh_var()
        }
    };
    let mut val_ty = match synth(first_val, env, state) {
        Ok(ty) => ty,
        Err(e) => {
            state.push_error(e);
            state.fresh_var()
        }
    };

    for (key_node, val_node) in &entries[1..] {
        let kt = match synth(key_node, env, state) {
            Ok(ty) => ty,
            Err(e) => {
                state.push_error(e);
                state.fresh_var()
            }
        };
        let vt = match synth(val_node, env, state) {
            Ok(ty) => ty,
            Err(e) => {
                state.push_error(e);
                state.fresh_var()
            }
        };
        nexl_types::unify(&key_ty, &kt, &mut state.subst)?;
        nexl_types::unify(&val_ty, &vt, &mut state.subst)?;
        key_ty = state.subst.apply(&key_ty);
        val_ty = state.subst.apply(&val_ty);
    }

    Ok(Type::Map {
        key: Box::new(state.subst.apply(&key_ty)),
        val: Box::new(state.subst.apply(&val_ty)),
    })
}

/// Synthesize a type for a set literal `#{a b ...}` (homogeneous elements).
fn synth_set_literal(
    items: &[Node],
    env: &Env,
    state: &mut InferState,
) -> Result<Type, TypeError> {
    if items.is_empty() {
        return Ok(Type::Set(Box::new(state.fresh_var())));
    }

    let mut elem_ty = match synth(&items[0], env, state) {
        Ok(ty) => ty,
        Err(e) => {
            state.push_error(e);
            state.fresh_var()
        }
    };

    for item in &items[1..] {
        let ty = match synth(item, env, state) {
            Ok(ty) => ty,
            Err(e) => {
                state.push_error(e);
                state.fresh_var()
            }
        };
        nexl_types::unify(&elem_ty, &ty, &mut state.subst)?;
        elem_ty = state.subst.apply(&elem_ty);
    }

    Ok(Type::Set(Box::new(state.subst.apply(&elem_ty))))
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

/// Synthesize the type of a qualified variable `alias/name`.
fn synth_qualified_var(
    alias: &str,
    name: &str,
    env: &Env,
    state: &mut InferState,
) -> Result<Type, TypeError> {
    match env.lookup_qualified(alias, name) {
        Some(scheme) => Ok(scheme.instantiate(&mut state.supply)),
        None => Err(TypeError::new(TypeErrorKind::UnboundVariable {
            name: format!("{alias}/{name}"),
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

    // Accept (defn name params body) — 4 elements
    // or     (defn name params -> RetType body) — 6 elements.
    let (param_node, ret_annotation, body_node) = match items.len() {
        4 => (&items[2], None, &items[3]),
        6 => {
            let is_arrow = matches!(
                &items[3].kind,
                NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "->"
            );
            if !is_arrow {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: "defn with 6 elements expects (defn name params -> RetType body)"
                        .to_string(),
                }));
            }
            let ret_ty = parse_type_expr(&items[4])?;
            (&items[2], Some(ret_ty), &items[5])
        }
        n => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: format!(
                    "defn expects (defn name [params...] body) or \
                     (defn name [params...] -> RetType body), got {n} elements"
                ),
            }));
        }
    };

    // items[1] must be an unqualified symbol (the function name).
    let name = match &items[1].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "defn function name must be an unqualified symbol".to_string(),
            }));
        }
    };

    // Parse params with optional `: Type` annotations (scan-style).
    let param_nodes = match &param_node.kind {
        NodeKind::Vector(elems) => elems,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "defn parameter list must be a vector".to_string(),
            }));
        }
    };

    let mut param_types: Vec<Type> = Vec::new();
    let mut body_env = env.clone();
    let mut i = 0;
    while i < param_nodes.len() {
        let param_name = match &param_nodes[i].kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
            _ => {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: "defn parameter names must be unqualified symbols".to_string(),
                }));
            }
        };
        i += 1;

        let param_ty = if param_nodes.get(i).is_some_and(is_colon_node) {
            i += 1; // consume ":"
            let ann = parse_type_expr(&param_nodes[i])?;
            i += 1;
            ann
        } else {
            state.fresh_var()
        };

        param_types.push(param_ty.clone());
        body_env = body_env.extend(param_name, Scheme::mono(param_ty));
    }

    // Infer the body in the extended environment.
    let body_ty = synth(body_node, &body_env, state)?;

    // If a return annotation was provided, unify it with the body type.
    if let Some(ann_ret) = ret_annotation {
        nexl_types::unify(&ann_ret, &body_ty, &mut state.subst)?;
    }

    let param_types = param_types.iter().map(|t| state.subst.apply(t)).collect();
    let ret_ty = state.subst.apply(&body_ty);

    let fn_ty = Type::Fn {
        params: param_types,
        ret: Box::new(ret_ty),
        effects: EffectRow::empty(),
    };
    let new_env = env.extend(name.clone(), Scheme::mono(fn_ty.clone()));
    Ok((name, fn_ty, new_env))
}

// ---------------------------------------------------------------------------
// Type expression parser
// ---------------------------------------------------------------------------

/// Parse an AST node that appears in annotation position into a [`Type`].
///
/// Handles:
/// - Primitive type names: `Int`, `Float`, `Bool`, … (spec §5.2)
/// - Fixed-width numeric types: `Int8`, `Int32`, `U64`, `F32`, …
/// - Function types: `(Fn [param-types...] -> ret-type)` (spec §5.3)
pub fn parse_type_expr(node: &Node) -> Result<Type, TypeError> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => parse_type_name(name, node),
        NodeKind::List(items) => parse_fn_type(items, node),
        _ => Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!("expected a type expression, got {:?}", node.kind),
        })),
    }
}

/// Parse a bare type name symbol (e.g. `Int`, `Bool`, `Fn`).
fn parse_type_name(name: &str, node: &Node) -> Result<Type, TypeError> {
    match name {
        "Int" | "Int64" => Ok(Type::Int),
        "Float" | "F64" => Ok(Type::Float),
        "Ratio" => Ok(Type::Ratio),
        "Bool" => Ok(Type::Bool),
        "Char" => Ok(Type::Char),
        "Str" => Ok(Type::Str),
        "Keyword" => Ok(Type::Keyword),
        "Symbol" => Ok(Type::Symbol),
        "Unit" => Ok(Type::Unit),
        "Never" => Ok(Type::Never),
        "Int8" => Ok(Type::Int8),
        "Int16" => Ok(Type::Int16),
        "Int32" => Ok(Type::Int32),
        "U8" => Ok(Type::U8),
        "U16" => Ok(Type::U16),
        "U32" => Ok(Type::U32),
        "U64" => Ok(Type::U64),
        "F32" => Ok(Type::F32),
        _ => Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!("unknown type name `{name}`"),
        })
        .with_span(node.span)),
    }
}

/// Parse `(Fn [param-types...] -> ret-type)`.
fn parse_fn_type(items: &[Node], node: &Node) -> Result<Type, TypeError> {
    // Structure: (Fn <params-vec> -> <ret>)  — exactly 4 elements.
    let bad = || {
        TypeError::new(TypeErrorKind::MalformedForm {
            description: "Fn type expects (Fn [param-types...] -> ret-type)".to_string(),
        })
        .with_span(node.span)
    };

    if items.len() != 4 && items.len() != 6 {
        return Err(bad());
    }

    // items[0] must be Symbol("Fn")
    let is_fn_head = matches!(
        &items[0].kind,
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "Fn"
    );
    if !is_fn_head {
        return Err(bad());
    }

    // items[1] must be a Vector of type nodes
    let param_nodes = match &items[1].kind {
        NodeKind::Vector(elems) => elems,
        _ => return Err(bad()),
    };

    // items[2] must be Symbol("->")
    let is_arrow = matches!(
        &items[2].kind,
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "->"
    );
    if !is_arrow {
        return Err(bad());
    }

    let params: Vec<Type> = param_nodes
        .iter()
        .map(parse_type_expr)
        .collect::<Result<_, _>>()?;
    let ret = parse_type_expr(&items[3])?;

    let effects = if items.len() == 6 {
        let is_bang = matches!(
            &items[4].kind,
            NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "!"
        );
        if !is_bang {
            return Err(bad());
        }
        parse_effect_row(&items[5])?
    } else {
        EffectRow::empty()
    };

    Ok(Type::Fn {
        params,
        ret: Box::new(ret),
        effects,
    })
}

fn parse_effect_row(node: &Node) -> Result<EffectRow, TypeError> {
    let bad = || {
        TypeError::new(TypeErrorKind::MalformedForm {
            description: "effect row expects [Effect ... | r]".to_string(),
        })
        .with_span(node.span)
    };

    let items = match &node.kind {
        NodeKind::Vector(items) => items,
        _ => return Err(bad()),
    };

    if items.is_empty() {
        return Ok(EffectRow::empty());
    }

    let mut effects = Vec::new();
    let mut idx = 0;
    let mut row_var: Option<String> = None;

    while idx < items.len() {
        match &items[idx].kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "|" => {
                if row_var.is_some() || idx == 0 || idx + 1 >= items.len() {
                    return Err(bad());
                }
                let var = match &items[idx + 1].kind {
                    NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
                    _ => return Err(bad()),
                };
                if idx + 2 != items.len() {
                    return Err(bad());
                }
                row_var = Some(var);
                break;
            }
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => {
                effects.push(name.clone());
            }
            _ => return Err(bad()),
        }
        idx += 1;
    }

    Ok(EffectRow::new(effects, row_var))
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
    let forall: std::collections::HashSet<_> = ty_free.difference(&env_free).copied().collect();
    Scheme { forall, body: ty }
}

// ---------------------------------------------------------------------------
// Module performs validation
// ---------------------------------------------------------------------------

/// Validate or infer a module's `:performs` list from exported function effects.
///
/// If `declared` is `Some`, every exported effect must be contained in it.
/// If `declared` is `None`, the effect list is inferred as the union of all
/// exported effects.
pub fn validate_module_performs(
    declared: Option<&[String]>,
    export_effects: &HashMap<String, Vec<String>>,
) -> Result<Vec<String>, TypeError> {
    let mut inferred: HashSet<String> = HashSet::new();
    for effects in export_effects.values() {
        for effect in effects {
            inferred.insert(effect.clone());
        }
    }

    match declared {
        Some(declared) => {
            let declared_set: HashSet<String> = declared.iter().cloned().collect();
            for (export, effects) in export_effects {
                for effect in effects {
                    if !declared_set.contains(effect) {
                        return Err(TypeError::new(TypeErrorKind::MalformedForm {
                            description: format!(
                                "module :performs missing effect `{effect}` for export `{export}`"
                            ),
                        }));
                    }
                }
            }
            let mut normalized = declared.to_vec();
            normalized.sort();
            normalized.dedup();
            Ok(normalized)
        }
        None => {
            let mut normalized: Vec<String> = inferred.into_iter().collect();
            normalized.sort();
            Ok(normalized)
        }
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
    let actual = match (&node.kind, expected) {
        (NodeKind::Vector(items), Type::Tuple(_)) => synth_tuple_literal(items, env, state)?,
        _ => synth(node, env, state)?,
    };
    // Put `expected` first so Mismatch errors read "expected X but got Y".
    nexl_types::unify(expected, &actual, &mut state.subst).map_err(|e| {
        if e.span.is_none() && !node.span.is_synthetic() {
            e.with_span(node.span)
        } else {
            e
        }
    })
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
    let (name, annotation, body) = parse_def(node)?;
    let ty = synth(body, env, state)?;
    if let Some(ann_ty) = annotation {
        // Check: annotation must match the inferred type.
        nexl_types::unify(&ann_ty, &ty, &mut state.subst)?;
    }
    let resolved = state.subst.apply(&ty);
    let new_env = env.extend(name.clone(), Scheme::mono(resolved.clone()));
    Ok((name, resolved, new_env))
}

/// Parse `(def name expr)` or `(def name : Type expr)`.
///
/// Returns the binding name, an optional type annotation, and the body node.
fn parse_def(node: &Node) -> Result<(String, Option<Type>, &Node), TypeError> {
    let items = match &node.kind {
        NodeKind::List(items) => items,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "def must be a list".to_string(),
            }));
        }
    };

    // Accept either 3 elements (no annotation) or 5 (with `: Type`).
    if items.len() != 3 && items.len() != 5 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "def expects (def name expr) or (def name : Type expr), got {} elements",
                items.len()
            ),
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

    if items.len() == 3 {
        return Ok((binding_name, None, &items[2]));
    }

    // 5-element form: items[2] must be Symbol(":"), items[3] is the type, items[4] is the body.
    let is_colon = matches!(
        &items[2].kind,
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == ":"
    );
    if !is_colon {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "expected `:` after binding name in def annotation".to_string(),
        }));
    }

    let annotation = parse_type_expr(&items[3])?;
    Ok((binding_name, Some(annotation), &items[4]))
}

// ---------------------------------------------------------------------------
// deftype form (sum types only, for now)
// ---------------------------------------------------------------------------

/// A parsed `deftype` declaration (spec §5.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeftypeDecl {
    /// Sum type declaration with named constructors.
    Sum(TypeDef),
    /// Record type declaration with named fields.
    Record {
        name: String,
        params: Vec<TypeVar>,
        fields: Vec<(String, Type)>,
    },
}

/// Parse a `deftype` declaration.
#[allow(dead_code)]
pub fn parse_deftype(node: &Node) -> Result<DeftypeDecl, TypeError> {
    let items = match &node.kind {
        NodeKind::List(items) => items,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "deftype must be a list".to_string(),
            }));
        }
    };

    if items.len() < 3 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!(
                "deftype expects (deftype Name ...), got {} elements",
                items.len()
            ),
        }));
    }

    let is_head = matches!(
        &items[0].kind,
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "deftype"
    );
    if !is_head {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "first element of deftype form must be the symbol `deftype`".to_string(),
        }));
    }

    let name = match &items[1].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "deftype name must be an unqualified symbol".to_string(),
            }));
        }
    };

    let mut param_map: HashMap<String, TypeVar> = HashMap::new();
    let mut params: Vec<TypeVar> = Vec::new();
    let mut body_index = 2;

    if let NodeKind::Vector(param_nodes) = &items[body_index].kind {
        let mut supply = TypeVarSupply::new();
        for param_node in param_nodes {
            let param_name = match &param_node.kind {
                NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
                _ => {
                    return Err(TypeError::new(TypeErrorKind::MalformedForm {
                        description: "deftype type params must be unqualified symbols".to_string(),
                    }));
                }
            };
            if param_map.contains_key(&param_name) {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: format!("duplicate type param `{param_name}`"),
                }));
            }
            let tv = supply.fresh();
            param_map.insert(param_name, tv);
            params.push(tv);
        }
        body_index += 1;
    }

    if body_index >= items.len() {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "deftype missing body".to_string(),
        }));
    }

    match &items[body_index].kind {
        NodeKind::Map(entries) => {
            if items.len() != body_index + 1 {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: "deftype record expects (deftype Name {fields})".to_string(),
                }));
            }
            let mut fields = Vec::new();
            for (key_node, val_node) in entries {
                let field_name = match &key_node.kind {
                    NodeKind::Atom(Atom::Keyword { ns: None, name }) => name.clone(),
                    _ => {
                        return Err(TypeError::new(TypeErrorKind::MalformedForm {
                            description: "deftype record fields must be unqualified keywords"
                                .to_string(),
                        }));
                    }
                };
                let field_ty = parse_type_expr_with_params(val_node, &param_map)?;
                fields.push((field_name, field_ty));
            }
            Ok(DeftypeDecl::Record {
                name,
                params,
                fields,
            })
        }
        NodeKind::Atom(Atom::Symbol {
            ns: None,
            name: pipe,
        }) if pipe == "|" => {
            if items.len() < body_index + 2 {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: format!(
                        "deftype expects (deftype Name | Ctor1 ...), got {} elements",
                        items.len()
                    ),
                }));
            }
            let mut constructors = Vec::new();
            let mut i = body_index;
            while i < items.len() {
                let is_pipe = matches!(
                    &items[i].kind,
                    NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "|"
                );
                if !is_pipe {
                    return Err(TypeError::new(TypeErrorKind::MalformedForm {
                        description: "deftype sum type expects `|` before each constructor"
                            .to_string(),
                    }));
                }
                i += 1;
                if i >= items.len() {
                    return Err(TypeError::new(TypeErrorKind::MalformedForm {
                        description: "deftype sum type missing constructor after `|`".to_string(),
                    }));
                }

                let ctor_node = &items[i];
                let ctor = match &ctor_node.kind {
                    NodeKind::Atom(Atom::Symbol { ns: None, name }) => {
                        Constructor::nullary(name.clone())
                    }
                    NodeKind::List(ctor_items) => {
                        if ctor_items.is_empty() {
                            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                                description: "deftype constructor list must have a name"
                                    .to_string(),
                            }));
                        }
                        let ctor_name = match &ctor_items[0].kind {
                            NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
                            _ => {
                                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                                    description:
                                        "deftype constructor name must be an unqualified symbol"
                                            .to_string(),
                                }));
                            }
                        };
                        let mut fields = Vec::new();
                        for field_node in &ctor_items[1..] {
                            fields.push(parse_type_expr_with_params(field_node, &param_map)?);
                        }
                        Constructor::nary(ctor_name, fields)
                    }
                    _ => {
                        return Err(TypeError::new(TypeErrorKind::MalformedForm {
                            description: "deftype constructor must be a symbol or list".to_string(),
                        }));
                    }
                };
                constructors.push(ctor);
                i += 1;
            }

            Ok(DeftypeDecl::Sum(TypeDef {
                name,
                params,
                constructors,
            }))
        }
        _ => Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "deftype expects a `|`-prefixed constructor list or a map literal"
                .to_string(),
        })),
    }
}

/// Register a parsed `deftype` declaration in the environment.
#[allow(dead_code)]
pub fn register_deftype(env: &Env, decl: DeftypeDecl) -> Env {
    match decl {
        DeftypeDecl::Sum(td) => env.extend_type_def(td),
        DeftypeDecl::Record {
            name,
            params,
            fields,
        } => env.extend_record_def(RecordDef {
            name,
            params,
            fields,
        }),
    }
}

// ---------------------------------------------------------------------------
// match form (parsing only, for now)
// ---------------------------------------------------------------------------

fn synth_match(node: &Node, env: &Env, state: &mut InferState) -> Result<Type, TypeError> {
    let (scrutinee, arms) = parse_match(node)?;
    let scrutinee_ty = synth(scrutinee, env, state)?;
    let mut result_ty: Option<Type> = None;
    for arm in &arms {
        let current = state.subst.apply(&scrutinee_ty);
        let arm_env = check_pattern(&arm.pattern, &current, env, state)?;
        if let Some(guard) = arm.guard {
            check(guard, &Type::Bool, &arm_env, state)?;
        }
        let body_ty = synth(arm.body, &arm_env, state)?;
        let body_ty = state.subst.apply(&body_ty);
        if let Some(expected) = &result_ty {
            nexl_types::unify(expected, &body_ty, &mut state.subst)?;
            result_ty = Some(state.subst.apply(expected));
        } else {
            result_ty = Some(body_ty);
        }
    }
    let resolved_scrutinee = state.subst.apply(&scrutinee_ty);
    check_exhaustive(&resolved_scrutinee, &arms, env)?;
    check_redundant_patterns(&resolved_scrutinee, &arms, env, state);
    match result_ty {
        Some(ty) => Ok(state.subst.apply(&ty)),
        None => Ok(state.fresh_var()),
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
struct MatchArm<'a> {
    pattern: Pattern,
    guard: Option<&'a Node>,
    body: &'a Node,
}

#[allow(dead_code)]
fn is_when_node(node: &Node) -> bool {
    matches!(
        &node.kind,
        NodeKind::Atom(Atom::Keyword { ns: None, name }) if name == "when"
    )
}

#[allow(dead_code)]
fn parse_match<'a>(node: &'a Node) -> Result<(&'a Node, Vec<MatchArm<'a>>), TypeError> {
    let items = match &node.kind {
        NodeKind::List(items) => items,
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "match must be a list".to_string(),
            }));
        }
    };

    if items.len() < 3 {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "match expects (match expr pattern body ... )".to_string(),
        }));
    }

    let is_head = matches!(
        &items[0].kind,
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "match"
    );
    if !is_head {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "first element of match form must be the symbol `match`".to_string(),
        }));
    }

    let scrutinee = &items[1];
    let mut arms = Vec::new();
    let mut i = 2;
    while i < items.len() {
        let pattern_node = &items[i];
        let pattern = parse_pattern(pattern_node).map_err(|e| {
            TypeError::new(TypeErrorKind::MalformedForm {
                description: format!("invalid match pattern: {}", e.description),
            })
        })?;
        i += 1;
        if i >= items.len() {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "match arm missing body".to_string(),
            }));
        }

        let (guard, body) = if is_when_node(&items[i]) {
            if i + 2 >= items.len() {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: "match arm with :when must include guard and body".to_string(),
                }));
            }
            let guard = &items[i + 1];
            let body = &items[i + 2];
            i += 3;
            (Some(guard), body)
        } else {
            let body = &items[i];
            i += 1;
            (None, body)
        };

        arms.push(MatchArm {
            pattern,
            guard,
            body,
        });
    }

    Ok((scrutinee, arms))
}

fn check_pattern(
    pattern: &Pattern,
    scrutinee_ty: &Type,
    env: &Env,
    state: &mut InferState,
) -> Result<Env, TypeError> {
    match pattern {
        Pattern::Wildcard => Ok(env.clone()),
        Pattern::Var(name) => {
            if env.lookup_ctor(name).is_some() {
                check_constructor_pattern(name, &[], scrutinee_ty, env, state)
            } else {
                Ok(env.extend(name.clone(), Scheme::mono(scrutinee_ty.clone())))
            }
        }
        Pattern::Literal(atom) => {
            let lit_ty = synth_atom(atom, env, state)?;
            nexl_types::unify(scrutinee_ty, &lit_ty, &mut state.subst)?;
            Ok(env.clone())
        }
        Pattern::Constructor { name, args } => {
            check_constructor_pattern(name, args, scrutinee_ty, env, state)
        }
        Pattern::Tuple(items) => check_tuple_pattern(items, scrutinee_ty, env, state),
        Pattern::Record { fields } => check_record_pattern(fields, scrutinee_ty, env, state),
        _ => Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "unsupported match pattern".to_string(),
        })),
    }
}

fn check_constructor_pattern(
    name: &str,
    args: &[Pattern],
    scrutinee_ty: &Type,
    env: &Env,
    state: &mut InferState,
) -> Result<Env, TypeError> {
    let scheme = match env.lookup(name) {
        Some(scheme) => scheme,
        None => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: format!("unknown constructor `{name}`"),
            }));
        }
    };
    let ctor_ty = scheme.instantiate(&mut state.supply);
    match ctor_ty {
        Type::Fn { params, ret, .. } => {
            if params.len() != args.len() {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: params.len(),
                    found: args.len(),
                }));
            }
            nexl_types::unify(scrutinee_ty, &ret, &mut state.subst)?;
            let mut current_env = env.clone();
            for (pat, param_ty) in args.iter().zip(params.iter()) {
                let param_ty = state.subst.apply(param_ty);
                current_env = check_pattern(pat, &param_ty, &current_env, state)?;
            }
            Ok(current_env)
        }
        Type::Adt { .. } => {
            if !args.is_empty() {
                return Err(TypeError::new(TypeErrorKind::ArityMismatch {
                    expected: 0,
                    found: args.len(),
                }));
            }
            nexl_types::unify(scrutinee_ty, &ctor_ty, &mut state.subst)?;
            Ok(env.clone())
        }
        other => Err(TypeError::new(TypeErrorKind::Mismatch {
            expected: Type::Adt {
                name: name.to_string(),
                args: vec![],
            },
            found: other,
        })),
    }
}

fn check_tuple_pattern(
    items: &[Pattern],
    scrutinee_ty: &Type,
    env: &Env,
    state: &mut InferState,
) -> Result<Env, TypeError> {
    let elems = match scrutinee_ty {
        Type::Tuple(elems) => elems,
        other => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: format!("tuple pattern expects a tuple type, got {other}"),
            }));
        }
    };
    if elems.len() != items.len() {
        return Err(TypeError::new(TypeErrorKind::ArityMismatch {
            expected: elems.len(),
            found: items.len(),
        }));
    }
    let mut current_env = env.clone();
    for (pat, elem_ty) in items.iter().zip(elems.iter()) {
        let elem_ty = state.subst.apply(elem_ty);
        current_env = check_pattern(pat, &elem_ty, &current_env, state)?;
    }
    Ok(current_env)
}

fn check_record_pattern(
    fields: &[(String, Pattern)],
    scrutinee_ty: &Type,
    env: &Env,
    state: &mut InferState,
) -> Result<Env, TypeError> {
    let record_fields = match scrutinee_ty {
        Type::Record { fields, .. } => fields,
        other => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: format!("record pattern expects a record type, got {other}"),
            }));
        }
    };
    let mut field_map: HashMap<&str, &Type> = HashMap::new();
    for (name, ty) in record_fields {
        field_map.insert(name.as_str(), ty);
    }
    let mut current_env = env.clone();
    for (field_name, pat) in fields {
        let field_ty = match field_map.get(field_name.as_str()) {
            Some(ty) => *ty,
            None => {
                return Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: format!("record pattern has unknown field `:{field_name}`"),
                }));
            }
        };
        let field_ty = state.subst.apply(field_ty);
        current_env = check_pattern(pat, &field_ty, &current_env, state)?;
    }
    Ok(current_env)
}

fn is_catch_all_pattern(pat: &Pattern, env: &Env) -> bool {
    match pat {
        Pattern::Wildcard => true,
        Pattern::Var(name) => env.lookup_ctor(name).is_none(),
        _ => false,
    }
}

fn check_exhaustive(
    scrutinee_ty: &Type,
    arms: &[MatchArm<'_>],
    env: &Env,
) -> Result<(), TypeError> {
    if arms
        .iter()
        .any(|arm| is_catch_all_pattern(&arm.pattern, env))
    {
        return Ok(());
    }
    match scrutinee_ty {
        Type::Bool => {
            let mut seen_true = false;
            let mut seen_false = false;
            for arm in arms {
                if let Pattern::Literal(Atom::Bool(value)) = &arm.pattern {
                    if *value {
                        seen_true = true;
                    } else {
                        seen_false = true;
                    }
                }
            }
            if seen_true && seen_false {
                Ok(())
            } else {
                let mut missing = Vec::new();
                if !seen_true {
                    missing.push("true");
                }
                if !seen_false {
                    missing.push("false");
                }
                Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: format!("non-exhaustive match: missing {}", missing.join(", ")),
                }))
            }
        }
        Type::Adt { name, .. } => {
            let type_def = match env.lookup_type_def(name) {
                Some(def) => def,
                None => return Ok(()),
            };
            let mut missing: HashSet<String> = type_def
                .constructors
                .iter()
                .map(|ctor| ctor.name.clone())
                .collect();
            for arm in arms {
                match &arm.pattern {
                    Pattern::Constructor { name, .. } => {
                        missing.remove(name);
                    }
                    Pattern::Var(name) if env.lookup_ctor(name).is_some() => {
                        missing.remove(name);
                    }
                    _ => {}
                }
            }
            if missing.is_empty() {
                Ok(())
            } else {
                let mut missing: Vec<String> = missing.into_iter().collect();
                missing.sort();
                Err(TypeError::new(TypeErrorKind::MalformedForm {
                    description: format!("non-exhaustive match: missing {}", missing.join(", ")),
                }))
            }
        }
        _ => Ok(()),
    }
}

fn check_exhaustive_let_pattern(
    pattern: &Pattern,
    scrutinee_ty: &Type,
    env: &Env,
    node: &Node,
) -> Result<(), TypeError> {
    let non_exhaustive = || {
        TypeError::new(TypeErrorKind::MalformedForm {
            description: "non-exhaustive let pattern".to_string(),
        })
    };

    match pattern {
        Pattern::Literal(_) => Err(non_exhaustive()),
        Pattern::Constructor { .. } => {
            let arm = MatchArm {
                pattern: pattern.clone(),
                guard: None,
                body: node,
            };
            check_exhaustive(scrutinee_ty, std::slice::from_ref(&arm), env)
        }
        Pattern::Var(name) if env.lookup_ctor(name).is_some() => {
            let arm = MatchArm {
                pattern: pattern.clone(),
                guard: None,
                body: node,
            };
            check_exhaustive(scrutinee_ty, std::slice::from_ref(&arm), env)
        }
        _ => Ok(()),
    }
}

fn warn_redundant(state: &mut InferState) {
    state.push_warning(TypeError::new(TypeErrorKind::MalformedForm {
        description: "redundant match arm".to_string(),
    }));
}

fn check_redundant_patterns(
    scrutinee_ty: &Type,
    arms: &[MatchArm<'_>],
    env: &Env,
    state: &mut InferState,
) {
    let mut covered_all = false;
    let mut covered_true = false;
    let mut covered_false = false;
    let adt_constructors: Option<HashSet<String>> = match scrutinee_ty {
        Type::Adt { name, .. } => env.lookup_type_def(name).map(|def| {
            def.constructors
                .iter()
                .map(|ctor| ctor.name.clone())
                .collect()
        }),
        _ => None,
    };
    let mut covered_ctors: HashSet<String> = HashSet::new();

    for arm in arms {
        if covered_all {
            warn_redundant(state);
            continue;
        }

        if is_catch_all_pattern(&arm.pattern, env) {
            let already_exhaustive = match scrutinee_ty {
                Type::Bool => covered_true && covered_false,
                Type::Adt { .. } => adt_constructors
                    .as_ref()
                    .map(|ctors| covered_ctors.len() == ctors.len())
                    .unwrap_or(false),
                _ => false,
            };
            if already_exhaustive {
                warn_redundant(state);
            } else {
                covered_all = true;
            }
            continue;
        }

        match scrutinee_ty {
            Type::Bool => {
                if let Pattern::Literal(Atom::Bool(value)) = &arm.pattern {
                    if *value {
                        if covered_true {
                            warn_redundant(state);
                        } else {
                            covered_true = true;
                        }
                    } else if covered_false {
                        warn_redundant(state);
                    } else {
                        covered_false = true;
                    }
                }
            }
            Type::Adt { .. } => {
                if let Some(_ctors) = &adt_constructors {
                    let ctor_name = match &arm.pattern {
                        Pattern::Constructor { name, .. } => Some(name),
                        Pattern::Var(name) if env.lookup_ctor(name).is_some() => Some(name),
                        _ => None,
                    };
                    if let Some(name) = ctor_name {
                        if covered_ctors.contains(name) {
                            warn_redundant(state);
                        } else {
                            covered_ctors.insert(name.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn parse_type_expr_with_params(
    node: &Node,
    params: &HashMap<String, TypeVar>,
) -> Result<Type, TypeError> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => {
            if let Some(tv) = params.get(name) {
                Ok(Type::Var(*tv))
            } else {
                parse_type_name_or_adt(name)
            }
        }
        NodeKind::List(items) => parse_type_list_with_params(items, node, params),
        _ => Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: format!("expected a type expression, got {:?}", node.kind),
        })),
    }
}

fn parse_type_list_with_params(
    items: &[Node],
    node: &Node,
    params: &HashMap<String, TypeVar>,
) -> Result<Type, TypeError> {
    if items.is_empty() {
        return Err(TypeError::new(TypeErrorKind::MalformedForm {
            description: "empty type list".to_string(),
        })
        .with_span(node.span));
    }
    let head = match &items[0].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.as_str(),
        _ => {
            return Err(TypeError::new(TypeErrorKind::MalformedForm {
                description: "type list head must be a symbol".to_string(),
            })
            .with_span(node.span));
        }
    };
    if head == "Fn" {
        return parse_fn_type_with_params(items, node, params);
    }

    let mut args = Vec::new();
    for arg_node in &items[1..] {
        args.push(parse_type_expr_with_params(arg_node, params)?);
    }
    Ok(Type::Adt {
        name: head.to_string(),
        args,
    })
}

fn parse_fn_type_with_params(
    items: &[Node],
    node: &Node,
    params: &HashMap<String, TypeVar>,
) -> Result<Type, TypeError> {
    let bad = || {
        TypeError::new(TypeErrorKind::MalformedForm {
            description: "Fn type expects (Fn [param-types...] -> ret-type)".to_string(),
        })
        .with_span(node.span)
    };

    if items.len() != 4 {
        return Err(bad());
    }

    let is_fn_head = matches!(
        &items[0].kind,
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "Fn"
    );
    if !is_fn_head {
        return Err(bad());
    }

    let param_nodes = match &items[1].kind {
        NodeKind::Vector(elems) => elems,
        _ => return Err(bad()),
    };

    let is_arrow = matches!(
        &items[2].kind,
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "->"
    );
    if !is_arrow {
        return Err(bad());
    }

    let params_ty: Vec<Type> = param_nodes
        .iter()
        .map(|node| parse_type_expr_with_params(node, params))
        .collect::<Result<_, _>>()?;
    let ret = parse_type_expr_with_params(&items[3], params)?;

    Ok(Type::Fn {
        params: params_ty,
        ret: Box::new(ret),
        effects: EffectRow::empty(),
    })
}

fn parse_type_name_or_adt(name: &str) -> Result<Type, TypeError> {
    let ty = match name {
        "Int" | "Int64" => Ok(Type::Int),
        "Float" | "F64" => Ok(Type::Float),
        "Ratio" => Ok(Type::Ratio),
        "Bool" => Ok(Type::Bool),
        "Char" => Ok(Type::Char),
        "Str" => Ok(Type::Str),
        "Keyword" => Ok(Type::Keyword),
        "Symbol" => Ok(Type::Symbol),
        "Unit" => Ok(Type::Unit),
        "Never" => Ok(Type::Never),
        "Int8" => Ok(Type::Int8),
        "Int16" => Ok(Type::Int16),
        "Int32" => Ok(Type::Int32),
        "U8" => Ok(Type::U8),
        "U16" => Ok(Type::U16),
        "U32" => Ok(Type::U32),
        "U64" => Ok(Type::U64),
        "F32" => Ok(Type::F32),
        _ => Ok(Type::Adt {
            name: name.to_string(),
            args: vec![],
        }),
    }?;
    Ok(ty)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use nexl_ast::{Atom, FileId, FloatSuffix, IntSuffix, Node, Pattern, Span};
    use nexl_types::{Constructor, EffectRow, Scheme, Type, TypeDef, TypeErrorKind, TypeVar};

    use super::{
        DeftypeDecl, InferState, check, infer_def, infer_defn, parse_match, register_deftype, synth,
        validate_module_performs,
    };
    use crate::Env;
    use nexl_ast::NodeKind;
    use nexl_reader::read;

    /// Build `(defn name [param...] body)` as a List node.
    fn defn_node(name: &str, params: Vec<&str>, body: Node) -> Node {
        let head = sym_node("defn");
        let pvec = Node::new(
            NodeKind::Vector(params.iter().map(|p| sym_node(p)).collect()),
            syn_span(),
        );
        Node::new(
            NodeKind::List(vec![head, sym_node(name), pvec, body]),
            syn_span(),
        )
    }

    /// Build `(fn [param...] body)` as a List node.
    fn fn_node(params: Vec<&str>, body: Node) -> Node {
        let head = sym_node("fn");
        let pvec = Node::new(
            NodeKind::Vector(params.iter().map(|p| sym_node(p)).collect()),
            syn_span(),
        );
        Node::new(NodeKind::List(vec![head, pvec, body]), syn_span())
    }

    /// Build `(if cond then else)` as a List node.
    fn if_node(cond: Node, then: Node, else_: Node) -> Node {
        Node::new(
            NodeKind::List(vec![sym_node("if"), cond, then, else_]),
            syn_span(),
        )
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
        atom_node(Atom::Int {
            value,
            suffix: None,
        })
    }

    fn int_node_suf(value: i128, suffix: IntSuffix) -> Node {
        atom_node(Atom::Int {
            value,
            suffix: Some(suffix),
        })
    }

    fn float_node(value: f64) -> Node {
        atom_node(Atom::Float {
            value,
            suffix: None,
        })
    }

    fn float_node_suf(value: f64, suffix: FloatSuffix) -> Node {
        atom_node(Atom::Float {
            value,
            suffix: Some(suffix),
        })
    }

    fn sym_node(name: &str) -> Node {
        atom_node(Atom::Symbol {
            ns: None,
            name: name.to_string(),
        })
    }

    fn qualified_sym_node(alias: &str, name: &str) -> Node {
        atom_node(Atom::Symbol {
            ns: Some(alias.to_string()),
            name: name.to_string(),
        })
    }

    fn option_ty(inner: Type) -> Type {
        Type::Adt {
            name: "Option".to_string(),
            args: vec![inner],
        }
    }

    /// Build `(def name expr)` as a List node.
    fn def_node(name: &str, expr: Node) -> Node {
        let head = sym_node("def");
        let binding = sym_node(name);
        Node::new(NodeKind::List(vec![head, binding, expr]), syn_span())
    }

    /// Parse `src` and return the first top-level node, panicking on failure.
    fn parse_one(src: &str) -> Node {
        let nodes = read(src, FileId(0)).expect("parse failed");
        assert_eq!(nodes.len(), 1, "expected exactly one top-level form");
        nodes.into_iter().next().unwrap()
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
        assert_eq!(
            synth(&float_node(1.5), &env, &mut state).unwrap(),
            Type::Float
        );
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
        assert_eq!(
            synth(&atom_node(Atom::Bool(true)), &env, &mut state).unwrap(),
            Type::Bool
        );
    }

    // -- Test 9 --
    #[test]
    fn synth_char() {
        let (env, mut state) = empty();
        assert_eq!(
            synth(&atom_node(Atom::Char('a')), &env, &mut state).unwrap(),
            Type::Char
        );
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
        let node = atom_node(Atom::Keyword {
            ns: None,
            name: "ok".into(),
        });
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Keyword);
    }

    // -- Test 12 --
    #[test]
    fn synth_unit() {
        let (env, mut state) = empty();
        assert_eq!(
            synth(&atom_node(Atom::Unit), &env, &mut state).unwrap(),
            Type::Unit
        );
    }

    // --- vector literal tests ---

    #[test]
    fn synth_vec_literal_ints() {
        let (env, mut state) = empty();
        let node = Node::new(NodeKind::Vector(vec![int_node(1), int_node(2)]), syn_span());
        assert_eq!(
            synth(&node, &env, &mut state).unwrap(),
            Type::Vec(Box::new(Type::Int))
        );
    }

    #[test]
    fn synth_vec_literal_empty() {
        let (env, mut state) = empty();
        let node = Node::new(NodeKind::Vector(vec![]), syn_span());
        let ty = synth(&node, &env, &mut state).unwrap();
        match ty {
            Type::Vec(elem) => assert!(matches!(*elem, Type::Var(_))),
            other => panic!("expected Vec, got {other:?}"),
        }
    }

    #[test]
    fn synth_vec_literal_mismatch() {
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::Vector(vec![int_node(1), atom_node(Atom::Bool(true))]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch, got {err:?}"
        );
    }

    #[test]
    fn synth_map_literal_keywords() {
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::Map(vec![
                (
                    atom_node(Atom::Keyword {
                        ns: None,
                        name: "a".into(),
                    }),
                    int_node(1),
                ),
                (
                    atom_node(Atom::Keyword {
                        ns: None,
                        name: "b".into(),
                    }),
                    int_node(2),
                ),
            ]),
            syn_span(),
        );
        assert_eq!(
            synth(&node, &env, &mut state).unwrap(),
            Type::Map {
                key: Box::new(Type::Keyword),
                val: Box::new(Type::Int),
            }
        );
    }

    #[test]
    fn synth_map_literal_empty() {
        let (env, mut state) = empty();
        let node = Node::new(NodeKind::Map(vec![]), syn_span());
        let ty = synth(&node, &env, &mut state).unwrap();
        match ty {
            Type::Map { key, val } => {
                assert!(matches!(*key, Type::Var(_)));
                assert!(matches!(*val, Type::Var(_)));
            }
            other => panic!("expected Map, got {other:?}"),
        }
    }

    #[test]
    fn synth_map_literal_key_mismatch() {
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::Map(vec![
                (int_node(1), int_node(2)),
                (atom_node(Atom::Bool(true)), int_node(3)),
            ]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch, got {err:?}"
        );
    }

    #[test]
    fn synth_map_literal_value_mismatch() {
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::Map(vec![
                (int_node(1), int_node(2)),
                (int_node(3), atom_node(Atom::Bool(true))),
            ]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch, got {err:?}"
        );
    }

    #[test]
    fn synth_set_literal_ints() {
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::Set(vec![int_node(1), int_node(2), int_node(3)]),
            syn_span(),
        );
        assert_eq!(
            synth(&node, &env, &mut state).unwrap(),
            Type::Set(Box::new(Type::Int))
        );
    }

    #[test]
    fn synth_set_literal_empty() {
        let (env, mut state) = empty();
        let node = Node::new(NodeKind::Set(vec![]), syn_span());
        let ty = synth(&node, &env, &mut state).unwrap();
        match ty {
            Type::Set(elem) => assert!(matches!(*elem, Type::Var(_))),
            other => panic!("expected Set, got {other:?}"),
        }
    }

    #[test]
    fn synth_set_literal_mismatch() {
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::Set(vec![int_node(1), atom_node(Atom::Bool(true))]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch, got {err:?}"
        );
    }

    // --- collection operation inference tests ---

    #[test]
    fn infer_vec_ops_types() {
        let (env, mut state) = empty();
        let get_node = parse_one("(get [1 2] 0)");
        assert_eq!(
            synth(&get_node, &env, &mut state).unwrap(),
            option_ty(Type::Int)
        );

        let put_node = parse_one("(put [1 2] 0 3)");
        assert_eq!(
            synth(&put_node, &env, &mut state).unwrap(),
            Type::Vec(Box::new(Type::Int))
        );

        let first_node = parse_one("(first [1 2])");
        assert_eq!(
            synth(&first_node, &env, &mut state).unwrap(),
            option_ty(Type::Int)
        );

        let last_node = parse_one("(last [1 2])");
        assert_eq!(
            synth(&last_node, &env, &mut state).unwrap(),
            option_ty(Type::Int)
        );

        let slice_node = parse_one("(slice [1 2 3] 0 2)");
        assert_eq!(
            synth(&slice_node, &env, &mut state).unwrap(),
            Type::Vec(Box::new(Type::Int))
        );
    }

    #[test]
    fn infer_map_ops_types() {
        let (env, mut state) = empty();
        let get_node = parse_one(r#"(get {:a 1} :a)"#);
        assert_eq!(
            synth(&get_node, &env, &mut state).unwrap(),
            option_ty(Type::Int)
        );

        let keys_node = parse_one(r#"(keys {:a 1 :b 2})"#);
        assert_eq!(
            synth(&keys_node, &env, &mut state).unwrap(),
            Type::Vec(Box::new(Type::Keyword))
        );

        let entries_node = parse_one(r#"(entries {:a 1})"#);
        assert_eq!(
            synth(&entries_node, &env, &mut state).unwrap(),
            Type::Vec(Box::new(Type::Tuple(vec![Type::Keyword, Type::Int])))
        );

        let contains_node = parse_one(r#"(contains? {:a 1} :a)"#);
        assert_eq!(
            synth(&contains_node, &env, &mut state).unwrap(),
            Type::Bool
        );
    }

    #[test]
    fn infer_set_ops_types() {
        let (env, mut state) = empty();
        let add_node = parse_one(r#"(add #{1 2} 3)"#);
        assert_eq!(
            synth(&add_node, &env, &mut state).unwrap(),
            Type::Set(Box::new(Type::Int))
        );

        let union_node = parse_one(r#"(union #{1 2} #{2 3})"#);
        assert_eq!(
            synth(&union_node, &env, &mut state).unwrap(),
            Type::Set(Box::new(Type::Int))
        );

        let contains_node = parse_one(r#"(contains? #{1 2} 1)"#);
        assert_eq!(
            synth(&contains_node, &env, &mut state).unwrap(),
            Type::Bool
        );
    }

    #[test]
    fn infer_map_filter_reduce_vec_types() {
        let (env, mut state) = empty();
        let map_node = parse_one("(map (fn [x] x) [1 2])");
        assert_eq!(
            synth(&map_node, &env, &mut state).unwrap(),
            Type::Vec(Box::new(Type::Int))
        );

        let filter_node = parse_one("(filter (fn [x] true) [1 2])");
        assert_eq!(
            synth(&filter_node, &env, &mut state).unwrap(),
            Type::Vec(Box::new(Type::Int))
        );

        let reduce_node = parse_one("(reduce (fn [acc x] acc) 0 [1 2])");
        assert_eq!(synth(&reduce_node, &env, &mut state).unwrap(), Type::Int);
    }

    #[test]
    fn infer_map_filter_reduce_map_types() {
        let (env, mut state) = empty();
        let map_node = parse_one(r#"(map (fn [x] x) {:a 1})"#);
        assert_eq!(
            synth(&map_node, &env, &mut state).unwrap(),
            Type::Map {
                key: Box::new(Type::Keyword),
                val: Box::new(Type::Int),
            }
        );

        let filter_node = parse_one(r#"(filter (fn [x] true) {:a 1})"#);
        assert_eq!(
            synth(&filter_node, &env, &mut state).unwrap(),
            Type::Map {
                key: Box::new(Type::Keyword),
                val: Box::new(Type::Int),
            }
        );

        let reduce_node = parse_one(r#"(reduce (fn [acc x] acc) 0 {:a 1})"#);
        assert_eq!(synth(&reduce_node, &env, &mut state).unwrap(), Type::Int);
    }

    #[test]
    fn infer_map_filter_reduce_set_types() {
        let (env, mut state) = empty();
        let map_node = parse_one(r#"(map (fn [x] x) #{1 2})"#);
        assert_eq!(
            synth(&map_node, &env, &mut state).unwrap(),
            Type::Set(Box::new(Type::Int))
        );

        let filter_node = parse_one(r#"(filter (fn [x] true) #{1 2})"#);
        assert_eq!(
            synth(&filter_node, &env, &mut state).unwrap(),
            Type::Set(Box::new(Type::Int))
        );

        let reduce_node = parse_one(r#"(reduce (fn [acc x] acc) 0 #{1 2})"#);
        assert_eq!(synth(&reduce_node, &env, &mut state).unwrap(), Type::Int);
    }

    #[test]
    fn infer_map_filter_reduce_option_types() {
        let (env, mut state) = empty();
        let map_node = parse_one("(map (fn [x] x) (Some 1))");
        assert_eq!(
            synth(&map_node, &env, &mut state).unwrap(),
            option_ty(Type::Int)
        );

        let filter_node = parse_one("(filter (fn [x] true) (Some 1))");
        assert_eq!(
            synth(&filter_node, &env, &mut state).unwrap(),
            option_ty(Type::Int)
        );

        let reduce_node = parse_one("(reduce (fn [acc x] acc) 0 (Some 1))");
        assert_eq!(synth(&reduce_node, &env, &mut state).unwrap(), Type::Int);
    }

    #[test]
    fn infer_each_over_vec_returns_unit() {
        let (env, mut state) = empty();
        let node = parse_one("(each [x [1 2]] (if true x 0))");
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Unit);
    }

    #[test]
    fn infer_each_over_map_returns_unit() {
        let (env, mut state) = empty();
        let node = parse_one(r#"(each [x {:a 1}] (if true x 0))"#);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Unit);
    }

    #[test]
    fn infer_times_returns_unit() {
        let (env, mut state) = empty();
        let node = parse_one("(times [i 3] (if true i 0))");
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Unit);
    }

    #[test]
    fn infer_for_returns_vec() {
        let (env, mut state) = empty();
        let node = parse_one("(for [x [1 2]] x)");
        assert_eq!(
            synth(&node, &env, &mut state).unwrap(),
            Type::Vec(Box::new(Type::Int))
        );
    }

    #[test]
    fn infer_for_with_clauses_returns_vec() {
        let (env, mut state) = empty();
        let node = parse_one("(for [x [1 2] :let [y x] :when true :while true] y)");
        assert_eq!(
            synth(&node, &env, &mut state).unwrap(),
            Type::Vec(Box::new(Type::Int))
        );
    }

    #[test]
    fn infer_for_bang_returns_vec() {
        let (env, mut state) = empty();
        let node = parse_one("(for! [x [1 2]] x)");
        assert_eq!(
            synth(&node, &env, &mut state).unwrap(),
            Type::Vec(Box::new(Type::Int))
        );
    }

    // -- Test 13 --
    #[test]
    fn synth_int_i8_suffix() {
        let (env, mut state) = empty();
        assert_eq!(
            synth(&int_node_suf(1, IntSuffix::I8), &env, &mut state).unwrap(),
            Type::Int8
        );
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
            body: Type::Fn {
                params: vec![Type::Var(t0)],
                ret: Box::new(Type::Var(t0)),
                effects: EffectRow::empty(),
            },
        };
        let env = Env::new().extend("id", scheme);
        // instantiate will call state.supply.fresh() → TypeVar(1)
        let ty = synth(&sym_node("id"), &env, &mut state).unwrap();
        match ty {
            Type::Fn { params, ret, .. } => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0], *ret, "param and ret must be the same fresh var");
                assert_ne!(
                    params[0],
                    Type::Var(t0),
                    "must be a fresh var, not the original t0"
                );
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

    // -- Test 26 --
    #[test]
    fn synth_var_qualified() {
        let mut exports = HashMap::new();
        exports.insert(
            "add".to_string(),
            Scheme::mono(Type::Fn {
                params: vec![Type::Int, Type::Int],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }),
        );
        let env = Env::new().extend_module("math", exports);
        let mut state = InferState::new();
        let ty = synth(&qualified_sym_node("math", "add"), &env, &mut state).unwrap();
        assert_eq!(
            ty,
            Type::Fn {
                params: vec![Type::Int, Type::Int],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }
        );
    }

    // -- Test 27 --
    #[test]
    fn synth_var_qualified_unknown() {
        let mut exports = HashMap::new();
        exports.insert("add".to_string(), Scheme::mono(Type::Int));
        let env = Env::new().extend_module("math", exports);
        let mut state = InferState::new();
        let err = synth(&qualified_sym_node("math", "sub"), &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "math/sub"),
            "expected UnboundVariable(math/sub), got {err:?}"
        );
    }

    // -- Test 28 --
    #[test]
    fn module_performs_infers_union() {
        let mut exports = HashMap::new();
        exports.insert(
            "start!".to_string(),
            vec!["Net".to_string(), "IO".to_string()],
        );
        exports.insert("stop!".to_string(), vec!["IO".to_string()]);
        let inferred = validate_module_performs(None, &exports).expect("validate failed");
        assert_eq!(inferred, vec!["IO".to_string(), "Net".to_string()]);
    }

    // -- Test 29 --
    #[test]
    fn module_performs_rejects_missing_effect() {
        let mut exports = HashMap::new();
        exports.insert("start!".to_string(), vec!["Net".to_string()]);
        let declared = vec!["IO".to_string()];
        let err = validate_module_performs(Some(&declared), &exports).unwrap_err();
        match err.kind {
            TypeErrorKind::MalformedForm { description } => {
                assert!(
                    description.contains("Net") && description.contains("start!"),
                    "unexpected error description: {description}"
                );
            }
            other => panic!("expected MalformedForm, got {other:?}"),
        }
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
        assert_eq!(
            ty,
            Type::Fn {
                params: vec![],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }
        );
    }

    // -- Test 3 (defn) --
    #[test]
    fn infer_defn_one_param_identity_type() {
        // (defn f [x] x) → (Fn [t?] -> t?) where param == ret var
        let (env, mut state) = empty();
        let node = defn_node("f", vec!["x"], sym_node("x"));
        let (_name, ty, _env) = infer_defn(&node, &env, &mut state).unwrap();
        match ty {
            Type::Fn { params, ret, .. } => {
                assert_eq!(params.len(), 1);
                assert_eq!(
                    params[0], *ret,
                    "param and return type must be the same var"
                );
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
        assert_eq!(
            scheme.body,
            Type::Fn {
                params: vec![],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }
        );
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
            NodeKind::List(vec![
                sym_node("defn"),
                sym_node("f"),
                int_node(42),
                int_node(99),
            ]),
            syn_span(),
        );
        let err = infer_defn(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // deftype form tests
    // -----------------------------------------------------------------------

    // -- Test 1 (deftype sum) --
    #[test]
    fn parse_deftype_sum_nullary_constructors() {
        let node = parse_one("(deftype Color | Red | Green | Blue)");
        let td = match super::parse_deftype(&node).unwrap() {
            DeftypeDecl::Sum(td) => td,
            other => panic!("expected Sum deftype, got {other:?}"),
        };
        let expected = TypeDef {
            name: "Color".to_string(),
            params: vec![],
            constructors: vec![
                Constructor::nullary("Red"),
                Constructor::nullary("Green"),
                Constructor::nullary("Blue"),
            ],
        };
        assert_eq!(td, expected);
    }

    // -- Test 2 (deftype sum) --
    #[test]
    fn parse_deftype_sum_nary_constructor_fields() {
        let node = parse_one("(deftype Result | (Ok Int) | (Err Str))");
        let td = match super::parse_deftype(&node).unwrap() {
            DeftypeDecl::Sum(td) => td,
            other => panic!("expected Sum deftype, got {other:?}"),
        };
        let expected = TypeDef {
            name: "Result".to_string(),
            params: vec![],
            constructors: vec![
                Constructor::nary("Ok", vec![Type::Int]),
                Constructor::nary("Err", vec![Type::Str]),
            ],
        };
        assert_eq!(td, expected);
    }

    // -- Test 3 (deftype sum) --
    #[test]
    fn parse_deftype_sum_missing_pipe_is_malformed() {
        let node = parse_one("(deftype Color Red)");
        let err = super::parse_deftype(&node).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 4 (deftype record) --
    #[test]
    fn parse_deftype_record_fields() {
        let node = parse_one("(deftype Point {:x Float :y Float})");
        let (name, params, fields) = match super::parse_deftype(&node).unwrap() {
            DeftypeDecl::Record {
                name,
                params,
                fields,
            } => (name, params, fields),
            other => panic!("expected Record deftype, got {other:?}"),
        };
        assert_eq!(name, "Point");
        assert!(params.is_empty(), "record type params should be empty");
        assert_eq!(
            fields,
            vec![
                ("x".to_string(), Type::Float),
                ("y".to_string(), Type::Float)
            ]
        );
    }

    // -- Test 5 (deftype record) --
    #[test]
    fn parse_deftype_record_field_key_must_be_keyword() {
        let node = parse_one("(deftype Point {x Float})");
        let err = super::parse_deftype(&node).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 6 (deftype params) --
    #[test]
    fn parse_deftype_params_option() {
        let node = parse_one("(deftype Option [a] | None | (Some a))");
        let td = match super::parse_deftype(&node).unwrap() {
            DeftypeDecl::Sum(td) => td,
            other => panic!("expected Sum deftype, got {other:?}"),
        };
        assert_eq!(td.name, "Option");
        assert_eq!(td.params.len(), 1);
        let a = td.params[0];
        assert_eq!(
            td.constructors,
            vec![
                Constructor::nullary("None"),
                Constructor::nary("Some", vec![Type::Var(a)]),
            ]
        );
    }

    // -- Test 7 (deftype params) --
    #[test]
    fn parse_deftype_params_recursive_adt() {
        let node = parse_one("(deftype Tree [a] | Leaf | (Branch a (Tree a) (Tree a)))");
        let td = match super::parse_deftype(&node).unwrap() {
            DeftypeDecl::Sum(td) => td,
            other => panic!("expected Sum deftype, got {other:?}"),
        };
        assert_eq!(td.name, "Tree");
        assert_eq!(td.params.len(), 1);
        let a = td.params[0];
        let tree_a = Type::Adt {
            name: "Tree".to_string(),
            args: vec![Type::Var(a)],
        };
        assert_eq!(
            td.constructors,
            vec![
                Constructor::nullary("Leaf"),
                Constructor::nary("Branch", vec![Type::Var(a), tree_a.clone(), tree_a]),
            ]
        );
    }

    // -- Test 8 (deftype params) --
    #[test]
    fn parse_deftype_params_must_be_vector() {
        let node = parse_one("(deftype Option a | None)");
        let err = super::parse_deftype(&node).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // deftype registration tests
    // -----------------------------------------------------------------------

    // -- Test 9 (deftype register) --
    #[test]
    fn register_deftype_sum_adds_type_def_and_ctors() {
        let t0 = TypeVar(0);
        let td = TypeDef {
            name: "Option".to_string(),
            params: vec![t0],
            constructors: vec![
                Constructor::nullary("None"),
                Constructor::nary("Some", vec![Type::Var(t0)]),
            ],
        };
        let decl = DeftypeDecl::Sum(td.clone());
        let env = Env::new();
        let env = register_deftype(&env, decl);
        assert_eq!(env.lookup_type_def("Option"), Some(&td));
        let ctor = env.lookup_ctor("Some").expect("Some should be registered");
        assert_eq!(ctor.type_name, "Option");
        assert_eq!(ctor.ctor, Constructor::nary("Some", vec![Type::Var(t0)]));
    }

    // -- Test 10 (deftype register) --
    #[test]
    fn register_deftype_record_adds_record_def() {
        let decl = DeftypeDecl::Record {
            name: "Point".to_string(),
            params: vec![],
            fields: vec![("x".to_string(), Type::Float)],
        };
        let env = Env::new();
        let env = register_deftype(&env, decl);
        let rec = env
            .lookup_record_def("Point")
            .expect("Point record should be registered");
        assert_eq!(rec.name, "Point");
        assert_eq!(rec.fields, vec![("x".to_string(), Type::Float)]);
    }

    // -- Test 10b (deftype register) --
    #[test]
    fn parse_and_register_option_registers_schemes() {
        let def = parse_one("(deftype Option [a] | None | (Some a))");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);

        let td = env
            .lookup_type_def("Option")
            .expect("typedef Option should be present");
        assert_eq!(td.params.len(), 1);
        let a = td.params[0];

        let some = env
            .lookup("Some")
            .expect("Some scheme should be registered");
        assert!(
            some.forall.contains(&a),
            "Some should quantify its type parameter"
        );
        assert_eq!(
            some.body,
            Type::Fn {
                params: vec![Type::Var(a)],
                ret: Box::new(Type::Adt {
                    name: "Option".to_string(),
                    args: vec![Type::Var(a)]
                }),
                effects: EffectRow::empty(),
            }
        );

        let none = env
            .lookup("None")
            .expect("None scheme should be registered");
        assert!(
            none.forall.contains(&a),
            "None should quantify its type parameter"
        );
        assert_eq!(
            none.body,
            Type::Adt {
                name: "Option".to_string(),
                args: vec![Type::Var(a)]
            }
        );
    }

    // -- Test 10c (deftype register) --
    #[test]
    fn parse_and_register_record_registers_ctor_scheme() {
        let def = parse_one("(deftype Point {:x Float :y Float})");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);

        let rec = env
            .lookup_record_def("Point")
            .expect("Point record should be registered");
        assert!(rec.params.is_empty(), "record should be monomorphic here");

        let ctor = env
            .lookup("Point")
            .expect("record constructor should be bound in env");
        assert!(
            ctor.forall.is_empty(),
            "record constructor scheme should be monomorphic"
        );
        let record_ty = Type::Record {
            name: "Point".to_string(),
            fields: vec![
                ("x".to_string(), Type::Float),
                ("y".to_string(), Type::Float),
            ],
        };
        assert_eq!(
            ctor.body,
            Type::Fn {
                params: vec![record_ty.clone()],
                ret: Box::new(record_ty),
                effects: EffectRow::empty(),
            }
        );
    }

    // -----------------------------------------------------------------------
    // constructor application tests
    // -----------------------------------------------------------------------

    // -- Test 11 (constructor application) --
    #[test]
    fn infer_constructor_application_some_int() {
        let def = parse_one("(deftype Option [a] | None | (Some a))");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(Some 42)");
        let mut state = InferState::new();
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(
            ty,
            Type::Adt {
                name: "Option".to_string(),
                args: vec![Type::Int]
            }
        );
    }

    // -- Test 11b (constructor application) --
    #[test]
    fn infer_constructor_application_some_str() {
        let def = parse_one("(deftype Option [a] | None | (Some a))");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(Some \"hi\")");
        let mut state = InferState::new();
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(
            ty,
            Type::Adt {
                name: "Option".to_string(),
                args: vec![Type::Str]
            }
        );
    }

    // -- Test 12 (constructor application) --
    #[test]
    fn infer_nullary_constructor_usage_none() {
        let def = parse_one("(deftype Option [a] | None | (Some a))");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("None");
        let mut state = InferState::new();
        let ty = synth(&node, &env, &mut state).unwrap();
        match ty {
            Type::Adt { name, args } => {
                assert_eq!(name, "Option");
                assert_eq!(args.len(), 1);
                assert!(
                    matches!(args[0], Type::Var(_)),
                    "arg should be a fresh type var"
                );
            }
            other => panic!("expected Option type, got {other:?}"),
        }
    }

    // -- Test 13 (record construction) --
    #[test]
    fn infer_record_construction_point() {
        let def = parse_one("(deftype Point {:x Float :y Float})");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(Point {:x 1.0 :y 2.0})");
        let mut state = InferState::new();
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(
            ty,
            Type::Record {
                name: "Point".to_string(),
                fields: vec![
                    ("x".to_string(), Type::Float),
                    ("y".to_string(), Type::Float),
                ],
            }
        );
    }

    // -- Test 14 (field access) --
    #[test]
    fn infer_keyword_field_access() {
        let def = parse_one("(deftype Point {:x Float :y Float})");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(:x (Point {:x 1.0 :y 2.0}))");
        let mut state = InferState::new();
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Float);
    }

    // -----------------------------------------------------------------------
    // match form parsing tests
    // -----------------------------------------------------------------------

    // -- Test 15 (match parse) --
    #[test]
    fn parse_match_two_arms_no_guard() {
        let node = parse_one("(match x 0 1 _ 2)");
        let (scrutinee, arms) = parse_match(&node).unwrap();
        assert!(
            matches!(
                &scrutinee.kind,
                NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "x"
            ),
            "scrutinee should be symbol x"
        );
        assert_eq!(arms.len(), 2);
        assert_eq!(
            arms[0].pattern,
            Pattern::Literal(Atom::Int {
                value: 0,
                suffix: None
            })
        );
        assert!(arms[0].guard.is_none());
        assert!(
            matches!(
                &arms[0].body.kind,
                NodeKind::Atom(Atom::Int {
                    value: 1,
                    suffix: None
                })
            ),
            "first arm body should be 1"
        );
        assert_eq!(arms[1].pattern, Pattern::Wildcard);
        assert!(arms[1].guard.is_none());
        assert!(
            matches!(
                &arms[1].body.kind,
                NodeKind::Atom(Atom::Int {
                    value: 2,
                    suffix: None
                })
            ),
            "second arm body should be 2"
        );
    }

    // -- Test 16 (match parse) --
    #[test]
    fn parse_match_guard_arm() {
        let node = parse_one("(match x 0 :when (> x 0) 1 _ 2)");
        let (_scrutinee, arms) = parse_match(&node).unwrap();
        assert_eq!(arms.len(), 2);
        assert!(arms[0].guard.is_some());
        let guard = arms[0].guard.unwrap();
        assert!(
            matches!(
                &guard.kind,
                NodeKind::List(items)
                    if matches!(
                        &items[0].kind,
                        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == ">"
                    )
            ),
            "guard should be (> x 0)"
        );
    }

    // -- Test 17 (match parse) --
    #[test]
    fn parse_match_missing_body_is_malformed() {
        let node = parse_one("(match x 0)");
        let err = parse_match(&node).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 18 (match parse) --
    #[test]
    fn parse_match_guard_missing_body_is_malformed() {
        let node = parse_one("(match x 0 :when 1)");
        let err = parse_match(&node).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // match form inference tests (scrutinee only)
    // -----------------------------------------------------------------------

    // -- Test 19 (match infer) --
    #[test]
    fn infer_match_scrutinee_unbound_is_error() {
        let (env, mut state) = empty();
        let node = parse_one("(match unknown _ 0)");
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "expected UnboundVariable(unknown), got {err:?}"
        );
    }

    // -- Test 20 (match infer) --
    #[test]
    fn infer_match_scrutinee_inner_mismatch_is_error() {
        let (env, mut state) = empty();
        let node = parse_one("(match (if 0 1 2) _ 3)");
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

    // -- Test 21 (match infer) --
    #[test]
    fn infer_match_form_is_recognized() {
        let (env, mut state) = empty();
        let node = parse_one("(match 1 _ 2)");
        assert!(
            synth(&node, &env, &mut state).is_ok(),
            "match form should be synthesized, not treated as application"
        );
    }

    // -- Test 22 (match infer) --
    #[test]
    fn infer_match_too_few_elements_is_malformed() {
        let (env, mut state) = empty();
        let node = parse_one("(match 1)");
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 23 (match infer) --
    #[test]
    fn infer_match_pattern_type_mismatch_is_error() {
        let (env, mut state) = empty();
        let node = parse_one("(match 1 0 1 true 2)");
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

    // -- Test 24 (match infer) --
    #[test]
    fn infer_match_unifies_arm_body_types() {
        let (env, mut state) = empty();
        let node = parse_one("(match 1 0 1 _ 2)");
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int);
    }

    // -- Test 24b (match infer) --
    #[test]
    fn infer_match_option_exhaustive_returns_int() {
        let def = parse_one("(deftype Option [a] | None | (Some a))");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(match (Some 1) (Some x) x None 0)");
        let mut state = InferState::new();
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int);
    }

    // -- Test 25 (match infer) --
    #[test]
    fn infer_match_guard_must_be_bool() {
        let (env, mut state) = empty();
        let node = parse_one("(match 1 0 :when 42 1 _ 0)");
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

    // -- Test 26 (match infer) --
    #[test]
    fn infer_match_nullary_constructor_pattern_mismatch() {
        let def = parse_one("(deftype Option [a] | None | (Some a))");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(match 1 None 0 _ 1)");
        let mut state = InferState::new();
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::Mismatch { ref expected, ref found }
                    if *expected == Type::Int
                        && matches!(found, Type::Adt { name, .. } if name == "Option")
            ),
            "expected Mismatch(Int, Option), got {err:?}"
        );
    }

    // -- Test 27 (match infer) --
    #[test]
    fn infer_match_constructor_pattern_binds_arg() {
        let def = parse_one("(deftype Option [a] | None | (Some a))");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(match (Some 1) (Some x) x None 0)");
        let mut state = InferState::new();
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int);
    }

    // -- Test 28 (match infer) --
    #[test]
    fn infer_match_record_pattern_binds_fields() {
        let def = parse_one("(deftype Point {:x Float :y Float})");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(match (Point {:x 1.0 :y 2.0}) {:x x :y y} x)");
        let mut state = InferState::new();
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Float);
    }

    // -- Test 29 (match infer) --
    #[test]
    fn infer_match_non_exhaustive_option_is_error() {
        let def = parse_one("(deftype Option [a] | None | (Some a))");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(match (Some 1) (Some x) x)");
        let mut state = InferState::new();
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::MalformedForm { ref description }
                    if description.contains("non-exhaustive")
            ),
            "expected non-exhaustive match error, got {err:?}"
        );
    }

    // -- Test 30 (match infer) --
    #[test]
    fn infer_match_redundant_pattern_warns() {
        let def = parse_one("(deftype Option [a] | None | (Some a))");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(match (Some 1) None 0 (Some x) x _ 2)");
        let mut state = InferState::new();
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int);
        assert_eq!(
            state.warnings.len(),
            1,
            "expected one redundant pattern warning"
        );
    }

    // -- Test 31 (match infer) --
    #[test]
    fn infer_match_non_exhaustive_bool_is_error() {
        let (env, mut state) = empty();
        let node = parse_one("(match true true 1)");
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::MalformedForm { ref description }
                    if description.contains("non-exhaustive")
            ),
            "expected non-exhaustive match error, got {err:?}"
        );
    }

    // -- Test 32 (match infer) --
    #[test]
    fn infer_match_non_exhaustive_color_is_error() {
        let def = parse_one("(deftype Color | Red | Green | Blue)");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(match Red Red 1 Green 2)");
        let mut state = InferState::new();
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::MalformedForm { ref description }
                    if description.contains("non-exhaustive")
            ),
            "expected non-exhaustive match error, got {err:?}"
        );
    }

    // -- Test 33 (match infer) --
    #[test]
    fn infer_match_non_exhaustive_result_is_error() {
        let def = parse_one("(deftype Result [a b] | (Ok a) | (Err b))");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(match (Ok 1) (Ok x) x)");
        let mut state = InferState::new();
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::MalformedForm { ref description }
                    if description.contains("non-exhaustive")
            ),
            "expected non-exhaustive match error, got {err:?}"
        );
    }

    // -- Test 34 (let destructuring) --
    #[test]
    fn infer_let_constructor_pattern_non_exhaustive_is_error() {
        let def = parse_one("(deftype Option [a] | None | (Some a))");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(let [(Some v) (Some 1)] v)");
        let mut state = InferState::new();
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::MalformedForm { ref description }
                    if description.contains("non-exhaustive")
            ),
            "expected non-exhaustive let pattern error, got {err:?}"
        );
    }

    // -- Test 35 (let destructuring) --
    #[test]
    fn infer_let_record_destructuring_binds_fields() {
        let def = parse_one("(deftype Point {:x Float :y Float})");
        let decl = super::parse_deftype(&def).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let node = parse_one("(let [{:x x :y y} (Point {:x 1.0 :y 2.0})] x)");
        let mut state = InferState::new();
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Float);
    }

    // -- Test 36 (let destructuring) --
    #[test]
    fn infer_let_tuple_destructuring_binds_fields() {
        let (env, mut state) = empty();
        let node = parse_one("(let [[a b] [1 2]] a)");
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int);
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
        assert_eq!(
            ty,
            Type::Fn {
                params: vec![],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }
        );
    }

    // -- Test 2 (fn) --
    #[test]
    fn infer_fn_body_is_constant() {
        // (fn [x] 42) → (Fn [t?] -> Int); param stays as a free var
        let (env, mut state) = empty();
        let node = fn_node(vec!["x"], int_node(42));
        let ty = synth(&node, &env, &mut state).unwrap();
        match ty {
            Type::Fn { params, ret, .. } => {
                assert_eq!(params.len(), 1);
                assert!(
                    matches!(params[0], Type::Var(_)),
                    "param should be a free var"
                );
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
            Type::Fn { params, ret, .. } => {
                assert_eq!(params.len(), 1);
                assert_eq!(
                    params[0], *ret,
                    "param and return type must be the same var"
                );
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
            Type::Fn { params, ret, .. } => {
                assert_eq!(params.len(), 2);
                assert_ne!(
                    params[0], params[1],
                    "params should have distinct type vars"
                );
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
        let node = if_node(
            atom_node(Atom::Bool(true)),
            sym_node("unknown"),
            int_node(2),
        );
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
        let node = if_node(
            atom_node(Atom::Bool(true)),
            int_node(1),
            sym_node("unknown"),
        );
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
            NodeKind::List(vec![
                sym_node("if"),
                atom_node(Atom::Bool(true)),
                int_node(1),
            ]),
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
            float_node(1.5),
            atom_node(Atom::Str("hello".into())),
        ]);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Str);
    }

    // -- Test 4 (do) --
    #[test]
    fn infer_do_error_in_early_expr() {
        // (do unknown 42): early failure is collected, last expr still gives Int.
        let (env, mut state) = empty();
        let node = do_node(vec![sym_node("unknown"), int_node(42)]);
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int, "body type should still be Int");
        assert_eq!(state.errors.len(), 1, "one error should be collected");
        assert!(
            matches!(state.errors[0].kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "collected error should be UnboundVariable(unknown)"
        );
    }

    // -- Test 5 (do) --
    #[test]
    fn infer_do_error_in_last_expr() {
        // (do 42 unknown): the last expr fails; its error is collected.
        let (env, mut state) = empty();
        let node = do_node(vec![int_node(42), sym_node("unknown")]);
        let _ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(state.errors.len(), 1, "one error should be collected");
        assert!(
            matches!(state.errors[0].kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "collected error should be UnboundVariable(unknown)"
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
            vec![
                (sym_node("x"), int_node(42)),
                (sym_node("y"), sym_node("x")),
            ],
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
        // (let [x unknown] x): binding init fails; error is collected and x gets a
        // fresh type var so the body is still checked.
        let (env, mut state) = empty();
        let node = let_node(vec![(sym_node("x"), sym_node("unknown"))], sym_node("x"));
        let _ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(state.errors.len(), 1, "one error should be collected");
        assert!(
            matches!(state.errors[0].kind, TypeErrorKind::UnboundVariable { ref name } if name == "unknown"),
            "collected error should be UnboundVariable(unknown)"
        );
    }

    // -- Test 8 (let) --
    #[test]
    fn infer_let_malformed_not_list() {
        // passing an atom to synth with a List dispatch → only reachable via
        // direct synth_let call; test via a malformed node with wrong element count.
        // Use a let list with only 1 element: (let)
        let (env, mut state) = empty();
        let node = Node::new(NodeKind::List(vec![sym_node("let")]), syn_span());
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
        let node = Node::new(NodeKind::List(vec![sym_node("let"), bvec]), syn_span());
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
    // Type annotation tests
    // -----------------------------------------------------------------------

    use super::parse_type_expr;

    fn type_sym(name: &str) -> Node {
        sym_node(name)
    }

    /// Build `(Fn [param-types...] -> ret)` as a List node.
    fn fn_type_node(params: Vec<Node>, ret: Node) -> Node {
        let pvec = Node::new(NodeKind::Vector(params), syn_span());
        Node::new(
            NodeKind::List(vec![sym_node("Fn"), pvec, sym_node("->"), ret]),
            syn_span(),
        )
    }

    // -- Test 10 (annot) --
    #[test]
    fn defn_param_annotation() {
        // (defn f [x : Int] x) → (Fn [Int] -> Int); param type is Int, not a free var
        let (env, mut state) = empty();
        let pvec = Node::new(
            NodeKind::Vector(vec![sym_node("x"), sym_node(":"), sym_node("Int")]),
            syn_span(),
        );
        let node = Node::new(
            NodeKind::List(vec![sym_node("defn"), sym_node("f"), pvec, sym_node("x")]),
            syn_span(),
        );
        let (_name, ty, _env) = infer_defn(&node, &env, &mut state).unwrap();
        assert_eq!(
            ty,
            Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }
        );
    }

    // -- Test 11 (annot) --
    #[test]
    fn defn_return_annotation_matches() {
        // (defn f [] -> Int 42) → succeeds
        let (env, mut state) = empty();
        let pvec = Node::new(NodeKind::Vector(vec![]), syn_span());
        let node = Node::new(
            NodeKind::List(vec![
                sym_node("defn"),
                sym_node("f"),
                pvec,
                sym_node("->"),
                sym_node("Int"),
                int_node(42),
            ]),
            syn_span(),
        );
        let (_name, ty, _env) = infer_defn(&node, &env, &mut state).unwrap();
        assert_eq!(
            ty,
            Type::Fn {
                params: vec![],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }
        );
    }

    // -- Test 12 (annot) --
    #[test]
    fn defn_return_annotation_mismatch() {
        // (defn f [] -> Bool 42) → Mismatch { expected: Bool, found: Int }
        let (env, mut state) = empty();
        let pvec = Node::new(NodeKind::Vector(vec![]), syn_span());
        let node = Node::new(
            NodeKind::List(vec![
                sym_node("defn"),
                sym_node("f"),
                pvec,
                sym_node("->"),
                sym_node("Bool"),
                int_node(42),
            ]),
            syn_span(),
        );
        let err = infer_defn(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch, got {err:?}"
        );
    }

    // -- Test 13 (annot) --
    #[test]
    fn defn_full_annotation() {
        // (defn f [x : Int] -> Int x) → (Fn [Int] -> Int); both param and return annotated
        let (env, mut state) = empty();
        let pvec = Node::new(
            NodeKind::Vector(vec![sym_node("x"), sym_node(":"), sym_node("Int")]),
            syn_span(),
        );
        let node = Node::new(
            NodeKind::List(vec![
                sym_node("defn"),
                sym_node("f"),
                pvec,
                sym_node("->"),
                sym_node("Int"),
                sym_node("x"),
            ]),
            syn_span(),
        );
        let (_name, ty, _env) = infer_defn(&node, &env, &mut state).unwrap();
        assert_eq!(
            ty,
            Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }
        );
    }

    // -- Test 7 (annot) --
    #[test]
    fn let_annotation_matches() {
        // (let [x : Int 42] x) → Int
        let (env, mut state) = empty();
        let bvec = Node::new(
            NodeKind::Vector(vec![
                sym_node("x"),
                sym_node(":"),
                sym_node("Int"),
                int_node(42),
            ]),
            syn_span(),
        );
        let node = Node::new(
            NodeKind::List(vec![sym_node("let"), bvec, sym_node("x")]),
            syn_span(),
        );
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 8 (annot) --
    #[test]
    fn let_annotation_mismatch() {
        // (let [x : Bool 42] x) → Mismatch { expected: Bool, found: Int }
        let (env, mut state) = empty();
        let bvec = Node::new(
            NodeKind::Vector(vec![
                sym_node("x"),
                sym_node(":"),
                sym_node("Bool"),
                int_node(42),
            ]),
            syn_span(),
        );
        let node = Node::new(
            NodeKind::List(vec![sym_node("let"), bvec, sym_node("x")]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch, got {err:?}"
        );
    }

    // -- Test 9 (annot) --
    #[test]
    fn let_mixed_annotated_and_plain() {
        // (let [x : Int 42 y "hi"] x) → Int
        // First binding is annotated, second is plain.
        let (env, mut state) = empty();
        let bvec = Node::new(
            NodeKind::Vector(vec![
                sym_node("x"),
                sym_node(":"),
                sym_node("Int"),
                int_node(42),
                sym_node("y"),
                atom_node(Atom::Str("hi".into())),
            ]),
            syn_span(),
        );
        let node = Node::new(
            NodeKind::List(vec![sym_node("let"), bvec, sym_node("x")]),
            syn_span(),
        );
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Int);
    }

    // -- Test 5 (annot) --
    #[test]
    fn def_annotation_matches() {
        // (def x : Int 42) → succeeds; x has type Int
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::List(vec![
                sym_node("def"),
                sym_node("x"),
                sym_node(":"),
                sym_node("Int"),
                int_node(42),
            ]),
            syn_span(),
        );
        let (_name, ty, _env) = infer_def(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int);
    }

    // -- Test 6 (annot) --
    #[test]
    fn def_annotation_mismatch() {
        // (def x : Bool 42) → Mismatch { expected: Bool, found: Int }
        let (env, mut state) = empty();
        let node = Node::new(
            NodeKind::List(vec![
                sym_node("def"),
                sym_node("x"),
                sym_node(":"),
                sym_node("Bool"),
                int_node(42),
            ]),
            syn_span(),
        );
        let err = infer_def(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::Mismatch { ref expected, ref found }
                if *expected == Type::Bool && *found == Type::Int
            ),
            "expected Mismatch(Bool, Int), got {err:?}"
        );
    }

    // -- Test 2 (annot) --
    #[test]
    fn parse_type_expr_fn_two_params() {
        // (Fn [Int Str] -> Bool) → Fn { params: [Int, Str], ret: Bool }
        let node = fn_type_node(vec![type_sym("Int"), type_sym("Str")], type_sym("Bool"));
        assert_eq!(
            parse_type_expr(&node).unwrap(),
            Type::Fn {
                params: vec![Type::Int, Type::Str],
                ret: Box::new(Type::Bool),
                effects: EffectRow::empty(),
            }
        );
    }

    // -- Test 3 (annot) --
    #[test]
    fn parse_type_expr_fn_no_params() {
        // (Fn [] -> Int) → Fn { params: [], ret: Int }
        let node = fn_type_node(vec![], type_sym("Int"));
        assert_eq!(
            parse_type_expr(&node).unwrap(),
            Type::Fn {
                params: vec![],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }
        );
    }

    // -- Test 4 (annot) --
    #[test]
    fn parse_type_expr_fn_with_effects() {
        // (Fn [Str] -> Unit ! [Console]) → Fn { params: [Str], ret: Unit }
        let node = Node::new(
            NodeKind::List(vec![
                sym_node("Fn"),
                Node::new(NodeKind::Vector(vec![type_sym("Str")]), syn_span()),
                sym_node("->"),
                type_sym("Unit"),
                sym_node("!"),
                Node::new(NodeKind::Vector(vec![type_sym("Console")]), syn_span()),
            ]),
            syn_span(),
        );
        assert_eq!(
            parse_type_expr(&node).unwrap(),
            Type::Fn {
                params: vec![Type::Str],
                ret: Box::new(Type::Unit),
                effects: EffectRow {
                    effects: vec!["Console".to_string()],
                    tail: None,
                },
            }
        );
    }

    // -- Test 5 (annot) --
    #[test]
    fn parse_type_expr_fn_with_effect_row_var() {
        // (Fn [Str] -> Unit ! [Console | r]) → Fn { params: [Str], ret: Unit }
        let node = Node::new(
            NodeKind::List(vec![
                sym_node("Fn"),
                Node::new(NodeKind::Vector(vec![type_sym("Str")]), syn_span()),
                sym_node("->"),
                type_sym("Unit"),
                sym_node("!"),
                Node::new(
                    NodeKind::Vector(vec![type_sym("Console"), sym_node("|"), sym_node("r")]),
                    syn_span(),
                ),
            ]),
            syn_span(),
        );
        assert_eq!(
            parse_type_expr(&node).unwrap(),
            Type::Fn {
                params: vec![Type::Str],
                ret: Box::new(Type::Unit),
                effects: EffectRow {
                    effects: vec!["Console".to_string()],
                    tail: Some("r".to_string()),
                },
            }
        );
    }

    // -- Test 6 (annot) --
    #[test]
    fn parse_type_expr_unknown_name() {
        // Symbol("Blorp") → MalformedForm (unknown type name)
        let err = parse_type_expr(&type_sym("Blorp")).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::MalformedForm { .. }),
            "expected MalformedForm, got {err:?}"
        );
    }

    // -- Test 1 (annot) --
    #[test]
    fn parse_type_expr_primitives() {
        assert_eq!(parse_type_expr(&type_sym("Int")).unwrap(), Type::Int);
        assert_eq!(parse_type_expr(&type_sym("Float")).unwrap(), Type::Float);
        assert_eq!(parse_type_expr(&type_sym("Bool")).unwrap(), Type::Bool);
        assert_eq!(parse_type_expr(&type_sym("Str")).unwrap(), Type::Str);
        assert_eq!(parse_type_expr(&type_sym("Char")).unwrap(), Type::Char);
        assert_eq!(parse_type_expr(&type_sym("Unit")).unwrap(), Type::Unit);
        assert_eq!(parse_type_expr(&type_sym("Never")).unwrap(), Type::Never);
        assert_eq!(parse_type_expr(&type_sym("Ratio")).unwrap(), Type::Ratio);
        assert_eq!(parse_type_expr(&type_sym("Int8")).unwrap(), Type::Int8);
        assert_eq!(parse_type_expr(&type_sym("Int32")).unwrap(), Type::Int32);
        assert_eq!(parse_type_expr(&type_sym("U64")).unwrap(), Type::U64);
        assert_eq!(parse_type_expr(&type_sym("F32")).unwrap(), Type::F32);
        // Aliases
        assert_eq!(parse_type_expr(&type_sym("Int64")).unwrap(), Type::Int);
        assert_eq!(parse_type_expr(&type_sym("F64")).unwrap(), Type::Float);
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
        let node = Node::new(NodeKind::List(vec![sym_node("loop"), bvec]), syn_span());
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
            matches!(
                err.kind,
                TypeErrorKind::ArityMismatch {
                    expected: 1,
                    found: 2
                }
            ),
            "expected ArityMismatch(1,2), got {err:?}"
        );
    }

    // -- Test 6 (loop) --
    #[test]
    fn loop_recur_arity_too_few() {
        // (loop [i 0] (recur)) — 1 loop var but recur passes 0
        // → ArityMismatch { expected: 1, found: 0 }
        let (env, mut state) = empty();
        let node = loop_node(vec![(sym_node("i"), int_node(0))], recur_node(vec![]));
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::ArityMismatch {
                    expected: 1,
                    found: 0
                }
            ),
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
            body: Type::Fn {
                params: vec![Type::Var(t0)],
                ret: Box::new(Type::Var(t0)),
                effects: EffectRow::empty(),
            },
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
        let f_ty = Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Bool),
            effects: EffectRow::empty(),
        };
        let env = Env::new().extend("f", Scheme::mono(f_ty));
        let mut state = InferState::new();
        let node = app_node(sym_node("f"), vec![]);
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::ArityMismatch {
                    expected: 1,
                    found: 0
                }
            ),
            "expected ArityMismatch(1,0), got {err:?}"
        );
    }

    // -- Test 5 (apply) --
    #[test]
    fn apply_arity_too_many() {
        // (f 1 2) where f : (Fn [Int] -> Bool) → ArityMismatch {expected: 1, found: 2}
        let f_ty = Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Bool),
            effects: EffectRow::empty(),
        };
        let env = Env::new().extend("f", Scheme::mono(f_ty));
        let mut state = InferState::new();
        let node = app_node(sym_node("f"), vec![int_node(1), int_node(2)]);
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(
                err.kind,
                TypeErrorKind::ArityMismatch {
                    expected: 1,
                    found: 2
                }
            ),
            "expected ArityMismatch(1,2), got {err:?}"
        );
    }

    // -- Test 4 (apply) --
    #[test]
    fn apply_arg_type_mismatch() {
        // (f true) where f : (Fn [Int] -> Bool) → Mismatch {expected: Int, found: Bool}
        let f_ty = Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Bool),
            effects: EffectRow::empty(),
        };
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
        let f_ty = Type::Fn {
            params: vec![Type::Int, Type::Str],
            ret: Box::new(Type::Float),
            effects: EffectRow::empty(),
        };
        let env = Env::new().extend("f", Scheme::mono(f_ty));
        let mut state = InferState::new();
        let node = app_node(
            sym_node("f"),
            vec![int_node(42), atom_node(Atom::Str("hello".into()))],
        );
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Float);
    }

    // -- Test 2 (apply) --
    #[test]
    fn apply_one_arg_fn() {
        // (f 42) where f : (Fn [Int] -> Bool) → Bool
        let f_ty = Type::Fn {
            params: vec![Type::Int],
            ret: Box::new(Type::Bool),
            effects: EffectRow::empty(),
        };
        let env = Env::new().extend("f", Scheme::mono(f_ty));
        let mut state = InferState::new();
        let node = app_node(sym_node("f"), vec![int_node(42)]);
        assert_eq!(synth(&node, &env, &mut state).unwrap(), Type::Bool);
    }

    // -- Test 1 (apply) --
    #[test]
    fn apply_zero_arg_fn() {
        // (f) where f : (Fn [] -> Int) → Int
        let f_ty = Type::Fn {
            params: vec![],
            ret: Box::new(Type::Int),
            effects: EffectRow::empty(),
        };
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

    // -----------------------------------------------------------------------
    // Error message / span propagation tests (Principle 6)
    // -----------------------------------------------------------------------

    // -- Test 6 (span) --
    #[test]
    fn synth_inner_span_preserved_over_outer() {
        // When an error originates inside a nested expression (the condition of an `if`),
        // the error's span should be the inner node's span, not the outer `if` form's span.
        let (env, mut state) = empty();
        let inner_span = Span::new(FileId(0), 4, 1); // "x" at offset 4
        let outer_span = Span::new(FileId(0), 0, 10); // "(if x 1 0)" at offset 0
        let cond = Node::new(
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: "x".to_string(),
            }),
            inner_span,
        );
        let if_form = Node::new(
            NodeKind::List(vec![sym_node("if"), cond, int_node(1), int_node(0)]),
            outer_span,
        );
        let err = synth(&if_form, &env, &mut state).unwrap_err();
        // Error originates at the unbound variable "x" — inner_span must win.
        assert_eq!(err.span, Some(inner_span), "inner span must be preserved");
        assert_ne!(
            err.span,
            Some(outer_span),
            "outer span must not overwrite inner"
        );
    }

    // -- Test 5 (span) --
    #[test]
    fn check_attaches_real_span_to_mismatch() {
        // check() calls synth() then unify(). The unify() mismatch error has no span;
        // check() should attach the node's span to it.
        let (env, mut state) = empty();
        let real_span = Span::new(FileId(0), 5, 2);
        let node = Node::new(
            NodeKind::Atom(Atom::Int {
                value: 42,
                suffix: None,
            }),
            real_span,
        );
        let err = check(&node, &Type::Bool, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch, got {err:?}"
        );
        assert_eq!(
            err.span,
            Some(real_span),
            "check should attach the node's span"
        );
    }

    // -- Test 4 (span) --
    #[test]
    fn synth_attaches_real_span_to_unbound_var() {
        // When synth encounters an unbound variable in a node with a real span,
        // the UnboundVariable error should carry that span.
        let (env, mut state) = empty();
        let real_span = Span::new(FileId(0), 10, 3);
        let node = Node::new(
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: "z".to_string(),
            }),
            real_span,
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::UnboundVariable { ref name } if name == "z"),
            "expected UnboundVariable(z), got {err:?}"
        );
        assert_eq!(
            err.span,
            Some(real_span),
            "error should carry the node's span"
        );
    }

    // -- Test 6 (adr-006) --
    #[test]
    fn non_arithmetic_mismatch_has_no_help() {
        // A type mismatch that is NOT from an arithmetic operator must not
        // accidentally receive arithmetic help text (no false positives).
        let env = Env::new().extend(
            "stringify",
            Scheme::mono(Type::Fn {
                params: vec![Type::Str],
                ret: Box::new(Type::Str),
                effects: EffectRow::empty(),
            }),
        );
        let mut state = InferState::new();
        // (stringify 42) — Int passed where Str expected, not an arithmetic op
        let node = Node::new(
            NodeKind::List(vec![sym_node("stringify"), int_node(42)]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            err.help.is_none(),
            "non-arithmetic mismatch must not have arithmetic help, got: {:?}",
            err.help
        );
    }

    // -- Test 5 (adr-006) --
    #[test]
    fn arithmetic_minus_int_float_has_help() {
        // ADR-006 applies to all four arithmetic operators, not just +.
        let env = Env::new().extend(
            "-",
            Scheme::mono(Type::Fn {
                params: vec![Type::Int, Type::Int],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }),
        );
        let mut state = InferState::new();
        // (- 1 1.0) — mismatch: - expects Int but gets Float
        let node = Node::new(
            NodeKind::List(vec![sym_node("-"), int_node(1), float_node(1.0)]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            err.help.is_some(),
            "expected help for (- int float), got {err:?}"
        );
        let help = err.help.as_deref().unwrap();
        assert!(
            help.contains("subtract"),
            "help for '-' should say 'subtract', got: '{help}'"
        );
    }

    // -- Test 4 (adr-006) --
    #[test]
    fn arithmetic_help_text_appears_in_display() {
        // The help text set by arithmetic_help() must appear in err.to_string().
        let env = Env::new().extend(
            "+",
            Scheme::mono(Type::Fn {
                params: vec![Type::Int, Type::Int],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }),
        );
        let mut state = InferState::new();
        let node = Node::new(
            NodeKind::List(vec![sym_node("+"), int_node(1), float_node(1.0)]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("help:"),
            "expected 'help:' in display, got: '{msg}'"
        );
        assert!(
            msg.contains("->float") || msg.contains("convert"),
            "expected conversion hint in display, got: '{msg}'"
        );
    }

    // -- Test 3 (adr-006) --
    #[test]
    fn arithmetic_plus_int_float_has_help() {
        // ADR-006: calling `+` with Int and Float args must produce an error
        // whose `help` field is set and mentions the conversion function.
        let env = Env::new().extend(
            "+",
            Scheme::mono(Type::Fn {
                params: vec![Type::Int, Type::Int],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }),
        );
        let mut state = InferState::new();
        // (+ 1 1.0) — second arg is Float, + expects Int
        let node = Node::new(
            NodeKind::List(vec![sym_node("+"), int_node(1), float_node(1.0)]),
            syn_span(),
        );
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            err.help.is_some(),
            "expected help text for arithmetic type mismatch, got {err:?}"
        );
        let help = err.help.as_deref().unwrap();
        assert!(
            help.contains("Int") && help.contains("Float"),
            "help should mention Int and Float, got: '{help}'"
        );
        assert!(
            help.contains("->float") || help.contains("convert"),
            "help should mention conversion, got: '{help}'"
        );
    }

    // -----------------------------------------------------------------------
    // Multi-error collection tests (Principle 6: don't stop at first)
    // -----------------------------------------------------------------------

    // -- Test 4 (multi-error) --
    #[test]
    fn let_collects_binding_init_errors() {
        // (let [x unbound-a y unbound-b] 42): both init expressions fail.
        // Both errors should be collected; body is still checked and Ok(Int) returned.
        let (env, mut state) = empty();
        let bvec = Node::new(
            NodeKind::Vector(vec![
                sym_node("x"),
                sym_node("unbound_a"),
                sym_node("y"),
                sym_node("unbound_b"),
            ]),
            syn_span(),
        );
        let node = Node::new(
            NodeKind::List(vec![sym_node("let"), bvec, int_node(42)]),
            syn_span(),
        );
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int, "body type should be Int");
        assert_eq!(
            state.errors.len(),
            2,
            "both binding-init errors must be collected"
        );
    }

    // -- Test 3 (multi-error) --
    #[test]
    fn application_collects_arg_errors() {
        // (f unbound-a unbound-b) with f: (Fn [Bool Bool] -> Int).
        // Both argument syntheses fail; both errors should be collected rather
        // than stopping at the first bad argument.
        let env = Env::new().extend(
            "f",
            Scheme::mono(Type::Fn {
                params: vec![Type::Bool, Type::Bool],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }),
        );
        let mut state = InferState::new();
        let node = Node::new(
            NodeKind::List(vec![
                sym_node("f"),
                sym_node("unbound_a"),
                sym_node("unbound_b"),
            ]),
            syn_span(),
        );
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int, "return type should still be resolved");
        assert_eq!(state.errors.len(), 2, "both arg errors must be collected");
    }

    // -- Test 2 (multi-error) --
    #[test]
    fn do_successful_leaves_errors_empty() {
        // A fully successful (do 1 2 3) must not leave anything in state.errors.
        let (env, mut state) = empty();
        let node = do_node(vec![int_node(1), int_node(2), int_node(3)]);
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int);
        assert!(
            state.errors.is_empty(),
            "no errors expected for successful do"
        );
    }

    // -- Test 1 (multi-error) --
    #[test]
    fn do_collects_multiple_errors() {
        // (do unbound-x unbound-y 42): two failing sub-expressions followed by a
        // successful one.  synth should return Ok(Int) and state.errors should
        // hold both unbound-variable errors rather than stopping at the first.
        let (env, mut state) = empty();
        let node = do_node(vec![
            sym_node("unbound_x"),
            sym_node("unbound_y"),
            int_node(42),
        ]);
        let ty = synth(&node, &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int, "last expr type should be Int");
        assert_eq!(
            state.errors.len(),
            2,
            "both unbound-var errors must be collected"
        );
    }
}

// ---------------------------------------------------------------------------
// Integration tests (nexl-reader → AST → inference)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod integration_tests {
    use nexl_ast::FileId;
    use nexl_reader::read;
    use nexl_types::{EffectRow, Scheme, Type};

    use super::{InferState, infer_defn, parse_deftype, register_deftype, synth};
    use crate::Env;

    /// Parse `src` and return the first top-level node, panicking on failure.
    fn parse_one(src: &str) -> nexl_ast::Node {
        let nodes = read(src, FileId(0)).expect("parse failed");
        assert_eq!(nodes.len(), 1, "expected exactly one top-level form");
        nodes.into_iter().next().unwrap()
    }

    /// Build an environment containing the four standard Int arithmetic operators.
    fn arith_env() -> Env {
        let int_binop = Type::Fn {
            params: vec![Type::Int, Type::Int],
            ret: Box::new(Type::Int),
            effects: EffectRow::empty(),
        };
        Env::new()
            .extend("+", Scheme::mono(int_binop.clone()))
            .extend("-", Scheme::mono(int_binop.clone()))
            .extend("*", Scheme::mono(int_binop.clone()))
            .extend("/", Scheme::mono(int_binop))
    }

    // -- Test 1 (integration) --
    #[test]
    fn integration_infer_add_type() {
        // Parse the milestone's own example: (defn add [x y] (+ x y))
        // in an env that has + : (Fn [Int Int] -> Int).
        // Expected result: (Fn [Int Int] -> Int).
        let node = parse_one("(defn add [x y] (+ x y))");
        let env = arith_env();
        let mut state = InferState::new();
        let (_name, ty, _new_env) = infer_defn(&node, &env, &mut state).unwrap();
        assert_eq!(
            ty,
            Type::Fn {
                params: vec![Type::Int, Type::Int],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            },
            "add should have type (Fn [Int Int] -> Int)"
        );
    }

    // -- Test 2 (integration) --
    #[test]
    fn integration_type_error_add_str() {
        // Parse the milestone's own example: (add 1 "hello").
        // In an env where add : (Fn [Int Int] -> Int), passing "hello" (Str)
        // for the second Int parameter must produce a Mismatch type error.
        use nexl_types::TypeErrorKind;
        let node = parse_one(r#"(add 1 "hello")"#);
        let env = Env::new().extend(
            "add",
            Scheme::mono(Type::Fn {
                params: vec![Type::Int, Type::Int],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            }),
        );
        let mut state = InferState::new();
        let err = synth(&node, &env, &mut state).unwrap_err();
        assert!(
            matches!(err.kind, TypeErrorKind::Mismatch { .. }),
            "expected Mismatch (Int vs Str), got {err:?}"
        );
    }

    // -- Test 3 (integration) --
    #[test]
    fn integration_infer_fibonacci_type() {
        // Verify that a fibonacci-shaped function infers as (Fn [Int] -> Int).
        //
        // Uses the form: (defn fib [n] (if (= n 0) 0 1))
        // where `=` is supplied as (Fn [Int Int] -> Bool).  The `n` parameter
        // is unified to Int through the `=` call; both branches return Int,
        // so the overall result type is (Fn [Int] -> Int).
        //
        // Note: self-recursive defn (where `fib` calls itself) requires the
        // function's own type in the body env, which infer_defn does not yet
        // support.  That is tracked as a separate todo item.
        let node = parse_one("(defn fib [n] (if (= n 0) 0 1))");
        let eq_ty = Type::Fn {
            params: vec![Type::Int, Type::Int],
            ret: Box::new(Type::Bool),
            effects: EffectRow::empty(),
        };
        let env = Env::new().extend("=", Scheme::mono(eq_ty));
        let mut state = InferState::new();
        let (_name, ty, _new_env) = infer_defn(&node, &env, &mut state).unwrap();
        assert_eq!(
            ty,
            Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Int),
                effects: EffectRow::empty(),
            },
            "fib should have type (Fn [Int] -> Int)"
        );
    }

    // -- Test 4 (integration) --
    #[test]
    fn integration_deftype_match_end_to_end() {
        let nodes = read(
            "(deftype Option [a] | None | (Some a)) (match (Some 1) (Some x) x None 0)",
            FileId(0),
        )
        .expect("parse failed");
        assert_eq!(nodes.len(), 2, "expected two top-level forms");
        let decl = parse_deftype(&nodes[0]).unwrap();
        let env = register_deftype(&Env::new(), decl);
        let mut state = InferState::new();
        let ty = synth(&nodes[1], &env, &mut state).unwrap();
        assert_eq!(ty, Type::Int);
    }
}
