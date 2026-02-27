//! Dead code elimination pass.
//!
//! Two levels:
//! 1. **Module-level DCE** — Remove function definitions not reachable from entry points.
//! 2. **Block-level DCE** — Remove unused let-bindings within function bodies.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::{Atom, Block, FuncDef, FuncId, LetBind, Module, Rhs, Tail, VarId};

// ── Module-level: reachability analysis ────────────────────────────────────

/// Build a call graph: for each function, which other functions does it reference?
fn build_call_graph(module: &Module) -> HashMap<FuncId, HashSet<FuncId>> {
    let mut graph = HashMap::new();
    for func in &module.funcs {
        let mut refs = HashSet::new();
        collect_func_refs_block(&func.body, &mut refs);
        graph.insert(func.id, refs);
    }
    graph
}

fn collect_func_refs_block(block: &Block, out: &mut HashSet<FuncId>) {
    for bind in &block.binds {
        collect_func_refs_rhs(&bind.rhs, out);
    }
    collect_func_refs_tail(&block.tail, out);
}

fn collect_func_refs_atom(atom: &Atom, out: &mut HashSet<FuncId>) {
    if let Atom::FuncRef(id) = atom {
        out.insert(*id);
    }
}

fn collect_func_refs_rhs(rhs: &Rhs, out: &mut HashSet<FuncId>) {
    match rhs {
        Rhs::Atom(a) => collect_func_refs_atom(a, out),
        Rhs::Call { func, args } => {
            collect_func_refs_atom(func, out);
            for a in args {
                collect_func_refs_atom(a, out);
            }
        }
        Rhs::MakeClosure { func_id, captures } => {
            out.insert(*func_id);
            for (_, a) in captures {
                collect_func_refs_atom(a, out);
            }
        }
        Rhs::MakeTuple { fields, .. } => {
            for a in fields {
                collect_func_refs_atom(a, out);
            }
        }
        Rhs::Project { base, .. } => collect_func_refs_atom(base, out),
    }
}

fn collect_func_refs_tail(tail: &Tail, out: &mut HashSet<FuncId>) {
    match tail {
        Tail::Return(a) => collect_func_refs_atom(a, out),
        Tail::If { cond, then_block, else_block } => {
            collect_func_refs_atom(cond, out);
            collect_func_refs_block(then_block, out);
            collect_func_refs_block(else_block, out);
        }
        Tail::TailCall { func, args } => {
            collect_func_refs_atom(func, out);
            for a in args {
                collect_func_refs_atom(a, out);
            }
        }
        Tail::Match { scrutinee, arms } => {
            collect_func_refs_atom(scrutinee, out);
            for arm in arms {
                collect_func_refs_block(&arm.body, out);
            }
        }
        Tail::Panic(a) => collect_func_refs_atom(a, out),
        Tail::Loop { vars, body } => {
            for (_, a) in vars {
                collect_func_refs_atom(a, out);
            }
            collect_func_refs_block(body, out);
        }
        Tail::Recur { args } => {
            for a in args {
                collect_func_refs_atom(a, out);
            }
        }
    }
}

/// Find all functions reachable from the given entry points via BFS.
pub fn reachable_funcs(module: &Module, entry_points: &[FuncId]) -> HashSet<FuncId> {
    let graph = build_call_graph(module);
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    for ep in entry_points {
        if visited.insert(*ep) {
            queue.push_back(*ep);
        }
    }

    while let Some(fid) = queue.pop_front() {
        if let Some(refs) = graph.get(&fid) {
            for r in refs {
                if visited.insert(*r) {
                    queue.push_back(*r);
                }
            }
        }
    }

    visited
}

/// Remove functions not reachable from entry points.
///
/// Entry points are determined heuristically: all named functions (top-level `defn`s)
/// are considered potential entry points.
pub fn eliminate_dead_funcs(module: &Module) -> Module {
    let entry_points: Vec<FuncId> = module
        .funcs
        .iter()
        .filter(|f| f.name.is_some())
        .map(|f| f.id)
        .collect();

    let reachable = reachable_funcs(module, &entry_points);

    Module {
        name: module.name.clone(),
        funcs: module
            .funcs
            .iter()
            .filter(|f| reachable.contains(&f.id))
            .cloned()
            .collect(),
    }
}

