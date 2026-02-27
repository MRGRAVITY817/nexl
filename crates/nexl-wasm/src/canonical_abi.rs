//! Canonical ABI serialization for WASM Component Model boundaries (spec §15.2).
//!
//! Defines type sizes, alignments, and core WASM value type mappings used when
//! serializing Nexl values across component boundaries.

use nexl_types::Type;

/// WASM core value types used in the canonical ABI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmValType {
    /// 32-bit integer.
    I32,
    /// 64-bit integer.
    I64,
    /// 32-bit float.
    F32,
    /// 64-bit float.
    F64,
}

/// Errors during canonical ABI operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalAbiError {
    /// Type cannot be represented in the canonical ABI.
    UnsupportedType(String),
}

impl std::fmt::Display for CanonicalAbiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CanonicalAbiError::UnsupportedType(t) => {
                write!(f, "type not supported in canonical ABI: {t}")
            }
        }
    }
}

impl std::error::Error for CanonicalAbiError {}

/// Return the size in bytes of a Nexl type in the canonical ABI (spec §15.2, §15.3).
pub fn canonical_size(ty: &Type) -> Result<u32, CanonicalAbiError> {
    match ty {
        Type::Int | Type::Int64 | Type::U64 => Ok(8),
        Type::Int32 | Type::U32 => Ok(4),
        Type::Int16 | Type::U16 => Ok(2),
        Type::Int8 | Type::U8 => Ok(1),
        Type::Float | Type::F64 => Ok(8),
        Type::F32 => Ok(4),
        Type::Bool => Ok(1),
        Type::Str => Ok(8), // ptr (4) + len (4)
        Type::Unit => Ok(0),
        _ => Err(CanonicalAbiError::UnsupportedType(format!("{ty:?}"))),
    }
}

/// Return the alignment in bytes of a Nexl type in the canonical ABI.
pub fn canonical_alignment(ty: &Type) -> Result<u32, CanonicalAbiError> {
    match ty {
        Type::Int | Type::Int64 | Type::U64 | Type::Float | Type::F64 => Ok(8),
        Type::Int32 | Type::U32 | Type::F32 => Ok(4),
        Type::Int16 | Type::U16 => Ok(2),
        Type::Int8 | Type::U8 | Type::Bool => Ok(1),
        Type::Str => Ok(4), // aligned to pointer size
        Type::Unit => Ok(1),
        _ => Err(CanonicalAbiError::UnsupportedType(format!("{ty:?}"))),
    }
}

/// Map a Nexl type to its WASM core value type(s) (spec §15.2).
///
/// Sub-word types (Int8, Int16, U8, U16) are stored as i32 in WASM core
/// and narrowed/extended at component boundaries.
pub fn wasm_valtype(ty: &Type) -> Result<WasmValType, CanonicalAbiError> {
    match ty {
        Type::Int | Type::Int64 | Type::U64 => Ok(WasmValType::I64),
        Type::Int32 | Type::U32 | Type::Int16 | Type::U16 | Type::Int8 | Type::U8 => {
            Ok(WasmValType::I32)
        }
        Type::Bool => Ok(WasmValType::I32),
        Type::Float | Type::F64 => Ok(WasmValType::F64),
        Type::F32 => Ok(WasmValType::F32),
        _ => Err(CanonicalAbiError::UnsupportedType(format!("{ty:?}"))),
    }
}

