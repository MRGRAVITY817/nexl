//! ANF lowering pass: reader AST ([`meta::Node`]) → [`Module`].
//!
//! Walks top-level `defn` forms, lambda-lifts anonymous `fn`s into their own
//! [`FuncDef`]s with explicit capture parameters, and normalises expressions
//! into ANF (all intermediate results named via [`LetBind`]s).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use meta::{Atom as AstAtom, Node, NodeKind, Pattern, parse_pattern};

use crate::{Atom, Block, FuncDef, FuncId, LetBind, MatchArm, Module, Rhs, Tail, VarGen, VarId};

// ── Public error type ────────────────────────────────────────────────────────

/// Errors produced by the ANF lowering pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    /// A top-level form other than `defn` (or `def`) was encountered.
    UnsupportedTopLevel,
    /// `defn` form is malformed (wrong arity, missing params vector, …).
    MalformedDefn,
    /// `fn` form is malformed.
    MalformedFn,
    /// `let` form is malformed (odd binding vector, missing body, …).
    MalformedLet,
    /// `if` form is malformed (not exactly 3 arguments).
    MalformedIf,
    /// `match` form is malformed (odd number of arm slots, …).
    MalformedMatch,
    /// A function body (or let body) has no expressions.
    EmptyBody,
    /// A symbol was referenced that has no binding in scope.
    UnboundVariable(String),
    /// `loop` form is malformed (missing binding vector, odd bindings, …).
    MalformedLoop,
    /// An expression form not yet supported by this lowering pass.
    UnsupportedExpr,
    /// A pattern form not yet supported (non-constructor, non-variable, …).
    UnsupportedPattern,
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::UnsupportedTopLevel => write!(f, "unsupported top-level form"),
            LowerError::MalformedDefn => write!(f, "malformed defn"),
            LowerError::MalformedFn => write!(f, "malformed fn"),
            LowerError::MalformedLet => write!(f, "malformed let"),
            LowerError::MalformedIf => write!(f, "malformed if"),
            LowerError::MalformedMatch => write!(f, "malformed match"),
            LowerError::EmptyBody => write!(f, "empty body"),
            LowerError::UnboundVariable(n) => write!(f, "unbound variable: {n}"),
            LowerError::MalformedLoop => write!(f, "malformed loop"),
            LowerError::UnsupportedExpr => write!(f, "unsupported expression"),
            LowerError::UnsupportedPattern => write!(f, "unsupported pattern"),
        }
    }
}

impl std::error::Error for LowerError {}

// ── Lowerer ──────────────────────────────────────────────────────────────────

/// Stateful ANF lowerer.
///
/// Call [`Lowerer::lower_module`] to process a list of top-level nodes and
/// produce a [`Module`].
pub struct Lowerer {
    module_name: String,
    /// Function definitions keyed by `FuncId.0`.
    ///
    /// A `BTreeMap` guarantees that [`Module::funcs`] is ordered by `FuncId`,
    /// so `module.funcs[i].id == FuncId(i)` — matching WASM function indices.
    funcs: BTreeMap<u32, FuncDef>,
    /// Counter for assigning [`FuncId`]s.
    func_counter: u32,
    /// Counter for assigning [`VarId`]s (shared across the whole module).
    var_gen: VarGen,
    /// Top-level function names → pre-assigned [`FuncId`]s.
    ///
    /// Populated in a pre-pass so that cross-function references can be
    /// resolved to [`Atom::FuncRef`] during the main lowering pass.
    global_funcs: HashMap<String, FuncId>,
}

impl Lowerer {
    /// Create a new lowerer for a module with the given name.
    pub fn new(module_name: &str) -> Self {
        Lowerer {
            module_name: module_name.to_string(),
            funcs: BTreeMap::new(),
            func_counter: 0,
            var_gen: VarGen::new(),
            global_funcs: HashMap::new(),
        }
    }

    fn fresh_func_id(&mut self) -> FuncId {
        let id = FuncId(self.func_counter);
        self.func_counter += 1;
        id
    }

    /// Lower a list of top-level nodes into a [`Module`].
    ///
    /// Runs a pre-pass to assign [`FuncId`]s to all named `defn` forms so
    /// cross-function references resolve to [`Atom::FuncRef`] during the main
    /// lowering pass.
    pub fn lower_module(mut self, nodes: &[Node]) -> Result<Module, LowerError> {
        // Pre-pass: register top-level defn names → FuncIds.
        for node in nodes {
            if let NodeKind::List(items) = &node.kind
                && items.len() >= 4
                && let NodeKind::Atom(AstAtom::Symbol { ns: None, name }) = &items[0].kind
                && name == "defn"
                && let NodeKind::Atom(AstAtom::Symbol { name: fn_name, .. }) = &items[1].kind
            {
                let fid = self.fresh_func_id();
                self.global_funcs.insert(fn_name.clone(), fid);
            }
        }

        // Main pass: lower each form.
        for node in nodes {
            self.lower_top_level(node)?;
        }

        // Collect in FuncId order so module.funcs[i].id == FuncId(i).
        let funcs: Vec<FuncDef> = self.funcs.into_values().collect();
        Ok(Module { name: self.module_name, funcs })
    }

