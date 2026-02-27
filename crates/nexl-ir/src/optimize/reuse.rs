//! Perceus reuse analysis for the ANF IR.
//!
//! Identifies opportunities for in-place reuse of heap allocations:
//! when a `MakeTuple` value is consumed (last use) and immediately followed
//! by another `MakeTuple` of equal or smaller size, the old allocation can
//! be reused instead of allocating fresh memory.
//!
//! This is an IR-level analysis that produces annotations. The codegen
//! backends use these annotations to emit conditional reuse instructions
//! (check refcount == 1 at runtime; if so, reuse in-place).

use std::collections::{HashMap, HashSet};

use crate::{Atom, Block, FuncDef, FuncId, Module, Rhs, Tail, VarId};

/// A reuse opportunity: `consumer` can reuse the allocation of `producer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReuseOp {
    /// The `VarId` of the `MakeTuple` binding that produces the reusable allocation.
    pub producer: VarId,
    /// The `VarId` of the `MakeTuple` binding that can reuse the allocation.
    pub consumer: VarId,
    /// Number of fields in the producer (determines allocation size).
    pub producer_fields: usize,
    /// Number of fields in the consumer.
    pub consumer_fields: usize,
}

/// Analyze a module for reuse opportunities.
pub fn find_reuse_opportunities(module: &Module) -> Vec<(FuncId, Vec<ReuseOp>)> {
    module
        .funcs
        .iter()
        .map(|f| {
            let ops = analyze_func_reuse(f);
            (f.id, ops)
        })
        .collect()
}

fn analyze_func_reuse(func: &FuncDef) -> Vec<ReuseOp> {
    let mut ops = Vec::new();
    analyze_block_reuse(&func.body, &mut ops);
    ops
}

/// Analyze a block for reuse opportunities.
///
/// In Perceus, a reuse opportunity occurs when:
/// 1. A `MakeTuple` allocation (`producer`) has its last use before
///    another `MakeTuple` allocation (`consumer`).
/// 2. The consumer's field count is ≤ the producer's field count.
///
/// We track which tuple-bound variables have been "last used" (dropped)
/// by scanning the block. When we encounter a new `MakeTuple`, we check
/// if any dead tuple allocation can be reused.
fn analyze_block_reuse(block: &Block, ops: &mut Vec<ReuseOp>) {
    // Step 1: Build a map of tuple producers (var → field count).
    let mut tuple_producers: HashMap<VarId, usize> = HashMap::new();
    for bind in &block.binds {
        if let Rhs::MakeTuple { fields, .. } = &bind.rhs {
            tuple_producers.insert(bind.var, fields.len());
        }
    }

    if tuple_producers.is_empty() {
        // Recurse into sub-blocks for nested reuse opportunities.
        analyze_tail_reuse(&block.tail, ops);
        return;
    }

    // Step 2: Find last-use positions for each tuple producer.
    let last_uses = find_last_uses(block, &tuple_producers);

    // Step 3: Scan bindings. When a producer is dead and we see a new MakeTuple,
    // match the best reuse candidate (smallest sufficient allocation).
    let mut dead_producers: Vec<(VarId, usize)> = Vec::new(); // (var, field_count)
    let mut used_producers: HashSet<VarId> = HashSet::new();

    for (i, bind) in block.binds.iter().enumerate() {
        // Mark producers whose last use is before this binding index as dead.
        for (&var, &last_idx) in &last_uses {
            if last_idx < i && tuple_producers.contains_key(&var) && !used_producers.contains(&var)
            {
                dead_producers.push((var, tuple_producers[&var]));
                used_producers.insert(var);
            }
        }

        // Check if this is a MakeTuple that could reuse a dead producer.
        if let Rhs::MakeTuple { fields, .. } = &bind.rhs {
            let needed = fields.len();
            // Find best candidate: smallest dead producer with enough fields.
            let best = dead_producers
                .iter()
                .filter(|(_, sz)| *sz >= needed)
                .min_by_key(|(_, sz)| *sz);

            if let Some(&(producer_var, producer_fields)) = best {
                ops.push(ReuseOp {
                    producer: producer_var,
                    consumer: bind.var,
                    producer_fields,
                    consumer_fields: needed,
                });
                // Remove the used candidate.
                dead_producers.retain(|(v, _)| *v != producer_var);
            }
        }
    }

    // Recurse into sub-blocks.
    analyze_tail_reuse(&block.tail, ops);
}

