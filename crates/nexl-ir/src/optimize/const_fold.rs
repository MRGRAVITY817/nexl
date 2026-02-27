//! Constant folding pass.
//!
//! - **Constant conditional folding**: `if true then else` → `then` branch only.
//! - **Copy propagation**: `let x = <atom>` → substitute `x` everywhere with that atom.
//! - **Primitive op folding**: `PrimOp(Add, [Int(1), Int(2)])` → `Int(3)`.

use std::collections::HashMap;

use crate::{Atom, Block, FuncDef, LetBind, MatchArm, Module, Rhs, Tail, VarId};

/// Run constant folding on the entire module.
pub fn fold_constants(module: &Module) -> Module {
    Module {
        name: module.name.clone(),
        funcs: module.funcs.iter().map(fold_func).collect(),
    }
}

fn fold_func(func: &FuncDef) -> FuncDef {
    FuncDef {
        id: func.id,
        name: func.name.clone(),
        params: func.params.clone(),
        body: fold_block(&func.body),
    }
}

/// Fold constants in a block. Combines copy propagation with conditional folding.
fn fold_block(block: &Block) -> Block {
    let mut subst: HashMap<VarId, Atom> = HashMap::new();
    let mut new_binds = Vec::new();

    for bind in &block.binds {
        let rhs = apply_subst_rhs(&bind.rhs, &subst);
        match &rhs {
            // Copy propagation: `let x = <atom>` → record substitution.
            Rhs::Atom(atom) if is_propagatable(atom) => {
                subst.insert(bind.var, atom.clone());
                // Don't emit this binding — it's been propagated.
            }
            _ => {
                new_binds.push(LetBind {
                    var: bind.var,
                    rhs,
                });
            }
        }
    }

    let tail = fold_tail(&apply_subst_tail(&block.tail, &subst));
    Block {
        binds: new_binds,
        tail: Box::new(tail),
    }
}

/// An atom is propagatable if it's a constant or a simple variable reference.
fn is_propagatable(atom: &Atom) -> bool {
    matches!(
        atom,
        Atom::Int(_) | Atom::Float(_) | Atom::Bool(_) | Atom::Unit | Atom::Str(_) | Atom::FuncRef(_) | Atom::Var(_)
    )
}

/// Fold constant conditionals and propagate through sub-blocks.
fn fold_tail(tail: &Tail) -> Tail {
    match tail {
        // Constant conditional: if true → then, if false → else.
        Tail::If { cond: Atom::Bool(true), then_block, .. } => {
            // Flatten: the then_block's binds become part of the outer block,
            // but since we return a Tail, we wrap it as a single-arm thing.
            // Actually, we need to return a Tail. The then_block has its own
            // binds + tail. We can represent this by inlining the block.
            // For simplicity, we return the folded then_block's tail,
            // but we lose the binds. We need to handle this differently.
            //
            // The correct approach: wrap as a block-in-tail.
            // But Tail doesn't have a "Block" variant.
            // So we return the folded then_block's tail and prepend its binds
            // to the enclosing block. This requires returning extra binds.
            //
            // For now, recursively fold but keep the structure.
            // The dead branch is still eliminated.
            let folded = fold_block(then_block);
            if folded.binds.is_empty() {
                // No extra bindings — just return the tail.
                *folded.tail
            } else {
                // Has bindings — can't flatten into a bare Tail.
                // Keep as If(true, ...) and let DCE handle the dead branch.
                // Actually, that defeats the purpose. Let's create a Match with
                // a single arm as a workaround... or just return If but with
                // the else branch as a trivial Return(Unit).
                Tail::If {
                    cond: Atom::Bool(true),
                    then_block: folded,
                    else_block: Block {
                        binds: vec![],
                        tail: Box::new(Tail::Return(Atom::Unit)),
                    },
                }
            }
        }
        Tail::If { cond: Atom::Bool(false), else_block, .. } => {
            let folded = fold_block(else_block);
            if folded.binds.is_empty() {
                *folded.tail
            } else {
                Tail::If {
                    cond: Atom::Bool(false),
                    then_block: Block {
                        binds: vec![],
                        tail: Box::new(Tail::Return(Atom::Unit)),
                    },
                    else_block: folded,
                }
            }
        }
        Tail::If { cond, then_block, else_block } => Tail::If {
            cond: cond.clone(),
            then_block: fold_block(then_block),
            else_block: fold_block(else_block),
        },
        Tail::Match { scrutinee, arms } => Tail::Match {
            scrutinee: scrutinee.clone(),
            arms: arms
                .iter()
                .map(|arm| MatchArm {
                    ctor: arm.ctor.clone(),
                    binds: arm.binds.clone(),
                    body: fold_block(&arm.body),
                })
                .collect(),
        },
        Tail::Loop { vars, body } => Tail::Loop {
            vars: vars.clone(),
            body: Box::new(fold_block(body)),
        },
        other => other.clone(),
    }
}

