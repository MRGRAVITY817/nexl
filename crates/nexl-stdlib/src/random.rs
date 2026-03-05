//! `random` module — cryptographically secure random number generation.
//!
//! All randomness is sourced from the OS via `getrandom` — cryptographically
//! secure. For deterministic PRNG (tests), use `gen/`.

use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `random` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("bytes", bytes as fn(&[Value]) -> Result<Value, String>),
        ("int", int),
        ("float", float),
        ("bool", bool_fn),
        ("choice", choice),
        ("shuffle", shuffle),
        ("sample", sample),
        ("uuid", uuid_str),
        ("weighted-choice", weighted_choice),
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

fn some(v: Value) -> Value { adt("Option", "Some", vec![v]) }
fn none() -> Value { adt("Option", "None", vec![]) }

fn expect_int(name: &str, v: &Value) -> Result<i64, String> {
    match v {
        Value::Int(n) => Ok(*n),
        other => Err(format!("`random/{name}` expected Int, got {other}")),
    }
}

fn expect_float(name: &str, v: &Value) -> Result<f64, String> {
    match v {
        Value::Float(f) => Ok(*f),
        Value::Int(n) => Ok(*n as f64),
        other => Err(format!("`random/{name}` expected Float or Int, got {other}")),
    }
}

fn expect_vec<'a>(name: &str, v: &'a Value) -> Result<&'a [Value], String> {
    match v {
        Value::Vec(items) => Ok(items.as_ref()),
        other => Err(format!("`random/{name}` expected Vec, got {other}")),
    }
}

/// Generate `n` random bytes using the OS CSPRNG.
fn random_bytes_raw(n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Generate a random u64.
fn random_u64() -> Result<u64, String> {
    let bytes = random_bytes_raw(8)?;
    Ok(u64::from_le_bytes(bytes.try_into().expect("8 bytes")))
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// `(random/bytes n)` — return a `Vec` of `n` random bytes (each `Int` 0–255).
fn bytes(args: &[Value]) -> Result<Value, String> {
    let n = match args {
        [Value::Int(n)] => *n,
        [other] => return Err(format!("`random/bytes` expected Int, got {}", other.type_name())),
        _ => return Err(format!("`random/bytes` requires exactly 1 argument, got {}", args.len())),
    };
    if n < 0 {
        return Err(format!("`random/bytes` count must be non-negative, got {n}"));
    }
    let buf = random_bytes_raw(n as usize)?;
    let values: Vec<Value> = buf.into_iter().map(|b| Value::Int(b as i64)).collect();
    Ok(Value::Vec(Rc::new(values)))
}

/// `(random/int lo hi)` → `Int` — random integer in `[lo, hi)`.
fn int(args: &[Value]) -> Result<Value, String> {
    match args {
        [lo_val, hi_val] => {
            let lo = expect_int("int", lo_val)?;
            let hi = expect_int("int", hi_val)?;
            if hi <= lo {
                return Err(format!("`random/int` requires hi > lo, got lo={lo} hi={hi}"));
            }
            let range = (hi - lo) as u64;
            let r = random_u64()? % range;
            Ok(Value::Int(lo + r as i64))
        }
        _ => Err(format!("`random/int` requires 2 arguments (Int Int), got {}", args.len())),
    }
}

/// `(random/float)` → `Float` — random float in `[0.0, 1.0)`.
fn float(args: &[Value]) -> Result<Value, String> {
    match args {
        [] => {
            let n = random_u64()?;
            // Map u64 to [0.0, 1.0)
            let f = (n >> 11) as f64 / (1u64 << 53) as f64;
            Ok(Value::Float(f))
        }
        _ => Err(format!("`random/float` requires 0 arguments, got {}", args.len())),
    }
}

/// `(random/bool)` → `Bool` — random boolean.
fn bool_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [] => {
            let b = random_bytes_raw(1)?;
            Ok(Value::Bool(b[0] & 1 == 1))
        }
        _ => Err(format!("`random/bool` requires 0 arguments, got {}", args.len())),
    }
}

/// `(random/choice vec)` → `(Option a)` — pick a random element, or `None` if empty.
fn choice(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let items = expect_vec("choice", v)?;
            if items.is_empty() {
                return Ok(none());
            }
            let idx = (random_u64()? % items.len() as u64) as usize;
            Ok(some(items[idx].clone()))
        }
        _ => Err(format!("`random/choice` requires 1 argument (Vec), got {}", args.len())),
    }
}

/// `(random/shuffle vec)` → `Vec` — Fisher-Yates shuffle (returns new vec).
fn shuffle(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let items = expect_vec("shuffle", v)?;
            let mut result: Vec<Value> = items.to_vec();
            let n = result.len();
            for i in (1..n).rev() {
                let j = (random_u64()? % (i + 1) as u64) as usize;
                result.swap(i, j);
            }
            Ok(Value::Vec(Rc::new(result)))
        }
        _ => Err(format!("`random/shuffle` requires 1 argument (Vec), got {}", args.len())),
    }
}

/// `(random/sample n vec)` → `Vec` — pick `n` random elements without replacement.
fn sample(args: &[Value]) -> Result<Value, String> {
    match args {
        [n_val, v] => {
            let n = expect_int("sample", n_val)?;
            let items = expect_vec("sample", v)?;
            if n < 0 {
                return Err(format!("`random/sample` n must be non-negative, got {n}"));
            }
            let n = n as usize;
            if n > items.len() {
                return Err(format!(
                    "`random/sample` n={n} exceeds vec length {}",
                    items.len()
                ));
            }
            // Partial Fisher-Yates
            let mut pool: Vec<Value> = items.to_vec();
            let mut result = Vec::with_capacity(n);
            for i in 0..n {
                let remaining = pool.len() - i;
                let j = i + (random_u64()? % remaining as u64) as usize;
                pool.swap(i, j);
                result.push(pool[i].clone());
            }
            Ok(Value::Vec(Rc::new(result)))
        }
        _ => Err(format!("`random/sample` requires 2 arguments (Int Vec), got {}", args.len())),
    }
}

