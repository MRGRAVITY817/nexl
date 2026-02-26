//! Native value representation using tagged pointers (spec §13.2).
//!
//! On the native target, all values are represented as a single 64-bit word.
//! The low 3 bits encode the type tag; the upper 61 bits carry the payload.

/// Number of bits used for the tag.
pub const TAG_BITS: u64 = 3;

/// Mask to extract the tag from a raw value.
pub const TAG_MASK: u64 = 0x7;

/// Tag for heap-allocated objects (closures, records, ADTs, collections).
pub const TAG_HEAP: u64 = 0;

/// Tag for small (63-bit sign-extended) integers.
pub const TAG_INT: u64 = 1;

/// Tag for booleans.
pub const TAG_BOOL: u64 = 2;

/// Tag for the Unit value.
pub const TAG_UNIT: u64 = 3;

/// A native value represented as a tagged 64-bit word.
///
/// The low 3 bits encode the type tag; the upper bits carry the payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeValue(u64);

impl NativeValue {
    /// Encode a small integer (63-bit, sign-extended) as a tagged value.
    pub fn small_int(n: i64) -> Self {
        NativeValue(((n as u64) << TAG_BITS) | TAG_INT)
    }

    /// Encode a boolean as a tagged value.
    ///
    /// `false` = `0x2`, `true` = `0xA` (spec §13.2).
    pub fn bool(b: bool) -> Self {
        NativeValue(((b as u64) << TAG_BITS) | TAG_BOOL)
    }

    /// Encode the Unit value.
    pub fn unit() -> Self {
        NativeValue(TAG_UNIT)
    }

    /// Decode a small integer, returning `None` if the tag is wrong.
    pub fn as_small_int(self) -> Option<i64> {
        if self.tag() != TAG_INT {
            return None;
        }
        // Arithmetic right-shift to sign-extend the 63-bit payload.
        Some((self.0 as i64) >> TAG_BITS)
    }

    /// Decode a boolean, returning `None` if the tag is wrong.
    pub fn as_bool(self) -> Option<bool> {
        if self.tag() != TAG_BOOL {
            return None;
        }
        Some((self.0 >> TAG_BITS) != 0)
    }

    /// Check if this is a heap pointer (tag = 000).
    pub fn is_heap(self) -> bool {
        self.tag() == TAG_HEAP
    }

    /// Check if this is the Unit value.
    pub fn is_unit(self) -> bool {
        self.0 == TAG_UNIT
    }

    /// Extract the tag (low 3 bits).
    pub fn tag(self) -> u64 {
        self.0 & TAG_MASK
    }

    /// Construct a `NativeValue` from a raw 64-bit word (for testing/low-level use).
    pub fn from_raw(raw: u64) -> Self {
        NativeValue(raw)
    }

    /// Return the raw 64-bit representation.
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// The kind of object stored on the heap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HeapTag {
    /// A closure: code pointer + arity + captured environment.
    Closure = 0,
    /// A record (struct).
    Record = 1,
    /// An ADT variant (constructor tag + fields).
    Adt = 2,
    /// A string (pointer + length).
    Str = 3,
    /// A vector (persistent trie node).
    Vec = 4,
}

/// Header word for a heap-allocated object.
///
/// Layout: bits [0..8) = `HeapTag`, bits [8..40) = field count, bits [40..64) = reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeapHeader(u64);

impl HeapHeader {
    /// Create a new heap header.
    pub fn new(tag: HeapTag, field_count: u32) -> Self {
        HeapHeader((tag as u64) | ((field_count as u64) << 8))
    }

    /// Extract the heap tag.
    pub fn heap_tag(self) -> HeapTag {
        match self.0 & 0xFF {
            0 => HeapTag::Closure,
            1 => HeapTag::Record,
            2 => HeapTag::Adt,
            3 => HeapTag::Str,
            4 => HeapTag::Vec,
            other => panic!("invalid heap tag: {other}"),
        }
    }

