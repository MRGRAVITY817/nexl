//! Standard built-in functions pre-loaded into the top-level environment.

use std::collections::HashMap;
use std::rc::Rc;

use nexl_runtime::{NativeFn, Value};

use crate::{Env, eval::apply_value};

/// Adapter: call a Value as a function, for use by stdlib closures.
fn eval_call_value(callee: &Value, args: &[Value]) -> Result<Value, String> {
    apply_value(callee, args).map_err(|e| e.to_string())
}

/// Create a root [`Env`] pre-populated with all standard built-in functions.
pub fn standard_env() -> Rc<Env> {
    // Register the evaluator's apply_value so stdlib closures can call
    // arbitrary Value functions (including Nexl-defined Function values).
    nexl_runtime::register_call_value(eval_call_value);

    let env = Rc::new(Env::new());

    // Option constructors
    env.define("None", option_none());
    env.define("Some", native("Some", some_ctor));

    // Result constructors
    env.define("Ok", native("Ok", ok_ctor));
    env.define("Err", native("Err", err_ctor));

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

    // Logic (and/or are special forms for short-circuit evaluation)
    env.define("not", native("not", not));

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
    env.define("add", native("add", set_add));
    env.define("union", native("union", union));
    env.define("intersection", native("intersection", intersection));
    env.define("difference", native("difference", difference));
    env.define("map", native("map", map_fn));
    env.define("filter", native("filter", filter_fn));
    env.define("reduce", native("reduce", reduce_fn));
    env.define("sort", native("sort", sort_fn));
    env.define("sort-by", native("sort-by", sort_by_fn));
    env.define("reverse", native("reverse", reverse_fn));
    env.define("range", native("range", range_fn));
    env.define("flat-map", native("flat-map", flat_map_fn));
    env.define("group-by", native("group-by", group_by_fn));
    env.define("zip", native("zip", zip_fn));
    env.define("take", native("take", take_fn));
    env.define("drop", native("drop", drop_fn));
    env.define("take-while", native("take-while", take_while_fn));
    env.define("drop-while", native("drop-while", drop_while_fn));

    // Bitwise operations
    env.define("bit-and", native("bit-and", bit_and));
    env.define("bit-or", native("bit-or", bit_or));
    env.define("bit-xor", native("bit-xor", bit_xor));
    env.define("bit-not", native("bit-not", bit_not));
    env.define("bit-shift-left", native("bit-shift-left", bit_shift_left));
    env.define("bit-shift-right", native("bit-shift-right", bit_shift_right));

    // Register §11.1 stdlib modules as qualified module aliases
    register_stdlib_modules(&env);

    env
}

/// Register all `nexl-stdlib` modules as module aliases in the environment,
/// making them accessible via qualified names (e.g. `core/identity`, `str/split`).
fn register_stdlib_modules(env: &Env) {
    for (module_name, entries) in nexl_stdlib::all_modules() {
        let mut exports: HashMap<Rc<str>, Value> = HashMap::new();
        for (fn_name, f) in entries {
            let value = Value::NativeFunction(Rc::new(NativeFn { name: fn_name, f }));
            exports.insert(Rc::from(fn_name), value);
        }
        env.define_module_alias(Rc::from(module_name), Rc::new(exports));
    }
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

fn ok_ctor(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("Ok", args)?;
    Ok(Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Ok"),
        fields: Rc::new(vec![v.clone()]),
    })
}

