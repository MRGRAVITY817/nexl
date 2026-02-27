//! Escape analysis pass.
//!
//! Determines which heap-allocated values (closures and ADT tuples) can be
//! safely stack-allocated because they do not "escape" their defining scope.
//!
//! A value **escapes** if:
//! - It is returned from a function.
//! - It is passed as an argument to a call (the callee might store it).
//! - It is stored into another heap object (closure capture or tuple field).
//!
//! A value that does **not** escape can be stack-allocated, avoiding heap
//! allocation and GC overhead.

use std::collections::HashSet;

use crate::{Atom, Block, FuncDef, FuncId, Module, Rhs, Tail, VarId};

/// The result of escape analysis: which let-bound variables do NOT escape.
#[derive(Debug, Clone)]
pub struct EscapeInfo {
    /// Variables whose values are heap-allocated (`MakeClosure` or `MakeTuple`)
    /// and do NOT escape — these can be stack-allocated.
    pub non_escaping: HashSet<VarId>,
}

/// Analyze a module and return escape information.
pub fn analyze_escapes(module: &Module) -> Vec<(FuncId, EscapeInfo)> {
    module
        .funcs
        .iter()
        .map(|f| (f.id, analyze_func(f)))
        .collect()
}

fn analyze_func(func: &FuncDef) -> EscapeInfo {
    // Step 1: Find all variables bound to heap allocations.
    let mut heap_vars = HashSet::new();
    collect_heap_vars(&func.body, &mut heap_vars);

    // Step 2: Find all variables that escape.
    let mut escaping = HashSet::new();
    collect_escaping_block(&func.body, &mut escaping);

    // Step 3: Non-escaping = heap_vars - escaping.
    let non_escaping: HashSet<VarId> = heap_vars.difference(&escaping).copied().collect();

    EscapeInfo { non_escaping }
}

/// Collect variables bound to `MakeClosure` or `MakeTuple`.
fn collect_heap_vars(block: &Block, out: &mut HashSet<VarId>) {
    for bind in &block.binds {
        match &bind.rhs {
            Rhs::MakeClosure { .. } | Rhs::MakeTuple { .. } => {
                out.insert(bind.var);
            }
            _ => {}
        }
    }
    collect_heap_vars_tail(&block.tail, out);
}

fn collect_heap_vars_tail(tail: &Tail, out: &mut HashSet<VarId>) {
    match tail {
        Tail::If { then_block, else_block, .. } => {
            collect_heap_vars(then_block, out);
            collect_heap_vars(else_block, out);
        }
        Tail::Match { arms, .. } => {
            for arm in arms {
                collect_heap_vars(&arm.body, out);
            }
        }
        Tail::Loop { body, .. } => {
            collect_heap_vars(body, out);
        }
        _ => {}
    }
}

/// Collect variables that escape.
///
/// An atom escapes if it appears in:
/// - `Tail::Return(Var(v))`
/// - `Rhs::Call { args }` or `Tail::TailCall { args }` (passed to another function)
/// - `Rhs::MakeClosure { captures: (_, Var(v)) }` (captured by a closure)
/// - `Rhs::MakeTuple { fields: Var(v) }` (stored in a tuple)
fn collect_escaping_block(block: &Block, escaping: &mut HashSet<VarId>) {
    for bind in &block.binds {
        match &bind.rhs {
            Rhs::Call { func, args } => {
                // The function ref itself escapes if it's a var (indirect call).
                mark_if_var(func, escaping);
                // All arguments escape (callee may store them).
                for a in args {
                    mark_if_var(a, escaping);
                }
            }
            Rhs::MakeClosure { captures, .. } => {
                // Values captured by a closure escape (the closure may outlive the scope).
                for (_, a) in captures {
                    mark_if_var(a, escaping);
                }
            }
            Rhs::MakeTuple { fields, .. } => {
                // Values stored in a tuple escape (the tuple may be returned).
                for a in fields {
                    mark_if_var(a, escaping);
                }
            }
            Rhs::Project { .. } | Rhs::Atom(_) => {
                // Atom copy and projection don't cause escape.
            }
        }
    }
    collect_escaping_tail(&block.tail, escaping);
}

fn collect_escaping_tail(tail: &Tail, escaping: &mut HashSet<VarId>) {
    match tail {
        Tail::Return(a) => {
            // Returned values escape.
            mark_if_var(a, escaping);
        }
        Tail::TailCall { func, args } => {
            mark_if_var(func, escaping);
            for a in args {
                mark_if_var(a, escaping);
            }
        }
        Tail::If { then_block, else_block, .. } => {
            collect_escaping_block(then_block, escaping);
            collect_escaping_block(else_block, escaping);
        }
        Tail::Match { arms, .. } => {
            for arm in arms {
                collect_escaping_block(&arm.body, escaping);
            }
        }
        Tail::Loop { body, .. } => {
            collect_escaping_block(body, escaping);
        }
        Tail::Panic(_) | Tail::Recur { .. } => {}
    }
}

