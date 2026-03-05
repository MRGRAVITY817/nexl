//! `crypto` module — cryptographic functions.
//!
//! Provides hashing (SHA-256, SHA-512, BLAKE3, HMAC-SHA256), password hashing
//! (PBKDF2), hex encoding, and timing-safe comparison.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use blake3::Hasher as Blake3Hasher;
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use sha2::{Digest, Sha256, Sha512};
use nexl_runtime::Value;

use crate::StdlibEntry;

type HmacSha256 = Hmac<Sha256>;

/// Return all `crypto` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("hash", hash_fn as fn(&[Value]) -> Result<Value, String>),
        ("constant-time=", constant_time_eq),
        ("sha256", sha256),
        ("sha256-bytes", sha256_bytes),
        ("sha512", sha512),
        ("hmac-sha256", hmac_sha256),
        ("blake3", blake3_hash),
        ("random-bytes", random_bytes),
        ("pbkdf2", pbkdf2_hash),
        ("verify-pbkdf2", verify_pbkdf2),
        ("hex-encode", hex_encode),
        ("hex-decode", hex_decode),
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn adt(type_name: &str, ctor: &str, fields: Vec<Value>) -> Value {
    Value::Adt {
        type_name: Rc::from(type_name),
        ctor: Rc::from(ctor),
        fields: Rc::new(fields),
    }
}

fn ok(v: Value) -> Value { adt("Result", "Ok", vec![v]) }
fn err_val(msg: &str) -> Value { adt("Result", "Err", vec![Value::Str(Rc::from(msg))]) }

fn one_arg<'a>(op: &str, args: &'a [Value]) -> Result<&'a Value, String> {
    match args {
        [a] => Ok(a),
        _ => Err(format!("`crypto/{op}` requires exactly 1 argument, got {}", args.len())),
    }
}

fn two_args<'a>(op: &str, args: &'a [Value]) -> Result<(&'a Value, &'a Value), String> {
    match args {
        [a, b] => Ok((a, b)),
        _ => Err(format!("`crypto/{op}` requires exactly 2 arguments, got {}", args.len())),
    }
}

fn expect_str<'a>(op: &str, v: &'a Value) -> Result<&'a str, String> {
    match v {
        Value::Str(s) => Ok(s.as_ref()),
        other => Err(format!("`crypto/{op}` expected Str, got {}", other.type_name())),
    }
}

fn expect_int(op: &str, v: &Value) -> Result<i64, String> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(format!("`crypto/{op}` expected Int, got {other}")),
    }
}

fn expect_vec<'a>(op: &str, v: &'a Value) -> Result<&'a [Value], String> {
    match v {
        Value::Vec(items) => Ok(items.as_ref()),
        other => Err(format!("`crypto/{op}` expected Vec, got {other}")),
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn vec_to_bytes(op: &str, items: &[Value]) -> Result<Vec<u8>, String> {
    items.iter().map(|v| match v {
        Value::Int(n) => {
            if *n >= 0 && *n <= 255 {
                Ok(*n as u8)
            } else {
                Err(format!("`crypto/{op}` byte value out of range: {n}"))
            }
        }
        other => Err(format!("`crypto/{op}` expected Int byte, got {other}")),
    }).collect()
}

fn bytes_to_nexl_vec(bytes: &[u8]) -> Value {
    let items: Vec<Value> = bytes.iter().map(|&b| Value::Int(b as i64)).collect();
    Value::Vec(Rc::new(items))
}

// ---------------------------------------------------------------------------
// Original functions
// ---------------------------------------------------------------------------

/// `(crypto/hash s)` — compute a hash of a string (returns Int).
///
/// Uses Rust's DefaultHasher. NOT cryptographically secure.
fn hash_fn(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("hash", args)?;
    let s = expect_str("hash", v)?;
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    Ok(Value::Int(hasher.finish() as i64))
}

/// `(crypto/constant-time= a b)` — constant-time string comparison.
fn constant_time_eq(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("constant-time=", args)?;
    let a = expect_str("constant-time=", a)?;
    let b = expect_str("constant-time=", b)?;

    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();

    if a_bytes.len() != b_bytes.len() {
        return Ok(Value::Bool(false));
    }

    let mut diff: u8 = 0;
    for (x, y) in a_bytes.iter().zip(b_bytes.iter()) {
        diff |= x ^ y;
    }
    Ok(Value::Bool(diff == 0))
}

// ---------------------------------------------------------------------------
// New hash functions
// ---------------------------------------------------------------------------

/// `(crypto/sha256 str)` → `Str` — SHA-256 hex digest.
fn sha256(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("sha256", args)?;
    let s = expect_str("sha256", v)?;
    let digest = Sha256::digest(s.as_bytes());
    Ok(Value::Str(Rc::from(bytes_to_hex(&digest).as_str())))
}

/// `(crypto/sha256-bytes bytes)` → `(Vec Int)` — SHA-256 raw bytes from a Vec of byte Ints.
fn sha256_bytes(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let items = expect_vec("sha256-bytes", v)?;
            let input = vec_to_bytes("sha256-bytes", items)?;
            let digest = Sha256::digest(&input);
            Ok(bytes_to_nexl_vec(&digest))
        }
        _ => Err(format!("`crypto/sha256-bytes` requires 1 argument (Vec), got {}", args.len())),
    }
}

