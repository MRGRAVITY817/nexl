//! WASM GC backend — emits WASM modules using GC types (struct, array, ref)
//! instead of linear memory for heap allocations.
//!
//! When WASM GC is enabled, closures and ADT values become GC-managed struct
//! types rather than bump-allocated linear memory blocks. This allows the host
//! runtime (browser V8, Wasmtime with GC) to manage object lifetimes.
//!
//! # GC type mapping
//!
//! - **Closures** → `(struct (field funcref) (field (ref $cap_type)))` where
//!   `$cap_type` is a struct of captured values.
//! - **ADT (MakeTuple)** → `(struct (field i32 /* tag */) (field i64)...)` for
//!   each constructor's fields.
//! - **All other values** remain `i64` as before.

use std::collections::{HashMap, HashSet};

use nexl_ir::{Block, Module, Rhs, Tail};

/// Configuration for the GC backend.
#[derive(Debug, Clone)]
pub struct GcConfig {
    /// Use GC types for closures (struct-based, no linear memory).
    pub gc_closures: bool,
    /// Use GC types for ADT values (struct-based, no linear memory).
    pub gc_tuples: bool,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            gc_closures: true,
            gc_tuples: true,
        }
    }
}

/// Collect all constructor names and their maximum field counts used in the module.
/// This determines which GC struct types to declare.
pub fn collect_ctor_shapes(module: &Module) -> HashMap<String, usize> {
    let mut shapes: HashMap<String, usize> = HashMap::new();
    for func in &module.funcs {
        collect_ctor_shapes_block(&func.body, &mut shapes);
    }
    shapes
}

fn collect_ctor_shapes_block(block: &Block, shapes: &mut HashMap<String, usize>) {
    for bind in &block.binds {
        if let Rhs::MakeTuple { ctor, fields } = &bind.rhs {
            let entry = shapes.entry(ctor.clone()).or_insert(0);
            *entry = (*entry).max(fields.len());
        }
    }
    collect_ctor_shapes_tail(&block.tail, shapes);
}

fn collect_ctor_shapes_tail(tail: &Tail, shapes: &mut HashMap<String, usize>) {
    match tail {
        Tail::If {
            then_block,
            else_block,
            ..
        } => {
            collect_ctor_shapes_block(then_block, shapes);
            collect_ctor_shapes_block(else_block, shapes);
        }
        Tail::Match { arms, .. } => {
            for arm in arms {
                collect_ctor_shapes_block(&arm.body, shapes);
            }
        }
        Tail::Loop { body, .. } => {
            collect_ctor_shapes_block(body, shapes);
        }
        _ => {}
    }
}

/// Collect all closure shapes (number of captures) used in the module.
pub fn collect_closure_shapes(module: &Module) -> HashSet<usize> {
    let mut sizes = HashSet::new();
    for func in &module.funcs {
        collect_closure_shapes_block(&func.body, &mut sizes);
    }
    sizes
}

fn collect_closure_shapes_block(block: &Block, sizes: &mut HashSet<usize>) {
    for bind in &block.binds {
        if let Rhs::MakeClosure { captures, .. } = &bind.rhs {
            sizes.insert(captures.len());
        }
    }
    collect_closure_shapes_tail(&block.tail, sizes);
}

fn collect_closure_shapes_tail(tail: &Tail, sizes: &mut HashSet<usize>) {
    match tail {
        Tail::If {
            then_block,
            else_block,
            ..
        } => {
            collect_closure_shapes_block(then_block, sizes);
            collect_closure_shapes_block(else_block, sizes);
        }
        Tail::Match { arms, .. } => {
            for arm in arms {
                collect_closure_shapes_block(&arm.body, sizes);
            }
        }
        Tail::Loop { body, .. } => {
            collect_closure_shapes_block(body, sizes);
        }
        _ => {}
    }
}

/// Plan for GC type declarations. Produced before emission so the emitter
/// knows which type indices to use.
#[derive(Debug, Clone)]
pub struct GcTypePlan {
    /// For each constructor name, the WASM type index of its GC struct.
    pub ctor_type_indices: HashMap<String, u32>,
    /// For each closure capture count, the WASM type index of its GC struct.
    pub closure_type_indices: HashMap<usize, u32>,
    /// The total number of GC types declared (offset for function types).
    pub gc_type_count: u32,
}

