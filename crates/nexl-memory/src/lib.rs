//! Perceus reference-counting data structures for the Nexl compiler.
//!
//! This crate defines the RC header layout constants and the types used by
//! the dup/drop insertion pass ([`RcOp`], [`RcStep`], [`RcAnnotatedBlock`]).
//!
//! # Memory layout
//!
//! Every heap-allocated Nexl value (ADT, closure, string) has an RC header
//! prepended to its content:
//!
//! ```text
//! offset  0: [rc_count: i64]       ← Perceus ref-count
//! offset  8: [tag_or_func_id: i64] ← discriminant / code pointer
//! offset 16: [field_0: i64]        ← first field / capture
//!            …
//! ```
//!
//! Pipeline position: nexl-ir → **nexl-memory** (dup/drop pass) → nexl-wasm

use std::collections::HashMap;

use nexl_ir::{Atom, Block, FuncDef, LetBind, Module, Rhs, Tail, VarId};

// ── Layout constants ─────────────────────────────────────────────────────────

/// Size in bytes of the Perceus reference-count header prepended to every
/// heap-allocated value.
pub const RC_HEADER_BYTES: u32 = 8;

/// Byte offset of the RC count field from the start of an allocation.
pub const RC_COUNT_OFFSET: u64 = 0;

/// Byte offset at which the value's content (tag / data) begins, i.e.
/// immediately after the RC header.
pub const DATA_OFFSET: u64 = 8;

// ── RC operation ─────────────────────────────────────────────────────────────

/// A Perceus reference-count operation on a single heap-allocated value.
///
/// `Dup` and `Drop` are inserted around value uses by the
/// dup/drop insertion pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RcOp {
    /// Increment the reference count of the value stored in `var`.
    Dup { var: VarId },
    /// Decrement the reference count of the value stored in `var`.
    /// When the count reaches zero the allocation is freed.
    Drop { var: VarId },
}

// ── RC-annotated IR ──────────────────────────────────────────────────────────

/// One step in an [`RcAnnotatedBlock`]: either an RC operation or a
/// let-binding from the original ANF IR.
#[derive(Debug)]
pub enum RcStep {
    /// A reference-count operation inserted by the dup/drop pass.
    Rc(RcOp),
    /// An original let-binding from the ANF IR.
    Bind(LetBind),
}

/// An ANF block annotated with Perceus RC operations.
///
/// Produced by the dup/drop insertion pass; consumed by the WASM code
/// generator to emit ref-count updates around each use.
#[derive(Debug)]
pub struct RcAnnotatedBlock {
    /// Interleaved sequence of RC operations and let-bindings.
    pub steps: Vec<RcStep>,
    /// The final tail expression (unchanged from the original block).
    pub tail: Box<Tail>,
}

/// A function annotated with Perceus RC operations.
#[derive(Debug)]
pub struct RcAnnotatedFunc {
    /// Original function metadata (id, name, params).
    pub id: nexl_ir::FuncId,
    /// Human-readable name (absent for anonymous closures).
    pub name: Option<String>,
    /// Parameter variable IDs in declaration order.
    pub params: Vec<VarId>,
    /// The annotated function body.
    pub body: RcAnnotatedBlock,
}

/// A module annotated with Perceus RC operations.
#[derive(Debug)]
pub struct RcAnnotatedModule {
    /// Module name.
    pub name: String,
    /// All annotated function definitions.
    pub funcs: Vec<RcAnnotatedFunc>,
}

// ── Dup/drop insertion pass ───────────────────────────────────────────────────

/// Inserts Perceus dup/drop operations into an ANF [`Module`].
///
/// # Strategy (first pass — conservative)
///
/// * Values directly bound to [`Rhs::MakeTuple`] or [`Rhs::MakeClosure`] are
///   considered heap-allocated.
/// * A `Drop` is inserted at the end of a block for each heap-allocated
///   variable that is **not** the value returned by `Tail::Return(Atom::Var(v))`.
/// * No `Dup` is emitted in this first pass (all values assumed uniquely owned).
pub struct DupDropPass;

impl DupDropPass {
    /// Create a new pass instance.
    pub fn new() -> Self {
        DupDropPass
    }

    /// Annotate `module` with dup/drop operations.
    pub fn run(&self, module: &Module) -> RcAnnotatedModule {
        let funcs = module.funcs.iter().map(|f| self.annotate_func(f)).collect();
        RcAnnotatedModule { name: module.name.clone(), funcs }
    }

    fn annotate_func(&self, func: &FuncDef) -> RcAnnotatedFunc {
        RcAnnotatedFunc {
            id: func.id,
            name: func.name.clone(),
            params: func.params.clone(),
            body: annotate_block(&func.body),
        }
    }
}

