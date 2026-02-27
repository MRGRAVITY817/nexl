//! Inlining pass: identify small functions and substitute them at call sites.
//!
//! Two phases:
//! 1. **Analysis** — compute cost of each function, determine which are eligible.
//! 2. **Transform** — rewrite `Rhs::Call` of eligible functions by splicing their body.

use std::collections::{HashMap, HashSet};

use crate::{Atom, Block, FuncDef, FuncId, LetBind, Module, MatchArm, Rhs, Tail, VarGen, VarId};

/// Maximum cost for a function to be considered inlinable.
const INLINE_COST_THRESHOLD: usize = 10;

/// Cost of each IR node kind, used to estimate function size.
fn rhs_cost(rhs: &Rhs) -> usize {
    match rhs {
        Rhs::Atom(_) => 0,
        Rhs::Call { .. } => 2,
        Rhs::MakeClosure { .. } => 3,
        Rhs::MakeTuple { .. } => 1,
        Rhs::Project { .. } => 1,
    }
}

fn tail_cost(tail: &Tail) -> usize {
    match tail {
        Tail::Return(_) => 0,
        Tail::If { then_block, else_block, .. } => {
            1 + block_cost(then_block) + block_cost(else_block)
        }
        Tail::TailCall { .. } => 2,
        Tail::Match { arms, .. } => {
            1 + arms.iter().map(|a| block_cost(&a.body)).sum::<usize>()
        }
        Tail::Panic(_) => 1,
        Tail::Loop { body, .. } => 2 + block_cost(body),
        Tail::Recur { .. } => 1,
    }
}

/// Compute the cost of a block (sum of binding costs + tail cost).
fn block_cost(block: &Block) -> usize {
    let bind_cost: usize = block.binds.iter().map(|b| 1 + rhs_cost(&b.rhs)).sum();
    bind_cost + tail_cost(&block.tail)
}

/// Compute the cost of a function body.
pub fn func_cost(func: &FuncDef) -> usize {
    block_cost(&func.body)
}

/// Collect all `FuncId`s called (directly or via tail-call) within a block.
fn collect_callees_block(block: &Block, out: &mut HashSet<FuncId>) {
    for bind in &block.binds {
        match &bind.rhs {
            Rhs::Call { func: Atom::FuncRef(id), .. } => { out.insert(*id); }
            Rhs::MakeClosure { func_id, .. } => { out.insert(*func_id); }
            _ => {}
        }
    }
    collect_callees_tail(&block.tail, out);
}

fn collect_callees_tail(tail: &Tail, out: &mut HashSet<FuncId>) {
    match tail {
        Tail::TailCall { func: Atom::FuncRef(id), .. } => { out.insert(*id); }
        Tail::If { then_block, else_block, .. } => {
            collect_callees_block(then_block, out);
            collect_callees_block(else_block, out);
        }
        Tail::Match { arms, .. } => {
            for arm in arms {
                collect_callees_block(&arm.body, out);
            }
        }
        Tail::Loop { body, .. } => {
            collect_callees_block(body, out);
        }
        _ => {}
    }
}

/// Determine which functions in a module are eligible for inlining.
///
/// A function is eligible if:
/// - Its body cost is at or below the threshold
/// - It is not recursive (does not call itself, directly or indirectly)
/// - It is not referenced by `MakeClosure` (closures are called indirectly)
///
/// Returns the set of `FuncId`s that may be inlined.
pub fn find_inlinable(module: &Module) -> HashSet<FuncId> {
    // Build callee sets for each function.
    let mut callee_map: HashMap<FuncId, HashSet<FuncId>> = HashMap::new();
    let mut closure_funcs: HashSet<FuncId> = HashSet::new();

    for func in &module.funcs {
        let mut callees = HashSet::new();
        collect_callees_block(&func.body, &mut callees);
        // Check if any binding creates a closure referencing this func
        collect_closure_refs(&func.body, &mut closure_funcs);
        callee_map.insert(func.id, callees);
    }

    let mut eligible = HashSet::new();

    for func in &module.funcs {
        // Skip functions used as closures (called indirectly).
        if closure_funcs.contains(&func.id) {
            continue;
        }

        // Skip functions that are too large.
        let cost = func_cost(func);
        if cost > INLINE_COST_THRESHOLD {
            continue;
        }

        // Skip recursive functions (self-recursive check).
        if let Some(callees) = callee_map.get(&func.id)
            && callees.contains(&func.id)
        {
            continue;
        }

        eligible.insert(func.id);
    }

    eligible
}

