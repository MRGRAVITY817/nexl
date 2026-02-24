//! Standard built-in functions pre-loaded into the top-level environment.

use std::rc::Rc;

use nexl_runtime::{NativeFn, Value};

use crate::Env;

/// Create a root [`Env`] pre-populated with all standard built-in functions.
pub fn standard_env() -> Rc<Env> {
    let env = Rc::new(Env::new());

    // Arithmetic
    env.define("+", native("+", add));
    env.define("-", native("-", sub));
    env.define("*", native("*", mul));
    env.define("/", native("/", div));
    env.define("mod", native("mod", modulo));

    // Comparison
    env.define("=", native("=", eq));
    env.define("<", native("<", lt));
    env.define(">", native(">", gt));
    env.define("<=", native("<=", le));
    env.define(">=", native(">=", ge));

    // Logic
    env.define("not", native("not", not));
    env.define("and", native("and", and));
    env.define("or", native("or", or));

    // String
    env.define("str", native("str", str_fn));
    env.define("count", native("count", count));

    env
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn native(name: &'static str, f: fn(&[Value]) -> Result<Value, String>) -> Value {
    Value::NativeFunction(Rc::new(NativeFn { name, f }))
}

/// Unpack exactly two arguments.
fn two_args<'a>(op: &str, args: &'a [Value]) -> Result<(&'a Value, &'a Value), String> {
    match args {
        [a, b] => Ok((a, b)),
        _ => Err(format!(
            "`{op}` requires exactly 2 arguments, got {}",
            args.len()
        )),
    }
}

/// Unpack exactly one argument.
fn one_arg<'a>(op: &str, args: &'a [Value]) -> Result<&'a Value, String> {
    match args {
        [a] => Ok(a),
        _ => Err(format!(
            "`{op}` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

// ---------------------------------------------------------------------------
// Arithmetic
// ---------------------------------------------------------------------------

/// `(+ ...)` — variadic Int or Float addition.  Identity = 0 (Int) with zero args.
fn add(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Ok(Value::Int(0));
    }
    match &args[0] {
        Value::Int(_) => {
            let mut acc: i64 = 0;
            for v in args {
                match v {
                    Value::Int(n) => acc = acc.wrapping_add(*n),
                    other => return Err(type_mismatch("+", "Int", other)),
                }
            }
            Ok(Value::Int(acc))
        }
        Value::Float(_) => {
            let mut acc: f64 = 0.0;
            for v in args {
                match v {
                    Value::Float(n) => acc += n,
                    other => return Err(type_mismatch("+", "Float", other)),
                }
            }
            Ok(Value::Float(acc))
        }
        other => Err(type_mismatch("+", "Int or Float", other)),
    }
}

/// `(- x ...)` — unary negation or binary/variadic subtraction.
fn sub(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("`-` requires at least 1 argument".into());
    }
    match &args[0] {
        Value::Int(first) => {
            if args.len() == 1 {
                return Ok(Value::Int(first.wrapping_neg()));
            }
            let mut acc = *first;
            for v in &args[1..] {
                match v {
                    Value::Int(n) => acc = acc.wrapping_sub(*n),
                    other => return Err(type_mismatch("-", "Int", other)),
                }
            }
            Ok(Value::Int(acc))
        }
        Value::Float(first) => {
            if args.len() == 1 {
                return Ok(Value::Float(-first));
            }
            let mut acc = *first;
            for v in &args[1..] {
                match v {
                    Value::Float(n) => acc -= n,
                    other => return Err(type_mismatch("-", "Float", other)),
                }
            }
            Ok(Value::Float(acc))
        }
        other => Err(type_mismatch("-", "Int or Float", other)),
    }
}

/// `(* ...)` — variadic Int or Float multiplication.  Identity = 1 (Int) with zero args.
fn mul(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Ok(Value::Int(1));
    }
    match &args[0] {
        Value::Int(_) => {
            let mut acc: i64 = 1;
            for v in args {
                match v {
                    Value::Int(n) => acc = acc.wrapping_mul(*n),
                    other => return Err(type_mismatch("*", "Int", other)),
                }
            }
            Ok(Value::Int(acc))
        }
        Value::Float(_) => {
            let mut acc: f64 = 1.0;
            for v in args {
                match v {
                    Value::Float(n) => acc *= n,
                    other => return Err(type_mismatch("*", "Float", other)),
                }
            }
            Ok(Value::Float(acc))
        }
        other => Err(type_mismatch("*", "Int or Float", other)),
    }
}

