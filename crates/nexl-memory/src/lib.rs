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

use nexl_ir::{LetBind, Tail, VarId};

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

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nexl_ir::{Atom, Rhs};

    // ─── 1. Layout constants are sane ─────────────────────────────────────
    #[test]
    fn rc_constants_sane() {
        assert_eq!(RC_HEADER_BYTES, 8);
        assert_eq!(RC_COUNT_OFFSET, 0);
        assert_eq!(DATA_OFFSET, 8);
        // Content starts exactly one i64 after the header.
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
}