fn mark_if_var(atom: &Atom, escaping: &mut HashSet<VarId>) {
    if let Atom::Var(v) = atom {
        escaping.insert(*v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LetBind;

    fn ret_block(atom: Atom) -> Block {
        Block {
            binds: vec![],
            tail: Box::new(Tail::Return(atom)),
        }
    }

    #[test]
    fn closure_not_returned_does_not_escape() {
        // let %0 = MakeClosure(@fn1, []); return unit
        // %0 is heap-allocated but not returned/passed → non-escaping.
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(0),
                    rhs: Rhs::MakeClosure {
                        func_id: FuncId(1),
                        captures: vec![],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Unit)),
            },
        };
        let info = analyze_func(&func);
        assert!(info.non_escaping.contains(&VarId(0)));
    }

    #[test]
    fn closure_returned_escapes() {
        // let %0 = MakeClosure(@fn1, []); return %0
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(0),
                    rhs: Rhs::MakeClosure {
                        func_id: FuncId(1),
                        captures: vec![],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(0)))),
            },
        };
        let info = analyze_func(&func);
        assert!(!info.non_escaping.contains(&VarId(0)));
    }

    #[test]
    fn closure_passed_to_call_escapes() {
        // let %0 = MakeClosure(@fn1, []); let %1 = call @fn99(%0); return %1
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![
                    LetBind {
                        var: VarId(0),
                        rhs: Rhs::MakeClosure {
                            func_id: FuncId(1),
                            captures: vec![],
                        },
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
            },
        };
        let info = analyze_func(&func);
        assert!(!info.non_escaping.contains(&VarId(0)));
    }

    #[test]
    fn tuple_not_escaping() {
        // let %0 = MakeTuple("Pair", [1, 2]); let %1 = Project(%0, 0); return %1
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![
                    LetBind {
                        var: VarId(0),
                        rhs: Rhs::MakeTuple {
                            ctor: "Pair".to_string(),
                            fields: vec![Atom::Int(1), Atom::Int(2)],
                        },
                    },
                    LetBind {
                        var: VarId(1),
                        rhs: Rhs::Project {
                            base: Atom::Var(VarId(0)),
                            index: 0,
                        },
                    },
                ],
                tail: Box::new(Tail::Return(Atom::Var(VarId(1)))),
            },
        };
        let info = analyze_func(&func);
        // %0 is not returned or passed to a call — only projected from.
        assert!(info.non_escaping.contains(&VarId(0)));
    }

    #[test]
    fn tuple_stored_in_closure_escapes() {
        // let %0 = MakeTuple("Pair", [1, 2]);
        // let %1 = MakeClosure(@fn1, [(cap0, %0)]);
        // return unit
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![
                    LetBind {
                        var: VarId(0),
                        rhs: Rhs::MakeTuple {
                            ctor: "Pair".to_string(),
                            fields: vec![Atom::Int(1), Atom::Int(2)],
                        },
                    },
                    LetBind {
                        var: VarId(1),
                        rhs: Rhs::MakeClosure {
                            func_id: FuncId(1),
                            captures: vec![(VarId(10), Atom::Var(VarId(0)))],
                        },
                    },
                ],
                tail: Box::new(Tail::Return(Atom::Unit)),
            },
        };
        let info = analyze_func(&func);
        // %0 is captured by a closure → escapes.
        assert!(!info.non_escaping.contains(&VarId(0)));
        // %1 (the closure) is not returned/passed → non-escaping.
        assert!(info.non_escaping.contains(&VarId(1)));
    }

    #[test]
    fn analyze_module_returns_per_func_info() {
        let f0 = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(0),
                    rhs: Rhs::MakeTuple {
                        ctor: "X".to_string(),
                        fields: vec![],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Unit)),
            },
        };
        let f1 = FuncDef {
            id: FuncId(1),
            name: Some("g".to_string()),
            params: vec![],
            body: ret_block(Atom::Int(1)),
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![f0, f1],
        };
        let results = analyze_escapes(&module);
        assert_eq!(results.len(), 2);
        assert!(results[0].1.non_escaping.contains(&VarId(0)));
        assert!(results[1].1.non_escaping.is_empty());
    }

    #[test]
    fn escape_analysis_in_if_branches() {
        // let %0 = MakeTuple("X", []);
        // if cond { return %0 } else { return unit }
        // %0 escapes via the then-branch.
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![VarId(10)],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(0),
                    rhs: Rhs::MakeTuple {
                        ctor: "X".to_string(),
                        fields: vec![],
                    },
                }],
                tail: Box::new(Tail::If {
                    cond: Atom::Var(VarId(10)),
                    then_block: ret_block(Atom::Var(VarId(0))),
                    else_block: ret_block(Atom::Unit),
                }),
            },
        };
        let info = analyze_func(&func);
        assert!(!info.non_escaping.contains(&VarId(0)));
    }
}