fn err_ctor(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("Err", args)?;
    Ok(Value::Adt {
        type_name: Rc::from("Result"),
        ctor: Rc::from("Err"),
        fields: Rc::new(vec![v.clone()]),
    })
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

fn call1(func: &Value, arg: Value) -> Result<Value, String> {
    let args = [arg];
    apply_value(func, &args).map_err(|err| err.to_string())
}

fn call2(func: &Value, arg0: Value, arg1: Value) -> Result<Value, String> {
    let args = [arg0, arg1];
    apply_value(func, &args).map_err(|err| err.to_string())
}

fn expect_bool(op: &str, value: Value) -> Result<bool, String> {
    match value {
        Value::Bool(b) => Ok(b),
        other => Err(type_mismatch(op, "Bool", &other)),
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
            Value::Char(c) => out.push(*c),
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
        Value::Set(items) => Ok(Value::Int(items.len() as i64)),
        other => Err(type_mismatch("count", "Str, Vec, Map, or Set", other)),
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
        Value::Set(items) => {
            let next: Vec<Value> = items.iter().filter(|item| *item != key).cloned().collect();
            Ok(Value::Set(Rc::new(next)))
        }
        other => Err(type_mismatch("remove", "Map or Set", other)),
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
        Value::Map(entries) => Ok(Value::Bool(
            entries.iter().any(|(entry_key, _)| entry_key == key),
        )),
        Value::Set(items) => Ok(Value::Bool(items.iter().any(|item| item == key))),
        other => Err(type_mismatch("contains?", "Map or Set", other)),
    }
}

/// `(add s x)` — add element to a set if missing.
fn set_add(args: &[Value]) -> Result<Value, String> {
    let (coll, value) = two_args("add", args)?;
    match coll {
        Value::Set(items) => {
            if items.iter().any(|item| item == value) {
                return Ok(Value::Set(items.clone()));
            }
            let mut next = items.as_ref().clone();
            next.push(value.clone());
            Ok(Value::Set(Rc::new(next)))
        }
        other => Err(type_mismatch("add", "Set", other)),
    }
}

/// `(union a b)` — set union.
fn union(args: &[Value]) -> Result<Value, String> {
    let (left, right) = two_args("union", args)?;
    match (left, right) {
        (Value::Set(left_items), Value::Set(right_items)) => {
            let mut out = left_items.as_ref().clone();
            for item in right_items.iter() {
                if !out.iter().any(|existing| existing == item) {
                    out.push(item.clone());
                }
            }
            Ok(Value::Set(Rc::new(out)))
        }
        (Value::Set(_), other) => Err(type_mismatch("union", "Set", other)),
        (other, _) => Err(type_mismatch("union", "Set", other)),
    }
}

/// `(intersection a b)` — set intersection.
fn intersection(args: &[Value]) -> Result<Value, String> {
    let (left, right) = two_args("intersection", args)?;
    match (left, right) {
        (Value::Set(left_items), Value::Set(right_items)) => {
            let mut out = Vec::new();
            for item in left_items.iter() {
                if right_items.iter().any(|other| other == item)
                    && !out.iter().any(|existing| existing == item)
                {
                    out.push(item.clone());
                }
            }
            Ok(Value::Set(Rc::new(out)))
        }
        (Value::Set(_), other) => Err(type_mismatch("intersection", "Set", other)),
        (other, _) => Err(type_mismatch("intersection", "Set", other)),
    }
}

/// `(difference a b)` — set difference (elements in `a` not in `b`).
fn difference(args: &[Value]) -> Result<Value, String> {
    let (left, right) = two_args("difference", args)?;
    match (left, right) {
        (Value::Set(left_items), Value::Set(right_items)) => {
            let mut out = Vec::new();
            for item in left_items.iter() {
                if !right_items.iter().any(|other| other == item)
                    && !out.iter().any(|existing| existing == item)
                {
                    out.push(item.clone());
                }
            }
            Ok(Value::Set(Rc::new(out)))
        }
        (Value::Set(_), other) => Err(type_mismatch("difference", "Set", other)),
        (other, _) => Err(type_mismatch("difference", "Set", other)),
    }
}

// ---------------------------------------------------------------------------
// Sequence operations
// ---------------------------------------------------------------------------

fn map_fn(args: &[Value]) -> Result<Value, String> {
    let (func, coll) = two_args("map", args)?;
    match coll {
        Value::Vec(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items.iter() {
                let mapped = call1(func, item.clone())?;
                out.push(mapped);
            }
            Ok(Value::Vec(Rc::new(out)))
        }
        Value::Set(items) => {
            let mut out: Vec<Value> = Vec::new();
            for item in items.iter() {
                let mapped = call1(func, item.clone())?;
                if !out.iter().any(|existing| existing == &mapped) {
                    out.push(mapped);
                }
            }
            Ok(Value::Set(Rc::new(out)))
        }
        Value::Map(entries) => {
            let mut out = Vec::with_capacity(entries.len());
            for (key, value) in entries.iter() {
                let mapped = call1(func, value.clone())?;
                out.push((key.clone(), mapped));
            }
            Ok(Value::Map(Rc::new(out)))
        }
        Value::Adt {
            type_name,
            ctor,
            fields,
        } if type_name.as_ref() == "Option" => match ctor.as_ref() {
            "None" => Ok(option_none()),
            "Some" => {
                let value = fields
                    .first()
                    .ok_or_else(|| "`map` expected Option.Some with 1 field, got 0".to_string())?;
                let mapped = call1(func, value.clone())?;
                Ok(option_some(mapped))
            }
            other => Err(format!("`map` expected Option constructor, got {other}")),
        },
        other => Err(type_mismatch("map", "Vec, Map, Set, or Option", other)),
    }
}

fn filter_fn(args: &[Value]) -> Result<Value, String> {
    let (pred, coll) = two_args("filter", args)?;
    match coll {
        Value::Vec(items) => {
            let mut out = Vec::new();
            for item in items.iter() {
                let keep = expect_bool("filter", call1(pred, item.clone())?)?;
                if keep {
                    out.push(item.clone());
                }
            }
            Ok(Value::Vec(Rc::new(out)))
        }
        Value::Set(items) => {
            let mut out = Vec::new();
            for item in items.iter() {
                let keep = expect_bool("filter", call1(pred, item.clone())?)?;
                if keep && !out.iter().any(|existing| existing == item) {
                    out.push(item.clone());
                }
            }
            Ok(Value::Set(Rc::new(out)))
        }
        Value::Map(entries) => {
            let mut out = Vec::new();
            for (key, value) in entries.iter() {
                let keep = expect_bool("filter", call1(pred, value.clone())?)?;
                if keep {
                    out.push((key.clone(), value.clone()));
                }
            }
            Ok(Value::Map(Rc::new(out)))
        }
        Value::Adt {
            type_name,
            ctor,
            fields,
        } if type_name.as_ref() == "Option" => match ctor.as_ref() {
            "None" => Ok(option_none()),
            "Some" => {
                let value = fields.first().ok_or_else(|| {
                    "`filter` expected Option.Some with 1 field, got 0".to_string()
                })?;
                let keep = expect_bool("filter", call1(pred, value.clone())?)?;
                if keep {
                    Ok(option_some(value.clone()))
                } else {
                    Ok(option_none())
                }
            }
            other => Err(format!("`filter` expected Option constructor, got {other}")),
        },
        other => Err(type_mismatch("filter", "Vec, Map, Set, or Option", other)),
    }
}

fn reduce_fn(args: &[Value]) -> Result<Value, String> {
    let (func, init, coll) = three_args("reduce", args)?;
    match coll {
        Value::Vec(items) => {
            let mut acc = init.clone();
            for item in items.iter() {
                acc = call2(func, acc, item.clone())?;
            }
            Ok(acc)
        }
        Value::Set(items) => {
            let mut acc = init.clone();
            for item in items.iter() {
                acc = call2(func, acc, item.clone())?;
            }
            Ok(acc)
        }
        Value::Map(entries) => {
            let mut acc = init.clone();
            for (_key, value) in entries.iter() {
                acc = call2(func, acc, value.clone())?;
            }
            Ok(acc)
        }
        Value::Adt {
            type_name,
            ctor,
            fields,
        } if type_name.as_ref() == "Option" => match ctor.as_ref() {
            "None" => Ok(init.clone()),
            "Some" => {
                let value = fields.first().ok_or_else(|| {
                    "`reduce` expected Option.Some with 1 field, got 0".to_string()
                })?;
                call2(func, init.clone(), value.clone())
            }
            other => Err(format!("`reduce` expected Option constructor, got {other}")),
        },
        other => Err(type_mismatch("reduce", "Vec, Map, Set, or Option", other)),
    }
}

// ---------------------------------------------------------------------------
// Sort / Reverse
// ---------------------------------------------------------------------------

/// `(sort coll)` — stable sort a Vec using default comparison.
fn sort_fn(args: &[Value]) -> Result<Value, String> {
    let coll = one_arg("sort", args)?;
    let Value::Vec(items) = coll else {
        return Err(type_mismatch("sort", "Vec", coll));
    };
    let mut sorted: Vec<Value> = items.as_ref().clone();
    let mut err: Option<String> = None;
    sorted.sort_by(|a, b| match compare_values(a, b) {
        Ok(ord) => ord,
        Err(e) => {
            if err.is_none() {
                err = Some(e);
            }
            std::cmp::Ordering::Equal
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(Value::Vec(Rc::new(sorted)))
}

/// `(sort-by f coll)` — stable sort by key function.
fn sort_by_fn(args: &[Value]) -> Result<Value, String> {
    let (func, coll) = two_args("sort-by", args)?;
    let Value::Vec(items) = coll else {
        return Err(type_mismatch("sort-by", "Vec", coll));
    };
    // Pre-compute keys
    let mut keyed: Vec<(Value, Value)> = Vec::with_capacity(items.len());
    for item in items.iter() {
        let key = call1(func, item.clone())?;
        keyed.push((key, item.clone()));
    }
    let mut err: Option<String> = None;
    keyed.sort_by(|(ka, _), (kb, _)| match compare_values(ka, kb) {
        Ok(ord) => ord,
        Err(e) => {
            if err.is_none() {
                err = Some(e);
            }
            std::cmp::Ordering::Equal
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(Value::Vec(Rc::new(
        keyed.into_iter().map(|(_, v)| v).collect(),
    )))
}

/// Compare two values for ordering. Only works for Int, Float, Str.
fn compare_values(a: &Value, b: &Value) -> Result<std::cmp::Ordering, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => Ok(x.total_cmp(y)),
        (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),
        _ => Err(format!(
            "`sort` cannot compare {} and {}",
            a.type_name(),
            b.type_name()
        )),
    }
}

/// `(reverse coll)` — reverse a Vec.
fn reverse_fn(args: &[Value]) -> Result<Value, String> {
    let coll = one_arg("reverse", args)?;
    let Value::Vec(items) = coll else {
        return Err(type_mismatch("reverse", "Vec", coll));
    };
    let mut reversed: Vec<Value> = items.as_ref().clone();
    reversed.reverse();
    Ok(Value::Vec(Rc::new(reversed)))
}

// ---------------------------------------------------------------------------
// Range
// ---------------------------------------------------------------------------

/// `(range n)`, `(range start end)`, or `(range start end step)`.
fn range_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(n)] => {
            let items: Vec<Value> = (0..*n).map(Value::Int).collect();
            Ok(Value::Vec(Rc::new(items)))
        }
        [Value::Int(start), Value::Int(end)] => {
            let items: Vec<Value> = (*start..*end).map(Value::Int).collect();
            Ok(Value::Vec(Rc::new(items)))
        }
        [Value::Int(start), Value::Int(end), Value::Int(step)] => {
            if *step == 0 {
                return Err("`range` step cannot be zero".into());
            }
            let mut items = Vec::new();
            let mut i = *start;
            if *step > 0 {
                while i < *end {
                    items.push(Value::Int(i));
                    i += step;
                }
            } else {
                while i > *end {
                    items.push(Value::Int(i));
                    i += step;
                }
            }
            Ok(Value::Vec(Rc::new(items)))
        }
        _ => Err("`range` requires 1, 2, or 3 Int arguments".into()),
    }
}

// ---------------------------------------------------------------------------
// Flat-map, Group-by, Zip
// ---------------------------------------------------------------------------

/// `(flat-map f coll)` — map then flatten one level.
fn flat_map_fn(args: &[Value]) -> Result<Value, String> {
    let (func, coll) = two_args("flat-map", args)?;
    let Value::Vec(items) = coll else {
        return Err(type_mismatch("flat-map", "Vec", coll));
    };
    let mut result = Vec::new();
    for item in items.iter() {
        let mapped = call1(func, item.clone())?;
        match mapped {
            Value::Vec(inner) => result.extend(inner.iter().cloned()),
            other => result.push(other),
        }
    }
    Ok(Value::Vec(Rc::new(result)))
}

/// `(group-by f coll)` — group elements by key function, returns Map.
fn group_by_fn(args: &[Value]) -> Result<Value, String> {
    let (func, coll) = two_args("group-by", args)?;
    let Value::Vec(items) = coll else {
        return Err(type_mismatch("group-by", "Vec", coll));
    };
    // Use a Vec of pairs to preserve insertion order.
    let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
    for item in items.iter() {
        let key = call1(func, item.clone())?;
        if let Some(group) = groups.iter_mut().find(|(k, _)| k == &key) {
            group.1.push(item.clone());
        } else {
            groups.push((key, vec![item.clone()]));
        }
    }
    let pairs: Vec<(Value, Value)> = groups
        .into_iter()
        .map(|(k, vs)| (k, Value::Vec(Rc::new(vs))))
        .collect();
    Ok(Value::Map(Rc::new(pairs)))
}

/// `(zip a b)` — zip two Vecs into a Vec of two-element Vecs.
fn zip_fn(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("zip", args)?;
    let Value::Vec(va) = a else {
        return Err(type_mismatch("zip", "Vec", a));
    };
    let Value::Vec(vb) = b else {
        return Err(type_mismatch("zip", "Vec", b));
    };
    let result: Vec<Value> = va
        .iter()
        .zip(vb.iter())
        .map(|(x, y)| Value::Vec(Rc::new(vec![x.clone(), y.clone()])))
        .collect();
    Ok(Value::Vec(Rc::new(result)))
}

// ---------------------------------------------------------------------------
// Take / Drop
// ---------------------------------------------------------------------------

/// `(take n coll)` — take first n elements.
fn take_fn(args: &[Value]) -> Result<Value, String> {
    let (n, coll) = two_args("take", args)?;
    let Value::Int(n) = n else {
        return Err(type_mismatch("take", "Int", n));
    };
    let Value::Vec(items) = coll else {
        return Err(type_mismatch("take", "Vec", coll));
    };
    let n = (*n).max(0) as usize;
    Ok(Value::Vec(Rc::new(items.iter().take(n).cloned().collect())))
}

/// `(drop n coll)` — drop first n elements.
fn drop_fn(args: &[Value]) -> Result<Value, String> {
    let (n, coll) = two_args("drop", args)?;
    let Value::Int(n) = n else {
        return Err(type_mismatch("drop", "Int", n));
    };
    let Value::Vec(items) = coll else {
        return Err(type_mismatch("drop", "Vec", coll));
    };
    let n = (*n).max(0) as usize;
    Ok(Value::Vec(Rc::new(items.iter().skip(n).cloned().collect())))
}

/// `(take-while pred coll)` — take while predicate is true.
fn take_while_fn(args: &[Value]) -> Result<Value, String> {
    let (pred, coll) = two_args("take-while", args)?;
    let Value::Vec(items) = coll else {
        return Err(type_mismatch("take-while", "Vec", coll));
    };
    let mut result = Vec::new();
    for item in items.iter() {
        let keep = expect_bool("take-while", call1(pred, item.clone())?)?;
        if !keep {
            break;
        }
        result.push(item.clone());
    }
    Ok(Value::Vec(Rc::new(result)))
}

/// `(drop-while pred coll)` — drop while predicate is true.
fn drop_while_fn(args: &[Value]) -> Result<Value, String> {
    let (pred, coll) = two_args("drop-while", args)?;
    let Value::Vec(items) = coll else {
        return Err(type_mismatch("drop-while", "Vec", coll));
    };
    let mut dropping = true;
    let mut result = Vec::new();
    for item in items.iter() {
        if dropping {
            let drop = expect_bool("drop-while", call1(pred, item.clone())?)?;
            if drop {
                continue;
            }
            dropping = false;
        }
        result.push(item.clone());
    }
    Ok(Value::Vec(Rc::new(result)))
}

// ---------------------------------------------------------------------------
// Bitwise operations
// ---------------------------------------------------------------------------

/// `(bit-and a b)` — bitwise AND.
fn bit_and(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("bit-and", args)?;
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x & y)),
        _ => Err(format!(
            "`bit-and` requires two Int arguments, got {} and {}",
            a.type_name(),
            b.type_name()
        )),
    }
}

