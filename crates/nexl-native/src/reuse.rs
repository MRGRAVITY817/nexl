//! Reuse analysis for in-place mutation (spec §13.3).
//!
//! When a heap object is uniquely owned (refcount = 1), Perceus can reuse the
//! same memory for a new object of equal or smaller size, avoiding allocation.
//!
//! This module provides the analysis and helpers for reuse tokens:
//! - [`ReuseToken`] represents a potential reuse of a heap allocation.
//! - [`can_reuse`] checks if two heap object sizes are compatible for reuse.

/// A reuse token: a pointer that *might* be reusable at a given program point.
///
/// If the referenced object is uniquely owned at runtime, the allocation can be
/// reused; otherwise, a fresh allocation is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReuseToken {
    /// Size in bytes of the reusable allocation.
    pub size_bytes: usize,
}

impl ReuseToken {
    /// Create a new reuse token for an allocation of the given size.
    pub fn new(size_bytes: usize) -> Self {
        Self { size_bytes }
    }
}

/// Check if a reuse token can be used for an allocation of `needed_bytes`.
///
/// Reuse is valid when the existing allocation is at least as large as needed.
/// The object header is overwritten, so only total size matters.
pub fn can_reuse(token: &ReuseToken, needed_bytes: usize) -> bool {
    token.size_bytes >= needed_bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reuse_exact_match() {
        let token = ReuseToken::new(32);
        assert!(can_reuse(&token, 32));
    }

    #[test]
    fn test_reuse_larger_allocation() {
        let token = ReuseToken::new(64);
        assert!(can_reuse(&token, 32));
        assert!(can_reuse(&token, 64));
    }

    #[test]
    fn test_no_reuse_too_small() {
        let token = ReuseToken::new(16);
        assert!(!can_reuse(&token, 32));
    }

    #[test]
    fn test_reuse_zero_size() {
        let token = ReuseToken::new(0);
        assert!(can_reuse(&token, 0));
        assert!(!can_reuse(&token, 8));
    }
}