    fn lower_top_level(&mut self, node: &Node) -> Result<(), LowerError> {
        match &node.kind {
            NodeKind::List(items) if !items.is_empty() => {
                match &items[0].kind {
                    NodeKind::Atom(AstAtom::Symbol { ns: None, name }) if name == "defn" => {
                        let func = self.lower_defn(items)?;
                        self.funcs.insert(func.id.0, func);
                        Ok(())
                    }
                    _ => Err(LowerError::UnsupportedTopLevel),
                }
            }
            _ => Err(LowerError::UnsupportedTopLevel),
        }
    }

    fn lower_defn(&mut self, items: &[Node]) -> Result<FuncDef, LowerError> {
        // (defn name [params...] body-expr...)
        if items.len() < 4 {
            return Err(LowerError::MalformedDefn);
        }

        let name = match &items[1].kind {
            NodeKind::Atom(AstAtom::Symbol { name, .. }) => name.clone(),
            _ => return Err(LowerError::MalformedDefn),
        };

        let params_vec = match &items[2].kind {
            NodeKind::Vector(v) => v,
            _ => return Err(LowerError::MalformedDefn),
        };

        // Look up the FuncId assigned during the pre-pass.
        let func_id = *self.global_funcs.get(&name).ok_or(LowerError::MalformedDefn)?;

        let mut env: HashMap<String, VarId> = HashMap::new();
        let mut param_ids = vec![];
        for param in params_vec {
            match &param.kind {
                NodeKind::Atom(AstAtom::Symbol { name, .. }) => {
                    let var = self.var_gen.fresh();
                    env.insert(name.clone(), var);
                    param_ids.push(var);
                }
                _ => return Err(LowerError::MalformedDefn),
            }
        }

        let body = self.lower_body(&items[3..], &env)?;

        Ok(FuncDef {
            id: func_id,
            name: Some(name),
            params: param_ids,
            body,
        })
    }

    /// Lower a sequence of body expressions.  All but the last are lowered for
    /// side effects; the last is lowered in tail position.
    fn lower_body(
        &mut self,
        nodes: &[Node],
        env: &HashMap<String, VarId>,
    ) -> Result<Block, LowerError> {
        if nodes.is_empty() {
            return Err(LowerError::EmptyBody);
        }

        let mut all_binds: Vec<LetBind> = vec![];

        // Side-effect expressions (all but last).
        for node in &nodes[..nodes.len() - 1] {
            let (binds, _atom) = self.lower_expr(node, env)?;
            all_binds.extend(binds);
        }

        // Last expression in tail position.
        let (tail_binds, tail) = self.lower_tail(nodes.last().expect("non-empty"), env)?;
        all_binds.extend(tail_binds);

        Ok(Block {
            binds: all_binds,
            tail: Box::new(tail),
        })
    }

    // ── Tail-position lowering ───────────────────────────────────────────────

