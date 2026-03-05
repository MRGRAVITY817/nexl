//! `bit` module — bitwise operations on `Int` (i64).
//!
//! All functions operate on the full 64-bit signed integer representation.
//! Shift amounts are taken modulo 64 to match Rust's wrapping semantics.

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `bit` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("and", and as fn(&[Value]) -> Result<Value, String>),
        ("or", or),
        ("xor", xor),
        ("not", not),
        ("shift-left", shift_left),
        ("shift-right", shift_right),
        ("count-ones", count_ones),
        ("count-zeros", count_zeros),
        ("leading-zeros", leading_zeros),
        ("trailing-zeros", trailing_zeros),
        ("rotate-left", rotate_left),
        ("rotate-right", rotate_right),
        ("test", test),
        ("set", set_bit),
        ("clear", clear_bit),
        ("toggle", toggle_bit),
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn two_ints(name: &str, args: &[Value]) -> Result<(i64, i64), String> {
    match args {
        [Value::Int(a), Value::Int(b)] => Ok((*a, *b)),
        _ => Err(format!("`bit/{name}` requires 2 Int arguments, got {}", args.len())),
    }
}

fn one_int(name: &str, args: &[Value]) -> Result<i64, String> {
    match args {
        [Value::Int(a)] => Ok(*a),
        _ => Err(format!("`bit/{name}` requires 1 Int argument, got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// Core bitwise
// ---------------------------------------------------------------------------

/// `(bit/and a b)` → `Int` — bitwise AND.
fn and(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_ints("and", args)?;
    Ok(Value::Int(a & b))
}

/// `(bit/or a b)` → `Int` — bitwise OR.
fn or(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_ints("or", args)?;
    Ok(Value::Int(a | b))
}

/// `(bit/xor a b)` → `Int` — bitwise XOR.
fn xor(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_ints("xor", args)?;
    Ok(Value::Int(a ^ b))
}

/// `(bit/not a)` → `Int` — bitwise complement.
fn not(args: &[Value]) -> Result<Value, String> {
    let a = one_int("not", args)?;
    Ok(Value::Int(!a))
}

/// `(bit/shift-left a n)` → `Int` — left shift by n positions (wrapping).
fn shift_left(args: &[Value]) -> Result<Value, String> {
    let (a, n) = two_ints("shift-left", args)?;
    Ok(Value::Int(a.wrapping_shl(n as u32)))
}

/// `(bit/shift-right a n)` → `Int` — arithmetic right shift by n positions (wrapping).
fn shift_right(args: &[Value]) -> Result<Value, String> {
    let (a, n) = two_ints("shift-right", args)?;
    Ok(Value::Int(a.wrapping_shr(n as u32)))
}

// ---------------------------------------------------------------------------
// Counting
// ---------------------------------------------------------------------------

/// `(bit/count-ones a)` → `Int` — population count (number of 1 bits).
fn count_ones(args: &[Value]) -> Result<Value, String> {
    let a = one_int("count-ones", args)?;
    Ok(Value::Int(a.count_ones() as i64))
}

/// `(bit/count-zeros a)` → `Int` — number of 0 bits.
fn count_zeros(args: &[Value]) -> Result<Value, String> {
    let a = one_int("count-zeros", args)?;
    Ok(Value::Int(a.count_zeros() as i64))
}

/// `(bit/leading-zeros a)` → `Int` — number of leading 0 bits.
fn leading_zeros(args: &[Value]) -> Result<Value, String> {
    let a = one_int("leading-zeros", args)?;
    Ok(Value::Int(a.leading_zeros() as i64))
}

/// `(bit/trailing-zeros a)` → `Int` — number of trailing 0 bits.
fn trailing_zeros(args: &[Value]) -> Result<Value, String> {
    let a = one_int("trailing-zeros", args)?;
    Ok(Value::Int(a.trailing_zeros() as i64))
}

// ---------------------------------------------------------------------------
// Rotation
// ---------------------------------------------------------------------------

/// `(bit/rotate-left a n)` → `Int` — rotate bits left by n.
fn rotate_left(args: &[Value]) -> Result<Value, String> {
    let (a, n) = two_ints("rotate-left", args)?;
    Ok(Value::Int(a.rotate_left(n as u32)))
}

/// `(bit/rotate-right a n)` → `Int` — rotate bits right by n.
fn rotate_right(args: &[Value]) -> Result<Value, String> {
    let (a, n) = two_ints("rotate-right", args)?;
    Ok(Value::Int(a.rotate_right(n as u32)))
}

// ---------------------------------------------------------------------------
// Bit manipulation
// ---------------------------------------------------------------------------

/// `(bit/test a pos)` → `Bool` — is bit at position `pos` set (1)?
fn test(args: &[Value]) -> Result<Value, String> {
    let (a, pos) = two_ints("test", args)?;
    Ok(Value::Bool((a >> (pos & 63)) & 1 == 1))
}

/// `(bit/set a pos)` → `Int` — set bit at position `pos` to 1.
fn set_bit(args: &[Value]) -> Result<Value, String> {
    let (a, pos) = two_ints("set", args)?;
    Ok(Value::Int(a | (1i64 << (pos & 63))))
}

/// `(bit/clear a pos)` → `Int` — clear bit at position `pos` to 0.
fn clear_bit(args: &[Value]) -> Result<Value, String> {
    let (a, pos) = two_ints("clear", args)?;
    Ok(Value::Int(a & !(1i64 << (pos & 63))))
}

/// `(bit/toggle a pos)` → `Int` — flip bit at position `pos`.
fn toggle_bit(args: &[Value]) -> Result<Value, String> {
    let (a, pos) = two_ints("toggle", args)?;
    Ok(Value::Int(a ^ (1i64 << (pos & 63))))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn i(n: i64) -> Value { Value::Int(n) }

    #[test]
    fn test_and() {
        assert_eq!(and(&[i(0b1100), i(0b1010)]).unwrap(), i(0b1000));
    }

    #[test]
    fn test_or() {
        assert_eq!(or(&[i(0b1100), i(0b1010)]).unwrap(), i(0b1110));
    }

    #[test]
    fn test_xor() {
        assert_eq!(xor(&[i(0b1100), i(0b1010)]).unwrap(), i(0b0110));
    }

    #[test]
    fn test_not() {
        assert_eq!(not(&[i(0)]).unwrap(), i(-1));
        assert_eq!(not(&[i(-1)]).unwrap(), i(0));
    }

    #[test]
    fn test_shift_left() {
        assert_eq!(shift_left(&[i(1), i(4)]).unwrap(), i(16));
    }

    #[test]
    fn test_shift_right() {
        assert_eq!(shift_right(&[i(16), i(2)]).unwrap(), i(4));
        // arithmetic: sign bit preserved
        assert_eq!(shift_right(&[i(-8), i(1)]).unwrap(), i(-4));
    }

    #[test]
    fn test_count_ones() {
        assert_eq!(count_ones(&[i(0b10110)]).unwrap(), i(3));
    }

    #[test]
    fn test_count_zeros() {
        // i64 has 64 bits; 0b10110 has 3 ones → 61 zeros
        assert_eq!(count_zeros(&[i(0b10110)]).unwrap(), i(61));
    }

    #[test]
    fn test_leading_zeros() {
        assert_eq!(leading_zeros(&[i(1)]).unwrap(), i(63));
        assert_eq!(leading_zeros(&[i(0b1000)]).unwrap(), i(60));
    }

    #[test]
    fn test_trailing_zeros() {
        assert_eq!(trailing_zeros(&[i(8)]).unwrap(), i(3));
        assert_eq!(trailing_zeros(&[i(1)]).unwrap(), i(0));
    }

    #[test]
    fn test_rotate_left() {
        // rotating 1 left by 1 = 2
        assert_eq!(rotate_left(&[i(1), i(1)]).unwrap(), i(2));
    }

    #[test]
    fn test_rotate_right() {
        // rotating 2 right by 1 = 1
        assert_eq!(rotate_right(&[i(2), i(1)]).unwrap(), i(1));
    }

    #[test]
    fn test_test_bit() {
        assert_eq!(test(&[i(0b1010), i(1)]).unwrap(), Value::Bool(true));
        assert_eq!(test(&[i(0b1010), i(0)]).unwrap(), Value::Bool(false));
    }

    #[test]
    fn test_set_bit() {
        assert_eq!(set_bit(&[i(0b1000), i(0)]).unwrap(), i(0b1001));
    }

    #[test]
    fn test_clear_bit() {
        assert_eq!(clear_bit(&[i(0b1111), i(2)]).unwrap(), i(0b1011));
    }

    #[test]
    fn test_toggle_bit() {
        assert_eq!(toggle_bit(&[i(0b1010), i(0)]).unwrap(), i(0b1011));
        assert_eq!(toggle_bit(&[i(0b1010), i(1)]).unwrap(), i(0b1000));
    }
}