/// `(bit-or a b)` — bitwise OR.
fn bit_or(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("bit-or", args)?;
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x | y)),
        _ => Err(format!(
            "`bit-or` requires two Int arguments, got {} and {}",
            a.type_name(),
            b.type_name()
        )),
    }
}

/// `(bit-xor a b)` — bitwise XOR.
fn bit_xor(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("bit-xor", args)?;
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x ^ y)),
        _ => Err(format!(
            "`bit-xor` requires two Int arguments, got {} and {}",
            a.type_name(),
            b.type_name()
        )),
    }
}

/// `(bit-not x)` — bitwise NOT.
fn bit_not(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("bit-not", args)?;
    match v {
        Value::Int(x) => Ok(Value::Int(!x)),
        _ => Err(type_mismatch("bit-not", "Int", v)),
    }
}

/// `(bit-shift-left x n)` — shift left by n bits.
fn bit_shift_left(args: &[Value]) -> Result<Value, String> {
    let (x, n) = two_args("bit-shift-left", args)?;
    match (x, n) {
        (Value::Int(x), Value::Int(n)) => Ok(Value::Int(x << n)),
        _ => Err(format!(
            "`bit-shift-left` requires two Int arguments, got {} and {}",
            x.type_name(),
            n.type_name()
        )),
    }
}

/// `(bit-shift-right x n)` — arithmetic shift right by n bits.
fn bit_shift_right(args: &[Value]) -> Result<Value, String> {
    let (x, n) = two_args("bit-shift-right", args)?;
    match (x, n) {
        (Value::Int(x), Value::Int(n)) => Ok(Value::Int(x >> n)),
        _ => Err(format!(
            "`bit-shift-right` requires two Int arguments, got {} and {}",
            x.type_name(),
            n.type_name()
        )),
    }
}

// ---------------------------------------------------------------------------
// Error formatting
// ---------------------------------------------------------------------------

fn type_mismatch(op: &str, expected: &str, got: &Value) -> String {
    format!("`{op}` expected {expected}, got {}", got.type_name())
}