/// Find the last binding index at which each variable is used.
fn find_last_uses(block: &Block, producers: &HashMap<VarId, usize>) -> HashMap<VarId, usize> {
    let mut last_use: HashMap<VarId, usize> = HashMap::new();

    for (i, bind) in block.binds.iter().enumerate() {
        for_each_var_in_rhs(&bind.rhs, &mut |v| {
            if producers.contains_key(&v) {
                last_use.insert(v, i);
            }
        });
    }

    // Also check the tail for uses.
    let tail_idx = block.binds.len();
    for_each_var_in_tail(&block.tail, &mut |v| {
        if producers.contains_key(&v) {
            last_use.insert(v, tail_idx);
        }
    });

    last_use
}

fn for_each_var_in_atom(atom: &Atom, f: &mut dyn FnMut(VarId)) {
    if let Atom::Var(v) = atom {
        f(*v);
    }
}

fn for_each_var_in_rhs(rhs: &Rhs, f: &mut dyn FnMut(VarId)) {
    match rhs {
        Rhs::Atom(a) => for_each_var_in_atom(a, f),
        Rhs::Call { func, args } => {
            for_each_var_in_atom(func, f);
            for a in args {
                for_each_var_in_atom(a, f);
            }
        }
        Rhs::MakeClosure { captures, .. } => {
            for (_, a) in captures {
                for_each_var_in_atom(a, f);
            }
        }
        Rhs::MakeTuple { fields, .. } => {
            for a in fields {
                for_each_var_in_atom(a, f);
            }
        }
        Rhs::Project { base, .. } => for_each_var_in_atom(base, f),
    }
}

fn for_each_var_in_tail(tail: &Tail, f: &mut dyn FnMut(VarId)) {
    match tail {
        Tail::Return(a) => for_each_var_in_atom(a, f),
        Tail::If {
            cond,
            then_block,
            else_block,
        } => {
            for_each_var_in_atom(cond, f);
            for_each_var_in_block(then_block, f);
            for_each_var_in_block(else_block, f);
        }
        Tail::TailCall { func, args } => {
            for_each_var_in_atom(func, f);
            for a in args {
                for_each_var_in_atom(a, f);
            }
        }
        Tail::Match { scrutinee, arms } => {
            for_each_var_in_atom(scrutinee, f);
            for arm in arms {
                for_each_var_in_block(&arm.body, f);
            }
        }
        Tail::Panic(a) => for_each_var_in_atom(a, f),
        Tail::Loop { vars, body } => {
            for (_, a) in vars {
                for_each_var_in_atom(a, f);
            }
            for_each_var_in_block(body, f);
        }
        Tail::Recur { args } => {
            for a in args {
                for_each_var_in_atom(a, f);
            }
        }
    }
}

fn for_each_var_in_block(block: &Block, f: &mut dyn FnMut(VarId)) {
    for bind in &block.binds {
        for_each_var_in_rhs(&bind.rhs, f);
    }
    for_each_var_in_tail(&block.tail, f);
}