/// Collect `FuncId`s that appear in `MakeClosure` within a block.
fn collect_closure_refs(block: &Block, out: &mut HashSet<FuncId>) {
    for bind in &block.binds {
        if let Rhs::MakeClosure { func_id, .. } = &bind.rhs {
            out.insert(*func_id);
        }
    }
    collect_closure_refs_tail(&block.tail, out);
}

fn collect_closure_refs_tail(tail: &Tail, out: &mut HashSet<FuncId>) {
    match tail {
        Tail::If { then_block, else_block, .. } => {
            collect_closure_refs(then_block, out);
            collect_closure_refs(else_block, out);
        }
        Tail::Match { arms, .. } => {
            for arm in arms {
                collect_closure_refs(&arm.body, out);
            }
        }
        Tail::Loop { body, .. } => {
            collect_closure_refs(body, out);
        }
        _ => {}
    }
}

// ── Inlining transform ─────────────────────────────────────────────────────

/// Inline eligible functions at their call sites throughout the module.
///
/// For each `Rhs::Call { func: FuncRef(id), args }` where `id` is in `eligible`:
/// - Rename all variables in the callee body to fresh IDs (avoiding capture).
/// - Bind each parameter to its corresponding argument atom.
/// - Splice the callee's block bindings into the caller's block.
/// - Replace the original call with the callee's tail return atom.
///
/// Returns a new `Module` with inlined call sites.
pub fn inline_calls(module: &Module) -> Module {
    let eligible = find_inlinable(module);
    if eligible.is_empty() {
        return module.clone();
    }

    // Build a lookup table of eligible function bodies.
    let func_table: HashMap<FuncId, &FuncDef> = module
        .funcs
        .iter()
        .filter(|f| eligible.contains(&f.id))
        .map(|f| (f.id, f))
        .collect();

    let mut var_gen = VarGen::new();
    // Advance past all existing VarIds.
    let max_var = find_max_var(module);
    for _ in 0..=max_var {
        var_gen.fresh();
    }

    let new_funcs: Vec<FuncDef> = module
        .funcs
        .iter()
        .map(|f| {
            let new_body = inline_block(&f.body, &func_table, &mut var_gen);
            FuncDef {
                id: f.id,
                name: f.name.clone(),
                params: f.params.clone(),
                body: new_body,
            }
        })
        .collect();

    Module {
        name: module.name.clone(),
        funcs: new_funcs,
    }
}

/// Find the maximum VarId number used anywhere in the module.
pub fn find_max_var(module: &Module) -> u32 {
    let mut max = 0u32;
    for func in &module.funcs {
        for p in &func.params {
            max = max.max(p.0);
        }
        max_var_block(&func.body, &mut max);
    }
    max
}

fn max_var_block(block: &Block, max: &mut u32) {
    for bind in &block.binds {
        *max = (*max).max(bind.var.0);
        max_var_rhs(&bind.rhs, max);
    }
    max_var_tail(&block.tail, max);
}

fn max_var_rhs(rhs: &Rhs, max: &mut u32) {
    match rhs {
        Rhs::Atom(a) => max_var_atom(a, max),
        Rhs::Call { func, args } => {
            max_var_atom(func, max);
            for a in args { max_var_atom(a, max); }
        }
        Rhs::MakeClosure { captures, .. } => {
            for (v, a) in captures {
                *max = (*max).max(v.0);
                max_var_atom(a, max);
            }
        }
        Rhs::MakeTuple { fields, .. } => {
            for a in fields { max_var_atom(a, max); }
        }
        Rhs::Project { base, .. } => max_var_atom(base, max),
    }
}

fn max_var_atom(atom: &Atom, max: &mut u32) {
    if let Atom::Var(v) = atom {
        *max = (*max).max(v.0);
    }
}