// ── Block-level: unused binding elimination ────────────────────────────────

/// Collect all `VarId`s *used* (referenced) in a block's bindings and tail.
fn collect_used_vars_block(block: &Block, used: &mut HashSet<VarId>) {
    for bind in &block.binds {
        collect_used_vars_rhs(&bind.rhs, used);
    }
    collect_used_vars_tail(&block.tail, used);
}

fn collect_used_vars_atom(atom: &Atom, used: &mut HashSet<VarId>) {
    if let Atom::Var(v) = atom {
        used.insert(*v);
    }
}

fn collect_used_vars_rhs(rhs: &Rhs, used: &mut HashSet<VarId>) {
    match rhs {
        Rhs::Atom(a) => collect_used_vars_atom(a, used),
        Rhs::Call { func, args } => {
            collect_used_vars_atom(func, used);
            for a in args {
                collect_used_vars_atom(a, used);
            }
        }
        Rhs::MakeClosure { captures, .. } => {
            for (_, a) in captures {
                collect_used_vars_atom(a, used);
            }
        }
        Rhs::MakeTuple { fields, .. } => {
            for a in fields {
                collect_used_vars_atom(a, used);
            }
        }
        Rhs::Project { base, .. } => collect_used_vars_atom(base, used),
    }
}

fn collect_used_vars_tail(tail: &Tail, used: &mut HashSet<VarId>) {
    match tail {
        Tail::Return(a) => collect_used_vars_atom(a, used),
        Tail::If { cond, then_block, else_block } => {
            collect_used_vars_atom(cond, used);
            collect_used_vars_block(then_block, used);
            collect_used_vars_block(else_block, used);
        }
        Tail::TailCall { func, args } => {
            collect_used_vars_atom(func, used);
            for a in args {
                collect_used_vars_atom(a, used);
            }
        }
        Tail::Match { scrutinee, arms } => {
            collect_used_vars_atom(scrutinee, used);
            for arm in arms {
                collect_used_vars_block(&arm.body, used);
            }
        }
        Tail::Panic(a) => collect_used_vars_atom(a, used),
        Tail::Loop { vars, body } => {
            for (_, a) in vars {
                collect_used_vars_atom(a, used);
            }
            collect_used_vars_block(body, used);
        }
        Tail::Recur { args } => {
            for a in args {
                collect_used_vars_atom(a, used);
            }
        }
    }
}

/// Returns true if a binding's RHS has side effects (calls).
fn rhs_has_side_effects(rhs: &Rhs) -> bool {
    matches!(rhs, Rhs::Call { .. })
}

/// Remove unused let-bindings from a block (single pass, backward scan).
///
/// A binding is removed if:
/// - Its variable is not referenced anywhere in later bindings or the tail.
/// - Its RHS has no side effects (only calls are considered effectful).
fn eliminate_dead_binds_block(block: &Block) -> Block {
    // First, recursively clean sub-blocks in the tail.
    let clean_tail = eliminate_dead_binds_tail(&block.tail);

    // Collect all used variables in the whole block (bindings + tail).
    let mut used = HashSet::new();
    collect_used_vars_tail(&clean_tail, &mut used);

    // Backward pass: keep a binding if its var is used OR it has side effects.
    let mut kept: Vec<LetBind> = Vec::new();
    for bind in block.binds.iter().rev() {
        let is_used = used.contains(&bind.var);
        let has_effects = rhs_has_side_effects(&bind.rhs);
        if is_used || has_effects {
            // This binding is live: add its rhs's used vars.
            collect_used_vars_rhs(&bind.rhs, &mut used);
            kept.push(bind.clone());
        }
    }
    kept.reverse();

    Block {
        binds: kept,
        tail: Box::new(clean_tail),
    }
}

