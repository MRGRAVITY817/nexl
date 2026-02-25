//! Standard built-in functions pre-loaded into the top-level environment.

use std::rc::Rc;

use nexl_runtime::{NativeFn, Value};

use crate::Env;

/// Create a root [`Env`] pre-populated with all standard built-in functions.
pub fn standard_env() -> Rc<Env> {
    let env = Rc::new(Env::new());

    // Option constructors
    env.define("None", option_none());
    env.define("Some", native("Some", some_ctor));

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
    env.define("get", native("get", get));
    env.define("put", native("put", put));
    env.define("append", native("append", append));
    env.define("first", native("first", first));
    env.define("rest", native("rest", rest));
    env.define("last", native("last", last));
    env.define("slice", native("slice", slice));
    env.define("remove", native("remove", remove));
    env.define("keys", native("keys", keys));
    env.define("vals", native("vals", vals));
    env.define("entries", native("entries", entries));
    env.define("contains?", native("contains?", contains));

    env
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn native(name: &'static str, f: fn(&[Value]) -> Result<Value, String>) -> Value {
    Value::NativeFunction(Rc::new(NativeFn { name, f }))
}

fn option_none() -> Value {
    Value::Adt {
        type_name: Rc::from("Option"),
        ctor: Rc::from("None"),
        fields: Rc::new(vec![]),
    }
}

fn option_some(value: Value) -> Value {
    Value::Adt {
        type_name: Rc::from("Option"),
        ctor: Rc::from("Some"),
        fields: Rc::new(vec![value]),
    }
}

fn some_ctor(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("Some", args)?;
    Ok(option_some(v.clone()))
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

/// Unpack exactly three arguments.
fn three_args<'a>(
    op: &str,
    args: &'a [Value],
) -> Result<(&'a Value, &'a Value, &'a Value), String> {
    match args {
        [a, b, c] => Ok((a, b, c)),
        _ => Err(format!(
            "`{op}` requires exactly 3 arguments, got {}",
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
        Value::Vec(items) => Ok(Value::Int(items.len() as i64)),
        Value::Map(entries) => Ok(Value::Int(entries.len() as i64)),
        other => Err(type_mismatch("count", "Str, Vec, or Map", other)),
    }
}

// ---------------------------------------------------------------------------
// Collections (Vec)
// ---------------------------------------------------------------------------

/// `(get v i)` — return (Some value) if index is in bounds, else None.
fn get(args: &[Value]) -> Result<Value, String> {
    let (coll, idx) = two_args("get", args)?;
    match coll {
        Value::Vec(items) => match idx {
            Value::Int(i) => {
                if *i < 0 {
                    return Ok(option_none());
                }
                let idx = *i as usize;
                match items.get(idx) {
                    Some(value) => Ok(option_some(value.clone())),
                    None => Ok(option_none()),
                }
            }
            other => Err(type_mismatch("get", "Int", other)),
        },
        Value::Map(entries) => {
            for (key, value) in entries.iter() {
                if key == idx {
                    return Ok(option_some(value.clone()));
                }
            }
            Ok(option_none())
        }
        other => Err(type_mismatch("get", "Vec or Map", other)),
    }
}

/// `(put v i x)` — update the value at index `i`.
fn put(args: &[Value]) -> Result<Value, String> {
    let (coll, idx, value) = three_args("put", args)?;
    match coll {
        Value::Vec(items) => match idx {
            Value::Int(i) => {
                if *i < 0 {
                    return Err(format!("`put` index out of bounds: {i}"));
                }
                let idx = *i as usize;
                if idx >= items.len() {
                    return Err(format!(
                        "`put` index out of bounds: {i} for Vec of length {}",
                        items.len()
                    ));
                }
                let mut next = items.as_ref().clone();
                next[idx] = value.clone();
                Ok(Value::Vec(Rc::new(next)))
            }
            other => Err(type_mismatch("put", "Int", other)),
        },
        Value::Map(entries) => {
            let mut next = entries.as_ref().clone();
            let mut updated = false;
            for (key, val) in next.iter_mut() {
                if key == idx {
                    *val = value.clone();
                    updated = true;
                    break;
                }
            }
            if !updated {
                next.push((idx.clone(), value.clone()));
            }
            Ok(Value::Map(Rc::new(next)))
        }
        other => Err(type_mismatch("put", "Vec or Map", other)),
    }
}

/// `(append v x)` — append to the end of the vector.
fn append(args: &[Value]) -> Result<Value, String> {
    let (coll, value) = two_args("append", args)?;
    match coll {
        Value::Vec(items) => {
            let mut next = items.as_ref().clone();
            next.push(value.clone());
            Ok(Value::Vec(Rc::new(next)))
        }
        other => Err(type_mismatch("append", "Vec", other)),
    }
}

/// `(first v)` — return (Some x) for the first element, or None.
fn first(args: &[Value]) -> Result<Value, String> {
    let coll = one_arg("first", args)?;
    match coll {
        Value::Vec(items) => match items.first() {
            Some(value) => Ok(option_some(value.clone())),
            None => Ok(option_none()),
        },
        other => Err(type_mismatch("first", "Vec", other)),
    }
}

/// `(rest v)` — return the tail of the vector (empty if length <= 1).
fn rest(args: &[Value]) -> Result<Value, String> {
    let coll = one_arg("rest", args)?;
    match coll {
        Value::Vec(items) => {
            if items.is_empty() {
                return Ok(Value::Vec(Rc::new(vec![])));
            }
            let mut next = items.as_ref().clone();
            next.remove(0);
            Ok(Value::Vec(Rc::new(next)))
        }
        other => Err(type_mismatch("rest", "Vec", other)),
    }
}

/// `(last v)` — return (Some x) for the last element, or None.
fn last(args: &[Value]) -> Result<Value, String> {
    let coll = one_arg("last", args)?;
    match coll {
        Value::Vec(items) => match items.last() {
            Some(value) => Ok(option_some(value.clone())),
            None => Ok(option_none()),
        },
        other => Err(type_mismatch("last", "Vec", other)),
    }
}

/// `(slice v start end)` — return elements in [start, end).
fn slice(args: &[Value]) -> Result<Value, String> {
    let (coll, start, end) = three_args("slice", args)?;
    match (coll, start, end) {
        (Value::Vec(items), Value::Int(start), Value::Int(end)) => {
            if *start < 0 || *end < 0 {
                return Err("`slice` indices must be non-negative".into());
            }
            let start = *start as usize;
            let end = *end as usize;
            if start > end || end > items.len() {
                return Err(format!(
                    "`slice` index out of bounds: {start}..{end} for Vec of length {}",
                    items.len()
                ));
            }
            Ok(Value::Vec(Rc::new(items[start..end].to_vec())))
        }
        (Value::Vec(_), other, _) => Err(type_mismatch("slice", "Int", other)),
        (other, _, _) => Err(type_mismatch("slice", "Vec", other)),
    }
}

/// `(remove m k)` — remove key from map if present.
fn remove(args: &[Value]) -> Result<Value, String> {
    let (coll, key) = two_args("remove", args)?;
    match coll {
        Value::Map(entries) => {
            let next: Vec<(Value, Value)> = entries
                .iter()
                .filter(|(entry_key, _)| entry_key != key)
                .cloned()
                .collect();
            Ok(Value::Map(Rc::new(next)))
        }
        other => Err(type_mismatch("remove", "Map", other)),
    }
}

/// `(keys m)` — return map keys in insertion order.
fn keys(args: &[Value]) -> Result<Value, String> {
    let coll = one_arg("keys", args)?;
    match coll {
        Value::Map(entries) => Ok(Value::Vec(Rc::new(
            entries.iter().map(|(key, _)| key.clone()).collect(),
        ))),
        other => Err(type_mismatch("keys", "Map", other)),
    }
}

/// `(vals m)` — return map values in insertion order.
fn vals(args: &[Value]) -> Result<Value, String> {
    let coll = one_arg("vals", args)?;
    match coll {
        Value::Map(entries) => Ok(Value::Vec(Rc::new(
            entries.iter().map(|(_, value)| value.clone()).collect(),
        ))),
        other => Err(type_mismatch("vals", "Map", other)),
    }
}

/// `(entries m)` — return map entries as a Vec of 2-tuples.
fn entries(args: &[Value]) -> Result<Value, String> {
    let coll = one_arg("entries", args)?;
    match coll {
        Value::Map(items) => Ok(Value::Vec(Rc::new(
            items
                .iter()
                .map(|(key, value)| Value::Vec(Rc::new(vec![key.clone(), value.clone()])))
                .collect(),
        ))),
        other => Err(type_mismatch("entries", "Map", other)),
    }
}

/// `(contains? m k)` — check for key membership in a map.
fn contains(args: &[Value]) -> Result<Value, String> {
    let (coll, key) = two_args("contains?", args)?;
    match coll {
        Value::Map(entries) => Ok(Value::Bool(entries.iter().any(|(entry_key, _)| entry_key == key))),
        other => Err(type_mismatch("contains?", "Map", other)),
    }
}

// ---------------------------------------------------------------------------
// Error formatting
// ---------------------------------------------------------------------------

fn type_mismatch(op: &str, expected: &str, got: &Value) -> String {
    format!("`{op}` expected {expected}, got {}", got.type_name())
}
