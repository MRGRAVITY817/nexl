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
    env.define("inc", native("inc", inc));
    env.define("dec", native("dec", dec));
    env.define("rem", native("rem", rem_fn));
    env.define("quot", native("quot", quot));

    // Comparison
    env.define("=", native("=", eq));
    env.define("not=", native("not=", not_eq));
    env.define("<", native("<", lt));
    env.define(">", native(">", gt));
    env.define("<=", native("<=", le));
    env.define(">=", native(">=", ge));
    env.define("compare", native("compare", compare));
    env.define("clamp", native("clamp", clamp));

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
    env.define("empty?", native("empty?", empty_pred));
    env.define("type-of", native("type-of", type_of_fn));
    env.define("keyword", native("keyword", keyword_fn));
    env.define("nth", native("nth", nth_fn));
    env.define("get-in", native("get-in", get_in_fn));
    env.define("assoc-in", native("assoc-in", assoc_in_fn));
    env.define("update", native("update", update_fn));
    env.define("update-in", native("update-in", update_in_fn));
    env.define("conj", native("conj", conj_fn));
    env.define("into", native("into", into_fn));
    env.define("concat", native("concat", concat_fn));
    env.define("empty", native("empty", empty_fn));
    env.define("merge", native("merge", merge_fn));
    env.define("merge-with", native("merge-with", merge_with_fn));
    env.define("select-keys", native("select-keys", select_keys_fn));
    env.define("rename-keys", native("rename-keys", rename_keys_fn));
    env.define("zipmap", native("zipmap", zipmap_fn));
    env.define("dissoc", native("dissoc", dissoc_fn));
    env.define("disj", native("disj", disj_fn));
    env.define(
        "symmetric-difference",
        native("symmetric-difference", symmetric_difference_fn),
    );
    env.define("subset?", native("subset?", subset_pred));
    env.define("superset?", native("superset?", superset_pred));
    env.define("disjoint?", native("disjoint?", disjoint_pred));
    env.define("reject", native("reject", reject_fn));
    env.define("keep", native("keep", keep_fn));
    env.define("some", native("some", some_fn));
    env.define("every?", native("every?", every_pred));
    env.define("any?", native("any?", any_pred));
    env.define("not-any?", native("not-any?", not_any_pred));
    env.define("not-every?", native("not-every?", not_every_pred));
    env.define("find", native("find", find_fn));
    env.define("find-index", native("find-index", find_index_fn));
    env.define("map-indexed", native("map-indexed", map_indexed_fn));
    env.define("reduce-indexed", native("reduce-indexed", reduce_indexed_fn));
    env.define("sort-with", native("sort-with", sort_with_fn));
    env.define("distinct", native("distinct", distinct_fn));
    env.define("flatten", native("flatten", flatten_fn));
    env.define("frequencies", native("frequencies", frequencies_fn));
    env.define("partition-by", native("partition-by", partition_by_fn));
    env.define("interleave", native("interleave", interleave_fn));
    env.define("interpose", native("interpose", interpose_fn));
    env.define("zip-with", native("zip-with", zip_with_fn));
    env.define("pr-str", native("pr-str", pr_str_fn));

    // Bitwise operations
    env.define("bit-and", native("bit-and", bit_and));
    env.define("bit-or", native("bit-or", bit_or));
    env.define("bit-xor", native("bit-xor", bit_xor));
    env.define("bit-not", native("bit-not", bit_not));
    env.define("bit-shift-left", native("bit-shift-left", bit_shift_left));
    env.define(
        "bit-shift-right",
        native("bit-shift-right", bit_shift_right),
    );

    // Mutable reference cells (atoms)
    env.define("atom", native("atom", atom_fn));
    env.define("deref", native("deref", deref_fn));
    env.define("reset!", native("reset!", reset_fn));
    env.define("swap!", native("swap!", swap_fn));

    // Pre-defined test utility handlers
    env.define("SequentialExecutor", sequential_executor_handler());

    // Register §11.1 stdlib modules as qualified module aliases
    register_stdlib_modules(&env);

    // Evaluate Nexl-written stdlib modules (option, result, core combinators)
    eval_nexl_stdlib_sources(&env);

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

