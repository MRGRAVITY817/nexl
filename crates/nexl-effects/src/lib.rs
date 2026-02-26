//! Evidence passing representation for Nexl algebraic effects.
//!
//! Implements the evidence vector mechanism described in spec §13.5.
//! Every effectful function receives an implicit evidence vector containing
//! one [`HandlerRecord`] per effect in its effect row.  Effect operations
//! are dispatched by looking up `(effect_name, op_name)` in the vector.
//!
//! The representation is parameterised over `F`, the operation function type,
//! so it can be used both by the tree-walk evaluator (where
//! `F = Rc<dyn Fn(&[Value]) -> Value>`) and by future compilation backends.

pub mod builtin;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

const RESUME_PANIC_MSG: &str = "resume called more than once";

// ---------------------------------------------------------------------------
// Resume (one-shot)
// ---------------------------------------------------------------------------

/// One-shot continuation handle passed to continuation-style handlers.
///
/// Calling `resume` more than once is a runtime panic (ADR-003).
#[derive(Debug)]
pub struct Resume<F> {
    called: Cell<bool>,
    f: RefCell<Option<F>>,
}

impl<F> Resume<F> {
    /// Create a new one-shot resume handle.
    pub fn new(f: F) -> Self {
        Self {
            called: Cell::new(false),
            f: RefCell::new(Some(f)),
        }
    }

    /// Invoke the continuation exactly once.
    ///
    /// # Panics
    /// Panics if called more than once.
    pub fn call<Args, R>(&self, args: Args) -> R
    where
        F: FnOnce(Args) -> R,
    {
        if self.called.get() {
            panic!("{RESUME_PANIC_MSG}");
        }
        self.called.set(true);
        let f = {
            let mut slot = self.f.borrow_mut();
            slot.take()
        };
        match f {
            Some(f) => f(args),
            None => panic!("{RESUME_PANIC_MSG}"),
        }
    }
}

// ---------------------------------------------------------------------------
// HandlerRecord
// ---------------------------------------------------------------------------

/// An operation handler implementation.
#[derive(Debug, Clone, PartialEq)]
pub enum HandlerOp<F> {
    /// Simple (tail-resumptive) handler; resume is implicit.
    Simple(F),
    /// Continuation handler; resume is explicit.
    Continuation(F),
}

/// Handler record for a single effect.
///
/// Installed by a `handle` form; maps each operation name to its compiled
/// or interpreted implementation of type `F`.
#[derive(Debug, Clone)]
pub struct HandlerRecord<F> {
    /// Effect name, e.g. `"Console"`.
    pub effect_name: String,
    operations: HashMap<String, HandlerOp<F>>,
}

impl<F> HandlerRecord<F> {
    /// Create an empty handler record for the given effect.
    pub fn new(effect_name: impl Into<String>) -> Self {
        Self {
            effect_name: effect_name.into(),
            operations: HashMap::new(),
        }
    }

    /// Register a simple (tail-resumptive) operation implementation.
    pub fn insert_op(&mut self, op_name: impl Into<String>, handler: F) {
        self.operations
            .insert(op_name.into(), HandlerOp::Simple(handler));
    }

    /// Register a continuation-style operation implementation.
    pub fn insert_continuation_op(&mut self, op_name: impl Into<String>, handler: F) {
        self.operations
            .insert(op_name.into(), HandlerOp::Continuation(handler));
    }

    /// Look up an operation implementation by name.
    pub fn lookup(&self, op_name: &str) -> Option<&HandlerOp<F>> {
        self.operations.get(op_name)
    }
}

// ---------------------------------------------------------------------------
// EvidenceVector
// ---------------------------------------------------------------------------

