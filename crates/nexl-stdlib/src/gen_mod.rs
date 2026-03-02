//! `gen` module — property test generators (spec §12.3).
//!
//! A generator is a function `(fn [seed: Int]) -> Value`. Primitive generators
//! are registered as stdlib functions callable directly with a seed:
//!
//! - `(gen/int seed)` → Int (generated from seed)
//! - `(gen/bool seed)` → Bool
//! - `(gen/vec gen/int seed)` → Vec of Ints (combinators take inner gens)
//!
//! The `check` form calls generators with auto-incremented seeds.

use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// LCG step: multiply-add modular RNG (Knuth/Newlib constants).
///
/// Public so `nexl-eval`'s `check` form can generate seeds deterministically.
#[inline]
pub fn lcg_next(seed: i64) -> i64 {
    seed.wrapping_mul(6364136223846793005_i64)
        .wrapping_add(1442695040888963407_i64)
}

/// Map a seed to a usize in range `[0, max)`.
#[inline]
fn seed_to_usize(seed: i64, max: usize) -> usize {
    if max == 0 { return 0; }
    (seed.unsigned_abs() as usize) % max
}

/// Wrap a `fn(seed: i64) -> Value` as a `NativeClosure` generator value.
fn make_gen(name: &'static str, f: impl Fn(i64) -> Value + 'static) -> Value {
    Value::NativeClosure {
        name: Rc::from(name),
        f: Rc::new(move |args: &[Value]| match args {
            [Value::Int(seed)] => Ok(f(*seed)),
            _ => Err(format!(
                "generator `{name}` expects 1 Int argument (seed), got {}",
                args.len()
            )),
        }),
    }
}

// ---------------------------------------------------------------------------
// Primitive generators (registered as StdlibEntry — called directly w/ seed)
// ---------------------------------------------------------------------------

/// `(gen/int seed)` — generate an arbitrary i64.
fn int_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(seed)] => Ok(Value::Int(lcg_next(*seed))),
        _ => Err(format!("`gen/int` expects 1 Int (seed), got {}", args.len())),
    }
}

/// `(gen/bool seed)` — generate a Bool.
fn bool_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(seed)] => Ok(Value::Bool(lcg_next(*seed) & 1 == 0)),
        _ => Err(format!("`gen/bool` expects 1 Int (seed), got {}", args.len())),
    }
}

/// `(gen/float seed)` — generate a Float.
fn float_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(seed)] => {
            let bits = lcg_next(*seed) as u64;
            let f = (bits as f64 / u64::MAX as f64) * 2.0 - 1.0;
            Ok(Value::Float(f * 1_000_000.0))
        }
        _ => Err(format!("`gen/float` expects 1 Int (seed), got {}", args.len())),
    }
}

/// `(gen/str seed)` — generate an alphanumeric string of length 0–20.
fn str_gen(args: &[Value]) -> Result<Value, String> {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    match args {
        [Value::Int(seed)] => {
            let s0 = lcg_next(*seed);
            let len = seed_to_usize(s0, 20);
            let mut s = String::with_capacity(len);
            let mut cur = lcg_next(s0);
            for _ in 0..len {
                cur = lcg_next(cur);
                s.push(CHARS[seed_to_usize(cur, CHARS.len())] as char);
            }
            Ok(Value::Str(Rc::from(s.as_str())))
        }
        _ => Err(format!("`gen/str` expects 1 Int (seed), got {}", args.len())),
    }
}

/// `(gen/keyword seed)` — generate an arbitrary keyword.
fn keyword_gen(args: &[Value]) -> Result<Value, String> {
    const NAMES: &[&str] = &["foo", "bar", "baz", "qux", "alpha", "beta", "gamma"];
    match args {
        [Value::Int(seed)] => {
            let idx = seed_to_usize(lcg_next(*seed), NAMES.len());
            Ok(Value::Keyword { ns: None, name: Rc::from(NAMES[idx]) })
        }
        _ => Err(format!("`gen/keyword` expects 1 Int (seed), got {}", args.len())),
    }
}

// ---------------------------------------------------------------------------
// Bounded primitive generators (return generator closures)
// ---------------------------------------------------------------------------

/// `(gen/int-range lo hi)` — returns a generator for Int in [lo, hi].
fn int_range_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(lo), Value::Int(hi)] => {
            let (lo, hi) = (*lo, *hi);
            if lo > hi {
                return Err(format!("`gen/int-range` requires lo <= hi, got {lo} > {hi}"));
            }
            Ok(make_gen("gen/int-range", move |seed| {
                let range = (hi - lo + 1) as u64;
                let offset = (lcg_next(seed).unsigned_abs()) % range;
                Value::Int(lo + offset as i64)
            }))
        }
        _ => Err("`gen/int-range` requires 2 Int arguments (lo hi)".into()),
    }
}

