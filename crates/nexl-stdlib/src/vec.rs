//! `vec` module — vector-specific operations.
//!
//! Provides constructors, slicers, structural mutations, search, and combinatorics.
//! Folding and dedup combinators are written in Nexl (`vec_impl.nx`).

use std::rc::Rc;

use nexl_runtime::Value;

use crate::StdlibEntry;

/// Return all `vec` module function entries.
pub fn entries() -> Vec<StdlibEntry> {
    vec![
        ("of", of as fn(&[Value]) -> Result<Value, String>),
        ("repeat", repeat),
        ("init", init),
        ("chunk", chunk),
        ("window", window),
        ("split-at", split_at),
        ("span", span),
        ("insert", insert),
        ("remove-at", remove_at),
        ("swap", swap),
        ("rotate-left", rotate_left),
        ("rotate-right", rotate_right),
        ("permutations", permutations),
        ("combinations", combinations),
        ("binary-search", binary_search),
    ]
}

/// `(vec/of elem...)` — construct a Vec from variadic arguments.
fn of(args: &[Value]) -> Result<Value, String> {
    Ok(Value::Vec(Rc::new(args.to_vec())))
}

/// Maximum number of elements allowed in eagerly-allocated vectors.
const MAX_ALLOC: i64 = 10_000_000;

/// `(vec/repeat n val)` — return a Vec of `n` copies of `val`.
fn repeat(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(n), v] => {
            if *n < 0 {
                return Err(format!("`vec/repeat` count must be >= 0, got {n}"));
            }
            if *n > MAX_ALLOC {
                return Err(format!(
                    "`vec/repeat` count too large: {n} (max {MAX_ALLOC})"
                ));
            }
            Ok(Value::Vec(Rc::new(vec![v.clone(); *n as usize])))
        }
        _ => Err(format!(
            "`vec/repeat` requires 2 arguments (Int n, value), got {}",
            args.len()
        )),
    }
}

/// `(vec/init n f)` — build a Vec of length `n` by calling `(f i)` for each index `i`.
fn init(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Int(n), f] => {
            if *n < 0 {
                return Err(format!("`vec/init` count must be >= 0, got {n}"));
            }
            if *n > MAX_ALLOC {
                return Err(format!(
                    "`vec/init` count too large: {n} (max {MAX_ALLOC})"
                ));
            }
            let mut result = Vec::with_capacity(*n as usize);
            for i in 0..*n {
                result.push(call_value(f, &[Value::Int(i)])?);
            }
            Ok(Value::Vec(Rc::new(result)))
        }
        _ => Err(format!(
            "`vec/init` requires 2 arguments (Int n, Fn f), got {}",
            args.len()
        )),
    }
}

/// `(vec/chunk xs n)` — split `xs` into consecutive chunks of size `n` (last may be shorter).
fn chunk(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v), Value::Int(n)] => {
            if *n <= 0 {
                return Err(format!("`vec/chunk` size must be > 0, got {n}"));
            }
            let size = *n as usize;
            let chunks: Vec<Value> = v
                .chunks(size)
                .map(|c| Value::Vec(Rc::new(c.to_vec())))
                .collect();
            Ok(Value::Vec(Rc::new(chunks)))
        }
        [other, Value::Int(_)] => Err(format!(
            "`vec/chunk` requires (Vec, Int), got ({}, Int)",
            other.type_name()
        )),
        _ => Err(format!(
            "`vec/chunk` requires (Vec, Int), got {} args",
            args.len()
        )),
    }
}

/// `(vec/window xs n)` — return all sliding windows of size `n`.
fn window(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v), Value::Int(n)] => {
            if *n <= 0 {
                return Err(format!("`vec/window` size must be > 0, got {n}"));
            }
            let size = *n as usize;
            let windows: Vec<Value> = v
                .windows(size)
                .map(|w| Value::Vec(Rc::new(w.to_vec())))
                .collect();
            Ok(Value::Vec(Rc::new(windows)))
        }
        [other, Value::Int(_)] => Err(format!(
            "`vec/window` requires (Vec, Int), got ({}, Int)",
            other.type_name()
        )),
        _ => Err(format!(
            "`vec/window` requires (Vec, Int), got {} args",
            args.len()
        )),
    }
}

/// `(vec/split-at xs n)` — split `xs` at index `n`, returning `[[before] [after]]`.
fn split_at(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v), Value::Int(n)] => {
            let len = v.len() as i64;
            let idx = (*n).clamp(0, len) as usize;
            let left = Value::Vec(Rc::new(v[..idx].to_vec()));
            let right = Value::Vec(Rc::new(v[idx..].to_vec()));
            Ok(Value::Vec(Rc::new(vec![left, right])))
        }
        _ => Err(format!(
            "`vec/split-at` requires (Vec, Int), got {} args",
            args.len()
        )),
    }
}

