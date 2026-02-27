//! Function specialization pass.
//!
//! When a function is called with one or more constant arguments across all
//! call sites, create a specialized version with those constants baked in.
//! This enables further optimizations (constant folding, branch elimination)
//! within the specialized body.
//!
//! Since the ANF IR is untyped (all values are i64), this is constant-argument
//! specialization rather than type-based monomorphization.

use std::collections::HashMap;

use crate::{Atom, Block, FuncDef, FuncId, LetBind, Module, Rhs, Tail, VarGen, VarId};

/// Analyze call sites to find functions called with constant arguments.
///
/// Returns a map from `FuncId` to the constant arguments at each position.
/// A position is `Some(atom)` if ALL call sites pass the same constant there,
/// or `None` if different values are used.
pub fn find_specializable(module: &Module) -> HashMap<FuncId, Vec<Option<Atom>>> {
    // Collect all call sites for each function.
    let mut call_sites: HashMap<FuncId, Vec<Vec<Atom>>> = HashMap::new();

    for func in &module.funcs {
        collect_call_sites(&func.body, &mut call_sites);
    }

    let mut result = HashMap::new();

    for (fid, sites) in &call_sites {
        if sites.is_empty() {
            continue;
        }
        // Find the function definition to get param count.
        let Some(func_def) = module.funcs.iter().find(|f| f.id == *fid) else {
            continue;
        };
        let param_count = func_def.params.len();
        if param_count == 0 {
            continue;
        }

        let mut const_args: Vec<Option<Atom>> = Vec::with_capacity(param_count);

        for i in 0..param_count {
            let first = sites[0].get(i).cloned();
            let is_constant = first.as_ref().is_some_and(is_constant_atom);
            let all_same = is_constant
                && sites
                    .iter()
                    .all(|s| s.get(i).map(atom_display) == first.as_ref().map(atom_display));

            if all_same {
                const_args.push(first);
            } else {
                const_args.push(None);
            }
        }

        // Only specialize if at least one argument is constant across all sites.
        if const_args.iter().any(|a| a.is_some()) {
            result.insert(*fid, const_args);
        }
    }

    result
}

fn is_constant_atom(atom: &Atom) -> bool {
    matches!(
        atom,
        Atom::Int(_) | Atom::Float(_) | Atom::Bool(_) | Atom::Unit | Atom::Str(_)
    )
}

/// Cheap display key for comparing atoms.
fn atom_display(atom: &Atom) -> String {
    atom.to_string()
}

fn collect_call_sites(block: &Block, sites: &mut HashMap<FuncId, Vec<Vec<Atom>>>) {
    for bind in &block.binds {
        if let Rhs::Call { func: Atom::FuncRef(fid), args } = &bind.rhs {
            sites.entry(*fid).or_default().push(args.clone());
        }
    }
    collect_call_sites_tail(&block.tail, sites);
}

fn collect_call_sites_tail(tail: &Tail, sites: &mut HashMap<FuncId, Vec<Vec<Atom>>>) {
    match tail {
        Tail::TailCall { func: Atom::FuncRef(fid), args } => {
            sites.entry(*fid).or_default().push(args.clone());
        }
        Tail::If { then_block, else_block, .. } => {
            collect_call_sites(then_block, sites);
            collect_call_sites(else_block, sites);
        }
        Tail::Match { arms, .. } => {
            for arm in arms {
                collect_call_sites(&arm.body, sites);
            }
        }
        Tail::Loop { body, .. } => {
            collect_call_sites(body, sites);
        }
        _ => {}
    }
}

/// Create specialized copies of functions and rewrite call sites.
pub fn specialize(module: &Module) -> Module {
    let specializable = find_specializable(module);
    if specializable.is_empty() {
        return module.clone();
    }

    let mut next_func_id = module.funcs.iter().map(|f| f.id.0).max().unwrap_or(0) + 1;
    let mut var_gen = VarGen::new();
    // Advance var_gen past existing vars.
    let max_var = crate::optimize::inline::find_max_var(module);
    for _ in 0..=max_var {
        var_gen.fresh();
    }

    // Create specialized function copies and a rewrite map.
    let mut new_funcs: Vec<FuncDef> = Vec::new();
    let mut rewrite_map: HashMap<FuncId, (FuncId, Vec<Option<Atom>>)> = HashMap::new();

    for (fid, const_args) in &specializable {
        let Some(original) = module.funcs.iter().find(|f| f.id == *fid) else {
            continue;
        };

        let spec_id = FuncId(next_func_id);
        next_func_id += 1;

        // Build parameter substitutions for constant args.
        let mut subst: HashMap<VarId, Atom> = HashMap::new();
        let mut remaining_params = Vec::new();

        for (i, param) in original.params.iter().enumerate() {
            if let Some(Some(atom)) = const_args.get(i) {
                subst.insert(*param, atom.clone());
            } else {
                remaining_params.push(*param);
            }
        }

        // Create specialized body by applying substitutions.
        let spec_body = apply_subst_block(&original.body, &subst);
        let spec_name = original
            .name
            .as_ref()
            .map(|n| format!("{n}$spec{}", spec_id.0));

        new_funcs.push(FuncDef {
            id: spec_id,
            name: spec_name,
            params: remaining_params,
            body: spec_body,
        });

        rewrite_map.insert(*fid, (spec_id, const_args.clone()));
    }

    // Rewrite call sites in all functions.
    let mut result_funcs: Vec<FuncDef> = module
        .funcs
        .iter()
        .map(|f| FuncDef {
            id: f.id,
            name: f.name.clone(),
            params: f.params.clone(),
            body: rewrite_calls_block(&f.body, &rewrite_map),
        })
        .collect();

    result_funcs.extend(new_funcs);

    Module {
        name: module.name.clone(),
        funcs: result_funcs,
    }
}