/// Evaluate Nexl-written stdlib sources against the environment.
///
/// These `.nx` files define combinator functions (e.g. `option/some?`,
/// `result/map`) using qualified `defn` names. They run after Rust natives
/// are registered, so they can reference builtins and other modules freely.
fn eval_nexl_stdlib_sources(env: &Rc<Env>) {
    for (module_name, source) in nexl_stdlib::nexl_stdlib_sources() {
        let nodes = match nexl_reader::read(source, meta::FileId::SYNTHETIC) {
            Ok(nodes) => nodes,
            Err(e) => {
                eprintln!("nexl stdlib parse error in `{module_name}`: {e:?}");
                continue;
            }
        };
        for node in &nodes {
            if let Err(e) = crate::eval::eval(node, env) {
                eprintln!("nexl stdlib eval error in `{module_name}`: {e}");
            }
        }
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

/// `(inc x)` — increment by 1. Polymorphic over Int and Float.
fn inc(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("inc", args)?;
    match v {
        Value::Int(n) => Ok(Value::Int(n + 1)),
        Value::Float(n) => Ok(Value::Float(n + 1.0)),
        other => Err(type_mismatch("inc", "Int or Float", other)),
    }
}

/// `(dec x)` — decrement by 1. Polymorphic over Int and Float.
fn dec(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("dec", args)?;
    match v {
        Value::Int(n) => Ok(Value::Int(n - 1)),
        Value::Float(n) => Ok(Value::Float(n - 1.0)),
        other => Err(type_mismatch("dec", "Int or Float", other)),
    }
}

/// `(rem a b)` — remainder with sign of dividend (truncated division).
fn rem_fn(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("rem", args)?;
    match (a, b) {
        (Value::Int(n), Value::Int(d)) => {
            if *d == 0 {
                Err("remainder by zero".into())
            } else {
                Ok(Value::Int(n % d))
            }
        }
        (Value::Float(n), Value::Float(d)) => Ok(Value::Float(n % d)),
        (Value::Int(_), other) => Err(type_mismatch("rem", "Int", other)),
        (Value::Float(_), other) => Err(type_mismatch("rem", "Float", other)),
        (other, _) => Err(type_mismatch("rem", "Int or Float", other)),
    }
}

/// `(quot a b)` — truncated division quotient.
fn quot(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("quot", args)?;
    match (a, b) {
        (Value::Int(n), Value::Int(d)) => {
            if *d == 0 {
                Err("division by zero".into())
            } else {
                Ok(Value::Int(n / d))
            }
        }
        (Value::Float(n), Value::Float(d)) => Ok(Value::Float((n / d).trunc())),
        (Value::Int(_), other) => Err(type_mismatch("quot", "Int", other)),
        (Value::Float(_), other) => Err(type_mismatch("quot", "Float", other)),
        (other, _) => Err(type_mismatch("quot", "Int or Float", other)),
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

/// `(not= a b)` — structural inequality.
fn not_eq(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("not=", args)?;
    Ok(Value::Bool(a != b))
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

/// `(compare a b)` — returns `:lt`, `:eq`, or `:gt` keyword.
fn compare(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("compare", args)?;
    let ord = match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x
            .partial_cmp(y)
            .ok_or_else(|| "compare: NaN is not comparable".to_string())?,
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::Int(_), other) => return Err(type_mismatch("compare", "Int", other)),
        (Value::Float(_), other) => return Err(type_mismatch("compare", "Float", other)),
        (Value::Str(_), other) => return Err(type_mismatch("compare", "Str", other)),
        (other, _) => return Err(type_mismatch("compare", "Int, Float, or Str", other)),
    };
    let kw = match ord {
        std::cmp::Ordering::Less => "lt",
        std::cmp::Ordering::Equal => "eq",
        std::cmp::Ordering::Greater => "gt",
    };
    Ok(Value::Keyword {
        ns: None,
        name: Rc::from(kw),
    })
}

/// `(clamp x lo hi)` — restrict value to [lo, hi] range.
fn clamp(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(x), Value::Int(lo), Value::Int(hi)] => Ok(Value::Int((*x).clamp(*lo, *hi))),
        [Value::Float(x), Value::Float(lo), Value::Float(hi)] => {
            Ok(Value::Float(x.clamp(*lo, *hi)))
        }
        [_, _, _] => Err("clamp: all arguments must be the same numeric type".into()),
        _ => Err(format!(
            "`clamp` requires exactly 3 arguments, got {}",
            args.len()
        )),
    }
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
        Value::Map(entries) => match entries.get(idx) {
            Some(value) => Ok(option_some(value.clone())),
            None => Ok(option_none()),
        },
        other => Err(type_mismatch("get", "Vec or Map", other)),
    }
}

/// `(empty? coll)` — true if collection or string is empty.
fn empty_pred(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("empty?", args)?;
    match v {
        Value::Vec(items) => Ok(Value::Bool(items.is_empty())),
        Value::Map(entries) => Ok(Value::Bool(entries.is_empty())),
        Value::Set(items) => Ok(Value::Bool(items.is_empty())),
        Value::Str(s) => Ok(Value::Bool(s.is_empty())),
        other => Err(type_mismatch("empty?", "Vec, Map, Set, or Str", other)),
    }
}

/// `(type-of v)` — return the type name of a value as a string.
fn type_of_fn(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("type-of", args)?;
    Ok(Value::Str(Rc::from(v.type_name())))
}

/// `(keyword s)` — convert a string to a keyword.
///
/// `(keyword "foo")` → `:foo`
/// `(keyword "is-loading")` → `:is-loading`
fn keyword_fn(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("keyword", args)?;
    match v {
        Value::Str(s) => Ok(Value::Keyword {
            ns: None,
            name: Rc::from(s.as_ref()),
        }),
        other => Err(type_mismatch("keyword", "Str", other)),
    }
}

/// `(nth coll i)` — alias for `get` on indexed collections.
fn nth_fn(args: &[Value]) -> Result<Value, String> {
    get(args)
}