/// Flatten function parameters into canonical ABI value types.
///
/// Each parameter is mapped to its WASM core value type. Str parameters
/// are flattened into two i32 values (ptr + len).
pub fn flatten_params(params: &[Type]) -> Result<Vec<WasmValType>, CanonicalAbiError> {
    let mut flat = Vec::new();
    for p in params {
        match p {
            Type::Str => {
                flat.push(WasmValType::I32); // ptr
                flat.push(WasmValType::I32); // len
            }
            Type::Unit => {} // Unit takes no space
            _ => {
                flat.push(wasm_valtype(p)?);
            }
        }
    }
    Ok(flat)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test 1 ──

    #[test]
    fn test_canonical_size_primitives() {
        assert_eq!(canonical_size(&Type::Int).unwrap(), 8);
        assert_eq!(canonical_size(&Type::Int64).unwrap(), 8);
        assert_eq!(canonical_size(&Type::Int32).unwrap(), 4);
        assert_eq!(canonical_size(&Type::Int16).unwrap(), 2);
        assert_eq!(canonical_size(&Type::Int8).unwrap(), 1);
        assert_eq!(canonical_size(&Type::U64).unwrap(), 8);
        assert_eq!(canonical_size(&Type::U32).unwrap(), 4);
        assert_eq!(canonical_size(&Type::U16).unwrap(), 2);
        assert_eq!(canonical_size(&Type::U8).unwrap(), 1);
        assert_eq!(canonical_size(&Type::Float).unwrap(), 8);
        assert_eq!(canonical_size(&Type::F64).unwrap(), 8);
        assert_eq!(canonical_size(&Type::F32).unwrap(), 4);
        assert_eq!(canonical_size(&Type::Bool).unwrap(), 1);
    }

    // ── Test 2 ──

    #[test]
    fn test_canonical_alignment() {
        assert_eq!(canonical_alignment(&Type::Int).unwrap(), 8);
        assert_eq!(canonical_alignment(&Type::Int32).unwrap(), 4);
        assert_eq!(canonical_alignment(&Type::Int16).unwrap(), 2);
        assert_eq!(canonical_alignment(&Type::Int8).unwrap(), 1);
        assert_eq!(canonical_alignment(&Type::Bool).unwrap(), 1);
        assert_eq!(canonical_alignment(&Type::Str).unwrap(), 4);
        assert_eq!(canonical_alignment(&Type::F32).unwrap(), 4);
        assert_eq!(canonical_alignment(&Type::F64).unwrap(), 8);
    }

    // ── Test 3 ──

    #[test]
    fn test_canonical_size_str() {
        // Str = ptr (4 bytes) + len (4 bytes) = 8
        assert_eq!(canonical_size(&Type::Str).unwrap(), 8);
    }

    // ── Test 4 ──

    #[test]
    fn test_canonical_size_unit() {
        assert_eq!(canonical_size(&Type::Unit).unwrap(), 0);
    }

    // ── Test 5 ──

    #[test]
    fn test_canonical_wasm_valtype() {
        assert_eq!(wasm_valtype(&Type::Int).unwrap(), WasmValType::I64);
        assert_eq!(wasm_valtype(&Type::Int64).unwrap(), WasmValType::I64);
        assert_eq!(wasm_valtype(&Type::U64).unwrap(), WasmValType::I64);
        assert_eq!(wasm_valtype(&Type::Int32).unwrap(), WasmValType::I32);
        assert_eq!(wasm_valtype(&Type::U32).unwrap(), WasmValType::I32);
        assert_eq!(wasm_valtype(&Type::Bool).unwrap(), WasmValType::I32);
        assert_eq!(wasm_valtype(&Type::Float).unwrap(), WasmValType::F64);
        assert_eq!(wasm_valtype(&Type::F64).unwrap(), WasmValType::F64);
        assert_eq!(wasm_valtype(&Type::F32).unwrap(), WasmValType::F32);
    }

    // ── Test 6 ──

    #[test]
    fn test_canonical_sub_word_types() {
        // Sub-word types all map to i32 in WASM core
        assert_eq!(wasm_valtype(&Type::Int8).unwrap(), WasmValType::I32);
        assert_eq!(wasm_valtype(&Type::Int16).unwrap(), WasmValType::I32);
        assert_eq!(wasm_valtype(&Type::U8).unwrap(), WasmValType::I32);
        assert_eq!(wasm_valtype(&Type::U16).unwrap(), WasmValType::I32);
    }

    // ── Test 7 ──

    #[test]
    fn test_canonical_flat_params() {
        // (Fn [Int Str Bool] -> ...)
        let params = vec![Type::Int, Type::Str, Type::Bool];
        let flat = flatten_params(&params).unwrap();
        // Int → i64, Str → i32 + i32, Bool → i32
        assert_eq!(
            flat,
            vec![
                WasmValType::I64,
                WasmValType::I32,
                WasmValType::I32,
                WasmValType::I32,
            ]
        );
    }
}