/// `(/ a b)` — integer or float division.  Division by zero is a runtime error.
fn div(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("/", args)?;
    match (a, b) {
        (Value::Int(n), Value::Int(d)) => {
            if *d == 0 {
                Err("division by zero".into())
            } else {
                Ok(Value::Int(n / d))
            }
        }
        (Value::Float(n), Value::Float(d)) => Ok(Value::Float(n / d)),
        (Value::Int(_), other) => Err(type_mismatch("/", "Int", other)),
        (Value::Float(_), other) => Err(type_mismatch("/", "Float", other)),
        (other, _) => Err(type_mismatch("/", "Int or Float", other)),
    }
}

/// `(mod a b)` — integer remainder.
fn modulo(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("mod", args)?;
    match (a, b) {
        (Value::Int(n), Value::Int(d)) => {
            if *d == 0 {
                Err("modulo by zero".into())
            } else {
                Ok(Value::Int(n % d))
            }
        }
        (Value::Int(_), other) => Err(type_mismatch("mod", "Int", other)),
        (other, _) => Err(type_mismatch("mod", "Int", other)),
    }
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

/// `(= a b)` — structural equality.
fn eq(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("=", args)?;
    Ok(Value::Bool(a == b))
}

/// `(< a b)` — less-than on Int or Float.
fn lt(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("<", args)?;
    cmp_op("<", a, b, |o| o.is_lt())
}

/// `(> a b)` — greater-than on Int or Float.
fn gt(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args(">", args)?;
    cmp_op(">", a, b, |o| o.is_gt())
}

/// `(<= a b)` — less-than-or-equal on Int or Float.
fn le(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("<=", args)?;
    cmp_op("<=", a, b, |o| o.is_le())
}

/// `(>= a b)` — greater-than-or-equal on Int or Float.
fn ge(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args(">=", args)?;
    cmp_op(">=", a, b, |o| o.is_ge())
}

fn cmp_op(
    op: &str,
    a: &Value,
    b: &Value,
    pred: impl Fn(std::cmp::Ordering) -> bool,
) -> Result<Value, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Bool(pred(x.cmp(y)))),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(match x.partial_cmp(y) {
            Some(ord) => pred(ord),
            None => false, // NaN comparisons are false
        })),
        (Value::Int(_), other) => Err(type_mismatch(op, "Int", other)),
        (Value::Float(_), other) => Err(type_mismatch(op, "Float", other)),
        (other, _) => Err(type_mismatch(op, "Int or Float", other)),
    }
}

// ---------------------------------------------------------------------------
// Logic
// ---------------------------------------------------------------------------

/// `(not b)` — boolean negation.
fn not(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("not", args)?;
    match v {
        Value::Bool(b) => Ok(Value::Bool(!b)),
        other => Err(type_mismatch("not", "Bool", other)),
    }
}

/// `(and a b)` — strict (eager) boolean AND.
fn and(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("and", args)?;
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => Ok(Value::Bool(*x && *y)),
        (Value::Bool(_), other) => Err(type_mismatch("and", "Bool", other)),
        (other, _) => Err(type_mismatch("and", "Bool", other)),
    }
}

/// `(or a b)` — strict (eager) boolean OR.
fn or(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("or", args)?;
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => Ok(Value::Bool(*x || *y)),
        (Value::Bool(_), other) => Err(type_mismatch("or", "Bool", other)),
        (other, _) => Err(type_mismatch("or", "Bool", other)),
    }
}

// ---------------------------------------------------------------------------
// String
// ---------------------------------------------------------------------------

/// `(str ...)` — convert each argument to its display string and concatenate.
///
/// `Str` values contribute their raw content (no surrounding quotes);
/// all other values use their `Display` representation.
fn str_fn(args: &[Value]) -> Result<Value, String> {
    let mut out = String::new();
    for v in args {
        match v {
            Value::Str(s) => out.push_str(s),
            other => out.push_str(&other.to_string()),
        }
    }
    Ok(Value::Str(Rc::from(out.as_str())))
}

/// `(count s)` — number of Unicode scalar values in a string.
fn count(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("count", args)?;
    match v {
        Value::Str(s) => Ok(Value::Int(s.chars().count() as i64)),
        other => Err(type_mismatch("count", "Str", other)),
    }
}

// ---------------------------------------------------------------------------
// Error formatting
// ---------------------------------------------------------------------------

fn type_mismatch(op: &str, expected: &str, got: &Value) -> String {
    format!("`{op}` expected {expected}, got {}", got.type_name())
}
