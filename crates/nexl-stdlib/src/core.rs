//! `core` module — fundamental functions always available.
//!
//! Provides: `identity`, `comp`, `partial`, `constantly`, `juxt`, `apply`.

use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `core` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        (
            "identity",
            identity as fn(&[Value]) -> Result<Value, String>,
        ),
        ("comp", comp),
        ("partial", partial),
        ("constantly", constantly),
        ("juxt", juxt),
        ("apply", apply),
        ("complement", complement),
        ("pipe", pipe),
        ("tap", tap),
        ("memoize", memoize),
        ("trampoline", trampoline),
    ]
}

/// `(identity x)` — returns its argument unchanged.
fn identity(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => Ok(v.clone()),
        _ => Err(format!(
            "`identity` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(comp f g)` — returns a function that applies `g` then `f`.
///
/// `((comp f g) x)` is equivalent to `(f (g x))`.
fn comp(args: &[Value]) -> Result<Value, String> {
    match args {
        [f, g] => {
            let f = f.clone();
            let g = g.clone();
            Ok(Value::NativeClosure {
                name: Rc::from("comp"),
                f: Rc::new(move |inner_args: &[Value]| {
                    let intermediate = call_value(&g, inner_args)?;
                    call_value(&f, &[intermediate])
                }),
            })
        }
        _ => Err(format!(
            "`comp` requires exactly 2 arguments, got {}",
            args.len()
        )),
    }
}

/// `(partial f args...)` — returns a function with `args` pre-applied.
///
/// `((partial + 1) 2)` is equivalent to `(+ 1 2)`.
fn partial(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("`partial` requires at least 1 argument (the function)".into());
    }
    let func = args[0].clone();
    let bound: Vec<Value> = args[1..].to_vec();
    Ok(Value::NativeClosure {
        name: Rc::from("partial"),
        f: Rc::new(move |inner_args: &[Value]| {
            let mut all_args = bound.clone();
            all_args.extend_from_slice(inner_args);
            call_value(&func, &all_args)
        }),
    })
}

/// `(constantly x)` — returns a function that always returns `x`.
fn constantly(args: &[Value]) -> Result<Value, String> {
    match args {
        [v] => {
            let v = v.clone();
            Ok(Value::NativeClosure {
                name: Rc::from("constantly"),
                f: Rc::new(move |_: &[Value]| Ok(v.clone())),
            })
        }
        _ => Err(format!(
            "`constantly` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(juxt f g ...)` — returns a function that applies each fn and collects results into a Vec.
///
/// `((juxt first last) [1 2 3])` => `[(Some 1) (Some 3)]`.
fn juxt(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("`juxt` requires at least 1 argument".into());
    }
    let funcs: Vec<Value> = args.to_vec();
    Ok(Value::NativeClosure {
        name: Rc::from("juxt"),
        f: Rc::new(move |inner_args: &[Value]| {
            let mut results = Vec::with_capacity(funcs.len());
            for func in &funcs {
                results.push(call_value(func, inner_args)?);
            }
            Ok(Value::Vec(Rc::new(results)))
        }),
    })
}

/// `(apply f args-vec)` or `(apply f arg1 arg2 ... args-vec)` — call `f` with args.
///
/// The last argument must be a Vec; preceding arguments are prepended.
fn apply(args: &[Value]) -> Result<Value, String> {
    if args.len() < 2 {
        return Err(format!(
            "`apply` requires at least 2 arguments (function + args-vec), got {}",
            args.len()
        ));
    }
    let func = &args[0];
    let last = &args[args.len() - 1];
    let Value::Vec(trailing) = last else {
        return Err(format!(
            "`apply` last argument must be a Vec, got {}",
            last.type_name()
        ));
    };

    let mut all_args: Vec<Value> = args[1..args.len() - 1].to_vec();
    all_args.extend(trailing.iter().cloned());
    call_value(func, &all_args)
}

/// `(complement f)` — returns a function that negates f's boolean result.
fn complement(args: &[Value]) -> Result<Value, String> {
    match args {
        [f] => {
            let f = f.clone();
            Ok(Value::NativeClosure {
                name: Rc::from("complement"),
                f: Rc::new(move |inner_args: &[Value]| {
                    let result = call_value(&f, inner_args)?;
                    match result {
                        Value::Bool(b) => Ok(Value::Bool(!b)),
                        _ => Err(format!(
                            "`complement` function must return Bool, got {}",
                            result.type_name()
                        )),
                    }
                }),
            })
        }
        _ => Err(format!(
            "`complement` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(pipe x f1 f2 ...)` — thread x through f1, f2, etc.
fn pipe(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("`pipe` requires at least 1 argument".into());
    }
    let mut value = args[0].clone();
    for f in &args[1..] {
        value = call_value(f, &[value])?;
    }
    Ok(value)
}

/// `(tap x f)` — call f with x for side effects, return x. Subject-first.
fn tap(args: &[Value]) -> Result<Value, String> {
    match args {
        [x, f] => {
            let _ = call_value(f, std::slice::from_ref(x))?;
            Ok(x.clone())
        }
        _ => Err(format!(
            "`tap` requires exactly 2 arguments, got {}",
            args.len()
        )),
    }
}

/// `(memoize f)` — returns a memoized version of f.
type MemoCache = Rc<std::cell::RefCell<Vec<(Vec<Value>, Value)>>>;

fn memoize(args: &[Value]) -> Result<Value, String> {
    match args {
        [f] => {
            let f = f.clone();
            let cache: MemoCache = Rc::new(std::cell::RefCell::new(Vec::new()));
            Ok(Value::NativeClosure {
                name: Rc::from("memoize"),
                f: Rc::new(move |inner_args: &[Value]| {
                    let key: Vec<Value> = inner_args.to_vec();
                    {
                        let cache_ref = cache.borrow();
                        for (k, v) in cache_ref.iter() {
                            if k == &key {
                                return Ok(v.clone());
                            }
                        }
                    }
                    let result = call_value(&f, inner_args)?;
                    cache.borrow_mut().push((key, result.clone()));
                    Ok(result)
                }),
            })
        }
        _ => Err(format!(
            "`memoize` requires exactly 1 argument, got {}",
            args.len()
        )),
    }
}

/// `(trampoline f args...)` — call f, if result is a fn, call it again, repeat until non-fn.
fn trampoline(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("`trampoline` requires at least 1 argument".into());
    }
    let mut result = call_value(&args[0], &args[1..])?;
    loop {
        match &result {
            Value::NativeFunction(_) | Value::NativeClosure { .. } | Value::Function(_) => {
                result = call_value(&result, &[])?;
            }
            _ => return Ok(result),
        }
    }
}

/// Helper: call a Value as a function with the given args.
///
/// Delegates to `nexl_runtime::call_value`, which handles NativeFunction,
/// NativeClosure, and (via the registered evaluator) Function values.
fn call_value(callee: &Value, args: &[Value]) -> Result<Value, String> {
    nexl_runtime::call_value(callee, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_returns_arg() {
        let result = identity(&[Value::Int(42)]).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn test_identity_wrong_arity() {
        assert!(identity(&[]).is_err());
        assert!(identity(&[Value::Int(1), Value::Int(2)]).is_err());
    }

    #[test]
    fn test_comp_creates_closure() {
        let result = comp(&[
            Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
                name: "id",
                f: |args| Ok(args[0].clone()),
            })),
            Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
                name: "id",
                f: |args| Ok(args[0].clone()),
            })),
        ])
        .unwrap();
        assert!(matches!(result, Value::NativeClosure { .. }));
    }

    #[test]
    fn test_constantly_creates_closure() {
        let result = constantly(&[Value::Int(99)]).unwrap();
        match &result {
            Value::NativeClosure { f, .. } => {
                let v = f(&[Value::Str(Rc::from("ignored"))]).unwrap();
                assert_eq!(v, Value::Int(99));
            }
            _ => panic!("expected NativeClosure"),
        }
    }

    #[test]
    fn test_apply_basic() {
        let add_fn = Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
            name: "+",
            f: |args| {
                let mut sum = 0i64;
                for v in args {
                    match v {
                        Value::Int(n) => sum += n,
                        _ => return Err("expected Int".into()),
                    }
                }
                Ok(Value::Int(sum))
            },
        }));
        let args_vec = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        let result = apply(&[add_fn, args_vec]).unwrap();
        assert_eq!(result, Value::Int(6));
    }
}
