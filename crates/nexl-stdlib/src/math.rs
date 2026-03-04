//! `math` module — mathematical functions.
//!
//! Provides: `abs`, `floor`, `ceil`, `round`, `pow`, `sqrt`, `log`, `exp`,
//! trig functions (`sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`),
//! `min`, `max`, `clamp`, constants `pi` and `e`.

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `math` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("abs", abs as fn(&[Value]) -> Result<Value, String>),
        ("floor", floor),
        ("ceil", ceil),
        ("round", round),
        ("pow", pow),
        ("sqrt", sqrt),
        ("log", log),
        ("exp", exp),
        ("sin", sin),
        ("cos", cos),
        ("tan", tan),
        ("asin", asin),
        ("acos", acos),
        ("atan", atan),
        ("atan2", atan2),
        ("min", min),
        ("max", max),
        ("clamp", clamp),
        ("pi", pi),
        ("e", euler),
        ("tau", tau),
        ("inf", inf),
        ("neg-inf", neg_inf),
        ("nan", nan_const),
        ("sign", sign),
        ("truncate", truncate),
        ("cbrt", cbrt),
        ("log2", log2),
        ("log10", log10),
        ("exp2", exp2),
        ("sinh", sinh),
        ("cosh", cosh),
        ("tanh", tanh),
        ("nan?", nan_pred),
        ("infinite?", infinite_pred),
        ("finite?", finite_pred),
        ("gcd", gcd),
        ("lcm", lcm),
        ("divmod", divmod),
    ]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn one_arg<'a>(op: &str, args: &'a [Value]) -> Result<&'a Value, String> {
    match args {
        [a] => Ok(a),
        _ => Err(format!(
            "`math/{op}` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

fn two_args<'a>(op: &str, args: &'a [Value]) -> Result<(&'a Value, &'a Value), String> {
    match args {
        [a, b] => Ok((a, b)),
        _ => Err(format!(
            "`math/{op}` requires exactly 2 arguments, got {}",
            args.len()
        )),
    }
}

fn three_args<'a>(
    op: &str,
    args: &'a [Value],
) -> Result<(&'a Value, &'a Value, &'a Value), String> {
    match args {
        [a, b, c] => Ok((a, b, c)),
        _ => Err(format!(
            "`math/{op}` requires exactly 3 arguments, got {}",
            args.len()
        )),
    }
}

fn as_float(op: &str, v: &Value) -> Result<f64, String> {
    match v {
        Value::Float(n) => Ok(*n),
        Value::Int(n) => Ok(*n as f64),
        other => Err(format!(
            "`math/{op}` expected Float or Int, got {}",
            other.type_name()
        )),
    }
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// `(math/abs x)` — absolute value (works for Int and Float).
fn abs(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("abs", args)?;
    match v {
        Value::Int(n) => Ok(Value::Int(n.wrapping_abs())),
        Value::Float(n) => Ok(Value::Float(n.abs())),
        other => Err(format!(
            "`math/abs` expected Int or Float, got {}",
            other.type_name()
        )),
    }
}

/// `(math/floor x)` — floor (returns Float).
fn floor(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("floor", args)?;
    Ok(Value::Float(as_float("floor", v)?.floor()))
}

/// `(math/ceil x)` — ceiling (returns Float).
fn ceil(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("ceil", args)?;
    Ok(Value::Float(as_float("ceil", v)?.ceil()))
}

/// `(math/round x)` — round to nearest integer (returns Float).
fn round(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("round", args)?;
    Ok(Value::Float(as_float("round", v)?.round()))
}

/// `(math/pow base exp)` — exponentiation (returns Float).
fn pow(args: &[Value]) -> Result<Value, String> {
    let (base, exponent) = two_args("pow", args)?;
    let base = as_float("pow", base)?;
    let exp = as_float("pow", exponent)?;
    Ok(Value::Float(base.powf(exp)))
}

/// `(math/sqrt x)` — square root (returns Float).
fn sqrt(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("sqrt", args)?;
    Ok(Value::Float(as_float("sqrt", v)?.sqrt()))
}

/// `(math/log x)` — natural logarithm (returns Float).
fn log(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("log", args)?;
    Ok(Value::Float(as_float("log", v)?.ln()))
}

/// `(math/exp x)` — e^x (returns Float).
fn exp(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("exp", args)?;
    Ok(Value::Float(as_float("exp", v)?.exp()))
}

/// `(math/sin x)` — sine (radians, returns Float).
fn sin(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("sin", args)?;
    Ok(Value::Float(as_float("sin", v)?.sin()))
}

/// `(math/cos x)` — cosine (radians, returns Float).
fn cos(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("cos", args)?;
    Ok(Value::Float(as_float("cos", v)?.cos()))
}

/// `(math/tan x)` — tangent (radians, returns Float).
fn tan(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("tan", args)?;
    Ok(Value::Float(as_float("tan", v)?.tan()))
}

/// `(math/asin x)` — arc sine (returns Float in radians).
fn asin(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("asin", args)?;
    Ok(Value::Float(as_float("asin", v)?.asin()))
}

/// `(math/acos x)` — arc cosine (returns Float in radians).
fn acos(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("acos", args)?;
    Ok(Value::Float(as_float("acos", v)?.acos()))
}

/// `(math/atan x)` — arc tangent (returns Float in radians).
fn atan(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("atan", args)?;
    Ok(Value::Float(as_float("atan", v)?.atan()))
}

/// `(math/atan2 y x)` — two-argument arc tangent (returns Float in radians).
fn atan2(args: &[Value]) -> Result<Value, String> {
    let (y, x) = two_args("atan2", args)?;
    let y = as_float("atan2", y)?;
    let x = as_float("atan2", x)?;
    Ok(Value::Float(y.atan2(x)))
}

/// `(math/min a b)` — minimum of two numbers.
fn min(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("min", args)?;
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(*x.min(y))),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x.min(*y))),
        _ => {
            let fa = as_float("min", a)?;
            let fb = as_float("min", b)?;
            Ok(Value::Float(fa.min(fb)))
        }
    }
}

