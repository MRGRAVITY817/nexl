//! Evidence vectors as native arrays (spec §13.5).
//!
//! Every function that performs effects receives an implicit evidence vector.
//! The evidence vector holds handler records—one per effect in the function's
//! effect row. Each handler record contains function pointers for each operation.
//!
//! Representation:
//! - **Empty** (pure): no vector passed; optimized away.
//! - **Single effect**: a single pointer to the handler record.
//! - **Multiple effects**: a heap-allocated array of handler record pointers.

use crate::value;

/// Description of an evidence vector's shape for code generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceShape {
    /// Pure function: no evidence parameter needed.
    Empty,
    /// Single-effect function: one pointer (no array indirection).
    Single,
    /// Multi-effect function: an array of `n` handler record pointers.
    Array(usize),
}

impl EvidenceShape {
    /// Create the appropriate evidence shape for a given effect count.
    pub fn for_effects(count: usize) -> Self {
        match count {
            0 => EvidenceShape::Empty,
            1 => EvidenceShape::Single,
            n => EvidenceShape::Array(n),
        }
    }

    /// Number of extra parameters this evidence shape adds to a function signature.
    pub fn param_count(&self) -> usize {
        match self {
            EvidenceShape::Empty => 0,
            EvidenceShape::Single => 1,
            EvidenceShape::Array(_) => 1, // pointer to the array
        }
    }

    /// Size in bytes of the evidence array (0 for Empty/Single).
    pub fn array_size_bytes(&self) -> usize {
        match self {
            EvidenceShape::Empty | EvidenceShape::Single => 0,
            EvidenceShape::Array(n) => n * 8, // each slot is an i64 pointer
        }
    }
}

/// Layout of a handler record for one effect.
///
/// A handler record is a heap-allocated struct containing function pointers
/// for each operation of the effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerRecord {
    /// Number of operations in this effect.
    pub op_count: usize,
}

impl HandlerRecord {
    /// Create a handler record for an effect with `n` operations.
    pub fn new(op_count: usize) -> Self {
        Self { op_count }
    }

    /// Total size in bytes (header + refcount + op function pointers).
    pub fn size_bytes(&self) -> usize {
        // header + refcount + op_count function pointers
        (2 + self.op_count) * 8
    }

    /// Byte offset of the `i`-th operation's function pointer.
    pub fn op_offset(&self, i: usize) -> usize {
        assert!(i < self.op_count, "operation index out of range");
        // After header (8) + refcount (8)
        16 + i * 8
    }

    /// HeapHeader for this handler record.
    pub fn header(&self) -> value::HeapHeader {
        value::HeapHeader::new(value::HeapTag::Record, (2 + self.op_count) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evidence_shape_empty() {
        let shape = EvidenceShape::for_effects(0);
        assert_eq!(shape, EvidenceShape::Empty);
        assert_eq!(shape.param_count(), 0);
        assert_eq!(shape.array_size_bytes(), 0);
    }

    #[test]
    fn test_evidence_shape_single() {
        let shape = EvidenceShape::for_effects(1);
        assert_eq!(shape, EvidenceShape::Single);
        assert_eq!(shape.param_count(), 1);
        assert_eq!(shape.array_size_bytes(), 0);
    }

    #[test]
    fn test_evidence_shape_array() {
        let shape = EvidenceShape::for_effects(3);
        assert_eq!(shape, EvidenceShape::Array(3));
        assert_eq!(shape.param_count(), 1);
        assert_eq!(shape.array_size_bytes(), 24);
    }

    #[test]
    fn test_handler_record_layout() {
        // Console effect: 1 op (print)
        let hr = HandlerRecord::new(1);
        assert_eq!(hr.size_bytes(), 24); // header + rc + 1 op
        assert_eq!(hr.op_offset(0), 16);
    }

    #[test]
    fn test_handler_record_multiple_ops() {
        // Concurrent: 3 ops (fork, join, race)
        let hr = HandlerRecord::new(3);
        assert_eq!(hr.size_bytes(), 40); // header + rc + 3 ops
        assert_eq!(hr.op_offset(0), 16);
        assert_eq!(hr.op_offset(1), 24);
        assert_eq!(hr.op_offset(2), 32);
    }

    #[test]
    #[should_panic(expected = "operation index out of range")]
    fn test_handler_record_out_of_range() {
        HandlerRecord::new(2).op_offset(2);
    }

    #[test]
    fn test_handler_record_header() {
        let hr = HandlerRecord::new(2);
        let hdr = hr.header();
        assert_eq!(hdr.heap_tag(), value::HeapTag::Record);
        assert_eq!(hdr.field_count(), 4); // header + rc + 2 ops
    }
}