/// `(vec/span xs pred)` — split `xs` at the first element where `(pred x)` is false.
///
/// Returns `[[all-true-prefix] [rest]]`.
fn span(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v), f] => {
            let mut split_idx = v.len();
            for (i, item) in v.iter().enumerate() {
                match call_value(f, std::slice::from_ref(item))? {
                    Value::Bool(true) => {}
                    Value::Bool(false) => {
                        split_idx = i;
                        break;
                    }
                    other => {
                        return Err(format!(
                            "`vec/span` predicate must return Bool, got {}",
                            other.type_name()
                        ))
                    }
                }
            }
            let left = Value::Vec(Rc::new(v[..split_idx].to_vec()));
            let right = Value::Vec(Rc::new(v[split_idx..].to_vec()));
            Ok(Value::Vec(Rc::new(vec![left, right])))
        }
        _ => Err(format!(
            "`vec/span` requires (Vec, Fn), got {} args",
            args.len()
        )),
    }
}

/// `(vec/insert xs i val)` — return a new Vec with `val` inserted at index `i`.
fn insert(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v), Value::Int(n), val] => {
            let len = v.len() as i64;
            if *n < 0 || *n > len {
                return Err(format!(
                    "`vec/insert` index {n} out of bounds for length {len}"
                ));
            }
            let mut result = v.to_vec();
            result.insert(*n as usize, val.clone());
            Ok(Value::Vec(Rc::new(result)))
        }
        _ => Err(format!(
            "`vec/insert` requires (Vec, Int, value), got {} args",
            args.len()
        )),
    }
}

/// `(vec/remove-at xs i)` — return a new Vec with the element at index `i` removed.
fn remove_at(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v), Value::Int(n)] => {
            let len = v.len() as i64;
            if *n < 0 || *n >= len {
                return Err(format!(
                    "`vec/remove-at` index {n} out of bounds for length {len}"
                ));
            }
            let mut result = v.to_vec();
            result.remove(*n as usize);
            Ok(Value::Vec(Rc::new(result)))
        }
        _ => Err(format!(
            "`vec/remove-at` requires (Vec, Int), got {} args",
            args.len()
        )),
    }
}

/// `(vec/swap xs i j)` — return a new Vec with elements at indices `i` and `j` swapped.
fn swap(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v), Value::Int(i), Value::Int(j)] => {
            let len = v.len() as i64;
            if *i < 0 || *i >= len {
                return Err(format!(
                    "`vec/swap` index {i} out of bounds for length {len}"
                ));
            }
            if *j < 0 || *j >= len {
                return Err(format!(
                    "`vec/swap` index {j} out of bounds for length {len}"
                ));
            }
            let mut result = v.to_vec();
            result.swap(*i as usize, *j as usize);
            Ok(Value::Vec(Rc::new(result)))
        }
        _ => Err(format!(
            "`vec/swap` requires (Vec, Int, Int), got {} args",
            args.len()
        )),
    }
}

/// `(vec/rotate-left xs n)` — rotate elements left by `n` positions.
fn rotate_left(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v), Value::Int(n)] => {
            if v.is_empty() {
                return Ok(Value::Vec(Rc::new(vec![])));
            }
            let len = v.len();
            let shift = n.rem_euclid(len as i64) as usize;
            let mut result = v.to_vec();
            result.rotate_left(shift);
            Ok(Value::Vec(Rc::new(result)))
        }
        _ => Err(format!(
            "`vec/rotate-left` requires (Vec, Int), got {} args",
            args.len()
        )),
    }
}

/// `(vec/rotate-right xs n)` — rotate elements right by `n` positions.
fn rotate_right(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v), Value::Int(n)] => {
            if v.is_empty() {
                return Ok(Value::Vec(Rc::new(vec![])));
            }
            let len = v.len();
            let shift = n.rem_euclid(len as i64) as usize;
            let mut result = v.to_vec();
            result.rotate_right(shift);
            Ok(Value::Vec(Rc::new(result)))
        }
        _ => Err(format!(
            "`vec/rotate-right` requires (Vec, Int), got {} args",
            args.len()
        )),
    }
}

