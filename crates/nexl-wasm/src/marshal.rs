//! Automatic type marshaling between Nexl and C types (spec §15.3).
//!
//! Defines marshaling strategies for converting Nexl values to/from C
//! representations at FFI boundaries.

use nexl_types::Type;

/// How a value is marshaled at an FFI boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarshalStrategy {
    /// Direct pass — value is binary-compatible (e.g. Int ↔ int64_t).
    Direct,
    /// Widen — sub-word type widened to larger register (e.g. Int8 → i32).
    Widen,
    /// Narrow — value narrowed from wider register (e.g. i32 → Int8).
    Narrow,
    /// Pair — value is split into two values (e.g. Str → ptr + len).
    Pair,
    /// Void — no value to marshal (Unit).
    Void,
}

/// Error during marshaling lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarshalError {
    /// Type cannot be marshaled across FFI.
    UnsupportedType(String),
}

impl std::fmt::Display for MarshalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarshalError::UnsupportedType(t) => {
                write!(f, "type cannot be marshaled across FFI: {t}")
            }
        }
    }
}

impl std::error::Error for MarshalError {}

/// Determine the marshaling strategy for a Nexl type at a C FFI boundary.
pub fn marshal_strategy(ty: &Type) -> Result<MarshalStrategy, MarshalError> {
    match ty {
        // Direct: binary-compatible
        Type::Int | Type::Int64 | Type::U64 => Ok(MarshalStrategy::Direct),
        Type::Int32 | Type::U32 => Ok(MarshalStrategy::Direct),
        Type::Float | Type::F64 => Ok(MarshalStrategy::Direct),
        Type::F32 => Ok(MarshalStrategy::Direct),
        Type::Bool => Ok(MarshalStrategy::Direct),

        // Sub-word types: widen to register size on call, narrow on return
        Type::Int8 | Type::Int16 | Type::U8 | Type::U16 => Ok(MarshalStrategy::Widen),

        // String: split into pointer + length pair
        Type::Str => Ok(MarshalStrategy::Pair),

        // Unit: no value
        Type::Unit => Ok(MarshalStrategy::Void),

        _ => Err(MarshalError::UnsupportedType(format!("{ty:?}"))),
    }
}

/// Return the C type name for a Nexl type (spec §15.3 table).
pub fn c_type_name(ty: &Type) -> Result<&'static str, MarshalError> {
    match ty {
        Type::Int | Type::Int64 => Ok("int64_t"),
        Type::Int32 => Ok("int32_t"),
        Type::Int16 => Ok("int16_t"),
        Type::Int8 => Ok("int8_t"),
        Type::U64 => Ok("uint64_t"),
        Type::U32 => Ok("uint32_t"),
        Type::U16 => Ok("uint16_t"),
        Type::U8 => Ok("uint8_t"),
        Type::Float | Type::F64 => Ok("double"),
        Type::F32 => Ok("float"),
        Type::Bool => Ok("bool"),
        Type::Str => Ok("const char*"),
        Type::Unit => Ok("void"),
        _ => Err(MarshalError::UnsupportedType(format!("{ty:?}"))),
    }
}

/// Generate a C function declaration string for an exported Nexl function.
///
/// For example, `("add_ints", [Int, Int], Int)` produces:
/// `"int64_t add_ints(int64_t p0, int64_t p1)"`.
pub fn c_function_decl(
    name: &str,
    params: &[Type],
    ret: &Type,
) -> Result<String, MarshalError> {
    let ret_type = c_type_name(ret)?;
    let mut param_strs = Vec::with_capacity(params.len());
    for (i, p) in params.iter().enumerate() {
        let c_type = c_type_name(p)?;
        param_strs.push(format!("{c_type} p{i}"));
    }
    Ok(format!("{ret_type} {name}({})", param_strs.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test 1 ──

    #[test]
    fn test_marshal_strategy_int() {
        assert_eq!(marshal_strategy(&Type::Int).unwrap(), MarshalStrategy::Direct);
        assert_eq!(marshal_strategy(&Type::Int64).unwrap(), MarshalStrategy::Direct);
        assert_eq!(marshal_strategy(&Type::Int32).unwrap(), MarshalStrategy::Direct);
    }

    // ── Test 2 ──

    #[test]
    fn test_marshal_strategy_float() {
        assert_eq!(marshal_strategy(&Type::Float).unwrap(), MarshalStrategy::Direct);
        assert_eq!(marshal_strategy(&Type::F64).unwrap(), MarshalStrategy::Direct);
        assert_eq!(marshal_strategy(&Type::F32).unwrap(), MarshalStrategy::Direct);
    }

    // ── Test 3 ──

    #[test]
    fn test_marshal_strategy_bool() {
        assert_eq!(marshal_strategy(&Type::Bool).unwrap(), MarshalStrategy::Direct);
    }

    // ── Test 4 ──

    #[test]
    fn test_marshal_strategy_str() {
        assert_eq!(marshal_strategy(&Type::Str).unwrap(), MarshalStrategy::Pair);
    }

    // ── Test 5 ──

    #[test]
    fn test_marshal_strategy_unit() {
        assert_eq!(marshal_strategy(&Type::Unit).unwrap(), MarshalStrategy::Void);
    }

    // ── Test 6 ──

    #[test]
    fn test_marshal_strategy_sub_word() {
        assert_eq!(marshal_strategy(&Type::Int8).unwrap(), MarshalStrategy::Widen);
        assert_eq!(marshal_strategy(&Type::Int16).unwrap(), MarshalStrategy::Widen);
        assert_eq!(marshal_strategy(&Type::U8).unwrap(), MarshalStrategy::Widen);
        assert_eq!(marshal_strategy(&Type::U16).unwrap(), MarshalStrategy::Widen);
    }

    // ── Test 7 ──

    #[test]
    fn test_c_type_name() {
        assert_eq!(c_type_name(&Type::Int).unwrap(), "int64_t");
        assert_eq!(c_type_name(&Type::Int32).unwrap(), "int32_t");
        assert_eq!(c_type_name(&Type::Int16).unwrap(), "int16_t");
        assert_eq!(c_type_name(&Type::Int8).unwrap(), "int8_t");
        assert_eq!(c_type_name(&Type::U64).unwrap(), "uint64_t");
        assert_eq!(c_type_name(&Type::U32).unwrap(), "uint32_t");
        assert_eq!(c_type_name(&Type::Float).unwrap(), "double");
        assert_eq!(c_type_name(&Type::F32).unwrap(), "float");
        assert_eq!(c_type_name(&Type::Bool).unwrap(), "bool");
        assert_eq!(c_type_name(&Type::Str).unwrap(), "const char*");
        assert_eq!(c_type_name(&Type::Unit).unwrap(), "void");
    }

    // ── Test 8 ──

    #[test]
    fn test_c_function_decl() {
        let result = c_function_decl("add_ints", &[Type::Int, Type::Int], &Type::Int).unwrap();
        assert_eq!(result, "int64_t add_ints(int64_t p0, int64_t p1)");
    }

    // ── Test 9 ──

    #[test]
    fn test_c_function_decl_void_return() {
        let result = c_function_decl("log_msg", &[Type::Str], &Type::Unit).unwrap();
        assert_eq!(result, "void log_msg(const char* p0)");
    }
}