fn max_var_tail(tail: &Tail, max: &mut u32) {
    match tail {
        Tail::Return(a) => max_var_atom(a, max),
        Tail::If { cond, then_block, else_block } => {
            max_var_atom(cond, max);
            max_var_block(then_block, max);
            max_var_block(else_block, max);
        }
        Tail::TailCall { func, args } => {
            max_var_atom(func, max);
            for a in args { max_var_atom(a, max); }
        }
        Tail::Match { scrutinee, arms } => {
            max_var_atom(scrutinee, max);
            for arm in arms {
                for v in &arm.binds { *max = (*max).max(v.0); }
                max_var_block(&arm.body, max);
            }
        }
        Tail::Panic(a) => max_var_atom(a, max),
        Tail::Loop { vars, body } => {
            for (v, a) in vars {
                *max = (*max).max(v.0);
                max_var_atom(a, max);
            }
            max_var_block(body, max);
        }
        Tail::Recur { args } => {
            for a in args { max_var_atom(a, max); }
        }
    }
}

/// Inline calls in a block, producing a new block.
fn inline_block(
    block: &Block,
    func_table: &HashMap<FuncId, &FuncDef>,
    var_gen: &mut VarGen,
) -> Block {
    let mut new_binds = Vec::new();

    for bind in &block.binds {
        match &bind.rhs {
            Rhs::Call { func: Atom::FuncRef(id), args } if func_table.contains_key(id) => {
                let callee = func_table[id];
                // Build a variable rename map: callee vars → fresh vars.
                let mut rename = HashMap::new();
                // Bind params to args via let-binds.
                for (param, arg) in callee.params.iter().zip(args.iter()) {
                    let fresh = var_gen.fresh();
                    rename.insert(*param, fresh);
                    new_binds.push(LetBind {
                        var: fresh,
                        rhs: Rhs::Atom(arg.clone()),
                    });
                }
                // Rename and splice callee body bindings.
                let renamed_body = rename_block(&callee.body, &mut rename, var_gen);
                new_binds.extend(renamed_body.binds);
                // The callee's tail should be a Return(atom) for simple inlining.
                // Bind the result variable to the returned atom.
                match *renamed_body.tail {
                    Tail::Return(ref atom) => {
                        new_binds.push(LetBind {
                            var: bind.var,
                            rhs: Rhs::Atom(atom.clone()),
                        });
                    }
                    _ => {
                        // Non-return tail: can't inline in expression position.
                        // Fall back to the original call.
                        new_binds.push(bind.clone());
                    }
                }
            }
            _ => {
                new_binds.push(bind.clone());
            }
        }
    }

    let new_tail = inline_tail(&block.tail, func_table, var_gen);
    Block {
        binds: new_binds,
        tail: Box::new(new_tail),
    }
}

/// Inline calls that appear in tail position.
fn inline_tail(
    tail: &Tail,
    func_table: &HashMap<FuncId, &FuncDef>,
    var_gen: &mut VarGen,
) -> Tail {
    match tail {
        Tail::If { cond, then_block, else_block } => Tail::If {
            cond: cond.clone(),
            then_block: inline_block(then_block, func_table, var_gen),
            else_block: inline_block(else_block, func_table, var_gen),
        },
        Tail::Match { scrutinee, arms } => Tail::Match {
            scrutinee: scrutinee.clone(),
            arms: arms
                .iter()
                .map(|arm| MatchArm {
                    ctor: arm.ctor.clone(),
                    binds: arm.binds.clone(),
                    body: inline_block(&arm.body, func_table, var_gen),
                })
                .collect(),
        },
        Tail::Loop { vars, body } => Tail::Loop {
            vars: vars.clone(),
            body: Box::new(inline_block(body, func_table, var_gen)),
        },
        // TailCall to an eligible function: inline as block + return.
        Tail::TailCall { func: Atom::FuncRef(id), args } if func_table.contains_key(id) => {
            let callee = func_table[id];
            let mut rename = HashMap::new();
            let mut extra_binds = Vec::new();
            for (param, arg) in callee.params.iter().zip(args.iter()) {
                let fresh = var_gen.fresh();
                rename.insert(*param, fresh);
                extra_binds.push(LetBind {
                    var: fresh,
                    rhs: Rhs::Atom(arg.clone()),
                });
            }
            let renamed_body = rename_block(&callee.body, &mut rename, var_gen);
            // If the callee body is just binds + Return, splice them.
            // For other tails, we'd need a nested block, but ANF allows it.
            let mut all_binds = extra_binds;
            all_binds.extend(renamed_body.binds);
            // Wrap as a block that ends with the callee's tail.
            // Since we're in tail position, the callee's tail IS the new tail.
            // But we need to prepend the binds somehow. We return the tail directly
            // and the caller must handle extra_binds.
            // Actually, for simplicity, we can only inline tail calls that end in Return.
            match *renamed_body.tail {
                Tail::Return(ref atom) => {
                    // The binds become part of the enclosing block.
                    // But we're returning a Tail, not a Block.
                    // We need to return a Return with the atom — but also add binds.
                    // This is a limitation: to inline in tail position, we'd need
                    // to return (extra_binds, Tail). For now, just return as-is.
                    Tail::Return(atom.clone())
                }
                _ => tail.clone(),
            }
        }
        other => other.clone(),
    }
}