/// Evidence vector — the implicit parameter passed to effectful functions.
///
/// Optimised for the common cases (spec §13.5.1):
/// - [`EvidenceVector::Empty`]: pure functions receive no evidence.
/// - [`EvidenceVector::Single`]: single-effect functions carry a direct
///   handler record with no array indirection.
/// - [`EvidenceVector::Multi`]: multi-effect functions carry a small array
///   ordered from outermost (index 0) to innermost (last).  Lookup searches
///   from the innermost handler first so that nested `handle` forms shadow
///   outer ones (spec §6.8).
#[derive(Debug, Clone)]
pub enum EvidenceVector<F> {
    /// Pure — no effects, no evidence needed.
    Empty,
    /// Single effect — direct handler record, no array overhead.
    Single(HandlerRecord<F>),
    /// Multiple effects — ordered outermost-first; innermost searched first.
    Multi(Vec<HandlerRecord<F>>),
}

impl<F: Clone> EvidenceVector<F> {
    /// Create an empty (pure) evidence vector.
    pub fn empty() -> Self {
        Self::Empty
    }

    /// Create a single-effect evidence vector.
    pub fn single(record: HandlerRecord<F>) -> Self {
        Self::Single(record)
    }

    /// Build from a list of handler records, choosing the optimal variant.
    ///
    /// Records are ordered outermost-first; the last record in the slice is
    /// treated as the innermost handler.
    pub fn from_records(records: Vec<HandlerRecord<F>>) -> Self {
        match records.len() {
            0 => Self::Empty,
            1 => Self::Single(records.into_iter().next().expect("len == 1")),
            _ => Self::Multi(records),
        }
    }

    /// Look up the innermost handler for `(effect_name, op_name)`.
    ///
    /// Searches from the most recently installed handler (innermost `handle`)
    /// to the earliest (outermost), returning the first match.
    pub fn lookup(&self, effect_name: &str, op_name: &str) -> Option<&HandlerOp<F>> {
        match self {
            Self::Empty => None,
            Self::Single(record) => {
                if record.effect_name == effect_name {
                    record.lookup(op_name)
                } else {
                    None
                }
            }
            Self::Multi(records) => records
                .iter()
                .rev()
                .find(|r| r.effect_name == effect_name)
                .and_then(|r| r.lookup(op_name)),
        }
    }