/// `(get-in coll path)` — nested access via key path vector.
fn get_in_fn(args: &[Value]) -> Result<Value, String> {
    let (coll, path) = two_args("get-in", args)?;
    let keys = match path {
        Value::Vec(items) => items,
        other => return Err(type_mismatch("get-in", "Vec (key path)", other)),
    };
    let mut current = coll.clone();
    for key in keys.iter() {
        let result = get(&[current, key.clone()])?;
        match result {
            Value::Adt { ref ctor, ref fields, .. } if ctor.as_ref() == "Some" => {
                current = fields[0].clone();
            }
            _ => return Ok(option_none()),
        }
    }
    Ok(option_some(current))
}

/// `(assoc-in coll path value)` — nested put via key path.
fn assoc_in_fn(args: &[Value]) -> Result<Value, String> {
    let (coll, path, value) = three_args("assoc-in", args)?;
    let keys = match path {
        Value::Vec(items) => items,
        other => return Err(type_mismatch("assoc-in", "Vec (key path)", other)),
    };
    if keys.is_empty() {
        return Ok(value.clone());
    }
    assoc_in_recursive(coll, keys.as_ref(), value)
}

fn assoc_in_recursive(coll: &Value, keys: &[Value], value: &Value) -> Result<Value, String> {
    if keys.len() == 1 {
        return put(&[coll.clone(), keys[0].clone(), value.clone()]);
    }
    let inner = match get(&[coll.clone(), keys[0].clone()])? {
        Value::Adt { ref ctor, ref fields, .. } if ctor.as_ref() == "Some" => fields[0].clone(),
        _ => Value::Map(Rc::new(nexl_runtime::value::NexlMap::new())),
    };
    let updated = assoc_in_recursive(&inner, &keys[1..], value)?;
    put(&[coll.clone(), keys[0].clone(), updated])
}

/// `(update coll key f)` — apply f to the value at key.
fn update_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [coll, key, f] => {
            let current = get(&[coll.clone(), key.clone()])?;
            let inner = match current {
                Value::Adt { ref ctor, ref fields, .. } if ctor.as_ref() == "Some" => {
                    fields[0].clone()
                }
                _ => return Err("`update`: key not found".to_string()),
            };
            let new_val = nexl_runtime::call_value(f, &[inner])?;
            put(&[coll.clone(), key.clone(), new_val])
        }
        _ => Err(format!(
            "`update` requires exactly 3 arguments, got {}",
            args.len()
        )),
    }
}

/// `(update-in coll path f)` — apply f to the value at nested path.
fn update_in_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [coll, path, f] => {
            let keys = match path {
                Value::Vec(items) => items,
                other => return Err(type_mismatch("update-in", "Vec (key path)", other)),
            };
            if keys.is_empty() {
                let new_val = nexl_runtime::call_value(f, std::slice::from_ref(coll))?;
                return Ok(new_val);
            }
            update_in_recursive(coll, keys.as_ref(), f)
        }
        _ => Err(format!(
            "`update-in` requires exactly 3 arguments, got {}",
            args.len()
        )),
    }
}

fn update_in_recursive(coll: &Value, keys: &[Value], f: &Value) -> Result<Value, String> {
    if keys.len() == 1 {
        return update_fn(&[coll.clone(), keys[0].clone(), f.clone()]);
    }
    let inner = match get(&[coll.clone(), keys[0].clone()])? {
        Value::Adt { ref ctor, ref fields, .. } if ctor.as_ref() == "Some" => fields[0].clone(),
        _ => return Err("`update-in`: key not found in path".into()),
    };
    let updated = update_in_recursive(&inner, &keys[1..], f)?;
    put(&[coll.clone(), keys[0].clone(), updated])
}

/// `(conj coll elem)` — polymorphic append: Vec end, Set add, Map takes [k v].
fn conj_fn(args: &[Value]) -> Result<Value, String> {
    let (coll, elem) = two_args("conj", args)?;
    match coll {
        Value::Vec(items) => {
            let mut next = items.as_ref().clone();
            next.push(elem.clone());
            Ok(Value::Vec(Rc::new(next)))
        }
        Value::Set(items) => {
            let mut next = items.as_ref().clone();
            if !next.contains(elem) {
                next.push(elem.clone());
            }
            Ok(Value::Set(Rc::new(next)))
        }
        Value::Map(_) => match elem {
            Value::Vec(pair) if pair.len() == 2 => {
                put(&[coll.clone(), pair[0].clone(), pair[1].clone()])
            }
            _ => Err("`conj` on Map requires a [key value] pair".into()),
        },
        other => Err(type_mismatch("conj", "Vec, Set, or Map", other)),
    }
}

/// `(into dest src)` — pour elements from src into dest.
fn into_fn(args: &[Value]) -> Result<Value, String> {
    let (dest, src) = two_args("into", args)?;
    match (dest, src) {
        (Value::Vec(d), Value::Vec(s)) => {
            let mut next = d.as_ref().clone();
            next.extend(s.iter().cloned());
            Ok(Value::Vec(Rc::new(next)))
        }
        (Value::Set(d), Value::Vec(s)) => {
            let mut next = d.as_ref().clone();
            for item in s.iter() {
                if !next.contains(item) {
                    next.push(item.clone());
                }
            }
            Ok(Value::Set(Rc::new(next)))
        }
        (Value::Set(d), Value::Set(s)) => {
            let mut next = d.as_ref().clone();
            for item in s.iter() {
                if !next.contains(item) {
                    next.push(item.clone());
                }
            }
            Ok(Value::Set(Rc::new(next)))
        }
        (Value::Map(d), Value::Map(s)) => {
            let mut next = d.as_ref().clone();
            for (k, v) in s.iter() {
                next = next.put(k.clone(), v.clone());
            }
            Ok(Value::Map(Rc::new(next)))
        }
        (Value::Vec(d), Value::Set(s)) => {
            let mut next = d.as_ref().clone();
            for item in s.iter() {
                next.push(item.clone());
            }
            Ok(Value::Vec(Rc::new(next)))
        }
        (other, _) => Err(type_mismatch("into", "Vec, Set, or Map", other)),
    }
}