/// Build a GC type plan for the module.
pub fn plan_gc_types(module: &Module) -> GcTypePlan {
    let ctor_shapes = collect_ctor_shapes(module);
    let closure_shapes = collect_closure_shapes(module);

    let mut next_idx = 0u32;
    let mut ctor_type_indices = HashMap::new();
    let mut closure_type_indices = HashMap::new();

    // Each constructor gets a struct type: (tag: i32, field0: i64, field1: i64, ...)
    for ctor in ctor_shapes.keys() {
        ctor_type_indices.insert(ctor.clone(), next_idx);
        next_idx += 1;
    }

    // Each closure shape gets a struct type: (func_id: i64, cap0: i64, cap1: i64, ...)
    for &cap_count in &closure_shapes {
        closure_type_indices.insert(cap_count, next_idx);
        next_idx += 1;
    }

    GcTypePlan {
        ctor_type_indices,
        closure_type_indices,
        gc_type_count: next_idx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexl_ir::{Atom, FuncDef, FuncId, LetBind, VarId};

    #[test]
    fn collect_ctor_shapes_finds_constructors() {
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![
                    LetBind {
                        var: VarId(0),
                        rhs: Rhs::MakeTuple {
                            ctor: "Some".to_string(),
                            fields: vec![Atom::Int(1)],
                        },
                    },
                    LetBind {
                        var: VarId(1),
                        rhs: Rhs::MakeTuple {
                            ctor: "Pair".to_string(),
                            fields: vec![Atom::Int(1), Atom::Int(2)],
                        },
                    },
                ],
                tail: Box::new(Tail::Return(Atom::Unit)),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![func],
        };
        let shapes = collect_ctor_shapes(&module);
        assert_eq!(shapes["Some"], 1);
        assert_eq!(shapes["Pair"], 2);
    }

    #[test]
    fn collect_closure_shapes_finds_captures() {
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
                            captures: vec![(VarId(10), Atom::Int(1))],
                        },
                    },
                    LetBind {
                        var: VarId(1),
                        rhs: Rhs::MakeClosure {
                            func_id: FuncId(2),
                            captures: vec![(VarId(10), Atom::Int(1)), (VarId(11), Atom::Int(2))],
                        },
                    },
                ],
                tail: Box::new(Tail::Return(Atom::Unit)),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![func],
        };
        let shapes = collect_closure_shapes(&module);
        assert!(shapes.contains(&1));
        assert!(shapes.contains(&2));
    }

    #[test]
    fn plan_gc_types_assigns_unique_indices() {
        let func = FuncDef {
            id: FuncId(0),
            name: Some("f".to_string()),
            params: vec![],
            body: Block {
                binds: vec![
                    LetBind {
                        var: VarId(0),
                        rhs: Rhs::MakeTuple {
                            ctor: "Some".to_string(),
                            fields: vec![Atom::Int(1)],
                        },
                    },
                    LetBind {
                        var: VarId(1),
                        rhs: Rhs::MakeClosure {
                            func_id: FuncId(1),
                            captures: vec![(VarId(10), Atom::Int(1))],
                        },
                    },
                ],
                tail: Box::new(Tail::Return(Atom::Unit)),
            },
        };
        let module = Module {
            name: "test".to_string(),
            funcs: vec![func],
        };
        let plan = plan_gc_types(&module);
        // 1 ctor + 1 closure shape = 2 gc types.
        assert_eq!(plan.gc_type_count, 2);
        assert!(plan.ctor_type_indices.contains_key("Some"));
        assert!(plan.closure_type_indices.contains_key(&1));
        // Indices must be different.
        let ctor_idx = plan.ctor_type_indices["Some"];
        let closure_idx = plan.closure_type_indices[&1];
        assert_ne!(ctor_idx, closure_idx);
    }

    #[test]
    fn empty_module_produces_empty_plan() {
        let module = Module {
            name: "test".to_string(),
            funcs: vec![],
        };
        let plan = plan_gc_types(&module);
        assert_eq!(plan.gc_type_count, 0);
        assert!(plan.ctor_type_indices.is_empty());
        assert!(plan.closure_type_indices.is_empty());
    }

    #[test]
    fn gc_config_defaults() {
        let config = GcConfig::default();
        assert!(config.gc_closures);
        assert!(config.gc_tuples);
    }
}