/// `(crypto/sha512 str)` → `Str` — SHA-512 hex digest.
fn sha512(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("sha512", args)?;
    let s = expect_str("sha512", v)?;
    let digest = Sha512::digest(s.as_bytes());
    Ok(Value::Str(Rc::from(bytes_to_hex(&digest).as_str())))
}

/// `(crypto/hmac-sha256 key message)` → `Str` — HMAC-SHA256 hex digest.
fn hmac_sha256(args: &[Value]) -> Result<Value, String> {
    let (key_val, msg_val) = two_args("hmac-sha256", args)?;
    let key = expect_str("hmac-sha256", key_val)?;
    let msg = expect_str("hmac-sha256", msg_val)?;
    let mut mac = HmacSha256::new_from_slice(key.as_bytes())
        .map_err(|e| e.to_string())?;
    mac.update(msg.as_bytes());
    let result = mac.finalize();
    Ok(Value::Str(Rc::from(bytes_to_hex(&result.into_bytes()).as_str())))
}

/// `(crypto/blake3 str)` → `Str` — BLAKE3 hex digest.
fn blake3_hash(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("blake3", args)?;
    let s = expect_str("blake3", v)?;
    let mut hasher = Blake3Hasher::new();
    hasher.update(s.as_bytes());
    let digest = hasher.finalize();
    Ok(Value::Str(Rc::from(digest.to_hex().as_str())))
}

/// `(crypto/random-bytes n)` → `(Vec Int)` — generate `n` cryptographic random bytes.
fn random_bytes(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("random-bytes", args)?;
    let n = expect_int("random-bytes", v)?;
    if n < 0 || n > 65536 {
        return Err(format!("`crypto/random-bytes` n must be in [0, 65536], got {n}"));
    }
    let mut bytes = vec![0u8; n as usize];
    getrandom::getrandom(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes_to_nexl_vec(&bytes))
}

/// `(crypto/pbkdf2 password salt iterations)` → `Str` — PBKDF2-HMAC-SHA256 hex digest.
fn pbkdf2_hash(args: &[Value]) -> Result<Value, String> {
    match args {
        [pw_val, salt_val, iter_val] => {
            let password = expect_str("pbkdf2", pw_val)?;
            let salt = expect_str("pbkdf2", salt_val)?;
            let iterations = expect_int("pbkdf2", iter_val)?;
            if iterations <= 0 {
                return Err(format!("`crypto/pbkdf2` iterations must be > 0, got {iterations}"));
            }
            let mut output = [0u8; 32];
            pbkdf2_hmac::<Sha256>(
                password.as_bytes(),
                salt.as_bytes(),
                iterations as u32,
                &mut output,
            );
            Ok(Value::Str(Rc::from(bytes_to_hex(&output).as_str())))
        }
        _ => Err(format!(
            "`crypto/pbkdf2` requires 3 arguments (Str Str Int), got {}",
            args.len()
        )),
    }
}

/// `(crypto/verify-pbkdf2 password salt iterations expected-hash)` → `Bool`.
fn verify_pbkdf2(args: &[Value]) -> Result<Value, String> {
    match args {
        [pw_val, salt_val, iter_val, expected_val] => {
            let password = expect_str("verify-pbkdf2", pw_val)?;
            let salt = expect_str("verify-pbkdf2", salt_val)?;
            let iterations = expect_int("verify-pbkdf2", iter_val)?;
            let expected = expect_str("verify-pbkdf2", expected_val)?;
            if iterations <= 0 {
                return Ok(Value::Bool(false));
            }
            let mut output = [0u8; 32];
            pbkdf2_hmac::<Sha256>(
                password.as_bytes(),
                salt.as_bytes(),
                iterations as u32,
                &mut output,
            );
            let actual = bytes_to_hex(&output);
            // Constant-time comparison
            let a = actual.as_bytes();
            let b = expected.as_bytes();
            if a.len() != b.len() {
                return Ok(Value::Bool(false));
            }
            let mut diff: u8 = 0;
            for (x, y) in a.iter().zip(b.iter()) {
                diff |= x ^ y;
            }
            Ok(Value::Bool(diff == 0))
        }
        _ => Err(format!(
            "`crypto/verify-pbkdf2` requires 4 arguments, got {}",
            args.len()
        )),
    }
}

/// `(crypto/hex-encode bytes)` → `Str` — encode a `(Vec Int)` of bytes to hex string.
fn hex_encode(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let items = expect_vec("hex-encode", v)?;
            let bytes = vec_to_bytes("hex-encode", items)?;
            Ok(Value::Str(Rc::from(bytes_to_hex(&bytes).as_str())))
        }
        _ => Err(format!("`crypto/hex-encode` requires 1 argument (Vec), got {}", args.len())),
    }
}