fn analyze_tail_reuse(tail: &Tail, ops: &mut Vec<ReuseOp>) {
    match tail {
        Tail::If {
            then_block,
            else_block,
            ..
        } => {
            analyze_block_reuse(then_block, ops);
            analyze_block_reuse(else_block, ops);
        }
        Tail::Match { arms, .. } => {
            for arm in arms {
                analyze_block_reuse(&arm.body, ops);
            }
        }
        Tail::Loop { body, .. } => {
            analyze_block_reuse(body, ops);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LetBind;

    #[test]
    fn no_reuse_when_no_tuples() {
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(0),
                    rhs: Rhs::Atom(Atom::Int(1)),
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(0)))),
            },
        };
        let ops = analyze_func_reuse(&func);
        assert!(ops.is_empty());
    }

    #[test]
    fn reuse_dead_tuple_for_same_size_tuple() {
        // %0 = MakeTuple("A", [1, 2])
        // %1 = Project(%0, 0)          — last use of %0
        // %2 = MakeTuple("B", [%1, 3]) — can reuse %0's allocation
        // return %2
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![
                    LetBind {
                        var: VarId(0),
                        rhs: Rhs::MakeTuple {
                            ctor: "A".to_string(),
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
                    LetBind {
                        var: VarId(2),
                        rhs: Rhs::MakeTuple {
                            ctor: "B".to_string(),
                            fields: vec![Atom::Var(VarId(1)), Atom::Int(3)],
                        },
                    },
                ],
                tail: Box::new(Tail::Return(Atom::Var(VarId(2)))),
            },
        };
        let ops = analyze_func_reuse(&func);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].producer, VarId(0));
        assert_eq!(ops[0].consumer, VarId(2));
        assert_eq!(ops[0].producer_fields, 2);
        assert_eq!(ops[0].consumer_fields, 2);
    }

    #[test]
    fn no_reuse_when_producer_still_live() {
        // %0 = MakeTuple("A", [1, 2])
        // %1 = MakeTuple("B", [3, 4])  — %0 is still live (used in return)
        // return %0
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![
                    LetBind {
                        var: VarId(0),
                        rhs: Rhs::MakeTuple {
                            ctor: "A".to_string(),
                            fields: vec![Atom::Int(1), Atom::Int(2)],
                        },
                    },
                    LetBind {
                        var: VarId(1),
                        rhs: Rhs::MakeTuple {
                            ctor: "B".to_string(),
                            fields: vec![Atom::Int(3), Atom::Int(4)],
                        },
                    },
                ],
                tail: Box::new(Tail::Return(Atom::Var(VarId(0)))),
            },
        };
        let ops = analyze_func_reuse(&func);
        assert!(ops.is_empty());
    }

    #[test]
    fn no_reuse_when_consumer_needs_more_fields() {
        // %0 = MakeTuple("Small", [1])     — 1 field
        // %1 = Project(%0, 0)              — last use of %0
        // %2 = MakeTuple("Big", [%1, 2, 3]) — 3 fields, too big to reuse %0
        // return %2
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![
                    LetBind {
                        var: VarId(0),
                        rhs: Rhs::MakeTuple {
                            ctor: "Small".to_string(),
                            fields: vec![Atom::Int(1)],
                        },
                    },
                    LetBind {
                        var: VarId(1),
                        rhs: Rhs::Project {
                            base: Atom::Var(VarId(0)),
                            index: 0,
                        },
                    },
                    LetBind {
                        var: VarId(2),
                        rhs: Rhs::MakeTuple {
                            ctor: "Big".to_string(),
                            fields: vec![Atom::Var(VarId(1)), Atom::Int(2), Atom::Int(3)],
                        },
                    },
                ],
                tail: Box::new(Tail::Return(Atom::Var(VarId(2)))),
            },
        };
        let ops = analyze_func_reuse(&func);
        assert!(ops.is_empty());
    }

    #[test]
    fn reuse_larger_producer_for_smaller_consumer() {
        // %0 = MakeTuple("Big", [1, 2, 3])  — 3 fields
        // %1 = Project(%0, 0)
        // %2 = MakeTuple("Small", [%1])      — 1 field, fits in %0's allocation
        // return %2
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![
                    LetBind {
                        var: VarId(0),
                        rhs: Rhs::MakeTuple {
                            ctor: "Big".to_string(),
                            fields: vec![Atom::Int(1), Atom::Int(2), Atom::Int(3)],
                        },
                    },
                    LetBind {
                        var: VarId(1),
                        rhs: Rhs::Project {
                            base: Atom::Var(VarId(0)),
                            index: 0,
                        },
                    },
                    LetBind {
                        var: VarId(2),
                        rhs: Rhs::MakeTuple {
                            ctor: "Small".to_string(),
                            fields: vec![Atom::Var(VarId(1))],
                        },
                    },
                ],
                tail: Box::new(Tail::Return(Atom::Var(VarId(2)))),
            },
        };
        let ops = analyze_func_reuse(&func);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].producer, VarId(0));
        assert_eq!(ops[0].consumer, VarId(2));
    }

    #[test]
    fn module_level_reuse_analysis() {
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![],
                tail: Box::new(Tail::Return(Atom::Unit)),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![func],
        };
        let results = find_reuse_opportunities(&module);
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_empty());
    }
}