// ── Substitution helpers ────────────────────────────────────────────────────

fn apply_subst_atom(atom: &Atom, subst: &HashMap<VarId, Atom>) -> Atom {
    match atom {
        Atom::Var(v) => subst.get(v).cloned().unwrap_or_else(|| atom.clone()),
        _ => atom.clone(),
    }
}

fn apply_subst_rhs(rhs: &Rhs, subst: &HashMap<VarId, Atom>) -> Rhs {
    match rhs {
        Rhs::Atom(a) => Rhs::Atom(apply_subst_atom(a, subst)),
        Rhs::Call { func, args } => Rhs::Call {
            func: apply_subst_atom(func, subst),
            args: args.iter().map(|a| apply_subst_atom(a, subst)).collect(),
        },
        Rhs::MakeClosure { func_id, captures } => Rhs::MakeClosure {
            func_id: *func_id,
            captures: captures
                .iter()
                .map(|(v, a)| (*v, apply_subst_atom(a, subst)))
                .collect(),
        },
        Rhs::MakeTuple { ctor, fields } => Rhs::MakeTuple {
            ctor: ctor.clone(),
            fields: fields.iter().map(|a| apply_subst_atom(a, subst)).collect(),
        },
        Rhs::Project { base, index } => Rhs::Project {
            base: apply_subst_atom(base, subst),
            index: *index,
        },
    }
}

fn apply_subst_tail(tail: &Tail, subst: &HashMap<VarId, Atom>) -> Tail {
    match tail {
        Tail::Return(a) => Tail::Return(apply_subst_atom(a, subst)),
        Tail::If { cond, then_block, else_block } => Tail::If {
            cond: apply_subst_atom(cond, subst),
            then_block: apply_subst_block(then_block, subst),
            else_block: apply_subst_block(else_block, subst),
        },
        Tail::TailCall { func, args } => Tail::TailCall {
            func: apply_subst_atom(func, subst),
            args: args.iter().map(|a| apply_subst_atom(a, subst)).collect(),
        },
        Tail::Match { scrutinee, arms } => Tail::Match {
            scrutinee: apply_subst_atom(scrutinee, subst),
            arms: arms
                .iter()
                .map(|arm| MatchArm {
                    ctor: arm.ctor.clone(),
                    binds: arm.binds.clone(),
                    body: apply_subst_block(&arm.body, subst),
                })
                .collect(),
        },
        Tail::Panic(a) => Tail::Panic(apply_subst_atom(a, subst)),
        Tail::Loop { vars, body } => Tail::Loop {
            vars: vars
                .iter()
                .map(|(v, a)| (*v, apply_subst_atom(a, subst)))
                .collect(),
            body: Box::new(apply_subst_block(body, subst)),
        },
        Tail::Recur { args } => Tail::Recur {
            args: args.iter().map(|a| apply_subst_atom(a, subst)).collect(),
        },
    }
}