/// `(math/max a b)` — maximum of two numbers.
fn max(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("max", args)?;
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(*x.max(y))),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x.max(*y))),
        _ => {
            let fa = as_float("max", a)?;
            let fb = as_float("max", b)?;
            Ok(Value::Float(fa.max(fb)))
        }
    }
}

/// `(math/clamp x lo hi)` — clamp x to [lo, hi].
fn clamp(args: &[Value]) -> Result<Value, String> {
    let (x, lo, hi) = three_args("clamp", args)?;
    match (x, lo, hi) {
        (Value::Int(v), Value::Int(l), Value::Int(h)) => Ok(Value::Int(*v.max(l).min(h))),
        _ => {
            let v = as_float("clamp", x)?;
            let l = as_float("clamp", lo)?;
            let h = as_float("clamp", hi)?;
            Ok(Value::Float(v.max(l).min(h)))
        }
    }
}

/// `(math/pi)` — the constant π.
fn pi(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`math/pi` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Float(std::f64::consts::PI))
}

/// `(math/e)` — the constant e.
fn euler(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`math/e` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Float(std::f64::consts::E))
}

/// `(math/tau)` — 2π.
fn tau(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`math/tau` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Float(std::f64::consts::TAU))
}

/// `(math/inf)` — positive infinity.
fn inf(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`math/inf` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Float(f64::INFINITY))
}

/// `(math/neg-inf)` — negative infinity.
fn neg_inf(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`math/neg-inf` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Float(f64::NEG_INFINITY))
}

/// `(math/nan)` — NaN.
fn nan_const(args: &[Value]) -> Result<Value, String> {
    if !args.is_empty() {
        return Err(format!("`math/nan` takes no arguments, got {}", args.len()));
    }
    Ok(Value::Float(f64::NAN))
}

