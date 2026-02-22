/// Identifies a source file within the compilation unit.
///
/// An opaque index into the compiler's source-file table.
/// `FileId(u32::MAX)` is reserved as the "synthetic" sentinel for
/// compiler-generated nodes that have no source location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

impl FileId {
    /// The sentinel value used for synthetic (compiler-generated) spans.
    pub const SYNTHETIC: FileId = FileId(u32::MAX);
}

/// A contiguous byte range within a single source file.
///
/// `start` and `len` are byte offsets into the UTF-8 source text.
/// Both fields are `u32` to keep the struct small (8 bytes + `FileId`).
/// Source files larger than 4 GiB are not supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// The source file this span belongs to.
    pub file_id: FileId,
    /// Byte offset of the first byte in the span.
    pub start: u32,
    /// Number of bytes in the span.
    pub len: u32,
}

impl Span {
    /// Create a new span from a file, start offset, and length.
    pub fn new(file_id: FileId, start: u32, len: u32) -> Self {
        Self { file_id, start, len }
    }

    /// Create a span covering a byte range `start..end` (exclusive end).
    ///
    /// Panics in debug mode if `end < start`.
    pub fn from_range(file_id: FileId, start: u32, end: u32) -> Self {
        debug_assert!(end >= start, "span end {end} is before start {start}");
        Self::new(file_id, start, end - start)
    }

    /// One-past-the-last byte of this span.
    pub fn end(self) -> u32 {
        self.start + self.len
    }

    /// Merge two spans into the smallest span that covers both.
    ///
    /// Both spans must belong to the same file.
    pub fn merge(self, other: Span) -> Span {
        debug_assert_eq!(
            self.file_id, other.file_id,
            "cannot merge spans from different files"
        );
        let start = self.start.min(other.start);
        let end = self.end().max(other.end());
        Span::from_range(self.file_id, start, end)
    }

    /// A zero-length span at a single byte position.
    pub fn point(file_id: FileId, offset: u32) -> Self {
        Self::new(file_id, offset, 0)
    }

    /// A synthetic span for compiler-generated nodes with no source location.
    pub fn synthetic() -> Self {
        Self::new(FileId::SYNTHETIC, 0, 0)
    }

    /// Returns `true` if this span was synthetically generated (not from source text).
    pub fn is_synthetic(self) -> bool {
        self.file_id == FileId::SYNTHETIC
    }
}

/// A human-readable source position expressed as line and column numbers.
///
/// Both `line` and `col` are **1-based** to match conventional editor display.
/// Computed on demand from a `Span` by the source-file table; not stored in AST nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceLocation {
    /// The source file this position belongs to.
    pub file_id: FileId,
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number (counts Unicode scalar values, not bytes).
    pub col: u32,
}

impl SourceLocation {
    /// Create a source location from its components.
    pub fn new(file_id: FileId, line: u32, col: u32) -> Self {
        Self { file_id, line, col }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_end_equals_start_plus_len() {
        let id = FileId(0);
        let s = Span::new(id, 10, 5);
        assert_eq!(s.end(), 15);
    }

    #[test]
    fn span_from_range_roundtrips() {
        let id = FileId(1);
        let s = Span::from_range(id, 4, 12);
        assert_eq!(s.start, 4);
        assert_eq!(s.len, 8);
        assert_eq!(s.end(), 12);
    }

    #[test]
    fn span_merge_covers_both() {
        let id = FileId(0);
        let a = Span::new(id, 5, 3);  // bytes 5..8
        let b = Span::new(id, 10, 4); // bytes 10..14
        let m = a.merge(b);
        assert_eq!(m.start, 5);
        assert_eq!(m.end(), 14);
        assert_eq!(m.file_id, id);
    }

    #[test]
    fn span_merge_overlapping() {
        let id = FileId(0);
        let a = Span::new(id, 2, 8);  // bytes 2..10
        let b = Span::new(id, 6, 6);  // bytes 6..12
        let m = a.merge(b);
        assert_eq!(m.start, 2);
        assert_eq!(m.end(), 12);
    }

    #[test]
    fn span_merge_commutative() {
        let id = FileId(0);
        let a = Span::new(id, 0, 5);
        let b = Span::new(id, 3, 10);
        assert_eq!(a.merge(b), b.merge(a));
    }

    #[test]
    fn span_point_has_zero_len() {
        let s = Span::point(FileId(0), 42);
        assert_eq!(s.len, 0);
        assert_eq!(s.start, 42);
        assert_eq!(s.end(), 42);
    }

    #[test]
    fn synthetic_span_is_detected() {
        let s = Span::synthetic();
        assert!(s.is_synthetic());
        assert!(!Span::new(FileId(0), 0, 0).is_synthetic());
    }

    #[test]
    fn source_location_fields() {
        let loc = SourceLocation::new(FileId(2), 10, 5);
        assert_eq!(loc.line, 10);
        assert_eq!(loc.col, 5);
        assert_eq!(loc.file_id, FileId(2));
    }
}