/// `(concat a b)` — concatenate two collections of same type.
fn concat_fn(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("concat", args)?;
    match (a, b) {
        (Value::Vec(x), Value::Vec(y)) => {
            let mut next = x.as_ref().clone();
            next.extend(y.iter().cloned());
            Ok(Value::Vec(Rc::new(next)))
        }
        (Value::Str(x), Value::Str(y)) => {
            Ok(Value::Str(Rc::from(format!("{x}{y}").as_str())))
        }
        (other, _) => Err(type_mismatch("concat", "Vec or Str", other)),
    }
}

/// `(empty coll)` — return empty collection of same type.
fn empty_fn(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("empty", args)?;
    match v {
        Value::Vec(_) => Ok(Value::Vec(Rc::new(vec![]))),
        Value::Map(_) => Ok(Value::Map(Rc::new(nexl_runtime::value::NexlMap::new()))),
        Value::Set(_) => Ok(Value::Set(Rc::new(Default::default()))),
        Value::Str(_) => Ok(Value::Str(Rc::from(""))),
        other => Err(type_mismatch("empty", "Vec, Map, Set, or Str", other)),
    }
}

/// `(merge & maps)` — merge maps, rightmost wins on conflict.
fn merge_fn(args: &[Value]) -> Result<Value, String> {
    let mut result = nexl_runtime::value::NexlMap::new();
    for arg in args {
        match arg {
            Value::Map(m) => {
                for (k, v) in m.iter() {
                    result = result.put(k.clone(), v.clone());
                }
            }
            other => return Err(type_mismatch("merge", "Map", other)),
        }
    }
    Ok(Value::Map(Rc::new(result)))
}

/// `(merge-with f & maps)` — merge maps, resolve conflicts with f.
fn merge_with_fn(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("`merge-with` requires at least 1 argument (a function)".into());
    }
    let f = &args[0];
    let mut result = nexl_runtime::value::NexlMap::new();
    for arg in &args[1..] {
        match arg {
            Value::Map(m) => {
                for (k, v) in m.iter() {
                    if let Some(existing) = result.get(k) {
                        let merged = nexl_runtime::call_value(f, &[existing.clone(), v.clone()])?;
                        result = result.put(k.clone(), merged);
                    } else {
                        result = result.put(k.clone(), v.clone());
                    }
                }
            }
            other => return Err(type_mismatch("merge-with", "Map", other)),
        }
    }
    Ok(Value::Map(Rc::new(result)))
}

/// `(select-keys m ks)` — return submap with only the given keys.
fn select_keys_fn(args: &[Value]) -> Result<Value, String> {
    let (map_val, keys_val) = two_args("select-keys", args)?;
    match (map_val, keys_val) {
        (Value::Map(m), Value::Vec(keys)) => {
            let mut result = nexl_runtime::value::NexlMap::new();
            for k in keys.iter() {
                if let Some(v) = m.get(k) {
                    result = result.put(k.clone(), v.clone());
                }
            }
            Ok(Value::Map(Rc::new(result)))
        }
        (Value::Map(_), other) => Err(type_mismatch("select-keys", "Vec", other)),
        (other, _) => Err(type_mismatch("select-keys", "Map", other)),
    }
}

/// `(rename-keys m kmap)` — rename keys in map using kmap as old→new mapping.
fn rename_keys_fn(args: &[Value]) -> Result<Value, String> {
    let (map_val, kmap_val) = two_args("rename-keys", args)?;
    match (map_val, kmap_val) {
        (Value::Map(m), Value::Map(kmap)) => {
            let mut result = nexl_runtime::value::NexlMap::new();
            for (k, v) in m.iter() {
                let new_key = kmap.get(k).unwrap_or(k);
                result = result.put(new_key.clone(), v.clone());
            }
            Ok(Value::Map(Rc::new(result)))
        }
        (Value::Map(_), other) => Err(type_mismatch("rename-keys", "Map", other)),
        (other, _) => Err(type_mismatch("rename-keys", "Map", other)),
    }
}

/// `(zipmap keys vals)` — create map from parallel key/value vectors.
fn zipmap_fn(args: &[Value]) -> Result<Value, String> {
    let (keys_val, vals_val) = two_args("zipmap", args)?;
    match (keys_val, vals_val) {
        (Value::Vec(keys), Value::Vec(vals)) => {
            let mut result = nexl_runtime::value::NexlMap::new();
            for (k, v) in keys.iter().zip(vals.iter()) {
                result = result.put(k.clone(), v.clone());
            }
            Ok(Value::Map(Rc::new(result)))
        }
        (Value::Vec(_), other) => Err(type_mismatch("zipmap", "Vec", other)),
        (other, _) => Err(type_mismatch("zipmap", "Vec", other)),
    }
}