/// `(math/sign x)` — return -1, 0, or 1.
fn sign(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("sign", args)?;
    match v {
        Value::Int(n) => Ok(Value::Int(n.signum())),
        Value::Float(n) => {
            if n.is_nan() {
                Ok(Value::Float(f64::NAN))
            } else {
                Ok(Value::Float(n.signum()))
            }
        }
        other => Err(format!("`math/sign` expected Int or Float, got {}", other.type_name())),
    }
}

/// `(math/truncate x)` — truncate toward zero.
fn truncate(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("truncate", args)?;
    Ok(Value::Float(as_float("truncate", v)?.trunc()))
}

/// `(math/cbrt x)` — cube root.
fn cbrt(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("cbrt", args)?;
    Ok(Value::Float(as_float("cbrt", v)?.cbrt()))
}

/// `(math/log2 x)` — base-2 logarithm.
fn log2(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("log2", args)?;
    Ok(Value::Float(as_float("log2", v)?.log2()))
}

/// `(math/log10 x)` — base-10 logarithm.
fn log10(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("log10", args)?;
    Ok(Value::Float(as_float("log10", v)?.log10()))
}

/// `(math/exp2 x)` — 2^x.
fn exp2(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("exp2", args)?;
    Ok(Value::Float(as_float("exp2", v)?.exp2()))
}

/// `(math/sinh x)` — hyperbolic sine.
fn sinh(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("sinh", args)?;
    Ok(Value::Float(as_float("sinh", v)?.sinh()))
}

/// `(math/cosh x)` — hyperbolic cosine.
fn cosh(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("cosh", args)?;
    Ok(Value::Float(as_float("cosh", v)?.cosh()))
}

/// `(math/tanh x)` — hyperbolic tangent.
fn tanh(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("tanh", args)?;
    Ok(Value::Float(as_float("tanh", v)?.tanh()))
}

/// `(math/nan? x)` — true if x is NaN.
fn nan_pred(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("nan?", args)?;
    match v {
        Value::Float(n) => Ok(Value::Bool(n.is_nan())),
        Value::Int(_) => Ok(Value::Bool(false)),
        other => Err(format!("`math/nan?` expected Float or Int, got {}", other.type_name())),
    }
}

/// `(math/infinite? x)` — true if x is ±infinity.
fn infinite_pred(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("infinite?", args)?;
    match v {
        Value::Float(n) => Ok(Value::Bool(n.is_infinite())),
        Value::Int(_) => Ok(Value::Bool(false)),
        other => Err(format!("`math/infinite?` expected Float or Int, got {}", other.type_name())),
    }
}

/// `(math/finite? x)` — true if x is a finite number (not NaN or ±infinity).
fn finite_pred(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("finite?", args)?;
    match v {
        Value::Float(n) => Ok(Value::Bool(n.is_finite())),
        Value::Int(_) => Ok(Value::Bool(true)),
        other => Err(format!("`math/finite?` expected Float or Int, got {}", other.type_name())),
    }
}

/// `(math/gcd a b)` — greatest common divisor.
fn gcd(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("gcd", args)?;
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            let mut a = x.unsigned_abs();
            let mut b = y.unsigned_abs();
            while b != 0 {
                let t = b;
                b = a % b;
                a = t;
            }
            Ok(Value::Int(a as i64))
        }
        _ => Err("`math/gcd` requires two Int arguments".into()),
    }
}

/// `(math/lcm a b)` — least common multiple.
fn lcm(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("lcm", args)?;
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            if *x == 0 || *y == 0 {
                return Ok(Value::Int(0));
            }
            let g = {
                let mut a = x.unsigned_abs();
                let mut b = y.unsigned_abs();
                while b != 0 {
                    let t = b;
                    b = a % b;
                    a = t;
                }
                a as i64
            };
            Ok(Value::Int((x / g * y).abs()))
        }
        _ => Err("`math/lcm` requires two Int arguments".into()),
    }
}