    /// Lower a node in tail position.
    ///
    /// Returns `(extra_binds_to_prepend, tail_expr)`.  The caller should
    /// extend its own bind-list with `extra_binds` before appending the tail.
    fn lower_tail(
        &mut self,
        node: &Node,
        env: &HashMap<String, VarId>,
    ) -> Result<(Vec<LetBind>, Tail), LowerError> {
        match &node.kind {
            NodeKind::List(items) if !items.is_empty() => {
                if let NodeKind::Atom(AstAtom::Symbol { ns: None, name }) = &items[0].kind {
                    match name.as_str() {
                        "if" => return self.lower_if_tail(items, env),
                        "let" => return self.lower_let_tail(items, env),
                        "match" => return self.lower_match_tail(items, env),
                        "loop" => return self.lower_loop_tail(items, env),
                        "recur" => return self.lower_recur_tail(items, env),
                        "do" => {
                            let block = self.lower_body(&items[1..], env)?;
                            return Ok((block.binds, *block.tail));
                        }
                        "fn" => {
                            let (binds, atom) = self.lower_fn_expr(items, env)?;
                            return Ok((binds, Tail::Return(atom)));
                        }
                        _ => {
                            // Uppercase head → constructor application.
                            if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                                let (binds, atom) =
                                    self.lower_ctor_expr(name, &items[1..], env)?;
                                return Ok((binds, Tail::Return(atom)));
                            }
                        }
                    }
                }
                // Generic function call in tail position.
                // Lower func and args to atoms.  If the function resolves to a
                // known FuncRef (direct call), emit Tail::TailCall for TCO.
                // Variable calls fall back to Rhs::Call + Tail::Return since
                // indirect tail calls through closures are not yet supported.
                let mut all_binds: Vec<LetBind> = vec![];
                let (f_binds, f_atom) = self.lower_expr(&items[0], env)?;
                all_binds.extend(f_binds);
                let mut args: Vec<Atom> = vec![];
                for arg in &items[1..] {
                    let (arg_binds, arg_atom) = self.lower_expr(arg, env)?;
                    all_binds.extend(arg_binds);
                    args.push(arg_atom);
                }
                if matches!(f_atom, Atom::FuncRef(_)) {
                    Ok((all_binds, Tail::TailCall { func: f_atom, args }))
                } else {
                    let result_var = self.var_gen.fresh();
                    all_binds.push(LetBind { var: result_var, rhs: Rhs::Call { func: f_atom, args } });
                    Ok((all_binds, Tail::Return(Atom::Var(result_var))))
                }
            }
            _ => {
                let (binds, atom) = self.lower_expr(node, env)?;
                Ok((binds, Tail::Return(atom)))
            }
        }
    }

    fn lower_if_tail(
        &mut self,
        items: &[Node],
        env: &HashMap<String, VarId>,
    ) -> Result<(Vec<LetBind>, Tail), LowerError> {
        // (if cond then else)
        if items.len() != 4 {
            return Err(LowerError::MalformedIf);
        }

        let (cond_binds, cond_atom) = self.lower_expr(&items[1], env)?;

        let (then_binds, then_tail) = self.lower_tail(&items[2], env)?;
        let then_block = Block {
            binds: then_binds,
            tail: Box::new(then_tail),
        };

        let (else_binds, else_tail) = self.lower_tail(&items[3], env)?;
        let else_block = Block {
            binds: else_binds,
            tail: Box::new(else_tail),
        };

        Ok((cond_binds, Tail::If { cond: cond_atom, then_block, else_block }))
    }

    fn lower_let_tail(
        &mut self,
        items: &[Node],
        env: &HashMap<String, VarId>,
    ) -> Result<(Vec<LetBind>, Tail), LowerError> {
        // (let [x rhs-x  y rhs-y  ...] body...)
        if items.len() < 3 {
            return Err(LowerError::MalformedLet);
        }

        let bindings = match &items[1].kind {
            NodeKind::Vector(v) => v,
            _ => return Err(LowerError::MalformedLet),
        };

        if bindings.len() % 2 != 0 {
            return Err(LowerError::MalformedLet);
        }

        let mut all_binds: Vec<LetBind> = vec![];
        let mut inner_env = env.clone();

        for pair in bindings.chunks(2) {
            let bname = match &pair[0].kind {
                NodeKind::Atom(AstAtom::Symbol { name, .. }) => name.clone(),
                _ => return Err(LowerError::MalformedLet),
            };

            let (expr_binds, atom) = self.lower_expr(&pair[1], &inner_env)?;
            all_binds.extend(expr_binds);

            let var = self.var_gen.fresh();
            all_binds.push(LetBind { var, rhs: Rhs::Atom(atom) });
            inner_env.insert(bname, var);
        }

        let body_block = self.lower_body(&items[2..], &inner_env)?;
        all_binds.extend(body_block.binds);

        Ok((all_binds, *body_block.tail))
    }

    fn lower_match_tail(
        &mut self,
        items: &[Node],
        env: &HashMap<String, VarId>,
    ) -> Result<(Vec<LetBind>, Tail), LowerError> {
        // (match scrutinee  pattern body  pattern body  ...)
        // items[0] = "match", items[1] = scrutinee, items[2..] = pairs
        if items.len() < 4 || !(items.len() - 2).is_multiple_of(2) {
            return Err(LowerError::MalformedMatch);
        }

        let (scrut_binds, scrut_atom) = self.lower_expr(&items[1], env)?;

        let mut arms: Vec<MatchArm> = vec![];
        let mut i = 2;
        while i + 1 < items.len() {
            let pat = parse_pattern(&items[i]).map_err(|_| LowerError::MalformedMatch)?;
            let body_node = &items[i + 1];
            i += 2;

            let mut arm_env = env.clone();
            let mut pre_binds: Vec<LetBind> = vec![];

            let (ctor, field_ids) =
                extract_ctor_binds(&pat, &mut self.var_gen, &mut arm_env, &mut pre_binds)?;

            let (body_extra, body_tail) = self.lower_tail(body_node, &arm_env)?;
            pre_binds.extend(body_extra);

            arms.push(MatchArm {
                ctor,
                binds: field_ids,
                body: Block {
                    binds: pre_binds,
                    tail: Box::new(body_tail),
                },
            });
        }

        Ok((scrut_binds, Tail::Match { scrutinee: scrut_atom, arms }))
    }

    fn lower_loop_tail(
        &mut self,
        items: &[Node],
        env: &HashMap<String, VarId>,
    ) -> Result<(Vec<LetBind>, Tail), LowerError> {
        // (loop [var init, var init, ...] body...)
        if items.len() < 3 {
            return Err(LowerError::MalformedLoop);
        }
        let bindings = match &items[1].kind {
            NodeKind::Vector(v) => v,
            _ => return Err(LowerError::MalformedLoop),
        };
        if bindings.len() % 2 != 0 {
            return Err(LowerError::MalformedLoop);
        }

        let mut all_binds: Vec<LetBind> = vec![];
        let mut loop_vars: Vec<(VarId, Atom)> = vec![];
        let mut current_env = env.clone();

        // Bindings are sequential: each init is evaluated with previous loop vars in scope.
        for pair in bindings.chunks(2) {
            let var_name = match &pair[0].kind {
                NodeKind::Atom(AstAtom::Symbol { name, .. }) => name.clone(),
                _ => return Err(LowerError::MalformedLoop),
            };
            let (init_binds, init_atom) = self.lower_expr(&pair[1], &current_env)?;
            all_binds.extend(init_binds);
            let var_id = self.var_gen.fresh();
            current_env.insert(var_name, var_id);
            loop_vars.push((var_id, init_atom));
        }

        let body = self.lower_body(&items[2..], &current_env)?;
        Ok((all_binds, Tail::Loop { vars: loop_vars, body: Box::new(body) }))
    }

    fn lower_recur_tail(
        &mut self,
        items: &[Node],
        env: &HashMap<String, VarId>,
    ) -> Result<(Vec<LetBind>, Tail), LowerError> {
        // (recur arg1 arg2 ...)
        let mut all_binds: Vec<LetBind> = vec![];
        let mut args: Vec<Atom> = vec![];
        for arg in &items[1..] {
            let (binds, atom) = self.lower_expr(arg, env)?;
            all_binds.extend(binds);
            args.push(atom);
        }
        Ok((all_binds, Tail::Recur { args }))
    }

    // ── Expression-position lowering ─────────────────────────────────────────

    /// Lower a node in expression position.
    ///
    /// Returns `(let_binds_to_prepend, result_atom)`.
    fn lower_expr(
        &mut self,
        node: &Node,
        env: &HashMap<String, VarId>,
    ) -> Result<(Vec<LetBind>, Atom), LowerError> {
        match &node.kind {
            // Literals → atoms; no bindings needed.
            NodeKind::Atom(AstAtom::Int { value, .. }) => Ok((vec![], Atom::Int(*value as i64))),
            NodeKind::Atom(AstAtom::Float { value, .. }) => Ok((vec![], Atom::Float(*value))),
            NodeKind::Atom(AstAtom::Bool(b)) => Ok((vec![], Atom::Bool(*b))),
            NodeKind::Atom(AstAtom::Unit) => Ok((vec![], Atom::Unit)),
            NodeKind::Atom(AstAtom::Str(s)) => Ok((vec![], Atom::Str(Rc::from(s.as_str())))),

            // Variable reference — local first, then global function, then nullary ctor.
            NodeKind::Atom(AstAtom::Symbol { ns: None, name }) => {
                if let Some(&var) = env.get(name) {
                    Ok((vec![], Atom::Var(var)))
                } else if let Some(&fid) = self.global_funcs.get(name) {
                    Ok((vec![], Atom::FuncRef(fid)))
                } else if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                    // Nullary constructor (e.g. `None`, `True`).
                    self.lower_ctor_expr(name, &[], env)
                } else {
                    Err(LowerError::UnboundVariable(name.clone()))
                }
            }

            // Complex forms.
            NodeKind::List(items) if !items.is_empty() => {
                if let NodeKind::Atom(AstAtom::Symbol { ns: None, name }) = &items[0].kind {
                    // Uppercase head → constructor application.
                    if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                        return self.lower_ctor_expr(name, &items[1..], env);
                    }
                    match name.as_str() {
                        "fn" => return self.lower_fn_expr(items, env),
                        "let" => return self.lower_let_expr(items, env),
                        _ => {}
                    }
                }
                self.lower_call(items, env)
            }

            _ => Err(LowerError::UnsupportedExpr),
        }
    }

    fn lower_call(
        &mut self,
        items: &[Node],
        env: &HashMap<String, VarId>,
    ) -> Result<(Vec<LetBind>, Atom), LowerError> {
        let mut all_binds: Vec<LetBind> = vec![];

        let (f_binds, f_atom) = self.lower_expr(&items[0], env)?;
        all_binds.extend(f_binds);

        let mut args: Vec<Atom> = vec![];
        for arg in &items[1..] {
            let (arg_binds, arg_atom) = self.lower_expr(arg, env)?;
            all_binds.extend(arg_binds);
            args.push(arg_atom);
        }

        let result_var = self.var_gen.fresh();
        all_binds.push(LetBind {
            var: result_var,
            rhs: Rhs::Call { func: f_atom, args },
        });

        Ok((all_binds, Atom::Var(result_var)))
    }

    fn lower_fn_expr(
        &mut self,
        items: &[Node],
        outer_env: &HashMap<String, VarId>,
    ) -> Result<(Vec<LetBind>, Atom), LowerError> {
        // (fn [params...] body...)
        if items.len() < 3 {
            return Err(LowerError::MalformedFn);
        }

        let params_vec = match &items[1].kind {
            NodeKind::Vector(v) => v,
            _ => return Err(LowerError::MalformedFn),
        };

        let func_id = self.fresh_func_id();

        let mut fn_env: HashMap<String, VarId> = HashMap::new();
        let mut param_ids: Vec<VarId> = vec![];

        for param in params_vec {
            match &param.kind {
                NodeKind::Atom(AstAtom::Symbol { name, .. }) => {
                    let var = self.var_gen.fresh();
                    fn_env.insert(name.clone(), var);
                    param_ids.push(var);
                }
                _ => return Err(LowerError::MalformedFn),
            }
        }

        let body_nodes = &items[2..];
        let free_vars = collect_free_vars(body_nodes, &fn_env, outer_env);

        // Capture parameters: prepended before regular params.
        let mut captures: Vec<(VarId, Atom)> = vec![];
        for (vname, outer_var) in &free_vars {
            let capture_param = self.var_gen.fresh();
            fn_env.insert(vname.clone(), capture_param);
            param_ids.insert(captures.len(), capture_param);
            captures.push((capture_param, Atom::Var(*outer_var)));
        }

        let body = self.lower_body(body_nodes, &fn_env)?;
        self.funcs.insert(func_id.0, FuncDef {
            id: func_id,
            name: None,
            params: param_ids,
            body,
        });

        let closure_var = self.var_gen.fresh();
        Ok((
            vec![LetBind {
                var: closure_var,
                rhs: Rhs::MakeClosure { func_id, captures },
            }],
            Atom::Var(closure_var),
        ))
    }

    /// Lower a constructor application `Ctor(args...)` in expression position.
    ///
    /// Returns `(binds, Atom::Var(result))` where `result` is bound to
    /// `Rhs::MakeTuple { ctor, fields }`.
    fn lower_ctor_expr(
        &mut self,
        ctor: &str,
        arg_nodes: &[Node],
        env: &HashMap<String, VarId>,
    ) -> Result<(Vec<LetBind>, Atom), LowerError> {
        let mut all_binds: Vec<LetBind> = vec![];
        let mut fields: Vec<Atom> = vec![];
        for arg in arg_nodes {
            let (binds, atom) = self.lower_expr(arg, env)?;
            all_binds.extend(binds);
            fields.push(atom);
        }
        let result_var = self.var_gen.fresh();
        all_binds.push(LetBind {
            var: result_var,
            rhs: Rhs::MakeTuple { ctor: ctor.to_string(), fields },
        });
        Ok((all_binds, Atom::Var(result_var)))
    }

    fn lower_let_expr(
        &mut self,
        items: &[Node],
        env: &HashMap<String, VarId>,
    ) -> Result<(Vec<LetBind>, Atom), LowerError> {
        // (let [x rhs-x ...] body...)  in expression position: return last expr as atom.
        if items.len() < 3 {
            return Err(LowerError::MalformedLet);
        }

        let bindings = match &items[1].kind {
            NodeKind::Vector(v) => v,
            _ => return Err(LowerError::MalformedLet),
        };

        if bindings.len() % 2 != 0 {
            return Err(LowerError::MalformedLet);
        }

        let mut all_binds: Vec<LetBind> = vec![];
        let mut inner_env = env.clone();

        for pair in bindings.chunks(2) {
            let bname = match &pair[0].kind {
                NodeKind::Atom(AstAtom::Symbol { name, .. }) => name.clone(),
                _ => return Err(LowerError::MalformedLet),
            };

            let (expr_binds, atom) = self.lower_expr(&pair[1], &inner_env)?;
            all_binds.extend(expr_binds);

            let var = self.var_gen.fresh();
            all_binds.push(LetBind { var, rhs: Rhs::Atom(atom) });
            inner_env.insert(bname, var);
        }

        let body_nodes = &items[2..];
        if body_nodes.is_empty() {
            return Err(LowerError::EmptyBody);
        }

        for node in &body_nodes[..body_nodes.len() - 1] {
            let (binds, _) = self.lower_expr(node, &inner_env)?;
            all_binds.extend(binds);
        }

        let (last_binds, last_atom) =
            self.lower_expr(body_nodes.last().expect("non-empty"), &inner_env)?;
        all_binds.extend(last_binds);

        Ok((all_binds, last_atom))
    }
}