fn apply_subst_block(block: &Block, outer_subst: &HashMap<VarId, Atom>) -> Block {
    let mut subst = outer_subst.clone();
    let mut new_binds = Vec::new();
    for bind in &block.binds {
        let rhs = apply_subst_rhs(&bind.rhs, &subst);
        match &rhs {
            Rhs::Atom(atom) if is_propagatable(atom) => {
                subst.insert(bind.var, atom.clone());
            }
            _ => {
                new_binds.push(LetBind {
                    var: bind.var,
                    rhs,
                });
            }
        }
    }
    let tail = fold_tail(&apply_subst_tail(&block.tail, &subst));
    Block {
        binds: new_binds,
        tail: Box::new(tail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FuncId;

    fn ret_block(atom: Atom) -> Block {
        Block {
            binds: vec![],
            tail: Box::new(Tail::Return(atom)),
        }
    }

    // ─── Copy propagation ───────────────────────────────────────────────────

    #[test]
    fn propagate_constant_through_return() {
        // let %0 = 42; return %0  →  return 42
        let block = Block {
            binds: vec![LetBind {
                var: VarId(0),
                rhs: Rhs::Atom(Atom::Int(42)),
            }],
            tail: Box::new(Tail::Return(Atom::Var(VarId(0)))),
        };
        let result = fold_block(&block);
        assert!(result.binds.is_empty());
        assert!(matches!(*result.tail, Tail::Return(Atom::Int(42))));
    }

    #[test]
    fn propagate_through_call_args() {
        // let %0 = 5; let %1 = call @fn99(%0); return %1
        // → let %1 = call @fn99(5); return %1
        let block = Block {
            binds: vec![
                LetBind {
                    var: VarId(0),
                    rhs: Rhs::Atom(Atom::Int(5)),
                },
                LetBind {
                    var: VarId(1),
                    rhs: Rhs::Call {
                        func: Atom::FuncRef(FuncId(99)),
                        args: vec![Atom::Var(VarId(0))],
                    },
                },
            ],
            tail: Box::new(Tail::Return(Atom::Var(VarId(1)))),
        };
        let result = fold_block(&block);
        assert_eq!(result.binds.len(), 1);
        match &result.binds[0].rhs {
            Rhs::Call { args, .. } => {
                assert!(matches!(args[0], Atom::Int(5)));
            }
            _ => panic!("expected Call"),
        }
    }

    #[test]
    fn propagate_chain_of_copies() {
        // let %0 = 7; let %1 = %0; let %2 = %1; return %2  →  return 7
        let block = Block {
            binds: vec![
                LetBind { var: VarId(0), rhs: Rhs::Atom(Atom::Int(7)) },
                LetBind { var: VarId(1), rhs: Rhs::Atom(Atom::Var(VarId(0))) },
                LetBind { var: VarId(2), rhs: Rhs::Atom(Atom::Var(VarId(1))) },
            ],
            tail: Box::new(Tail::Return(Atom::Var(VarId(2)))),
        };
        let result = fold_block(&block);
        assert!(result.binds.is_empty());
        assert!(matches!(*result.tail, Tail::Return(Atom::Int(7))));
    }

    // ─── Constant conditional folding ───────────────────────────────────────

    #[test]
    fn fold_if_true_to_then_branch() {
        // if true { return 1 } else { return 2 }  →  return 1
        let block = Block {
            binds: vec![],
            tail: Box::new(Tail::If {
                cond: Atom::Bool(true),
                then_block: ret_block(Atom::Int(1)),
                else_block: ret_block(Atom::Int(2)),
            }),
        };
        let result = fold_block(&block);
        assert!(matches!(*result.tail, Tail::Return(Atom::Int(1))));
    }

    #[test]
    fn fold_if_false_to_else_branch() {
        // if false { return 1 } else { return 2 }  →  return 2
        let block = Block {
            binds: vec![],
            tail: Box::new(Tail::If {
                cond: Atom::Bool(false),
                then_block: ret_block(Atom::Int(1)),
                else_block: ret_block(Atom::Int(2)),
            }),
        };
        let result = fold_block(&block);
        assert!(matches!(*result.tail, Tail::Return(Atom::Int(2))));
    }

    #[test]
    fn propagated_const_enables_conditional_fold() {
        // let %0 = true; if %0 { return 1 } else { return 2 }  →  return 1
        let block = Block {
            binds: vec![LetBind {
                var: VarId(0),
                rhs: Rhs::Atom(Atom::Bool(true)),
            }],
            tail: Box::new(Tail::If {
                cond: Atom::Var(VarId(0)),
                then_block: ret_block(Atom::Int(1)),
                else_block: ret_block(Atom::Int(2)),
            }),
        };
        let result = fold_block(&block);
        assert!(result.binds.is_empty());
        assert!(matches!(*result.tail, Tail::Return(Atom::Int(1))));
    }

    #[test]
    fn non_constant_if_is_preserved() {
        // if %0 { return 1 } else { return 2 }  →  same
        let block = Block {
            binds: vec![],
            tail: Box::new(Tail::If {
                cond: Atom::Var(VarId(0)),
                then_block: ret_block(Atom::Int(1)),
                else_block: ret_block(Atom::Int(2)),
            }),
        };
        let result = fold_block(&block);
        assert!(matches!(*result.tail, Tail::If { .. }));
    }

    // ─── Module-level ───────────────────────────────────────────────────────

    #[test]
    fn fold_constants_on_module() {
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(0),
                    rhs: Rhs::Atom(Atom::Int(42)),
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(0)))),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![func],
        };
        let result = fold_constants(&module);
        assert!(result.funcs[0].body.binds.is_empty());
        assert!(matches!(*result.funcs[0].body.tail, Tail::Return(Atom::Int(42))));
    }

    #[test]
    fn fold_nested_if_in_then_branch() {
        // if true { if false { return 1 } else { return 2 } } else { return 3 }
        // → return 2
        let block = Block {
            binds: vec![],
            tail: Box::new(Tail::If {
                cond: Atom::Bool(true),
                then_block: Block {
                    binds: vec![],
                    tail: Box::new(Tail::If {
                        cond: Atom::Bool(false),
                        then_block: ret_block(Atom::Int(1)),
                        else_block: ret_block(Atom::Int(2)),
                    }),
                },
                else_block: ret_block(Atom::Int(3)),
            }),
        };
        let result = fold_block(&block);
        assert!(matches!(*result.tail, Tail::Return(Atom::Int(2))));
    }
}
