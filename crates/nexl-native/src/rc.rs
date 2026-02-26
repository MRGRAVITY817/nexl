//! Perceus reference counting primitives (spec §13.3).
//!
//! Provides the inc/dec/drop operations for heap-allocated objects.
//! On the native target, every heap object has a reference count field
//! immediately after the [`HeapHeader`](crate::value::HeapHeader).
//!
//! Layout: `[header: i64] [refcount: i64] [fields...]`
//!
//! The runtime extern functions are:
//! - `nexl_rc_inc(ptr: i64)` — increment refcount
//! - `nexl_rc_dec(ptr: i64)` — decrement refcount; drops if zero
//! - `nexl_rc_drop(ptr: i64)` — deallocate the object

/// Byte offset of the reference count field in a heap object.
pub const RC_OFFSET: i32 = 8; // right after the 8-byte header

/// Byte offset where payload fields begin (after header + refcount).
pub const FIELDS_OFFSET: i32 = 16;

/// Initial reference count for a freshly allocated object.
pub const INITIAL_RC: i64 = 1;

/// Check if a reference count indicates unique ownership.
pub fn is_unique(rc: i64) -> bool {
    rc == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rc_constants() {
        assert_eq!(RC_OFFSET, 8);
        assert_eq!(FIELDS_OFFSET, 16);
        assert_eq!(INITIAL_RC, 1);
    }

    #[test]
    fn test_is_unique() {
        assert!(is_unique(1));
        assert!(!is_unique(0));
        assert!(!is_unique(2));
        assert!(!is_unique(100));
    }
}