/// `(math/divmod a b)` — return [quotient remainder] as a Vec.
fn divmod(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("divmod", args)?;
    match (a, b) {
        (Value::Int(n), Value::Int(d)) => {
            if *d == 0 {
                return Err("division by zero".into());
            }
            Ok(Value::Vec(std::rc::Rc::new(vec![
                Value::Int(n / d),
                Value::Int(n % d),
            ])))
        }
        _ => Err("`math/divmod` requires two Int arguments".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abs_int() {
        assert_eq!(abs(&[Value::Int(-5)]).unwrap(), Value::Int(5));
        assert_eq!(abs(&[Value::Int(5)]).unwrap(), Value::Int(5));
    }

    #[test]
    fn test_abs_float() {
        assert_eq!(abs(&[Value::Float(-3.5)]).unwrap(), Value::Float(3.5));
    }

    #[test]
    fn test_floor() {
        assert_eq!(floor(&[Value::Float(3.7)]).unwrap(), Value::Float(3.0));
        assert_eq!(floor(&[Value::Float(-3.2)]).unwrap(), Value::Float(-4.0));
    }

    #[test]
    fn test_ceil() {
        assert_eq!(ceil(&[Value::Float(3.2)]).unwrap(), Value::Float(4.0));
    }

    #[test]
    fn test_round() {
        assert_eq!(round(&[Value::Float(3.5)]).unwrap(), Value::Float(4.0));
        assert_eq!(round(&[Value::Float(3.4)]).unwrap(), Value::Float(3.0));
    }

    #[test]
    fn test_pow() {
        assert_eq!(
            pow(&[Value::Float(2.0), Value::Float(3.0)]).unwrap(),
            Value::Float(8.0)
        );
    }

    #[test]
    fn test_sqrt() {
        assert_eq!(sqrt(&[Value::Float(9.0)]).unwrap(), Value::Float(3.0));
    }

    #[test]
    fn test_log_exp_roundtrip() {
        let e_val = exp(&[Value::Float(1.0)]).unwrap();
        if let Value::Float(e) = e_val {
            let result = log(&[Value::Float(e)]).unwrap();
            if let Value::Float(r) = result {
                assert!((r - 1.0).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn test_sin_cos() {
        let s = sin(&[Value::Float(0.0)]).unwrap();
        let c = cos(&[Value::Float(0.0)]).unwrap();
        assert_eq!(s, Value::Float(0.0));
        assert_eq!(c, Value::Float(1.0));
    }

    #[test]
    fn test_min_max_int() {
        assert_eq!(min(&[Value::Int(3), Value::Int(5)]).unwrap(), Value::Int(3));
        assert_eq!(max(&[Value::Int(3), Value::Int(5)]).unwrap(), Value::Int(5));
    }

    #[test]
    fn test_clamp() {
        assert_eq!(
            clamp(&[Value::Int(10), Value::Int(0), Value::Int(5)]).unwrap(),
            Value::Int(5)
        );
        assert_eq!(
            clamp(&[Value::Int(-1), Value::Int(0), Value::Int(5)]).unwrap(),
            Value::Int(0)
        );
        assert_eq!(
            clamp(&[Value::Int(3), Value::Int(0), Value::Int(5)]).unwrap(),
            Value::Int(3)
        );
    }

    #[test]
    fn test_pi() {
        assert_eq!(pi(&[]).unwrap(), Value::Float(std::f64::consts::PI));
    }

    #[test]
    fn test_e() {
        assert_eq!(euler(&[]).unwrap(), Value::Float(std::f64::consts::E));
    }

    #[test]
    fn test_int_auto_promote() {
        // math functions accepting Float should also accept Int via promotion
        assert_eq!(sqrt(&[Value::Int(9)]).unwrap(), Value::Float(3.0));
    }
}