/// `(dissoc m & keys)` — remove key(s) from map.
fn dissoc_fn(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("`dissoc` requires at least 1 argument".into());
    }
    let map_val = &args[0];
    match map_val {
        Value::Map(m) => {
            let mut result = m.as_ref().clone();
            for key in &args[1..] {
                result = result.remove(key);
            }
            Ok(Value::Map(Rc::new(result)))
        }
        other => Err(type_mismatch("dissoc", "Map", other)),
    }
}

/// `(disj s & elems)` — remove element(s) from set.
fn disj_fn(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("`disj` requires at least 1 argument".into());
    }
    let set_val = &args[0];
    match set_val {
        Value::Set(items) => {
            let next: Vec<Value> = items
                .iter()
                .filter(|item| !args[1..].contains(item))
                .cloned()
                .collect();
            Ok(Value::Set(Rc::new(next)))
        }
        other => Err(type_mismatch("disj", "Set", other)),
    }
}

/// `(symmetric-difference a b)` — elements in either set but not both.
fn symmetric_difference_fn(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("symmetric-difference", args)?;
    match (a, b) {
        (Value::Set(x), Value::Set(y)) => {
            let mut result = Vec::new();
            for item in x.iter() {
                if !y.contains(item) {
                    result.push(item.clone());
                }
            }
            for item in y.iter() {
                if !x.contains(item) {
                    result.push(item.clone());
                }
            }
            Ok(Value::Set(Rc::new(result)))
        }
        (Value::Set(_), other) => Err(type_mismatch("symmetric-difference", "Set", other)),
        (other, _) => Err(type_mismatch("symmetric-difference", "Set", other)),
    }
}

/// `(subset? a b)` — true if every element of a is in b.
fn subset_pred(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("subset?", args)?;
    match (a, b) {
        (Value::Set(x), Value::Set(y)) => {
            Ok(Value::Bool(x.iter().all(|item| y.contains(item))))
        }
        (Value::Set(_), other) => Err(type_mismatch("subset?", "Set", other)),
        (other, _) => Err(type_mismatch("subset?", "Set", other)),
    }
}

/// `(superset? a b)` — true if every element of b is in a.
fn superset_pred(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("superset?", args)?;
    match (a, b) {
        (Value::Set(x), Value::Set(y)) => {
            Ok(Value::Bool(y.iter().all(|item| x.contains(item))))
        }
        (Value::Set(_), other) => Err(type_mismatch("superset?", "Set", other)),
        (other, _) => Err(type_mismatch("superset?", "Set", other)),
    }
}

/// `(disjoint? a b)` — true if a and b share no elements.
fn disjoint_pred(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("disjoint?", args)?;
    match (a, b) {
        (Value::Set(x), Value::Set(y)) => {
            Ok(Value::Bool(!x.iter().any(|item| y.contains(item))))
        }
        (Value::Set(_), other) => Err(type_mismatch("disjoint?", "Set", other)),
        (other, _) => Err(type_mismatch("disjoint?", "Set", other)),
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
            Ok(Value::Map(Rc::new(entries.put(idx.clone(), value.clone()))))
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
        Value::Map(entries) => Ok(Value::Map(Rc::new(entries.remove(key)))),
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
            entries.keys().cloned().collect(),
        ))),
        other => Err(type_mismatch("keys", "Map", other)),
    }
}