/// `(gen/float-range lo hi)` — returns a generator for Float in [lo, hi].
fn float_range_gen(args: &[Value]) -> Result<Value, String> {
    let lo = match args.first() {
        Some(Value::Float(f)) => *f,
        Some(Value::Int(i)) => *i as f64,
        _ => return Err("`gen/float-range` requires 2 numeric arguments".into()),
    };
    let hi = match args.get(1) {
        Some(Value::Float(f)) => *f,
        Some(Value::Int(i)) => *i as f64,
        _ => return Err("`gen/float-range` requires 2 numeric arguments".into()),
    };
    if args.len() != 2 {
        return Err("`gen/float-range` requires exactly 2 arguments".into());
    }
    Ok(make_gen("gen/float-range", move |seed| {
        let t = (lcg_next(seed).unsigned_abs() as f64) / (u64::MAX as f64);
        Value::Float(lo + t * (hi - lo))
    }))
}

// ---------------------------------------------------------------------------
// Collection generators (return generator closures)
// ---------------------------------------------------------------------------

/// `(gen/vec inner-gen)` — returns a generator for Vec.
fn vec_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [inner] => {
            let inner = inner.clone();
            Ok(make_gen("gen/vec", move |seed| {
                let len = seed_to_usize(lcg_next(seed), 10);
                let mut items = Vec::with_capacity(len);
                let mut cur = lcg_next(lcg_next(seed));
                for _ in 0..len {
                    cur = lcg_next(cur);
                    if let Ok(v) = nexl_runtime::call_value(&inner, &[Value::Int(cur)]) {
                        items.push(v);
                    }
                }
                Value::Vec(Rc::new(items))
            }))
        }
        [inner, Value::Int(min), Value::Int(max)] => {
            let inner = inner.clone();
            let (min, max) = (*min as usize, *max as usize);
            Ok(make_gen("gen/vec(bounded)", move |seed| {
                let range = if max > min { max - min + 1 } else { 1 };
                let len = min + seed_to_usize(lcg_next(seed), range);
                let mut items = Vec::with_capacity(len);
                let mut cur = lcg_next(lcg_next(seed));
                for _ in 0..len {
                    cur = lcg_next(cur);
                    if let Ok(v) = nexl_runtime::call_value(&inner, &[Value::Int(cur)]) {
                        items.push(v);
                    }
                }
                Value::Vec(Rc::new(items))
            }))
        }
        _ => Err("`gen/vec` requires 1 or 3 arguments (inner-gen [min max])".into()),
    }
}

/// `(gen/tuple gen1 gen2 ...)` — fixed-length heterogeneous Vec generator.
fn tuple_gen(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("`gen/tuple` requires at least 1 generator".into());
    }
    let gens: Vec<Value> = args.to_vec();
    Ok(make_gen("gen/tuple", move |seed| {
        let mut items = Vec::with_capacity(gens.len());
        let mut cur = seed;
        for g in &gens {
            cur = lcg_next(cur);
            if let Ok(v) = nexl_runtime::call_value(g, &[Value::Int(cur)]) {
                items.push(v);
            }
        }
        Value::Vec(Rc::new(items))
    }))
}

/// `(gen/option inner-gen)` — returns a generator for None or Some(value).
fn option_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [inner] => {
            let inner = inner.clone();
            Ok(make_gen("gen/option", move |seed| {
                if lcg_next(seed) & 1 == 0 {
                    Value::Adt {
                        type_name: Rc::from("Option"),
                        ctor: Rc::from("None"),
                        fields: Rc::new(vec![]),
                    }
                } else {
                    let iseed = lcg_next(lcg_next(seed));
                    let iv = nexl_runtime::call_value(&inner, &[Value::Int(iseed)])
                        .unwrap_or(Value::Unit);
                    Value::Adt {
                        type_name: Rc::from("Option"),
                        ctor: Rc::from("Some"),
                        fields: Rc::new(vec![iv]),
                    }
                }
            }))
        }
        _ => Err("`gen/option` requires 1 generator".into()),
    }
}

/// `(gen/result ok-gen err-gen)` — returns a generator for Ok(v) or Err(v).
fn result_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [og, eg] => {
            let (og, eg) = (og.clone(), eg.clone());
            Ok(make_gen("gen/result", move |seed| {
                let next = lcg_next(seed);
                if next & 1 == 0 {
                    let iv = nexl_runtime::call_value(&og, &[Value::Int(lcg_next(next))])
                        .unwrap_or(Value::Unit);
                    Value::Adt { type_name: Rc::from("Result"), ctor: Rc::from("Ok"), fields: Rc::new(vec![iv]) }
                } else {
                    let iv = nexl_runtime::call_value(&eg, &[Value::Int(lcg_next(next))])
                        .unwrap_or(Value::Unit);
                    Value::Adt { type_name: Rc::from("Result"), ctor: Rc::from("Err"), fields: Rc::new(vec![iv]) }
                }
            }))
        }
        _ => Err("`gen/result` requires 2 generators (ok-gen err-gen)".into()),
    }
}