// ── Free-variable collection ─────────────────────────────────────────────────

/// Collect variables that are referenced in `nodes`, not bound in `bound`,
/// but present in `outer`.  Used for lambda-lifting.
fn collect_free_vars(
    nodes: &[Node],
    bound: &HashMap<String, VarId>,
    outer: &HashMap<String, VarId>,
) -> Vec<(String, VarId)> {
    let mut free: Vec<(String, VarId)> = vec![];
    let mut seen: HashSet<String> = HashSet::new();
    for node in nodes {
        collect_free_in_node(node, bound, outer, &mut free, &mut seen);
    }
    free
}

fn collect_free_in_node(
    node: &Node,
    bound: &HashMap<String, VarId>,
    outer: &HashMap<String, VarId>,
    free: &mut Vec<(String, VarId)>,
    seen: &mut HashSet<String>,
) {
    match &node.kind {
        NodeKind::Atom(AstAtom::Symbol { ns: None, name }) => {
            if !bound.contains_key(name)
                && let Some(&var) = outer.get(name)
                && seen.insert(name.clone())
            {
                free.push((name.clone(), var));
            }
        }
        NodeKind::List(items) => {
            // Nested fn/defn have their own scope — skip them here; we'll
            // handle their free-variable sets when we lower them.
            if !items.is_empty()
                && let NodeKind::Atom(AstAtom::Symbol { ns: None, name }) = &items[0].kind
                && (name == "fn" || name == "defn")
            {
                return;
            }
            for item in items {
                collect_free_in_node(item, bound, outer, free, seen);
            }
        }
        NodeKind::Vector(items) => {
            for item in items {
                collect_free_in_node(item, bound, outer, free, seen);
            }
        }
        _ => {}
    }
}