/// `(vals m)` — return map values in insertion order.
fn vals(args: &[Value]) -> Result<Value, String> {
    let coll = one_arg("vals", args)?;
    match coll {
        Value::Map(entries) => Ok(Value::Vec(Rc::new(
            entries.values().cloned().collect(),
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
        Value::Map(entries) => Ok(Value::Bool(entries.contains(key))),
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
            Ok(Value::Map(Rc::new(out.into())))
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
            Ok(Value::Map(Rc::new(out.into())))
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

/// Maximum number of elements `range` will eagerly allocate.
const RANGE_MAX: i64 = 10_000_000;

/// `(range n)`, `(range start end)`, or `(range start end step)`.
fn range_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(n)] => {
            if *n > RANGE_MAX {
                return Err(format!(
                    "`range` count too large: {n} (max {RANGE_MAX})"
                ));
            }
            let items: Vec<Value> = (0..*n).map(Value::Int).collect();
            Ok(Value::Vec(Rc::new(items)))
        }
        [Value::Int(start), Value::Int(end)] => {
            let count = end.saturating_sub(*start).max(0);
            if count > RANGE_MAX {
                return Err(format!(
                    "`range` span too large: {count} elements (max {RANGE_MAX})"
                ));
            }
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
                    if items.len() as i64 >= RANGE_MAX {
                        return Err(format!(
                            "`range` would produce more than {RANGE_MAX} elements"
                        ));
                    }
                    items.push(Value::Int(i));
                    i += step;
                }
            } else {
                while i > *end {
                    if items.len() as i64 >= RANGE_MAX {
                        return Err(format!(
                            "`range` would produce more than {RANGE_MAX} elements"
                        ));
                    }
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
    Ok(Value::Map(Rc::new(pairs.into())))
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

/// `(reject pred coll)` — complement of filter: keep where pred is false.
fn reject_fn(args: &[Value]) -> Result<Value, String> {
    let (pred, coll) = two_args("reject", args)?;
    match coll {
        Value::Vec(items) => {
            let mut out = Vec::new();
            for item in items.iter() {
                let keep = expect_bool("reject", call1(pred, item.clone())?)?;
                if !keep {
                    out.push(item.clone());
                }
            }
            Ok(Value::Vec(Rc::new(out)))
        }
        other => Err(type_mismatch("reject", "Vec", other)),
    }
}

/// `(keep f coll)` — map + filter-None in one pass.
fn keep_fn(args: &[Value]) -> Result<Value, String> {
    let (func, coll) = two_args("keep", args)?;
    match coll {
        Value::Vec(items) => {
            let mut out = Vec::new();
            for item in items.iter() {
                let result = call1(func, item.clone())?;
                match &result {
                    Value::Adt { ctor, fields, .. } if ctor.as_ref() == "Some" => {
                        out.push(fields[0].clone());
                    }
                    Value::Adt { ctor, .. } if ctor.as_ref() == "None" => {}
                    _ => out.push(result),
                }
            }
            Ok(Value::Vec(Rc::new(out)))
        }
        other => Err(type_mismatch("keep", "Vec", other)),
    }
}

/// `(some f coll)` — first non-None result of applying f.
fn some_fn(args: &[Value]) -> Result<Value, String> {
    let (func, coll) = two_args("some", args)?;
    match coll {
        Value::Vec(items) => {
            for item in items.iter() {
                let result = call1(func, item.clone())?;
                match &result {
                    Value::Adt { ctor, .. } if ctor.as_ref() == "None" => {}
                    Value::Bool(false) => {}
                    _ => return Ok(result),
                }
            }
            Ok(option_none())
        }
        other => Err(type_mismatch("some", "Vec", other)),
    }
}

/// `(every? pred coll)` — true if pred is true for all elements.
fn every_pred(args: &[Value]) -> Result<Value, String> {
    let (pred, coll) = two_args("every?", args)?;
    match coll {
        Value::Vec(items) => {
            for item in items.iter() {
                let v = expect_bool("every?", call1(pred, item.clone())?)?;
                if !v {
                    return Ok(Value::Bool(false));
                }
            }
            Ok(Value::Bool(true))
        }
        other => Err(type_mismatch("every?", "Vec", other)),
    }
}

/// `(any? pred coll)` — true if pred is true for any element.
fn any_pred(args: &[Value]) -> Result<Value, String> {
    let (pred, coll) = two_args("any?", args)?;
    match coll {
        Value::Vec(items) => {
            for item in items.iter() {
                let v = expect_bool("any?", call1(pred, item.clone())?)?;
                if v {
                    return Ok(Value::Bool(true));
                }
            }
            Ok(Value::Bool(false))
        }
        other => Err(type_mismatch("any?", "Vec", other)),
    }
}

/// `(not-any? pred coll)` — true if pred is false for all elements.
fn not_any_pred(args: &[Value]) -> Result<Value, String> {
    let result = any_pred(args)?;
    match result {
        Value::Bool(b) => Ok(Value::Bool(!b)),
        _ => Ok(result),
    }
}

/// `(not-every? pred coll)` — true if pred is false for any element.
fn not_every_pred(args: &[Value]) -> Result<Value, String> {
    let result = every_pred(args)?;
    match result {
        Value::Bool(b) => Ok(Value::Bool(!b)),
        _ => Ok(result),
    }
}

/// `(find pred coll)` — return first element where pred is true, as Option.
fn find_fn(args: &[Value]) -> Result<Value, String> {
    let (pred, coll) = two_args("find", args)?;
    match coll {
        Value::Vec(items) => {
            for item in items.iter() {
                let v = expect_bool("find", call1(pred, item.clone())?)?;
                if v {
                    return Ok(option_some(item.clone()));
                }
            }
            Ok(option_none())
        }
        other => Err(type_mismatch("find", "Vec", other)),
    }
}

/// `(find-index pred coll)` — return index of first matching element, as Option.
fn find_index_fn(args: &[Value]) -> Result<Value, String> {
    let (pred, coll) = two_args("find-index", args)?;
    match coll {
        Value::Vec(items) => {
            for (i, item) in items.iter().enumerate() {
                let v = expect_bool("find-index", call1(pred, item.clone())?)?;
                if v {
                    return Ok(option_some(Value::Int(i as i64)));
                }
            }
            Ok(option_none())
        }
        other => Err(type_mismatch("find-index", "Vec", other)),
    }
}

/// `(map-indexed f coll)` — like map but f receives (index, element).
fn map_indexed_fn(args: &[Value]) -> Result<Value, String> {
    let (func, coll) = two_args("map-indexed", args)?;
    match coll {
        Value::Vec(items) => {
            let mut out = Vec::new();
            for (i, item) in items.iter().enumerate() {
                out.push(call2(func, Value::Int(i as i64), item.clone())?);
            }
            Ok(Value::Vec(Rc::new(out)))
        }
        other => Err(type_mismatch("map-indexed", "Vec", other)),
    }
}

/// `(reduce-indexed f init coll)` — like reduce but f receives (acc, index, element).
fn reduce_indexed_fn(args: &[Value]) -> Result<Value, String> {
    let (func, init, coll) = three_args("reduce-indexed", args)?;
    match coll {
        Value::Vec(items) => {
            let mut acc = init.clone();
            for (i, item) in items.iter().enumerate() {
                let args = [acc, Value::Int(i as i64), item.clone()];
                acc = apply_value(func, &args).map_err(|e| e.to_string())?;
            }
            Ok(acc)
        }
        other => Err(type_mismatch("reduce-indexed", "Vec", other)),
    }
}

/// `(sort-with comparator coll)` — sort using a custom comparator function.
fn sort_with_fn(args: &[Value]) -> Result<Value, String> {
    let (cmp_fn, coll) = two_args("sort-with", args)?;
    match coll {
        Value::Vec(items) => {
            let mut out = items.as_ref().clone();
            let mut error: Option<String> = None;
            out.sort_by(|a, b| {
                if error.is_some() {
                    return std::cmp::Ordering::Equal;
                }
                match call2(cmp_fn, a.clone(), b.clone()) {
                    Ok(Value::Int(n)) => {
                        if n < 0 {
                            std::cmp::Ordering::Less
                        } else if n > 0 {
                            std::cmp::Ordering::Greater
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    }
                    Ok(other) => {
                        error = Some(format!(
                            "`sort-with` comparator must return Int, got {}",
                            other.type_name()
                        ));
                        std::cmp::Ordering::Equal
                    }
                    Err(e) => {
                        error = Some(e);
                        std::cmp::Ordering::Equal
                    }
                }
            });
            if let Some(e) = error {
                return Err(e);
            }
            Ok(Value::Vec(Rc::new(out)))
        }
        other => Err(type_mismatch("sort-with", "Vec", other)),
    }
}

/// `(distinct coll)` — remove duplicates, preserving first occurrence order.
fn distinct_fn(args: &[Value]) -> Result<Value, String> {
    let coll = one_arg("distinct", args)?;
    match coll {
        Value::Vec(items) => {
            let mut seen = Vec::new();
            let mut out = Vec::new();
            for item in items.iter() {
                if !seen.contains(item) {
                    seen.push(item.clone());
                    out.push(item.clone());
                }
            }
            Ok(Value::Vec(Rc::new(out)))
        }
        other => Err(type_mismatch("distinct", "Vec", other)),
    }
}

/// `(flatten coll)` — flatten one level of nesting.
fn flatten_fn(args: &[Value]) -> Result<Value, String> {
    let coll = one_arg("flatten", args)?;
    match coll {
        Value::Vec(items) => {
            let mut out = Vec::new();
            for item in items.iter() {
                match item {
                    Value::Vec(inner) => out.extend(inner.iter().cloned()),
                    other => out.push(other.clone()),
                }
            }
            Ok(Value::Vec(Rc::new(out)))
        }
        other => Err(type_mismatch("flatten", "Vec", other)),
    }
}

/// `(frequencies coll)` — count occurrences of each element.
fn frequencies_fn(args: &[Value]) -> Result<Value, String> {
    let coll = one_arg("frequencies", args)?;
    match coll {
        Value::Vec(items) => {
            let mut result = nexl_runtime::value::NexlMap::new();
            for item in items.iter() {
                let count = result
                    .get(item)
                    .map(|v| match v {
                        Value::Int(n) => *n,
                        _ => 0,
                    })
                    .unwrap_or(0);
                result = result.put(item.clone(), Value::Int(count + 1));
            }
            Ok(Value::Map(Rc::new(result)))
        }
        other => Err(type_mismatch("frequencies", "Vec", other)),
    }
}

/// `(partition-by f coll)` — split into groups when f's return value changes.
fn partition_by_fn(args: &[Value]) -> Result<Value, String> {
    let (func, coll) = two_args("partition-by", args)?;
    match coll {
        Value::Vec(items) => {
            if items.is_empty() {
                return Ok(Value::Vec(Rc::new(vec![])));
            }
            let mut groups: Vec<Value> = Vec::new();
            let mut current_group: Vec<Value> = Vec::new();
            let mut current_key = call1(func, items[0].clone())?;
            current_group.push(items[0].clone());
            for item in items.iter().skip(1) {
                let key = call1(func, item.clone())?;
                if key == current_key {
                    current_group.push(item.clone());
                } else {
                    groups.push(Value::Vec(Rc::new(current_group)));
                    current_group = vec![item.clone()];
                    current_key = key;
                }
            }
            groups.push(Value::Vec(Rc::new(current_group)));
            Ok(Value::Vec(Rc::new(groups)))
        }
        other => Err(type_mismatch("partition-by", "Vec", other)),
    }
}

/// `(interleave a b)` — interleave elements from two vectors.
fn interleave_fn(args: &[Value]) -> Result<Value, String> {
    let (a, b) = two_args("interleave", args)?;
    match (a, b) {
        (Value::Vec(x), Value::Vec(y)) => {
            let mut out = Vec::new();
            let len = x.len().min(y.len());
            for i in 0..len {
                out.push(x[i].clone());
                out.push(y[i].clone());
            }
            Ok(Value::Vec(Rc::new(out)))
        }
        (Value::Vec(_), other) => Err(type_mismatch("interleave", "Vec", other)),
        (other, _) => Err(type_mismatch("interleave", "Vec", other)),
    }
}

/// `(interpose sep coll)` — insert sep between each element.
fn interpose_fn(args: &[Value]) -> Result<Value, String> {
    let (sep, coll) = two_args("interpose", args)?;
    match coll {
        Value::Vec(items) => {
            let mut out = Vec::new();
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(sep.clone());
                }
                out.push(item.clone());
            }
            Ok(Value::Vec(Rc::new(out)))
        }
        other => Err(type_mismatch("interpose", "Vec", other)),
    }
}

/// `(zip-with f a b)` — zip two vectors applying f to each pair.
fn zip_with_fn(args: &[Value]) -> Result<Value, String> {
    let (func, a, b) = three_args("zip-with", args)?;
    match (a, b) {
        (Value::Vec(x), Value::Vec(y)) => {
            let mut out = Vec::new();
            let len = x.len().min(y.len());
            for i in 0..len {
                out.push(call2(func, x[i].clone(), y[i].clone())?);
            }
            Ok(Value::Vec(Rc::new(out)))
        }
        (Value::Vec(_), other) => Err(type_mismatch("zip-with", "Vec", other)),
        (other, _) => Err(type_mismatch("zip-with", "Vec", other)),
    }
}

/// `(pr-str v)` — readable string representation with quotes and escapes.
fn pr_str_fn(args: &[Value]) -> Result<Value, String> {
    let v = one_arg("pr-str", args)?;
    Ok(Value::Str(Rc::from(format!("{v}").as_str())))
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

// ---------------------------------------------------------------------------
// Atom functions (mutable reference cells)
// ---------------------------------------------------------------------------

use std::cell::RefCell;

/// `(atom val)` — create a mutable reference cell holding `val`.
fn atom_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [val] => Ok(Value::Atom(Rc::new(RefCell::new(val.clone())))),
        _ => Err(format!("`atom` requires exactly 1 argument, got {}", args.len())),
    }
}

/// `(deref atom)` — return the current value held by the atom.
fn deref_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Atom(cell)] => Ok(cell.borrow().clone()),
        [other] => Err(format!("`deref` expected Atom, got {}", other.type_name())),
        _ => Err(format!("`deref` requires exactly 1 argument, got {}", args.len())),
    }
}