    /// Extract the field count.
    pub fn field_count(self) -> u32 {
        ((self.0 >> 8) & 0xFFFF_FFFF) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_int_max_min() {
        // With 3 tag bits, payload is 61 signed bits: [-(2^60), 2^60 - 1]
        let max = (1i64 << 60) - 1;
        let min = -(1i64 << 60);
        assert_eq!(NativeValue::small_int(max).as_small_int(), Some(max));
        assert_eq!(NativeValue::small_int(min).as_small_int(), Some(min));
        assert_eq!(max, 1_152_921_504_606_846_975);
        assert_eq!(min, -1_152_921_504_606_846_976);
    }

    #[test]
    fn test_heap_object_header() {
        let hdr = HeapHeader::new(HeapTag::Closure, 3);
        assert_eq!(hdr.heap_tag(), HeapTag::Closure);
        assert_eq!(hdr.field_count(), 3);

        let hdr2 = HeapHeader::new(HeapTag::Record, 0);
        assert_eq!(hdr2.heap_tag(), HeapTag::Record);
        assert_eq!(hdr2.field_count(), 0);

        let hdr3 = HeapHeader::new(HeapTag::Adt, 5);
        assert_eq!(hdr3.heap_tag(), HeapTag::Adt);
        assert_eq!(hdr3.field_count(), 5);
    }

    #[test]
    fn test_is_heap_pointer() {
        // 8-byte aligned address → tag 000 → heap
        assert!(NativeValue::from_raw(0x1000).is_heap());
        assert!(NativeValue::from_raw(0x7fff_ffff_fff8).is_heap());
        // Non-heap values
        assert!(!NativeValue::small_int(0).is_heap());
        assert!(!NativeValue::bool(true).is_heap());
        assert!(!NativeValue::unit().is_heap());
    }

    #[test]
    fn test_tag_of() {
        assert_eq!(NativeValue::small_int(99).tag(), TAG_INT);
        assert_eq!(NativeValue::bool(true).tag(), TAG_BOOL);
        assert_eq!(NativeValue::bool(false).tag(), TAG_BOOL);
        assert_eq!(NativeValue::unit().tag(), TAG_UNIT);
        // A heap pointer (8-byte aligned) has tag 0
        assert_eq!(NativeValue::from_raw(0x1000).tag(), TAG_HEAP);
    }

    #[test]
    fn test_unit_encoding() {
        let u = NativeValue::unit();
        assert_eq!(u.raw(), 0x3);
        assert_eq!(u.tag(), TAG_UNIT);
        assert!(u.is_unit());
        // Other values are not unit
        assert!(!NativeValue::small_int(0).is_unit());
        assert!(!NativeValue::bool(false).is_unit());
    }

    #[test]
    fn test_decode_bool() {
        assert_eq!(NativeValue::bool(false).as_bool(), Some(false));
        assert_eq!(NativeValue::bool(true).as_bool(), Some(true));
        // Wrong tag returns None
        assert_eq!(NativeValue::small_int(0).as_bool(), None);
    }

    #[test]
    fn test_encode_bool() {
        assert_eq!(NativeValue::bool(false).raw(), 0x2);
        assert_eq!(NativeValue::bool(true).raw(), 0xA);
    }

    #[test]
    fn test_decode_small_int() {
        // Positive round-trip
        assert_eq!(NativeValue::small_int(42).as_small_int(), Some(42));
        // Negative round-trip
        assert_eq!(NativeValue::small_int(-1).as_small_int(), Some(-1));
        // Zero
        assert_eq!(NativeValue::small_int(0).as_small_int(), Some(0));
        // Wrong tag returns None
        assert_eq!(NativeValue::unit().as_small_int(), None);
    }

    #[test]
    fn test_encode_small_int() {
        let v = NativeValue::small_int(42);
        assert_eq!(v.raw(), (42u64 << 3) | TAG_INT);
        assert_eq!(v.raw(), 337);

        let neg = NativeValue::small_int(-1);
        // -1 sign-extended into 63 bits, shifted left 3, OR'd with TAG_INT
        assert_eq!(neg.raw() & TAG_MASK, TAG_INT);
    }

    #[test]
    fn test_tag_constants() {
        assert_eq!(TAG_BITS, 3);
        assert_eq!(TAG_MASK, 0b111);
        assert_eq!(TAG_HEAP, 0b000);
        assert_eq!(TAG_INT, 0b001);
        assert_eq!(TAG_BOOL, 0b010);
        assert_eq!(TAG_UNIT, 0b011);
    }
}