// ---------------------------------------------------------------------------
// Picker generators (return generator closures)
// ---------------------------------------------------------------------------

/// `(gen/element coll)` — returns a generator that picks a random element.
fn element_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(items)] => {
            let items = Rc::clone(items);
            Ok(make_gen("gen/element", move |seed| {
                if items.is_empty() {
                    Value::Unit
                } else {
                    items[seed_to_usize(lcg_next(seed), items.len())].clone()
                }
            }))
        }
        _ => Err("`gen/element` requires a Vec argument".into()),
    }
}

/// `(gen/one-of [gen1 gen2 ...])` — picks uniformly from a list of generators.
fn one_of_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(gens)] => {
            let gens = Rc::clone(gens);
            Ok(make_gen("gen/one-of", move |seed| {
                if gens.is_empty() {
                    return Value::Unit;
                }
                let idx = seed_to_usize(lcg_next(seed), gens.len());
                nexl_runtime::call_value(&gens[idx], &[Value::Int(lcg_next(lcg_next(seed)))])
                    .unwrap_or(Value::Unit)
            }))
        }
        _ => Err("`gen/one-of` requires a Vec of generators".into()),
    }
}

/// `(gen/constant val)` — always produces the same value.
fn constant_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [val] => {
            let val = val.clone();
            Ok(make_gen("gen/constant", move |_seed| val.clone()))
        }
        _ => Err("`gen/constant` requires 1 argument".into()),
    }
}

// ---------------------------------------------------------------------------
// Combinators (return generator closures)
// ---------------------------------------------------------------------------

/// `(gen/fmap f gen)` — transform generated values with `f`.
fn fmap_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [f, gv] => {
            let (f, gv) = (f.clone(), gv.clone());
            Ok(make_gen("gen/fmap", move |seed| {
                let val = nexl_runtime::call_value(&gv, &[Value::Int(seed)])
                    .unwrap_or(Value::Unit);
                nexl_runtime::call_value(&f, &[val]).unwrap_or(Value::Unit)
            }))
        }
        _ => Err("`gen/fmap` requires 2 arguments (f gen)".into()),
    }
}

/// `(gen/such-that pred gen)` — filter (retries up to 100 times).
fn such_that_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [pred, gv] => {
            let (pred, gv) = (pred.clone(), gv.clone());
            Ok(make_gen("gen/such-that", move |seed| {
                let mut cur = seed;
                for _ in 0..100 {
                    cur = lcg_next(cur);
                    let val = nexl_runtime::call_value(&gv, &[Value::Int(cur)])
                        .unwrap_or(Value::Unit);
                    if let Ok(Value::Bool(true)) = nexl_runtime::call_value(&pred, std::slice::from_ref(&val)) {
                        return val;
                    }
                }
                Value::Unit
            }))
        }
        _ => Err("`gen/such-that` requires 2 arguments (pred gen)".into()),
    }
}

/// `(gen/sample gen n)` — generate `n` sample values for debugging.
fn sample_gen(args: &[Value]) -> Result<Value, String> {
    match args {
        [gv, Value::Int(n)] => {
            let (gv, n) = (gv.clone(), *n);
            let mut items = Vec::with_capacity(n as usize);
            let mut seed = 42_i64;
            for _ in 0..n {
                seed = lcg_next(seed);
                match nexl_runtime::call_value(&gv, &[Value::Int(seed)]) {
                    Ok(v) => items.push(v),
                    Err(_) => break,
                }
            }
            Ok(Value::Vec(Rc::new(items)))
        }
        _ => Err("`gen/sample` requires 2 arguments (gen n)".into()),
    }
}

// ---------------------------------------------------------------------------
// Public entries
// ---------------------------------------------------------------------------

/// Return all entries in the `gen` module.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("int",         int_gen as fn(&[Value]) -> Result<Value, String>),
        ("bool",        bool_gen),
        ("float",       float_gen),
        ("str",         str_gen),
        ("keyword",     keyword_gen),
        ("int-range",   int_range_gen),
        ("float-range", float_range_gen),
        ("vec",         vec_gen),
        ("tuple",       tuple_gen),
        ("option",      option_gen),
        ("result",      result_gen),
        ("element",     element_gen),
        ("one-of",      one_of_gen),
        ("constant",    constant_gen),
        ("fmap",        fmap_gen),
        ("such-that",   such_that_gen),
        ("sample",      sample_gen),
    ]
}