/// `(vec/permutations xs)` — return all orderings of `xs` as a Vec of Vecs.
///
/// O(n!) — input is limited to 10 elements to prevent memory exhaustion.
fn permutations(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v)] => {
            if v.len() > 10 {
                return Err(format!(
                    "`vec/permutations` input too large: {} elements (max 10, since 11! = 39,916,800 results)",
                    v.len()
                ));
            }
            let perms = gen_permutations(v);
            Ok(Value::Vec(Rc::new(
                perms.into_iter().map(|p| Value::Vec(Rc::new(p))).collect(),
            )))
        }
        _ => Err(format!(
            "`vec/permutations` requires 1 argument (Vec), got {}",
            args.len()
        )),
    }
}

fn gen_permutations(v: &[Value]) -> Vec<Vec<Value>> {
    if v.is_empty() {
        return vec![vec![]];
    }
    let mut result = Vec::new();
    for (i, item) in v.iter().enumerate() {
        let mut rest = v.to_vec();
        rest.remove(i);
        for mut perm in gen_permutations(&rest) {
            perm.insert(0, item.clone());
            result.push(perm);
        }
    }
    result
}

/// `(vec/combinations xs k)` — return all ways to choose `k` elements from `xs`.
///
/// O(C(n,k)) — elements preserve original order. n is limited to 20 to prevent
/// memory exhaustion (C(20,10) = 184,756; C(21,10) = 352,716 — safe upper bound).
fn combinations(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v), Value::Int(k)] => {
            if *k < 0 {
                return Err(format!("`vec/combinations` k must be >= 0, got {k}"));
            }
            if v.len() > 20 {
                return Err(format!(
                    "`vec/combinations` input too large: {} elements (max 20)",
                    v.len()
                ));
            }
            let combs = gen_combinations(v, *k as usize);
            Ok(Value::Vec(Rc::new(
                combs.into_iter().map(|c| Value::Vec(Rc::new(c))).collect(),
            )))
        }
        _ => Err(format!(
            "`vec/combinations` requires (Vec, Int), got {} args",
            args.len()
        )),
    }
}

fn gen_combinations(v: &[Value], k: usize) -> Vec<Vec<Value>> {
    if k == 0 {
        return vec![vec![]];
    }
    if k > v.len() {
        return vec![];
    }
    let mut result = Vec::new();
    for (i, item) in v.iter().enumerate() {
        for mut comb in gen_combinations(&v[i + 1..], k - 1) {
            comb.insert(0, item.clone());
            result.push(comb);
        }
    }
    result
}

/// `(vec/binary-search xs target)` — search a sorted Vec.
///
/// Returns `(Ok index)` if `target` is found at `index`, or
/// `(Err insertion-point)` if not found (the index where target would be inserted).
fn binary_search(args: &[Value]) -> Result<Value, String> {
    match args {
        [Value::Vec(v), target] => {
            let mut low = 0usize;
            let mut high = v.len();
            while low < high {
                let mid = low + (high - low) / 2;
                match compare_ord(&v[mid], target)? {
                    std::cmp::Ordering::Equal => {
                        return Ok(Value::Adt {
                            type_name: Rc::from("Result"),
                            ctor: Rc::from("Ok"),
                            fields: Rc::new(vec![Value::Int(mid as i64)]),
                        });
                    }
                    std::cmp::Ordering::Less => low = mid + 1,
                    std::cmp::Ordering::Greater => high = mid,
                }
            }
            Ok(Value::Adt {
                type_name: Rc::from("Result"),
                ctor: Rc::from("Err"),
                fields: Rc::new(vec![Value::Int(low as i64)]),
            })
        }
        _ => Err(format!(
            "`vec/binary-search` requires (Vec, value), got {} args",
            args.len()
        )),
    }
}

fn compare_ord(a: &Value, b: &Value) -> Result<std::cmp::Ordering, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => {
            Ok(x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))
        }
        (Value::Str(x), Value::Str(y)) => Ok(x.as_ref().cmp(y.as_ref())),
        _ => Err(format!(
            "`vec/binary-search` elements must be comparable (Int, Float, or Str), got {}",
            a.type_name()
        )),
    }
}