// ── Pattern helper ───────────────────────────────────────────────────────────

/// Extract a constructor name and field [`VarId`] bindings from a [`Pattern`].
///
/// Field variables are added to `arm_env` so that the arm body can reference
/// them.  Any projections needed to bind fields are appended to `pre_binds`.
fn extract_ctor_binds(
    pat: &Pattern,
    var_gen: &mut VarGen,
    arm_env: &mut HashMap<String, VarId>,
    _pre_binds: &mut Vec<LetBind>,
) -> Result<(String, Vec<VarId>), LowerError> {
    match pat {
        Pattern::Constructor { name, args } => {
            let mut field_ids: Vec<VarId> = vec![];
            for arg in args {
                match arg {
                    Pattern::Var(vname) => {
                        let var = var_gen.fresh();
                        arm_env.insert(vname.clone(), var);
                        field_ids.push(var);
                    }
                    Pattern::Wildcard => {
                        field_ids.push(var_gen.fresh());
                    }
                    _ => return Err(LowerError::UnsupportedPattern),
                }
            }
            Ok((name.clone(), field_ids))
        }
        Pattern::Var(name) => {
            // A bare name in match position.  If it starts with an uppercase
            // letter we treat it as a nullary constructor; otherwise a wildcard
            // binding of the whole scrutinee (not yet supported).
            let first = name.chars().next().unwrap_or('_');
            if first.is_uppercase() || name == "_" {
                Ok((name.clone(), vec![]))
            } else {
                Err(LowerError::UnsupportedPattern)
            }
        }
        Pattern::Wildcard => Ok(("_".to_string(), vec![])),
        _ => Err(LowerError::UnsupportedPattern),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn lower(src: &str) -> Result<Module, LowerError> {
        let nodes =
            nexl_reader::read(src, meta::FileId::SYNTHETIC).expect("parse error in test");
        Lowerer::new("test").lower_module(&nodes)
    }

    // ─── 1. Int literal ───────────────────────────────────────────────────────
    #[test]
    fn lower_int_literal() {
        let m = lower("(defn f [] 42)").unwrap();
        assert_eq!(m.funcs.len(), 1);
        let func = &m.funcs[0];
        assert!(func.body.binds.is_empty());
        assert!(matches!(*func.body.tail, Tail::Return(Atom::Int(42))));
    }

    // ─── 2. Bool literal ─────────────────────────────────────────────────────
    #[test]
    fn lower_bool_literal() {
        let m = lower("(defn f [] true)").unwrap();
        assert!(matches!(*m.funcs[0].body.tail, Tail::Return(Atom::Bool(true))));
    }

    // ─── 3. Unit literal ─────────────────────────────────────────────────────
    #[test]
    fn lower_unit_literal() {
        let m = lower("(defn f [] unit)").unwrap();
        assert!(matches!(*m.funcs[0].body.tail, Tail::Return(Atom::Unit)));
    }

    // ─── 4. Str literal ──────────────────────────────────────────────────────
    #[test]
    fn lower_str_literal() {
        let m = lower(r#"(defn f [] "hi")"#).unwrap();
        let Tail::Return(Atom::Str(ref s)) = *m.funcs[0].body.tail else {
            panic!("expected Return(Str)")
        };
        assert_eq!(s.as_ref(), "hi");
    }

    // ─── 5. Single let binding ───────────────────────────────────────────────
    #[test]
    fn lower_let_single_binding() {
        // (let [x 1] x) — 1 bind (x=Int(1)), Return(Var(x))
        let m = lower("(defn f [] (let [x 1] x))").unwrap();
        let body = &m.funcs[0].body;
        assert_eq!(body.binds.len(), 1);
        let x_var = body.binds[0].var;
        assert!(matches!(body.binds[0].rhs, Rhs::Atom(Atom::Int(1))));
        assert!(matches!(*body.tail, Tail::Return(Atom::Var(v)) if v == x_var));
    }

    // ─── 6. Sequential let bindings ──────────────────────────────────────────
    #[test]
    fn lower_let_sequential_bindings() {
        // (let [x 1 y 2] y) — 2 binds, Return(Var(y))
        let m = lower("(defn f [] (let [x 1 y 2] y))").unwrap();
        let body = &m.funcs[0].body;
        assert_eq!(body.binds.len(), 2);
    }

    // ─── 7. Function call ────────────────────────────────────────────────────
    #[test]
    fn lower_call_expr() {
        // (defn apply-f [f x y] (f x y)) — call binds to a fresh var
        let m = lower("(defn apply-f [f x y] (f x y))").unwrap();
        let body = &m.funcs[0].body;
        // Expect 1 bind: tmp = Call(Var(f), [Var(x), Var(y)])
        assert_eq!(body.binds.len(), 1);
        assert!(matches!(body.binds[0].rhs, Rhs::Call { .. }));
        let tmp = body.binds[0].var;
        assert!(matches!(*body.tail, Tail::Return(Atom::Var(v)) if v == tmp));
    }

    // ─── 8. if in tail position (trivial cond) ───────────────────────────────
    #[test]
    fn lower_if_tail_bool_cond() {
        // (defn f [] (if true 1 2))
        let m = lower("(defn f [] (if true 1 2))").unwrap();
        let body = &m.funcs[0].body;
        assert!(body.binds.is_empty());
        let Tail::If { ref cond, .. } = *body.tail else {
            panic!("expected Tail::If")
        };
        assert!(matches!(cond, Atom::Bool(true)));
    }

    // ─── 9. if cond has its own binding ──────────────────────────────────────
    #[test]
    fn lower_if_cond_with_binding() {
        // (defn f [g x] (if (g x) 1 2))
        // Cond (g x) produces 1 let-bind; if produces no additional outer binds.
        let m = lower("(defn f [g x] (if (g x) 1 2))").unwrap();
        let body = &m.funcs[0].body;
        // The cond bind is prepended before Tail::If.
        assert_eq!(body.binds.len(), 1, "expect 1 bind for the (g x) call");
        assert!(matches!(*body.tail, Tail::If { .. }));
    }

    // ─── 10. defn with params ────────────────────────────────────────────────
    #[test]
    fn lower_defn_with_params() {
        let m = lower("(defn id [x] x)").unwrap();
        let func = &m.funcs[0];
        assert_eq!(func.params.len(), 1);
        let x_var = func.params[0];
        assert!(matches!(*func.body.tail, Tail::Return(Atom::Var(v)) if v == x_var));
    }

    // ─── 11. fn creates extra FuncDef ────────────────────────────────────────
    #[test]
    fn lower_fn_creates_extra_funcdef() {
        // (defn f [] (fn [x] x)) — outer + inner = 2 FuncDefs
        let m = lower("(defn f [] (fn [x] x))").unwrap();
        assert_eq!(m.funcs.len(), 2, "outer defn + lifted lambda = 2");
    }

    // ─── 12. fn captures free variable ───────────────────────────────────────
    #[test]
    fn lower_fn_captures_free_var() {
        // (defn f [y] (fn [x] y))
        // The lifted lambda captures `y` from outer scope.
        let m = lower("(defn f [y] (fn [x] y))").unwrap();
        assert_eq!(m.funcs.len(), 2);

        // Find the outer `f` by name (the lifted lambda has no name).
        let outer = m.funcs.iter().find(|fd| fd.name.as_deref() == Some("f"))
            .expect("defn f not found");

        // Outer body has 1 bind: closure_var = MakeClosure { captures: [...] }
        assert_eq!(outer.body.binds.len(), 1);
        let Rhs::MakeClosure { ref captures, .. } = outer.body.binds[0].rhs else {
            panic!("expected MakeClosure")
        };
        assert_eq!(captures.len(), 1, "captures y from outer scope");
    }

    // ─── 13. match with two constructor arms ────────────────────────────────
    #[test]
    fn lower_match_two_arms() {
        // (defn f [v d] (match v (Some x) x None d))
        let m = lower("(defn f [v d] (match v (Some x) x None d))").unwrap();
        let body = &m.funcs[0].body;
        assert!(body.binds.is_empty(), "scrutinee is a plain var, no binds");
        let Tail::Match { ref arms, .. } = *body.tail else {
            panic!("expected Tail::Match")
        };
        assert_eq!(arms.len(), 2);
        assert_eq!(arms[0].ctor, "Some");
        assert_eq!(arms[0].binds.len(), 1, "Some has 1 field (x)");
        assert_eq!(arms[1].ctor, "None");
        assert!(arms[1].binds.is_empty(), "None has no fields");
    }

    // ─── 14. multiple defns in one module ────────────────────────────────────
    #[test]
    fn lower_multiple_defns() {
        let m = lower("(defn a [] 1)\n(defn b [] 2)").unwrap();
        assert_eq!(m.funcs.len(), 2);
        assert_eq!(m.funcs[0].name.as_deref(), Some("a"));
        assert_eq!(m.funcs[1].name.as_deref(), Some("b"));
    }

    // ─── 15. constructor application (Some x) ────────────────────────────────
    #[test]
    fn lower_adt_constructor() {
        // (defn wrap [x] (Some x))
        // Body: 1 bind: %r = MakeTuple { ctor: "Some", fields: [Var(x)] }
        //       tail: Return(Var(%r))
        let m = lower("(defn wrap [x] (Some x))").unwrap();
        let body = &m.funcs[0].body;
        assert_eq!(body.binds.len(), 1);
        let Rhs::MakeTuple { ref ctor, ref fields } = body.binds[0].rhs else {
            panic!("expected MakeTuple, got {:?}", body.binds[0].rhs)
        };
        assert_eq!(ctor, "Some");
        assert_eq!(fields.len(), 1);
        let x_var = m.funcs[0].params[0];
        assert!(matches!(fields[0], Atom::Var(v) if v == x_var));
    }

    // ─── 17. direct tail call lowered to Tail::TailCall ─────────────────────
    #[test]
    fn lower_tail_call_direct() {
        // (defn f [x] (f x)) — self-recursive tail call
        // f is a known global FuncRef, so lower_tail should produce Tail::TailCall.
        let m = lower("(defn f [x] (f x))").unwrap();
        let body = &m.funcs[0].body;
        // No Rhs::Call bind — the tail call is in Tail::TailCall directly.
        let Tail::TailCall { ref func, ref args } = *body.tail else {
            panic!("expected Tail::TailCall, got {:?}", body.tail)
        };
        assert!(matches!(func, Atom::FuncRef(_)), "func should be a FuncRef");
        assert_eq!(args.len(), 1, "one argument");
    }

    // ─── 18. loop with single binding ────────────────────────────────────────
    #[test]
    fn lower_loop_single_var() {
        // (defn f [n] (loop [i n] i))
        // Tail should be Tail::Loop with 1 var
        let m = lower("(defn f [n] (loop [i n] i))").unwrap();
        let func = &m.funcs[0];
        let Tail::Loop { ref vars, .. } = *func.body.tail else {
            panic!("expected Tail::Loop, got {:?}", func.body.tail)
        };
        assert_eq!(vars.len(), 1, "one loop variable");
    }

    // ─── 18. loop with recur in else branch ──────────────────────────────────
    #[test]
    fn lower_loop_with_recur() {
        // (defn f [n] (loop [i n] (if true i (recur 0))))
        // Loop body tail should be Tail::If; else branch tail should be Tail::Recur
        let m = lower("(defn f [n] (loop [i n] (if true i (recur 0))))").unwrap();
        let func = &m.funcs[0];
        let Tail::Loop { ref body, .. } = *func.body.tail else {
            panic!("expected Tail::Loop, got {:?}", func.body.tail)
        };
        let Tail::If { ref else_block, .. } = *body.tail else {
            panic!("expected Tail::If inside loop body, got {:?}", body.tail)
        };
        let Tail::Recur { ref args } = *else_block.tail else {
            panic!("expected Tail::Recur in else branch, got {:?}", else_block.tail)
        };
        assert_eq!(args.len(), 1, "recur passes 1 new value");
    }

    // ─── 16. nullary constructor (None) ──────────────────────────────────────
    #[test]
    fn lower_adt_nullary() {
        // (defn nothing [] None)
        // Body: 1 bind: %r = MakeTuple { ctor: "None", fields: [] }
        //       tail: Return(Var(%r))
        let m = lower("(defn nothing [] None)").unwrap();
        let body = &m.funcs[0].body;
        assert_eq!(body.binds.len(), 1);
        let Rhs::MakeTuple { ref ctor, ref fields } = body.binds[0].rhs else {
            panic!("expected MakeTuple, got {:?}", body.binds[0].rhs)
        };
        assert_eq!(ctor, "None");
        assert!(fields.is_empty());
    }

}