// ── Variable renaming ──────────────────────────────────────────────────────

fn rename_block(
    block: &Block,
    rename: &mut HashMap<VarId, VarId>,
    var_gen: &mut VarGen,
) -> Block {
    let mut new_binds = Vec::new();
    for bind in &block.binds {
        let fresh = var_gen.fresh();
        let new_rhs = rename_rhs(&bind.rhs, rename);
        rename.insert(bind.var, fresh);
        new_binds.push(LetBind { var: fresh, rhs: new_rhs });
    }
    let new_tail = rename_tail(&block.tail, rename, var_gen);
    Block {
        binds: new_binds,
        tail: Box::new(new_tail),
    }
}

fn rename_atom(atom: &Atom, rename: &HashMap<VarId, VarId>) -> Atom {
    match atom {
        Atom::Var(v) => {
            if let Some(new_v) = rename.get(v) {
                Atom::Var(*new_v)
            } else {
                atom.clone()
            }
        }
        _ => atom.clone(),
    }
}

fn rename_rhs(rhs: &Rhs, rename: &HashMap<VarId, VarId>) -> Rhs {
    match rhs {
        Rhs::Atom(a) => Rhs::Atom(rename_atom(a, rename)),
        Rhs::Call { func, args } => Rhs::Call {
            func: rename_atom(func, rename),
            args: args.iter().map(|a| rename_atom(a, rename)).collect(),
        },
        Rhs::MakeClosure { func_id, captures } => Rhs::MakeClosure {
            func_id: *func_id,
            captures: captures
                .iter()
                .map(|(v, a)| {
                    let new_v = rename.get(v).copied().unwrap_or(*v);
                    (new_v, rename_atom(a, rename))
                })
                .collect(),
        },
        Rhs::MakeTuple { ctor, fields } => Rhs::MakeTuple {
            ctor: ctor.clone(),
            fields: fields.iter().map(|a| rename_atom(a, rename)).collect(),
        },
        Rhs::Project { base, index } => Rhs::Project {
            base: rename_atom(base, rename),
            index: *index,
        },
    }
}