fn call_value(callee: &Value, args: &[Value]) -> Result<Value, String> {
    nexl_runtime::call_value(callee, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_of_builds_vec() {
        let result = of(&[Value::Int(1), Value::Int(2), Value::Int(3)]).unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]))
        );
    }

    #[test]
    fn test_of_empty() {
        let result = of(&[]).unwrap();
        assert_eq!(result, Value::Vec(Rc::new(vec![])));
    }

    #[test]
    fn test_repeat_basic() {
        let result = repeat(&[Value::Int(3), Value::Int(0)]).unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![Value::Int(0), Value::Int(0), Value::Int(0)]))
        );
    }

    #[test]
    fn test_repeat_zero() {
        let result = repeat(&[Value::Int(0), Value::Int(42)]).unwrap();
        assert_eq!(result, Value::Vec(Rc::new(vec![])));
    }

    #[test]
    fn test_repeat_negative_err() {
        assert!(repeat(&[Value::Int(-1), Value::Int(0)]).is_err());
    }

    #[test]
    fn test_chunk_basic() {
        let v = Value::Vec(Rc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
            Value::Int(5),
        ]));
        let result = chunk(&[v, Value::Int(2)]).unwrap();
        let Value::Vec(chunks) = result else {
            panic!("expected Vec")
        };
        assert_eq!(chunks.len(), 3);
        assert_eq!(
            chunks[0],
            Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2)]))
        );
        assert_eq!(
            chunks[2],
            Value::Vec(Rc::new(vec![Value::Int(5)]))
        );
    }

    #[test]
    fn test_window_basic() {
        let v = Value::Vec(Rc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]));
        let result = window(&[v, Value::Int(3)]).unwrap();
        let Value::Vec(wins) = result else {
            panic!("expected Vec")
        };
        assert_eq!(wins.len(), 2);
        assert_eq!(
            wins[0],
            Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]))
        );
        assert_eq!(
            wins[1],
            Value::Vec(Rc::new(vec![Value::Int(2), Value::Int(3), Value::Int(4)]))
        );
    }

    #[test]
    fn test_split_at_basic() {
        let v = Value::Vec(Rc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]));
        let result = split_at(&[v, Value::Int(2)]).unwrap();
        let Value::Vec(parts) = result else {
            panic!("expected Vec")
        };
        assert_eq!(
            parts[0],
            Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2)]))
        );
        assert_eq!(
            parts[1],
            Value::Vec(Rc::new(vec![Value::Int(3), Value::Int(4)]))
        );
    }

    #[test]
    fn test_insert_basic() {
        let v = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        let result = insert(&[v, Value::Int(1), Value::Int(99)]).unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![
                Value::Int(1),
                Value::Int(99),
                Value::Int(2),
                Value::Int(3)
            ]))
        );
    }

    #[test]
    fn test_remove_at_basic() {
        let v = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        let result = remove_at(&[v, Value::Int(1)]).unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(3)]))
        );
    }

    #[test]
    fn test_swap_basic() {
        let v = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        let result = swap(&[v, Value::Int(0), Value::Int(2)]).unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![Value::Int(3), Value::Int(2), Value::Int(1)]))
        );
    }

    #[test]
    fn test_rotate_left() {
        let v = Value::Vec(Rc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]));
        let result = rotate_left(&[v, Value::Int(1)]).unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![
                Value::Int(2),
                Value::Int(3),
                Value::Int(4),
                Value::Int(1)
            ]))
        );
    }

    #[test]
    fn test_rotate_right() {
        let v = Value::Vec(Rc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]));
        let result = rotate_right(&[v, Value::Int(1)]).unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![
                Value::Int(4),
                Value::Int(1),
                Value::Int(2),
                Value::Int(3)
            ]))
        );
    }

    #[test]
    fn test_permutations_two_elements() {
        let v = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2)]));
        let result = permutations(&[v]).unwrap();
        let Value::Vec(perms) = result else {
            panic!("expected Vec")
        };
        assert_eq!(perms.len(), 2);
    }

    #[test]
    fn test_combinations_choose_two() {
        let v = Value::Vec(Rc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ]));
        let result = combinations(&[v, Value::Int(2)]).unwrap();
        let Value::Vec(combs) = result else {
            panic!("expected Vec")
        };
        assert_eq!(combs.len(), 3); // C(3,2) = 3
    }

    #[test]
    fn test_binary_search_found() {
        let v = Value::Vec(Rc::new(vec![
            Value::Int(1),
            Value::Int(3),
            Value::Int(5),
            Value::Int(7),
        ]));
        let result = binary_search(&[v, Value::Int(5)]).unwrap();
        assert_eq!(
            result,
            Value::Adt {
                type_name: Rc::from("Result"),
                ctor: Rc::from("Ok"),
                fields: Rc::new(vec![Value::Int(2)]),
            }
        );
    }

    #[test]
    fn test_binary_search_not_found() {
        let v = Value::Vec(Rc::new(vec![
            Value::Int(1),
            Value::Int(3),
            Value::Int(5),
            Value::Int(7),
        ]));
        let result = binary_search(&[v, Value::Int(4)]).unwrap();
        assert_eq!(
            result,
            Value::Adt {
                type_name: Rc::from("Result"),
                ctor: Rc::from("Err"),
                fields: Rc::new(vec![Value::Int(2)]),
            }
        );
    }
}