impl Default for DupDropPass {
    fn default() -> Self {
        DupDropPass
    }
}

/// Annotate a single [`Block`] by inserting `Drop` ops after the last
/// bind for heap-allocated variables that are not returned.
fn annotate_block(block: &Block) -> RcAnnotatedBlock {
    // Collect heap-allocated variables in bind order.
    let mut heap_vars: Vec<VarId> = vec![];
    let mut steps: Vec<RcStep> = vec![];

    for bind in &block.binds {
        if is_heap_alloc(&bind.rhs) {
            heap_vars.push(bind.var);
        }
        steps.push(RcStep::Bind(bind.clone()));
    }

    // Determine which variable (if any) is directly returned.
    let returned = match block.tail.as_ref() {
        Tail::Return(Atom::Var(v)) => Some(*v),
        _ => None,
    };

    // Drop heap-allocated vars that are not the return value.
    for var in heap_vars {
        if Some(var) != returned {
            steps.push(RcStep::Rc(RcOp::Drop { var }));
        }
    }

    RcAnnotatedBlock { steps, tail: Box::new(block.tail.as_ref().clone()) }
}

/// Returns `true` if `rhs` directly allocates a new heap value.
fn is_heap_alloc(rhs: &Rhs) -> bool {
    matches!(rhs, Rhs::MakeTuple { .. } | Rhs::MakeClosure { .. })
}

// ── Reuse analysis ───────────────────────────────────────────────────────────

/// A reuse token: a dropped allocation whose memory slot can be repurposed for
/// a subsequent allocation of the same size.
///
/// When a new [`RcStep::Bind`] creates a heap value of size `slot_count` and a
/// matching token is available, the new allocation can write in-place into the
/// dropped slot instead of calling the bump allocator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReuseToken {
    /// Variable whose slot is available for reuse.
    pub dropped_var: VarId,
    /// Number of `i64` words in the allocation (including header slot).
    pub slot_count: usize,
}

/// Maps each new-allocation variable to the reuse token it should consume.
///
/// Produced by [`ReusePass`] and consumed by the WASM code generator to
/// emit in-place mutation instead of bump-allocating a new object.
pub type ReuseMap = HashMap<VarId, ReuseToken>;

/// Identifies uniquely-owned allocations that can be mutated in-place.
///
/// Scans each [`RcAnnotatedBlock`] for the pattern:
/// `Drop(dead_var)` → `Bind(new_var = MakeTuple/MakeClosure)`
/// where both sides have the same `slot_count`.  When found, records
/// `new_var → ReuseToken { dropped_var: dead_var, slot_count }` in the map.
pub struct ReusePass;

impl ReusePass {
    /// Create a new reuse-analysis pass.
    pub fn new() -> Self {
        ReusePass
    }

    /// Analyse `module` and return a [`ReuseMap`] of reusable allocations.
    pub fn run(&self, module: &RcAnnotatedModule) -> ReuseMap {
        let mut map = ReuseMap::new();
        for func in &module.funcs {
            analyze_block_for_reuse(&func.body, &mut map);
        }
        map
    }
}

impl Default for ReusePass {
    fn default() -> Self {
        ReusePass
    }
}

/// Returns the number of `i64` allocation slots for a heap-allocating rhs,
/// or `None` if it does not heap-allocate.
///
/// * `MakeTuple { fields }` → `1 + fields.len()` (1 for the tag word)
/// * `MakeClosure { captures }` → `1 + captures.len()` (1 for the func_id word)
fn alloc_slot_count(rhs: &Rhs) -> Option<usize> {
    match rhs {
        Rhs::MakeTuple { fields, .. } => Some(1 + fields.len()),
        Rhs::MakeClosure { captures, .. } => Some(1 + captures.len()),
        _ => None,
    }
}