/// `(reset! atom new-val)` — replace the atom's value, returning the new value.
fn reset_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Atom(cell), new_val] => {
            *cell.borrow_mut() = new_val.clone();
            Ok(new_val.clone())
        }
        [other, _] => Err(format!("`reset!` expected Atom as first arg, got {}", other.type_name())),
        _ => Err(format!("`reset!` requires exactly 2 arguments, got {}", args.len())),
    }
}

/// `(swap! atom f)` — apply `f` to current atom value, store and return result.
fn swap_fn(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Atom(cell), f] => {
            let current = cell.borrow().clone();
            let new_val = nexl_runtime::call_value(f, &[current])
                .map_err(|e| format!("`swap!` function error: {e}"))?;
            *cell.borrow_mut() = new_val.clone();
            Ok(new_val)
        }
        [other, _] => Err(format!("`swap!` expected Atom as first arg, got {}", other.type_name())),
        _ => Err(format!("`swap!` requires exactly 2 arguments, got {}", args.len())),
    }
}

/// Build the pre-defined `SequentialExecutor` handler (spec §12.6).
///
/// Handles the `Concurrent` effect by running all concurrent operations
/// synchronously inline, making concurrent code deterministic under test.
///
/// ```nexl
/// (handle [SequentialExecutor]
///   (fork (fn [] expensive-computation)))
/// ```
fn sequential_executor_handler() -> Value {
    use nexl_runtime::{BuiltHandlerEffect, value::HandlerDef};

    // (fork thunk) → run thunk immediately and return result
    let fork_fn = Value::NativeClosure {
        name: Rc::from("fork"),
        f: Rc::new(|args: &[Value]| match args {
            [thunk] => nexl_runtime::call_value(thunk, &[]),
            _ => Err(format!("`fork` expects 1 argument (thunk), got {}", args.len())),
        }),
    };

    // (join future) → return the value as-is (already computed by fork)
    let join_fn = Value::NativeClosure {
        name: Rc::from("join"),
        f: Rc::new(|args: &[Value]| match args {
            [future] => Ok(future.clone()),
            _ => Err(format!("`join` expects 1 argument (future), got {}", args.len())),
        }),
    };

    Value::Handler(Rc::new(HandlerDef {
        name: Rc::from("SequentialExecutor"),
        params: vec![],
        effects: vec![],
        built_ops: vec![BuiltHandlerEffect {
            name: "Concurrent".to_string(),
            ops: vec![
                ("fork".to_string(), fork_fn),
                ("join".to_string(), join_fn),
            ],
        }],
    }))
}