/// `(crypto/hex-decode str)` → `(Result (Vec Int) Str)` — decode hex to bytes.
fn hex_decode(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let s = expect_str("hex-decode", v)?;
            match hex::decode(s) {
                Ok(bytes) => Ok(ok(bytes_to_nexl_vec(&bytes))),
                Err(e) => Ok(err_val(&e.to_string())),
            }
        }
        _ => Err(format!("`crypto/hex-decode` requires 1 argument (Str), got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Value { Value::Str(Rc::from(text)) }

    #[test]
    fn test_hash_deterministic() {
        let h1 = hash_fn(&[s("hello")]).unwrap();
        let h2 = hash_fn(&[s("hello")]).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_different_inputs() {
        let h1 = hash_fn(&[s("hello")]).unwrap();
        let h2 = hash_fn(&[s("world")]).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_constant_time_eq_same() {
        assert_eq!(
            constant_time_eq(&[s("secret"), s("secret")]).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn test_constant_time_eq_different() {
        assert_eq!(
            constant_time_eq(&[s("secret"), s("other!")]).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn test_constant_time_eq_different_lengths() {
        assert_eq!(
            constant_time_eq(&[s("short"), s("longer")]).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn test_sha256_known() {
        // SHA-256("") = e3b0c44298fc1c149afb...
        let result = sha256(&[s("")]).unwrap();
        if let Value::Str(hex) = result {
            assert!(hex.starts_with("e3b0c442"));
            assert_eq!(hex.len(), 64);
        }
    }

    #[test]
    fn test_sha256_hello() {
        let result = sha256(&[s("hello")]).unwrap();
        if let Value::Str(hex) = result {
            assert_eq!(hex.as_ref(), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        }
    }

    #[test]
    fn test_sha512_len() {
        let result = sha512(&[s("test")]).unwrap();
        if let Value::Str(hex) = result {
            assert_eq!(hex.len(), 128);
        }
    }

    #[test]
    fn test_hmac_sha256_deterministic() {
        let r1 = hmac_sha256(&[s("key"), s("message")]).unwrap();
        let r2 = hmac_sha256(&[s("key"), s("message")]).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_hmac_sha256_different_keys() {
        let r1 = hmac_sha256(&[s("key1"), s("msg")]).unwrap();
        let r2 = hmac_sha256(&[s("key2"), s("msg")]).unwrap();
        assert_ne!(r1, r2);
    }

    #[test]
    fn test_blake3_len() {
        let result = blake3_hash(&[s("hello")]).unwrap();
        if let Value::Str(hex) = result {
            assert_eq!(hex.len(), 64);
        }
    }

    #[test]
    fn test_hex_encode_decode_roundtrip() {
        let bytes = Value::Vec(Rc::new(vec![
            Value::Int(0xDE), Value::Int(0xAD), Value::Int(0xBE), Value::Int(0xEF),
        ]));
        let encoded = hex_encode(&[bytes]).unwrap();
        if let Value::Str(ref hex) = encoded {
            assert_eq!(hex.as_ref(), "deadbeef");
        }
        let decoded = hex_decode(&[encoded]).unwrap();
        if let Value::Adt { ctor, fields, .. } = decoded {
            assert_eq!(ctor.as_ref(), "Ok");
            if let Value::Vec(items) = &fields[0] {
                assert_eq!(items.len(), 4);
                assert_eq!(items[0], Value::Int(0xDE));
            }
        }
    }

    #[test]
    fn test_hex_decode_invalid() {
        let result = hex_decode(&[s("not-hex!")]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Err"));
    }

    #[test]
    fn test_sha256_bytes_len() {
        let bytes = Value::Vec(Rc::new(vec![Value::Int(0), Value::Int(1), Value::Int(2)]));
        let result = sha256_bytes(&[bytes]).unwrap();
        if let Value::Vec(items) = result {
            assert_eq!(items.len(), 32);
        }
    }

    #[test]
    fn test_pbkdf2_deterministic() {
        let r1 = pbkdf2_hash(&[s("password"), s("salt"), Value::Int(1000)]).unwrap();
        let r2 = pbkdf2_hash(&[s("password"), s("salt"), Value::Int(1000)]).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_verify_pbkdf2() {
        let hash = pbkdf2_hash(&[s("pass"), s("salt"), Value::Int(1000)]).unwrap();
        if let Value::Str(ref hex) = hash {
            let ok_result = verify_pbkdf2(&[s("pass"), s("salt"), Value::Int(1000), Value::Str(Rc::clone(hex))]).unwrap();
            assert_eq!(ok_result, Value::Bool(true));
            let fail_result = verify_pbkdf2(&[s("wrong"), s("salt"), Value::Int(1000), Value::Str(Rc::clone(hex))]).unwrap();
            assert_eq!(fail_result, Value::Bool(false));
        }
    }
}