/// `(random/uuid)` → `Str` — random UUID v4 string.
fn uuid_str(args: &[Value]) -> Result<Value, String> {
    match args {
        [] => {
            let u = uuid::Uuid::new_v4();
            Ok(Value::Str(Rc::from(u.hyphenated().to_string().as_str())))
        }
        _ => Err(format!("`random/uuid` requires 0 arguments, got {}", args.len())),
    }
}

/// `(random/weighted-choice weights-and-items)` → `(Option a)`.
///
/// Argument is `(Vec (Tuple a Float))` — pairs of `[item weight]`.
/// Weights must be non-negative. Returns `None` if input is empty.
fn weighted_choice(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let items = expect_vec("weighted-choice", v)?;
            if items.is_empty() {
                return Ok(none());
            }
            // Each element should be a Vec [item weight]
            let mut pairs: Vec<(&Value, f64)> = Vec::new();
            let mut total = 0.0f64;
            for item in items {
                match item {
                    Value::Vec(pair) if pair.len() == 2 => {
                        let weight = expect_float("weighted-choice", &pair[1])?;
                        if weight < 0.0 {
                            return Err(format!(
                                "`random/weighted-choice` weights must be non-negative, got {weight}"
                            ));
                        }
                        total += weight;
                        pairs.push((&pair[0], weight));
                    }
                    other => {
                        return Err(format!(
                            "`random/weighted-choice` expected [item weight] pair, got {other}"
                        ))
                    }
                }
            }
            if total <= 0.0 {
                return Ok(none());
            }
            // Random float in [0.0, total)
            let n = random_u64()?;
            let r = ((n >> 11) as f64 / (1u64 << 53) as f64) * total;
            let mut cumulative = 0.0f64;
            for (val, weight) in &pairs {
                cumulative += weight;
                if r < cumulative {
                    return Ok(some((*val).clone()));
                }
            }
            // Fallback due to floating-point rounding
            Ok(some(pairs.last().expect("non-empty").0.clone()))
        }
        _ => Err(format!("`random/weighted-choice` requires 1 argument (Vec), got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_bytes_correct_length() {
        let result = bytes(&[Value::Int(16)]).unwrap();
        match result {
            Value::Vec(v) => assert_eq!(v.len(), 16),
            _ => panic!("expected Vec"),
        }
    }

    #[test]
    fn test_random_bytes_values_in_range() {
        let result = bytes(&[Value::Int(32)]).unwrap();
        if let Value::Vec(v) = result {
            for elem in v.iter() {
                if let Value::Int(b) = elem {
                    assert!(*b >= 0 && *b <= 255);
                }
            }
        }
    }

    #[test]
    fn test_random_bytes_zero_length() {
        let result = bytes(&[Value::Int(0)]).unwrap();
        match result {
            Value::Vec(v) => assert!(v.is_empty()),
            _ => panic!("expected Vec"),
        }
    }

    #[test]
    fn test_int_in_range() {
        for _ in 0..20 {
            let result = int(&[Value::Int(0), Value::Int(10)]).unwrap();
            if let Value::Int(n) = result {
                assert!(n >= 0 && n < 10, "expected [0,10), got {n}");
            }
        }
    }

    #[test]
    fn test_int_single_range_error() {
        let result = int(&[Value::Int(5), Value::Int(5)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_float_in_unit() {
        for _ in 0..20 {
            let result = float(&[]).unwrap();
            if let Value::Float(f) = result {
                assert!(f >= 0.0 && f < 1.0, "expected [0,1), got {f}");
            }
        }
    }

    #[test]
    fn test_bool_is_bool() {
        let result = bool_fn(&[]).unwrap();
        assert!(matches!(result, Value::Bool(_)));
    }

    #[test]
    fn test_choice_nonempty() {
        let v = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        let result = choice(&[v]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Some"));
    }

    #[test]
    fn test_choice_empty() {
        let v = Value::Vec(Rc::new(vec![]));
        let result = choice(&[v]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "None"));
    }

    #[test]
    fn test_shuffle_same_length() {
        let v = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        let result = shuffle(&[v]).unwrap();
        if let Value::Vec(items) = result {
            assert_eq!(items.len(), 3);
        }
    }

    #[test]
    fn test_sample_correct_count() {
        let v = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)]));
        let result = sample(&[Value::Int(2), v]).unwrap();
        if let Value::Vec(items) = result {
            assert_eq!(items.len(), 2);
        }
    }

    #[test]
    fn test_uuid_format() {
        let result = uuid_str(&[]).unwrap();
        if let Value::Str(s) = result {
            assert_eq!(s.len(), 36);
            assert_eq!(s.chars().filter(|&c| c == '-').count(), 4);
        }
    }

    #[test]
    fn test_weighted_choice_empty() {
        let v = Value::Vec(Rc::new(vec![]));
        let result = weighted_choice(&[v]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "None"));
    }

    #[test]
    fn test_weighted_choice_single() {
        let pair = Value::Vec(Rc::new(vec![Value::Int(42), Value::Float(1.0)]));
        let v = Value::Vec(Rc::new(vec![pair]));
        let result = weighted_choice(&[v]).unwrap();
        assert!(matches!(result, Value::Adt { ref ctor, .. } if ctor.as_ref() == "Some"));
        if let Value::Adt { fields, .. } = result {
            assert_eq!(fields[0], Value::Int(42));
        }
    }
}