    /// Extend the evidence vector with a new handler record.
    ///
    /// The new record becomes the innermost handler and shadows any existing
    /// record for the same effect.  Used when entering a `handle` form.
    pub fn extend(&self, record: HandlerRecord<F>) -> Self {
        match self {
            Self::Empty => Self::Single(record),
            Self::Single(existing) => Self::Multi(vec![existing.clone(), record]),
            Self::Multi(records) => {
                let mut new_records = records.clone();
                new_records.push(record);
                Self::Multi(new_records)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{EvidenceVector, HandlerOp, HandlerRecord, Resume};

    // -- Test 1 --
    #[test]
    fn handler_record_new_is_empty() {
        let rec: HandlerRecord<i32> = HandlerRecord::new("Console");
        assert_eq!(rec.effect_name, "Console");
        assert!(rec.lookup("print").is_none());
    }

    // -- Test 2 --
    #[test]
    fn handler_record_insert_and_lookup() {
        let mut rec = HandlerRecord::new("Console");
        rec.insert_op("print", 42_i32);
        assert_eq!(rec.lookup("print"), Some(&HandlerOp::Simple(42_i32)));
    }

    // -- Test 3 --
    #[test]
    fn handler_record_lookup_missing_op() {
        let mut rec: HandlerRecord<i32> = HandlerRecord::new("Console");
        rec.insert_op("print", 1);
        assert!(rec.lookup("read-line").is_none());
    }

    // -- Test 4 --
    #[test]
    fn handler_record_insert_continuation_op() {
        let mut rec = HandlerRecord::new("Console");
        rec.insert_continuation_op("print", 7_i32);
        assert_eq!(
            rec.lookup("print"),
            Some(&HandlerOp::Continuation(7_i32))
        );
    }

    // -- Test 4 --
    #[test]
    fn evidence_empty_lookup_returns_none() {
        let ev: EvidenceVector<i32> = EvidenceVector::empty();
        assert!(ev.lookup("Console", "print").is_none());
        assert!(ev.lookup("Net", "get").is_none());
    }

    // -- Test 5: single-effect direct-pointer path (spec §13.5.1) --
    #[test]
    fn evidence_single_lookup_found() {
        let mut rec = HandlerRecord::new("Console");
        rec.insert_op("print", 99_i32);
        let ev = EvidenceVector::single(rec);
        assert_eq!(
            ev.lookup("Console", "print"),
            Some(&HandlerOp::Simple(99_i32))
        );
    }

    // -- Test 6 --
    #[test]
    fn evidence_single_wrong_effect_returns_none() {
        let mut rec = HandlerRecord::new("Console");
        rec.insert_op("print", 1_i32);
        let ev = EvidenceVector::single(rec);
        assert!(ev.lookup("Net", "get").is_none());
    }

    // -- Test 7 --
    #[test]
    fn evidence_multi_lookup_first_effect() {
        let mut console = HandlerRecord::new("Console");
        console.insert_op("print", 1_i32);
        let mut net = HandlerRecord::new("Net");
        net.insert_op("get", 2_i32);
        let ev = EvidenceVector::from_records(vec![console, net]);
        assert_eq!(
            ev.lookup("Console", "print"),
            Some(&HandlerOp::Simple(1_i32))
        );
    }

    // -- Test 8 --
    #[test]
    fn evidence_multi_lookup_second_effect() {
        let mut console = HandlerRecord::new("Console");
        console.insert_op("print", 1_i32);
        let mut net = HandlerRecord::new("Net");
        net.insert_op("get", 2_i32);
        let ev = EvidenceVector::from_records(vec![console, net]);
        assert_eq!(
            ev.lookup("Net", "get"),
            Some(&HandlerOp::Simple(2_i32))
        );
    }

    // -- Test 9 --
    #[test]
    fn evidence_extend_empty_gives_single() {
        let ev: EvidenceVector<i32> = EvidenceVector::empty();
        let mut rec = HandlerRecord::new("Console");
        rec.insert_op("print", 1);
        let ev2 = ev.extend(rec);
        assert!(matches!(ev2, EvidenceVector::Single(_)));
    }

    // -- Test 10 --
    #[test]
    fn evidence_extend_single_gives_multi() {
        let mut rec1 = HandlerRecord::new("Console");
        rec1.insert_op("print", 1_i32);
        let ev = EvidenceVector::single(rec1);
        let mut rec2 = HandlerRecord::new("Net");
        rec2.insert_op("get", 2_i32);
        let ev2 = ev.extend(rec2);
        assert!(matches!(ev2, EvidenceVector::Multi(_)));
    }

    // -- Test 11: innermost handler wins (spec §6.8) --
    #[test]
    fn evidence_innermost_handler_wins() {
        // Outer Console handler: print → 10
        let mut outer = HandlerRecord::new("Console");
        outer.insert_op("print", 10_i32);
        // Inner Console handler: print → 20 (should shadow outer)
        let mut inner = HandlerRecord::new("Console");
        inner.insert_op("print", 20_i32);
        // Build: outer first, then extend with inner
        let ev = EvidenceVector::single(outer).extend(inner);
        assert_eq!(
            ev.lookup("Console", "print"),
            Some(&HandlerOp::Simple(20_i32)),
            "innermost (last added) handler must shadow the outer one"
        );
    }

    // -- Test 12 --
    #[test]
    fn evidence_from_records_empty() {
        let ev: EvidenceVector<i32> = EvidenceVector::from_records(vec![]);
        assert!(matches!(ev, EvidenceVector::Empty));
    }

    // -- Test 13 --
    #[test]
    fn evidence_from_records_single() {
        let rec: HandlerRecord<i32> = HandlerRecord::new("Log");
        let ev = EvidenceVector::from_records(vec![rec]);
        assert!(matches!(ev, EvidenceVector::Single(_)));
    }

    // -- Test 14 --
    #[test]
    fn resume_call_once_returns_value() {
        let resume = Resume::new(|n: i32| n + 1);
        let value = resume.call(41);
        assert_eq!(value, 42);
    }

    // -- Test 15 --
    #[test]
    #[should_panic(expected = "resume called more than once")]
    fn resume_call_twice_panics() {
        let resume = Resume::new(|n: i32| n + 1);
        let _ = resume.call(1);
        let _ = resume.call(2);
    }
}
