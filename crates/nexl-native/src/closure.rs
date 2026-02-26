//! Closure representation for the native backend (spec §13.2).
//!
//! A closure is a heap-allocated struct:
//! ```text
//! [header: i64] [refcount: i64] [code_ptr: i64] [arity: i64] [cap_0: i64] ...
//! ```
//! The header is a [`HeapHeader`](crate::value::HeapHeader) with tag `Closure`.
//! All slots are 8 bytes (i64 tagged values).

use crate::rc;

/// Layout description for a closure with `n` captures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureLayout {
    /// Number of captured variables.
    pub capture_count: usize,
}

impl ClosureLayout {
    /// Create a layout for a closure capturing `n` variables.
    pub fn new(capture_count: usize) -> Self {
        Self { capture_count }
    }

    /// Total size in bytes of the closure struct (including header and refcount).
    pub fn size_bytes(&self) -> usize {
        // header + refcount + code_ptr + arity + captures
        (4 + self.capture_count) * 8
    }

    /// Byte offset of the code pointer field.
    pub fn code_ptr_offset(&self) -> usize {
        rc::FIELDS_OFFSET as usize // after header + refcount
    }

    /// Byte offset of the arity field.
    pub fn arity_offset(&self) -> usize {
        rc::FIELDS_OFFSET as usize + 8 // after header + refcount + code_ptr
    }

    /// Byte offset of the `i`-th capture slot.
    pub fn capture_offset(&self, i: usize) -> usize {
        assert!(i < self.capture_count, "capture index out of range");
        rc::FIELDS_OFFSET as usize + 16 + i * 8 // after header + refcount + code_ptr + arity
    }

    /// Total number of i64 fields (including header and refcount).
    pub fn field_count(&self) -> u32 {
        (4 + self.capture_count) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_closure_layout_no_captures() {
        let layout = ClosureLayout::new(0);
        assert_eq!(layout.size_bytes(), 32); // header + rc + code_ptr + arity
        assert_eq!(layout.code_ptr_offset(), 16);
        assert_eq!(layout.arity_offset(), 24);
        assert_eq!(layout.field_count(), 4);
    }

    #[test]
    fn test_closure_layout_with_captures() {
        let layout = ClosureLayout::new(3);
        assert_eq!(layout.size_bytes(), 56); // 32 + 3*8
        assert_eq!(layout.capture_offset(0), 32);
        assert_eq!(layout.capture_offset(1), 40);
        assert_eq!(layout.capture_offset(2), 48);
        assert_eq!(layout.field_count(), 7);
    }

    #[test]
    #[should_panic(expected = "capture index out of range")]
    fn test_closure_layout_out_of_range() {
        ClosureLayout::new(2).capture_offset(2);
    }
}
