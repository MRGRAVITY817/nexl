//! Continuation capture for non-tail-resumptive effect handlers (spec §13.5.3).
//!
//! When a handler uses `resume` in non-tail position (or doesn't call `resume`),
//! the runtime must capture the call stack between the operation site and the
//! handler as a one-shot continuation.
//!
//! This module defines the data structures for captured continuations.
//! The actual capture/restore is implemented by runtime extern functions:
//! - `nexl_continuation_capture(handler_marker: i64) -> i64` — capture stack, return continuation ptr
//! - `nexl_continuation_resume(cont: i64, value: i64) -> i64` — restore and resume with a value

/// A continuation is a heap-allocated object holding captured stack frames.
///
/// Layout: `[header: i64] [refcount: i64] [frame_count: i64] [frames...]`
///
/// Continuations are one-shot: resuming a continuation consumes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationLayout {
    /// Number of captured stack frames.
    pub frame_count: usize,
}

impl ContinuationLayout {
    /// Create a layout for a continuation with `n` captured frames.
    pub fn new(frame_count: usize) -> Self {
        Self { frame_count }
    }

    /// Minimum size in bytes (header + refcount + frame_count field).
    ///
    /// The actual size depends on frame contents, determined at runtime.
    /// This gives the fixed overhead.
    pub fn overhead_bytes(&self) -> usize {
        24 // header (8) + refcount (8) + frame_count field (8)
    }

    /// Check if this is a one-shot continuation (always true in Nexl).
    pub fn is_one_shot(&self) -> bool {
        true // ADR-003: one-shot continuations only
    }
}

/// Extern function names for the continuation runtime.
pub const CAPTURE_FUNC: &str = "nexl_continuation_capture";
/// Extern function name for resuming a continuation.
pub const RESUME_FUNC: &str = "nexl_continuation_resume";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_continuation_layout() {
        let layout = ContinuationLayout::new(5);
        assert_eq!(layout.frame_count, 5);
        assert_eq!(layout.overhead_bytes(), 24);
    }

    #[test]
    fn test_one_shot_only() {
        // ADR-003: Nexl only supports one-shot continuations
        let layout = ContinuationLayout::new(0);
        assert!(layout.is_one_shot());
    }

    #[test]
    fn test_extern_names() {
        assert_eq!(CAPTURE_FUNC, "nexl_continuation_capture");
        assert_eq!(RESUME_FUNC, "nexl_continuation_resume");
    }
}
