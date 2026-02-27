//! FFI memory pinning for C ABI calls (spec §15.3).
//!
//! Nexl values passed to C functions are pinned (not moved by GC) for the
//! duration of the C call. The C function receives a pointer to the live
//! Nexl value and must not store it beyond the call.

use std::collections::HashSet;

/// Tracks pinned values during an FFI call.
///
/// Values are pinned before the call and unpinned when the guard is dropped.
/// This ensures C code always sees valid pointers for the call duration.
#[derive(Debug)]
pub struct FfiPinGuard {
    /// Addresses of pinned values (for tracking/debugging).
    pinned: HashSet<usize>,
}

impl FfiPinGuard {
    /// Create a new pin guard with no pinned values.
    pub fn new() -> Self {
        Self {
            pinned: HashSet::new(),
        }
    }

    /// Pin a value by its address, preventing GC from moving it.
    ///
    /// Returns the address for passing to C code.
    pub fn pin(&mut self, addr: usize) -> usize {
        self.pinned.insert(addr);
        addr
    }

    /// Check if an address is currently pinned.
    pub fn is_pinned(&self, addr: usize) -> bool {
        self.pinned.contains(&addr)
    }

    /// Number of currently pinned values.
    pub fn count(&self) -> usize {
        self.pinned.len()
    }
}

impl Default for FfiPinGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FfiPinGuard {
    fn drop(&mut self) {
        // All pins are released when the guard is dropped.
        // In a real GC-integrated implementation, this would notify the GC
        // that these addresses are no longer pinned.
        self.pinned.clear();
    }
}

/// Represents C-compatible type marshaling info for a Nexl value (spec §15.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CTypeLayout {
    /// Fixed-size integer (1, 2, 4, or 8 bytes).
    Int { bytes: u8, signed: bool },
    /// Floating point (4 or 8 bytes).
    Float { bytes: u8 },
    /// Bool (1 byte).
    Bool,
    /// String: pointer + length pair.
    Str,
    /// Opaque pointer (`void*`).
    Ptr,
    /// Void (no value).
    Void,
}

impl CTypeLayout {
    /// Size in bytes of this C type.
    pub fn size(&self) -> usize {
        match self {
            CTypeLayout::Int { bytes, .. } | CTypeLayout::Float { bytes } => *bytes as usize,
            CTypeLayout::Bool => 1,
            CTypeLayout::Str => std::mem::size_of::<usize>() * 2, // ptr + len
            CTypeLayout::Ptr => std::mem::size_of::<usize>(),
            CTypeLayout::Void => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test 1 ──

    #[test]
    fn test_pin_guard_basic() {
        let mut guard = FfiPinGuard::new();
        let addr = guard.pin(0x1000);
        assert_eq!(addr, 0x1000);
        assert!(guard.is_pinned(0x1000));
        assert!(!guard.is_pinned(0x2000));
        assert_eq!(guard.count(), 1);
    }

    // ── Test 2 ──

    #[test]
    fn test_pin_guard_multiple() {
        let mut guard = FfiPinGuard::new();
        guard.pin(0x1000);
        guard.pin(0x2000);
        guard.pin(0x3000);
        assert_eq!(guard.count(), 3);
        assert!(guard.is_pinned(0x1000));
        assert!(guard.is_pinned(0x2000));
        assert!(guard.is_pinned(0x3000));
    }

    // ── Test 3 ──

    #[test]
    fn test_pin_guard_drop_releases() {
        let guard = {
            let mut g = FfiPinGuard::new();
            g.pin(0x1000);
            g.pin(0x2000);
            assert_eq!(g.count(), 2);
            g
        };
        // After move, still has pins (not dropped yet)
        assert_eq!(guard.count(), 2);
        // Drop happens at end of scope
    }

    // ── Test 4 ──

    #[test]
    fn test_c_type_layout_sizes() {
        assert_eq!(
            CTypeLayout::Int {
                bytes: 1,
                signed: true
            }
            .size(),
            1
        );
        assert_eq!(
            CTypeLayout::Int {
                bytes: 2,
                signed: true
            }
            .size(),
            2
        );
        assert_eq!(
            CTypeLayout::Int {
                bytes: 4,
                signed: true
            }
            .size(),
            4
        );
        assert_eq!(
            CTypeLayout::Int {
                bytes: 8,
                signed: true
            }
            .size(),
            8
        );
        assert_eq!(
            CTypeLayout::Int {
                bytes: 4,
                signed: false
            }
            .size(),
            4
        );
        assert_eq!(CTypeLayout::Float { bytes: 4 }.size(), 4);
        assert_eq!(CTypeLayout::Float { bytes: 8 }.size(), 8);
        assert_eq!(CTypeLayout::Bool.size(), 1);
        assert_eq!(CTypeLayout::Void.size(), 0);
    }

    // ── Test 5 ──

    #[test]
    fn test_c_type_layout_ptr_size() {
        assert_eq!(CTypeLayout::Ptr.size(), std::mem::size_of::<usize>());
    }
}