fn analyze_block_for_reuse(block: &RcAnnotatedBlock, map: &mut ReuseMap) {
    // Pre-compute slot counts for all bound variables in this block.
    let mut slot_counts: HashMap<VarId, usize> = HashMap::new();
    for step in &block.steps {
        if let RcStep::Bind(bind) = step
            && let Some(n) = alloc_slot_count(&bind.rhs)
        {
            slot_counts.insert(bind.var, n);
        }
    }

    // Scan for Drop → Bind(heap) pattern.
    let mut available: Vec<ReuseToken> = vec![];
    for step in &block.steps {
        match step {
            RcStep::Rc(RcOp::Drop { var }) => {
                if let Some(&n) = slot_counts.get(var) {
                    available.push(ReuseToken { dropped_var: *var, slot_count: n });
                }
            }
            RcStep::Bind(bind) => {
                if let Some(needed) = alloc_slot_count(&bind.rhs)
                    && let Some(pos) = available.iter().position(|t| t.slot_count == needed)
                {
                    let token = available.remove(pos);
                    map.insert(bind.var, token);
                }
            }
            RcStep::Rc(RcOp::Dup { .. }) => {}
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nexl_ir::{Atom, FuncId, LetBind, Rhs, Tail};

    // ─── 1. Layout constants are sane ─────────────────────────────────────
    #[test]
    fn rc_constants_sane() {
        assert_eq!(RC_HEADER_BYTES, 8);
        assert_eq!(RC_COUNT_OFFSET, 0);
        assert_eq!(DATA_OFFSET, 8);
        assert_eq!(DATA_OFFSET, RC_COUNT_OFFSET + RC_HEADER_BYTES as u64);
    }

    // ─── 2. RcOp::Dup construction ────────────────────────────────────────
    #[test]
    fn rc_op_dup() {
        let op = RcOp::Dup { var: VarId(0) };
        assert!(matches!(op, RcOp::Dup { var: VarId(0) }));
    }

    // ─── 3. RcOp::Drop construction ───────────────────────────────────────
    #[test]
    fn rc_op_drop() {
        let op = RcOp::Drop { var: VarId(0) };
        assert!(matches!(op, RcOp::Drop { var: VarId(0) }));
    }

    // ─── 4. RcStep::Rc wraps RcOp ─────────────────────────────────────────
    #[test]
    fn rc_step_rc() {
        let step = RcStep::Rc(RcOp::Dup { var: VarId(1) });
        let RcStep::Rc(RcOp::Dup { var }) = step else {
            panic!("expected RcStep::Rc(RcOp::Dup)")
        };
        assert_eq!(var, VarId(1));
    }

    // ─── 5. RcStep::Bind wraps LetBind ────────────────────────────────────
    #[test]
    fn rc_step_bind() {
        let bind = LetBind { var: VarId(2), rhs: Rhs::Atom(Atom::Int(42)) };
        let step = RcStep::Bind(bind);
        let RcStep::Bind(b) = step else { panic!("expected RcStep::Bind") };
        assert_eq!(b.var, VarId(2));
        assert!(matches!(b.rhs, Rhs::Atom(Atom::Int(42))));
    }

    // ─── 6. RcAnnotatedBlock with empty steps ─────────────────────────────
    #[test]
    fn rc_annotated_block_empty() {
        let block = RcAnnotatedBlock {
            steps: vec![],
            tail: Box::new(Tail::Return(Atom::Unit)),
        };
        assert!(block.steps.is_empty());
        assert!(matches!(*block.tail, Tail::Return(Atom::Unit)));
    }

    // ─── helpers for pass tests ───────────────────────────────────────────

    fn make_block(binds: Vec<LetBind>, tail: Tail) -> Block {
        Block { binds, tail: Box::new(tail) }
    }

    fn make_func(block: Block) -> FuncDef {
        FuncDef { id: FuncId(0), name: Some("f".to_string()), params: vec![], body: block }
    }

    fn make_module(block: Block) -> Module {
        Module { name: "test".to_string(), funcs: vec![make_func(block)] }
    }

    // ─── 7. No RC ops for int-only binds ─────────────────────────────────
    #[test]
    fn pass_no_ops_for_int_return() {
        // Block: x = Int(42); Return(Var(x)) — no heap allocations
        let x = VarId(0);
        let block = make_block(
            vec![LetBind { var: x, rhs: Rhs::Atom(Atom::Int(42)) }],
            Tail::Return(Atom::Var(x)),
        );
        let m = make_module(block);
        let annotated = DupDropPass::new().run(&m);
        let body = &annotated.funcs[0].body;
        // 1 Bind step, 0 Rc steps.
        assert_eq!(body.steps.len(), 1, "only 1 step (the bind)");
        assert!(matches!(body.steps[0], RcStep::Bind(_)));
    }

    // ─── 8. Drop inserted for ADT not returned ────────────────────────────
    #[test]
    fn pass_drop_for_adt_not_returned() {
        // Block: v = MakeTuple("None", []); Return(Int(0)) — v is heap, not returned
        let v = VarId(0);
        let block = make_block(
            vec![LetBind { var: v, rhs: Rhs::MakeTuple { ctor: "None".to_string(), fields: vec![] } }],
            Tail::Return(Atom::Int(0)),
        );
        let m = make_module(block);
        let annotated = DupDropPass::new().run(&m);
        let body = &annotated.funcs[0].body;
        // 1 Bind + 1 Rc(Drop).
        assert_eq!(body.steps.len(), 2);
        assert!(matches!(body.steps[0], RcStep::Bind(_)));
        assert!(matches!(&body.steps[1], RcStep::Rc(RcOp::Drop { var }) if *var == v));
    }

    // ─── 9. No Drop when ADT is the return value ─────────────────────────
    #[test]
    fn pass_no_drop_for_returned_adt() {
        // Block: v = MakeTuple("Some", [Int(1)]); Return(Var(v)) — ownership transferred
        let v = VarId(0);
        let block = make_block(
            vec![LetBind {
                var: v,
                rhs: Rhs::MakeTuple { ctor: "Some".to_string(), fields: vec![Atom::Int(1)] },
            }],
            Tail::Return(Atom::Var(v)),
        );
        let m = make_module(block);
        let annotated = DupDropPass::new().run(&m);
        let body = &annotated.funcs[0].body;
        // Only 1 step (the bind); no Drop because v is returned.
        assert_eq!(body.steps.len(), 1);
        assert!(matches!(body.steps[0], RcStep::Bind(_)));
    }

    // ─── helper: manually build an RcAnnotatedBlock ───────────────────────

    fn make_annotated_block(steps: Vec<RcStep>, tail: Tail) -> RcAnnotatedBlock {
        RcAnnotatedBlock { steps, tail: Box::new(tail) }
    }

    fn make_annotated_module(block: RcAnnotatedBlock) -> RcAnnotatedModule {
        use nexl_ir::FuncId;
        RcAnnotatedModule {
            name: "test".to_string(),
            funcs: vec![RcAnnotatedFunc {
                id: FuncId(0),
                name: Some("f".to_string()),
                params: vec![],
                body: block,
            }],
        }
    }

    // ─── 10. Reuse token for same-shape ADT after drop ────────────────────
    #[test]
    fn reuse_identifies_same_shape_adt() {
        // Steps: Bind(v1=None), Drop(v1), Bind(v2=None)
        // v2 should reuse v1's slot (both 1 word: tag only).
        let v1 = VarId(0);
        let v2 = VarId(1);
        let block = make_annotated_block(
            vec![
                RcStep::Bind(LetBind {
                    var: v1,
                    rhs: Rhs::MakeTuple { ctor: "None".to_string(), fields: vec![] },
                }),
                RcStep::Rc(RcOp::Drop { var: v1 }),
                RcStep::Bind(LetBind {
                    var: v2,
                    rhs: Rhs::MakeTuple { ctor: "None".to_string(), fields: vec![] },
                }),
            ],
            Tail::Return(Atom::Var(v2)),
        );
        let m = make_annotated_module(block);
        let reuse_map = ReusePass::new().run(&m);
        let token = reuse_map.get(&v2).expect("v2 should have a reuse token");
        assert_eq!(token.dropped_var, v1);
        assert_eq!(token.slot_count, 1); // tag only; None has 0 fields
    }

    // ─── 11. No reuse when sizes differ ──────────────────────────────────
    #[test]
    fn reuse_no_match_different_sizes() {
        // Steps: Bind(v1=Some(1)), Drop(v1), Bind(v2=None)
        // v1 has 2 slots (tag + 1 field); v2 has 1 slot — no match.
        let v1 = VarId(0);
        let v2 = VarId(1);
        let block = make_annotated_block(
            vec![
                RcStep::Bind(LetBind {
                    var: v1,
                    rhs: Rhs::MakeTuple {
                        ctor: "Some".to_string(),
                        fields: vec![Atom::Int(1)],
                    },
                }),
                RcStep::Rc(RcOp::Drop { var: v1 }),
                RcStep::Bind(LetBind {
                    var: v2,
                    rhs: Rhs::MakeTuple { ctor: "None".to_string(), fields: vec![] },
                }),
            ],
            Tail::Return(Atom::Var(v2)),
        );
        let m = make_annotated_module(block);
        let reuse_map = ReusePass::new().run(&m);
        assert!(!reuse_map.contains_key(&v2), "different sizes → no reuse");
    }

    // ─── 12. No reuse when there is no preceding drop ─────────────────────
    #[test]
    fn reuse_no_token_without_drop() {
        // Steps: Bind(v1=None) — no Drop before it.
        let v1 = VarId(0);
        let block = make_annotated_block(
            vec![RcStep::Bind(LetBind {
                var: v1,
                rhs: Rhs::MakeTuple { ctor: "None".to_string(), fields: vec![] },
            })],
            Tail::Return(Atom::Var(v1)),
        );
        let m = make_annotated_module(block);
        let reuse_map = ReusePass::new().run(&m);
        assert!(reuse_map.is_empty(), "no drop → no reuse token");
    }
}