fn rewrite_calls_block(block: &Block, rewrites: &HashMap<FuncId, (FuncId, Vec<Option<Atom>>)>) -> Block {
    let new_binds = block
        .binds
        .iter()
        .map(|bind| {
            let new_rhs = match &bind.rhs {
                Rhs::Call { func: Atom::FuncRef(fid), args } if rewrites.contains_key(fid) => {
                    let (spec_id, const_args) = &rewrites[fid];
                    let remaining_args: Vec<Atom> = args
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| {
                            const_args.get(*i).is_none_or(|ca| ca.is_none())
                        })
                        .map(|(_, a)| a.clone())
                        .collect();
                    Rhs::Call {
                        func: Atom::FuncRef(*spec_id),
                        args: remaining_args,
                    }
                }
                other => other.clone(),
            };
            LetBind {
                var: bind.var,
                rhs: new_rhs,
            }
        })
        .collect();

    Block {
        binds: new_binds,
        tail: Box::new(rewrite_calls_tail(&block.tail, rewrites)),
    }
}

fn rewrite_calls_tail(tail: &Tail, rewrites: &HashMap<FuncId, (FuncId, Vec<Option<Atom>>)>) -> Tail {
    match tail {
        Tail::TailCall { func: Atom::FuncRef(fid), args } if rewrites.contains_key(fid) => {
            let (spec_id, const_args) = &rewrites[fid];
            let remaining_args: Vec<Atom> = args
                .iter()
                .enumerate()
                .filter(|(i, _)| const_args.get(*i).is_none_or(|ca| ca.is_none()))
                .map(|(_, a)| a.clone())
                .collect();
            Tail::TailCall {
                func: Atom::FuncRef(*spec_id),
                args: remaining_args,
            }
        }
        Tail::If { cond, then_block, else_block } => Tail::If {
            cond: cond.clone(),
            then_block: rewrite_calls_block(then_block, rewrites),
            else_block: rewrite_calls_block(else_block, rewrites),
        },
        Tail::Match { scrutinee, arms } => Tail::Match {
            scrutinee: scrutinee.clone(),
            arms: arms
                .iter()
                .map(|arm| crate::MatchArm {
                    ctor: arm.ctor.clone(),
                    binds: arm.binds.clone(),
                    body: rewrite_calls_block(&arm.body, rewrites),
                })
                .collect(),
        },
        Tail::Loop { vars, body } => Tail::Loop {
            vars: vars.clone(),
            body: Box::new(rewrite_calls_block(body, rewrites)),
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

fn apply_subst_block(block: &Block, subst: &HashMap<VarId, Atom>) -> Block {
    Block {
        binds: block
            .binds
            .iter()
            .map(|b| LetBind {
                var: b.var,
                rhs: apply_subst_rhs(&b.rhs, subst),
            })
            .collect(),
        tail: Box::new(apply_subst_tail(&block.tail, subst)),
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
                .map(|arm| crate::MatchArm {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_specialization_when_no_constant_args() {
        // fn add(a, b) { let r = call @fn99(a, b); return r }
        // fn main() { let x = call @fn0(%0, %1); return x }
        let add_fn = FuncDef {
            id: FuncId(0),
            name: Some("add".to_string()),
            params: vec![VarId(0), VarId(1)],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(2),
                    rhs: Rhs::Call {
                        func: Atom::FuncRef(FuncId(99)),
                        args: vec![Atom::Var(VarId(0)), Atom::Var(VarId(1))],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(2)))),
            },
        };
        let main_fn = FuncDef {
            id: FuncId(1),
            name: Some("main".to_string()),
            params: vec![VarId(10), VarId(11)],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(12),
                    rhs: Rhs::Call {
                        func: Atom::FuncRef(FuncId(0)),
                        args: vec![Atom::Var(VarId(10)), Atom::Var(VarId(11))],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(12)))),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![add_fn, main_fn],
        };
        let specializable = find_specializable(&module);
        assert!(specializable.is_empty());
    }

    #[test]
    fn specialization_with_constant_first_arg() {
        // fn f(mode, x) { ... }
        // fn main() { call @fn0(42, %10); call @fn0(42, %11) }
        // mode=42 is constant across all call sites.
        let f_fn = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![VarId(0), VarId(1)],
            body: Block {
                binds: vec![],
                tail: Box::new(Tail::Return(Atom::Var(VarId(1)))),
            },
        };
        let main_fn = FuncDef {
            id: FuncId(1),
            name: Some("main".to_string()),
            params: vec![VarId(10), VarId(11)],
            body: Block {
                binds: vec![
                    LetBind {
                        var: VarId(12),
                        rhs: Rhs::Call {
                            func: Atom::FuncRef(FuncId(0)),
                            args: vec![Atom::Int(42), Atom::Var(VarId(10))],
                        },
                    },
                    LetBind {
                        var: VarId(13),
                        rhs: Rhs::Call {
                            func: Atom::FuncRef(FuncId(0)),
                            args: vec![Atom::Int(42), Atom::Var(VarId(11))],
                        },
                    },
                ],
                tail: Box::new(Tail::Return(Atom::Var(VarId(13)))),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![f_fn, main_fn],
        };

        let specializable = find_specializable(&module);
        assert!(specializable.contains_key(&FuncId(0)));
        let args = &specializable[&FuncId(0)];
        assert!(matches!(args[0], Some(Atom::Int(42))));
        assert!(args[1].is_none()); // second arg varies
    }

    #[test]
    fn specialize_creates_specialized_copy() {
        // Same setup: f(mode, x) called with mode=42 at all sites.
        let f_fn = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![VarId(0), VarId(1)],
            body: Block {
                binds: vec![],
                tail: Box::new(Tail::Return(Atom::Var(VarId(1)))),
            },
        };
        let main_fn = FuncDef {
            id: FuncId(1),
            name: Some("main".to_string()),
            params: vec![VarId(10)],
            body: Block {
                binds: vec![LetBind {
                    var: VarId(12),
                    rhs: Rhs::Call {
                        func: Atom::FuncRef(FuncId(0)),
                        args: vec![Atom::Int(42), Atom::Var(VarId(10))],
                    },
                }],
                tail: Box::new(Tail::Return(Atom::Var(VarId(12)))),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![f_fn, main_fn],
        };

        let result = specialize(&module);
        // Should have 3 functions: original f, main, and specialized f$spec.
        assert_eq!(result.funcs.len(), 3);

        // The specialized function should have 1 param (x), not 2.
        let spec_fn = &result.funcs[2];
        assert_eq!(spec_fn.params.len(), 1);
        assert!(spec_fn.name.as_ref().unwrap().contains("spec"));

        // main's call should now reference the specialized function with only 1 arg.
        let main = &result.funcs[1];
        let call_bind = &main.body.binds[0];
        match &call_bind.rhs {
            Rhs::Call { func: Atom::FuncRef(fid), args } => {
                assert_eq!(*fid, spec_fn.id);
                assert_eq!(args.len(), 1); // only the non-constant arg
            }
            _ => panic!("expected rewritten call"),
        }
    }

    #[test]
    fn no_specialize_when_different_constants() {
        // fn f(mode) { return mode }
        // call @fn0(1); call @fn0(2) — different constants.
        let f_fn = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![VarId(0)],
            body: Block {
                binds: vec![],
                tail: Box::new(Tail::Return(Atom::Var(VarId(0)))),
            },
        };
        let main_fn = FuncDef {
            id: FuncId(1),
            name: Some("main".to_string()),
            params: vec![],
            body: Block {
                binds: vec![
                    LetBind {
                        var: VarId(10),
                        rhs: Rhs::Call {
                            func: Atom::FuncRef(FuncId(0)),
                            args: vec![Atom::Int(1)],
                        },
                    },
                    LetBind {
                        var: VarId(11),
                        rhs: Rhs::Call {
                            func: Atom::FuncRef(FuncId(0)),
                            args: vec![Atom::Int(2)],
                        },
                    },
                ],
                tail: Box::new(Tail::Return(Atom::Var(VarId(11)))),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![f_fn, main_fn],
        };

        let specializable = find_specializable(&module);
        assert!(specializable.is_empty());
    }
}