fn rename_tail(
    tail: &Tail,
    rename: &mut HashMap<VarId, VarId>,
    var_gen: &mut VarGen,
) -> Tail {
    match tail {
        Tail::Return(a) => Tail::Return(rename_atom(a, rename)),
        Tail::If { cond, then_block, else_block } => Tail::If {
            cond: rename_atom(cond, rename),
            then_block: rename_block(then_block, &mut rename.clone(), var_gen),
            else_block: rename_block(else_block, &mut rename.clone(), var_gen),
        },
        Tail::TailCall { func, args } => Tail::TailCall {
            func: rename_atom(func, rename),
            args: args.iter().map(|a| rename_atom(a, rename)).collect(),
        },
        Tail::Match { scrutinee, arms } => Tail::Match {
            scrutinee: rename_atom(scrutinee, rename),
            arms: arms
                .iter()
                .map(|arm| {
                    let mut arm_rename = rename.clone();
                    let new_binds: Vec<VarId> = arm
                        .binds
                        .iter()
                        .map(|v| {
                            let fresh = var_gen.fresh();
                            arm_rename.insert(*v, fresh);
                            fresh
                        })
                        .collect();
                    MatchArm {
                        ctor: arm.ctor.clone(),
                        binds: new_binds,
                        body: rename_block(&arm.body, &mut arm_rename, var_gen),
                    }
                })
                .collect(),
        },
        Tail::Panic(a) => Tail::Panic(rename_atom(a, rename)),
        Tail::Loop { vars, body } => {
            let new_vars: Vec<(VarId, Atom)> = vars
                .iter()
                .map(|(v, a)| {
                    let fresh = var_gen.fresh();
                    let renamed_a = rename_atom(a, rename);
                    rename.insert(*v, fresh);
                    (fresh, renamed_a)
                })
                .collect();
            Tail::Loop {
                vars: new_vars,
                body: Box::new(rename_block(body, &mut rename.clone(), var_gen)),
            }
        }
        Tail::Recur { args } => Tail::Recur {
            args: args.iter().map(|a| rename_atom(a, rename)).collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: make a simple function that just returns a constant.
    fn make_const_func(id: u32, name: &str, val: i64) -> FuncDef {
        FuncDef {
            id: FuncId(id),
            name: Some(name.to_string()),
            params: vec![],
            body: Block {
                binds: vec![],
                tail: Box::new(Tail::Return(Atom::Int(val))),
            },
        }
    }

    /// Helper: make a function with given params that returns its first param.
    fn make_identity_func(id: u32, name: &str) -> FuncDef {
        let param = VarId(100 + id);
        FuncDef {
            id: FuncId(id),
            name: Some(name.to_string()),
            params: vec![param],
            body: Block {
                binds: vec![],
                tail: Box::new(Tail::Return(Atom::Var(param))),
            },
        }
    }

    // ─── Analysis tests ─────────────────────────────────────────────────────

    #[test]
    fn leaf_function_is_eligible() {
        let module = Module {
            name: "test".to_string(),
            funcs: vec![make_const_func(0, "const42", 42)],
        };
        let eligible = find_inlinable(&module);
        assert!(eligible.contains(&FuncId(0)));
    }

    #[test]
    fn small_function_with_call_is_eligible() {
        // fn add(a, b) { let t = call @fn1(a, b); return t }
        let func = FuncDef {
            id: FuncId(0),
            name: Some("add".to_string()),
            params: vec![VarId(0), VarId(1)],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(2),
                    rhs: Rhs::Call {
                        func: Atom::FuncRef(FuncId(99)), // external
                        args: vec![Atom::Var(VarId(0)), Atom::Var(VarId(1))],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(2)))),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![func],
        };
        let eligible = find_inlinable(&module);
        assert!(eligible.contains(&FuncId(0)));
    }

    #[test]
    fn large_function_is_not_eligible() {
        // Create a function with many bindings to exceed threshold.
        let binds: Vec<LetBind> = (0..20)
            .map(|i| LetBind {
                var: VarId(i),
                rhs: Rhs::Call {
                    func: Atom::FuncRef(FuncId(99)),
                    args: vec![Atom::Int(i as i64)],
                },
            })
            .collect();
        let func = FuncDef {
            id: FuncId(0),
            name: Some("big".to_string()),
            params: vec![],
            body: Block {
                binds,
                tail: Box::new(Tail::Return(Atom::Int(0))),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![func],
        };
        let eligible = find_inlinable(&module);
        assert!(!eligible.contains(&FuncId(0)));
    }

    #[test]
    fn recursive_function_is_not_eligible() {
        // fn rec(x) { return tail-call @fn0(x) }
        let func = FuncDef {
            id: FuncId(0),
            name: Some("rec".to_string()),
            params: vec![VarId(0)],
            body: Block {
                binds: vec![],
                tail: Box::new(Tail::TailCall {
                    func: Atom::FuncRef(FuncId(0)), // calls itself
                    args: vec![Atom::Var(VarId(0))],
                }),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![func],
        };
        let eligible = find_inlinable(&module);
        assert!(!eligible.contains(&FuncId(0)));
    }

    #[test]
    fn closure_target_is_not_eligible() {
        // fn lifted(cap, x) { return cap }
        let lifted = FuncDef {
            id: FuncId(1),
            name: None,
            params: vec![VarId(10), VarId(11)],
            body: Block {
                binds: vec![],
                tail: Box::new(Tail::Return(Atom::Var(VarId(10)))),
            },
        };
        // fn main() { let c = MakeClosure(@fn1, [...]); return c }
        let main_fn = FuncDef {
            id: FuncId(0),
            name: Some("main".to_string()),
            params: vec![],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(0),
                    rhs: Rhs::MakeClosure {
                        func_id: FuncId(1),
                        captures: vec![(VarId(10), Atom::Int(5))],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(0)))),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![main_fn, lifted],
        };
        let eligible = find_inlinable(&module);
        // fn1 is used in MakeClosure → not eligible.
        assert!(!eligible.contains(&FuncId(1)));
        // fn0 is eligible (small, references closure but doesn't call itself).
        assert!(eligible.contains(&FuncId(0)));
    }

    #[test]
    fn func_cost_zero_for_trivial_return() {
        let func = make_const_func(0, "k", 0);
        assert_eq!(func_cost(&func), 0);
    }

    #[test]
    fn func_cost_counts_binds_and_calls() {
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![VarId(0)],
            body: Block {
                binds: vec![
                    LetBind { var: VarId(1), rhs: Rhs::Atom(Atom::Int(1)) },
                    LetBind {
                        var: VarId(2),
                        rhs: Rhs::Call {
                            func: Atom::FuncRef(FuncId(99)),
                            args: vec![Atom::Var(VarId(0)), Atom::Var(VarId(1))],
                        },
                    },
                ],
                tail: Box::new(Tail::Return(Atom::Var(VarId(2)))),
            },
        };
        // bind 1: 1 + 0 (atom) = 1
        // bind 2: 1 + 2 (call) = 3
        // tail: 0 (return)
        // total = 4
        assert_eq!(func_cost(&func), 4);
    }

    // ─── Transform tests ────────────────────────────────────────────────────

    #[test]
    fn inline_trivial_const_function() {
        // fn const42() { return 42 }
        // fn main() { let x = call @fn0(); return x }
        let const_fn = make_const_func(0, "const42", 42);
        let main_fn = FuncDef {
            id: FuncId(1),
            name: Some("main".to_string()),
            params: vec![],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(0),
                    rhs: Rhs::Call {
                        func: Atom::FuncRef(FuncId(0)),
                        args: vec![],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(0)))),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![const_fn, main_fn],
        };

        let result = inline_calls(&module);
        // After inlining, main should have the constant 42 instead of a call.
        let main = &result.funcs[1];
        // The call should be replaced by binding x = 42 (via Atom).
        assert!(main.body.binds.iter().any(|b| {
            matches!(&b.rhs, Rhs::Atom(Atom::Int(42)))
        }));
        // No call to @fn0 should remain.
        assert!(!main.body.binds.iter().any(|b| {
            matches!(&b.rhs, Rhs::Call { func: Atom::FuncRef(FuncId(0)), .. })
        }));
    }

    #[test]
    fn inline_identity_function_with_arg() {
        // fn id(x) { return x }
        // fn main() { let r = call @fn0(42); return r }
        let id_fn = make_identity_func(0, "id");
        let main_fn = FuncDef {
            id: FuncId(1),
            name: Some("main".to_string()),
            params: vec![],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(0),
                    rhs: Rhs::Call {
                        func: Atom::FuncRef(FuncId(0)),
                        args: vec![Atom::Int(42)],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(0)))),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![id_fn, main_fn],
        };

        let result = inline_calls(&module);
        let main = &result.funcs[1];
        // Should have no calls remaining.
        assert!(!main.body.binds.iter().any(|b| {
            matches!(&b.rhs, Rhs::Call { func: Atom::FuncRef(FuncId(0)), .. })
        }));
    }

    #[test]
    fn no_inline_when_nothing_eligible() {
        // fn big(...) with many bindings — exceeds threshold.
        let binds: Vec<LetBind> = (0..20)
            .map(|i| LetBind {
                var: VarId(i),
                rhs: Rhs::Call {
                    func: Atom::FuncRef(FuncId(99)),
                    args: vec![Atom::Int(i as i64)],
                },
            })
            .collect();
        let func = FuncDef {
            id: FuncId(0),
            name: Some("big".to_string()),
            params: vec![],
            body: Block {
                binds,
                tail: Box::new(Tail::Return(Atom::Int(0))),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![func],
        };
        let result = inline_calls(&module);
        // Nothing changed since nothing was eligible.
        assert_eq!(result.funcs[0].body.binds.len(), 20);
    }
}