fn eliminate_dead_binds_tail(tail: &Tail) -> Tail {
    match tail {
        Tail::If { cond, then_block, else_block } => Tail::If {
            cond: cond.clone(),
            then_block: eliminate_dead_binds_block(then_block),
            else_block: eliminate_dead_binds_block(else_block),
        },
        Tail::Match { scrutinee, arms } => Tail::Match {
            scrutinee: scrutinee.clone(),
            arms: arms
                .iter()
                .map(|arm| crate::MatchArm {
                    ctor: arm.ctor.clone(),
                    binds: arm.binds.clone(),
                    body: eliminate_dead_binds_block(&arm.body),
                })
                .collect(),
        },
        Tail::Loop { vars, body } => Tail::Loop {
            vars: vars.clone(),
            body: Box::new(eliminate_dead_binds_block(body)),
        },
        other => other.clone(),
    }
}

/// Full dead code elimination: remove unreachable functions and unused bindings.
pub fn eliminate_dead_code(module: &Module) -> Module {
    let pruned = eliminate_dead_funcs(module);
    Module {
        name: pruned.name,
        funcs: pruned
            .funcs
            .into_iter()
            .map(|f| FuncDef {
                id: f.id,
                name: f.name,
                params: f.params,
                body: eliminate_dead_binds_block(&f.body),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_func(id: u32, name: Option<&str>, body: Block) -> FuncDef {
        FuncDef {
            id: FuncId(id),
            name: name.map(|s| s.to_string()),
            params: vec![],
            body,
        }
    }

    fn ret_block(atom: Atom) -> Block {
        Block {
            binds: vec![],
            tail: Box::new(Tail::Return(atom)),
        }
    }

    // ─── Module-level DCE ───────────────────────────────────────────────────

    #[test]
    fn reachable_from_entry_keeps_transitive_deps() {
        // fn main() calls @fn1; fn helper() calls @fn2; fn unused() — orphan.
        let main_fn = make_func(
            0,
            Some("main"),
            Block {
                binds: vec![LetBind {
                    var: VarId(0),
                    rhs: Rhs::Call {
                        func: Atom::FuncRef(FuncId(1)),
                        args: vec![],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(0)))),
            },
        );
        let helper = make_func(
            1,
            Some("helper"),
            Block {
                binds: vec![LetBind {
                    var: VarId(0),
                    rhs: Rhs::Call {
                        func: Atom::FuncRef(FuncId(2)),
                        args: vec![],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(0)))),
            },
        );
        let deep = make_func(2, Some("deep"), ret_block(Atom::Int(42)));
        let unused = make_func(3, Some("unused"), ret_block(Atom::Int(0)));

        let module = Module {
            name: "test".to_string(),
            funcs: vec![main_fn, helper, deep, unused],
        };

        let result = eliminate_dead_funcs(&module);
        let ids: HashSet<FuncId> = result.funcs.iter().map(|f| f.id).collect();
        // main, helper, deep are reachable; unused is not reachable
        // BUT unused is named, so it's also an entry point.
        // All named functions are entry points → all kept.
        assert!(ids.contains(&FuncId(0)));
        assert!(ids.contains(&FuncId(1)));
        assert!(ids.contains(&FuncId(2)));
        assert!(ids.contains(&FuncId(3)));
    }

    #[test]
    fn unreachable_anonymous_closure_is_removed() {
        // Named fn main; anonymous lifted lambda @fn1 not referenced by anyone.
        let main_fn = make_func(0, Some("main"), ret_block(Atom::Int(1)));
        let orphan_closure = make_func(1, None, ret_block(Atom::Int(99)));

        let module = Module {
            name: "test".to_string(),
            funcs: vec![main_fn, orphan_closure],
        };

        let result = eliminate_dead_funcs(&module);
        let ids: HashSet<FuncId> = result.funcs.iter().map(|f| f.id).collect();
        assert!(ids.contains(&FuncId(0)));
        assert!(!ids.contains(&FuncId(1)));
    }

    #[test]
    fn closure_referenced_by_make_closure_is_kept() {
        let main_fn = make_func(
            0,
            Some("main"),
            Block {
                binds: vec![LetBind {
                    var: VarId(0),
                    rhs: Rhs::MakeClosure {
                        func_id: FuncId(1),
                        captures: vec![],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(0)))),
            },
        );
        let closure_fn = make_func(1, None, ret_block(Atom::Int(42)));

        let module = Module {
            name: "test".to_string(),
            funcs: vec![main_fn, closure_fn],
        };

        let result = eliminate_dead_funcs(&module);
        assert_eq!(result.funcs.len(), 2);
    }

    // ─── Block-level DCE ────────────────────────────────────────────────────

    #[test]
    fn unused_pure_binding_is_removed() {
        let block = Block {
            binds: vec![
                LetBind { var: VarId(0), rhs: Rhs::Atom(Atom::Int(42)) }, // unused
                LetBind { var: VarId(1), rhs: Rhs::Atom(Atom::Int(7)) },  // used in return
            ],
            tail: Box::new(Tail::Return(Atom::Var(VarId(1)))),
        };
        let result = eliminate_dead_binds_block(&block);
        assert_eq!(result.binds.len(), 1);
        assert_eq!(result.binds[0].var, VarId(1));
    }

    #[test]
    fn unused_call_binding_is_kept_for_side_effects() {
        let block = Block {
            binds: vec![LetBind {
                var: VarId(0),
                rhs: Rhs::Call {
                    func: Atom::FuncRef(FuncId(99)),
                    args: vec![],
                },
            }],
            tail: Box::new(Tail::Return(Atom::Int(0))),
        };
        let result = eliminate_dead_binds_block(&block);
        // Call is kept because it may have side effects.
        assert_eq!(result.binds.len(), 1);
    }

    #[test]
    fn chain_of_unused_pure_bindings_removed() {
        // %0 = 1; %1 = %0; %2 = %1 — but only %2 is used? No, none are used.
        let block = Block {
            binds: vec![
                LetBind { var: VarId(0), rhs: Rhs::Atom(Atom::Int(1)) },
                LetBind { var: VarId(1), rhs: Rhs::Atom(Atom::Var(VarId(0))) },
                LetBind { var: VarId(2), rhs: Rhs::Atom(Atom::Var(VarId(1))) },
            ],
            tail: Box::new(Tail::Return(Atom::Int(99))),
        };
        let result = eliminate_dead_binds_block(&block);
        // None are used by the tail, all are pure → all removed.
        assert_eq!(result.binds.len(), 0);
    }

    #[test]
    fn transitive_used_bindings_kept() {
        // %0 = 1; %1 = %0; return %1  → both kept.
        let block = Block {
            binds: vec![
                LetBind { var: VarId(0), rhs: Rhs::Atom(Atom::Int(1)) },
                LetBind { var: VarId(1), rhs: Rhs::Atom(Atom::Var(VarId(0))) },
            ],
            tail: Box::new(Tail::Return(Atom::Var(VarId(1)))),
        };
        let result = eliminate_dead_binds_block(&block);
        assert_eq!(result.binds.len(), 2);
    }

    #[test]
    fn dce_recurses_into_if_branches() {
        let block = Block {
            binds: vec![],
            tail: Box::new(Tail::If {
                cond: Atom::Bool(true),
                then_block: Block {
                    binds: vec![
                        LetBind { var: VarId(0), rhs: Rhs::Atom(Atom::Int(1)) }, // unused
                    ],
                    tail: Box::new(Tail::Return(Atom::Int(2))),
                },
                else_block: ret_block(Atom::Int(3)),
            }),
        };
        let result = eliminate_dead_binds_block(&block);
        if let Tail::If { then_block, .. } = &*result.tail {
            assert_eq!(then_block.binds.len(), 0);
        } else {
            panic!("expected If tail");
        }
    }

    // ─── Full DCE ───────────────────────────────────────────────────────────

    #[test]
    fn full_dce_removes_dead_funcs_and_binds() {
        let main_fn = FuncDef {
            id: FuncId(0),
            name: Some("main".to_string()),
            params: vec![],
            body: Block {
                binds: vec![
                    LetBind { var: VarId(0), rhs: Rhs::Atom(Atom::Int(99)) }, // unused
                ],
                tail: Box::new(Tail::Return(Atom::Int(1))),
            },
        };
        let orphan = make_func(1, None, ret_block(Atom::Int(0)));

        let module = Module {
            name: "test".to_string(),
            funcs: vec![main_fn, orphan],
        };

        let result = eliminate_dead_code(&module);
        // Orphan anonymous fn removed.
        assert_eq!(result.funcs.len(), 1);
        // Dead binding in main removed.
        assert_eq!(result.funcs[0].body.binds.len(), 0);
    }
}
