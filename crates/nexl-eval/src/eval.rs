use std::rc::Rc;

use meta::{
    Atom, HandledEffect, HandledOp, Node, NodeKind, Pattern, TryCatchForm,
    parse_defhandler_decl, parse_handle_form, parse_pattern, parse_try_form,
};
use nexl_runtime::{BuiltHandlerEffect, Value, value::Function, value::HandlerDef};

use crate::{Env, EvalError};

#[derive(Debug)]
enum EvalReturn {
    Value(Value),
    Recur(Vec<Value>),
}

#[derive(Debug, Clone, Copy)]
enum ForControl {
    Continue,
    Break,
}

struct LoopFrame<'a> {
    names: &'a [Rc<str>],
}

/// Evaluate a Nexl AST node within the given environment.
pub fn eval(node: &Node, env: &Rc<Env>) -> Result<Value, EvalError> {
    match eval_with_loop(node, env, None)? {
        EvalReturn::Value(v) => Ok(v),
        EvalReturn::Recur(_) => Err(EvalError::InvalidRecur),
    }
}

fn eval_with_loop<'a>(
    node: &Node,
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    match &node.kind {
        NodeKind::Atom(atom) => eval_atom(atom, env),
        NodeKind::List(items) => eval_list(items, env, loop_state),
        NodeKind::Vector(items) => eval_vector(items, env, loop_state),
        NodeKind::Map(entries) => eval_map(entries, env, loop_state),
        NodeKind::Set(items) => eval_set(items, env, loop_state),
        // #_ discarded forms are not evaluated.
        NodeKind::Discard(_) => Ok(EvalReturn::Value(Value::Unit)),
        _ => todo!("non-atom evaluation not yet implemented"),
    }
}

fn eval_atom(atom: &Atom, env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    let v = match atom {
        Atom::Int { value, .. } => Value::Int(*value as i64),
        Atom::Float { value, .. } => Value::Float(*value),
        Atom::Ratio { numer, denom } => Value::Ratio(*numer, *denom),
        Atom::Bool(b) => Value::Bool(*b),
        Atom::Char(c) => Value::Char(*c),
        Atom::Str(s) => Value::Str(Rc::from(s.as_str())),
        Atom::Unit => Value::Unit,
        Atom::Keyword { ns, name } => Value::Keyword {
            ns: ns.as_ref().map(|s| Rc::from(s.as_str())),
            name: Rc::from(name.as_str()),
        },
        Atom::Symbol { ns: None, name } => env
            .get(name)
            .ok_or_else(|| EvalError::UnboundSymbol(name.clone()))?,
        Atom::Symbol {
            ns: Some(alias),
            name,
        } => env
            .get_qualified(alias, name)
            .ok_or_else(|| {
                // If the module alias doesn't exist at all, it might be an unhandled effect
                if !env.has_module_alias(alias) {
                    EvalError::NativeError(format!(
                        "unhandled effect: `{alias}/{name}` — \
                         no `{alias}` handler is installed. \
                         Use `(handle [{alias}Handler] ...)` to provide one."
                    ))
                } else {
                    EvalError::UnboundSymbol(format!("{alias}/{name}"))
                }
            })?,
    };
    Ok(EvalReturn::Value(v))
}

fn eval_list<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.is_empty() {
        return Err(EvalError::Arity);
    }
    let head = &items[0];
    match &head.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "def" => eval_def(items, env),
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "let" => {
            eval_let(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "do" => {
            eval_do(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "if" => {
            eval_if(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "fn" => eval_fn(items, env),
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "defn" => eval_defn(items, env),
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "loop" => eval_loop(items, env),
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "recur" => {
            eval_recur(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "for" => eval_for(items, env),
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "for!" => eval_for(items, env),
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "each" => eval_each(items, env),
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "times" => {
            eval_times(items, env)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "set!" => {
            eval_set_bang(items, env)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "panic" => {
            eval_panic(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "assert!" => {
            eval_assert(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "assert-unreachable!" => {
            eval_assert_unreachable(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "?" => {
            eval_question(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "try" => {
            eval_try(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "match" => {
            eval_match(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "deftype" => {
            eval_deftype(items, env)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "defhandler" => {
            eval_defhandler(items, env)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "handle" => {
            eval_handle(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "and" => {
            eval_and(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "or" => {
            eval_or(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "cond" => {
            eval_cond(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "is" => {
            eval_is(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "deftest" => {
            eval_deftest(items, env)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "describe" => {
            eval_describe(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "throws?" => {
            eval_throws_q(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "is-match" => {
            eval_is_match(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name })
            if matches!(name.as_str(), "setup" | "teardown" | "setup-all" | "teardown-all") =>
        {
            eval_lifecycle_hook(name, items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "call-log" => {
            eval_call_log(items, env)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "submodule" => {
            eval_submodule(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "check" => {
            eval_check(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "bench" => {
            eval_bench(items, env, loop_state)
        }
        _ => eval_apply(items, env, loop_state),
    }
}

fn eval_vector<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        let value = match eval_with_loop(item, env, loop_state)? {
            EvalReturn::Value(v) => v,
            EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
        };
        values.push(value);
    }
    Ok(EvalReturn::Value(Value::Vec(Rc::new(values))))
}

fn eval_map<'a>(
    entries: &[(Node, Node)],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    let mut values = Vec::with_capacity(entries.len());
    for (key_node, value_node) in entries {
        let key = match eval_with_loop(key_node, env, loop_state)? {
            EvalReturn::Value(v) => v,
            EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
        };
        let value = match eval_with_loop(value_node, env, loop_state)? {
            EvalReturn::Value(v) => v,
            EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
        };
        values.push((key, value));
    }
    Ok(EvalReturn::Value(Value::Map(Rc::new(values.into()))))
}

fn eval_set<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        let value = match eval_with_loop(item, env, loop_state)? {
            EvalReturn::Value(v) => v,
            EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
        };
        values.push(value);
    }
    Ok(EvalReturn::Value(Value::Set(Rc::new(values))))
}

fn eval_def(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    if items.len() != 3 {
        return Err(EvalError::Arity);
    }
    let name_node = &items[1];
    let name = match &name_node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => return Err(EvalError::InvalidBindingTarget),
    };

    let value = eval(&items[2], env)?;
    env.define(name, value);
    Ok(EvalReturn::Value(Value::Unit))
}

/// Returns `true` if `node` is the `|` pipe symbol used in `let-else` bindings.
fn is_pipe_node(node: &Node) -> bool {
    matches!(
        &node.kind,
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if &**name == "|"
    )
}

/// Evaluate a `let` or `let-else` form (spec §4.11–§4.12).
///
/// Plain binding:    `[name    expr  ...]`
/// Pattern binding:  `[pattern expr  ...]`          — irrefutable (error on mismatch)
/// let-else binding: `[pattern expr | fallback ...]` — on mismatch, evaluates fallback
fn eval_let<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() < 3 {
        return Err(EvalError::Arity);
    }

    let bindings_node = &items[1];
    let bindings = match &bindings_node.kind {
        NodeKind::Vector(items) => items,
        _ => return Err(EvalError::Arity),
    };

    let child_env = Rc::new(Env::child(Rc::clone(env)));

    // Parse bindings: [mut? pattern expr [| fallback] ...]
    let mut i = 0;
    while i < bindings.len() {
        // Consume optional `mut` modifier (noted but not enforced).
        if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &bindings[i].kind
            && name == "mut"
        {
            i += 1;
        }

        // Pattern node — a plain symbol becomes Pattern::Var, compound patterns
        // like (Ok n) or (Some x) are supported via parse_pattern.
        let pat_node = bindings.get(i).ok_or(EvalError::Arity)?;
        i += 1;

        // Value expression.
        let val_node = bindings.get(i).ok_or(EvalError::Arity)?;
        i += 1;

        // Optional `| fallback` clause (let-else, spec §4.12).
        let fallback_node = if bindings.get(i).is_some_and(is_pipe_node) {
            i += 1; // consume `|`
            let fb = bindings.get(i).ok_or(EvalError::Arity)?;
            i += 1;
            Some(fb)
        } else {
            None
        };

        // Evaluate the value expression.
        let value = match eval_with_loop(val_node, &child_env, None)? {
            EvalReturn::Value(v) => v,
            EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
        };

        // Match the pattern against the value.
        let pattern = parse_pattern(pat_node)
            .map_err(|e| EvalError::NativeError(format!("let: {e}")))?;

        let mut binding_pairs: Vec<(Rc<str>, Value)> = Vec::new();
        if match_pattern(&pattern, &value, &mut binding_pairs) {
            for (name, val) in binding_pairs {
                child_env.define(name, val);
            }
        } else {
            // Pattern failed — evaluate fallback (let-else) or error.
            return match fallback_node {
                Some(fb) => eval_with_loop(fb, &child_env, loop_state),
                None => Err(EvalError::NativeError(format!(
                    "let: pattern did not match value {value}",
                ))),
            };
        }
    }

    // Body expressions — propagate loop_state so recur works inside let.
    let mut last = Value::Unit;
    for expr in &items[2..] {
        match eval_with_loop(expr, &child_env, loop_state)? {
            EvalReturn::Value(v) => last = v,
            recur @ EvalReturn::Recur(_) => return Ok(recur),
        }
    }
    Ok(EvalReturn::Value(last))
}

fn eval_do<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() < 2 {
        return Err(EvalError::Arity);
    }

    let mut last = Value::Unit;
    for expr in &items[1..] {
        match eval_with_loop(expr, env, loop_state)? {
            EvalReturn::Value(v) => last = v,
            recur @ EvalReturn::Recur(_) => return Ok(recur),
        }
    }
    Ok(EvalReturn::Value(last))
}

fn eval_if<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() != 4 {
        return Err(EvalError::Arity);
    }

    let cond = match eval_with_loop(&items[1], env, loop_state)? {
        EvalReturn::Value(v) => v,
        EvalReturn::Recur(values) => return Ok(EvalReturn::Recur(values)),
    };
    let cond_bool = match cond {
        Value::Bool(b) => b,
        _ => return Err(EvalError::InvalidConditionType),
    };

    if cond_bool {
        eval_with_loop(&items[2], env, loop_state)
    } else {
        eval_with_loop(&items[3], env, loop_state)
    }
}

/// `(match expr pattern1 body1 pattern2 body2 ...)`
/// Pattern matching on values. Tries each pattern in order.
fn eval_match<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    // (match expr pat1 body1 pat2 body2 ...)
    if items.len() < 4 || !(items.len() - 2).is_multiple_of(2) {
        return Err(EvalError::Arity);
    }

    let scrutinee = match eval_with_loop(&items[1], env, loop_state)? {
        EvalReturn::Value(v) => v,
        recur @ EvalReturn::Recur(_) => return Ok(recur),
    };

    let arms = &items[2..];
    for pair in arms.chunks_exact(2) {
        let (pat_node, body_node) = (&pair[0], &pair[1]);
        let pattern =
            parse_pattern(pat_node).map_err(|e| EvalError::NativeError(format!("match: {e}")))?;

        let mut bindings: Vec<(Rc<str>, Value)> = Vec::new();
        if match_pattern(&pattern, &scrutinee, &mut bindings) {
            let arm_env = Rc::new(Env::child(Rc::clone(env)));
            for (name, value) in bindings {
                arm_env.define(name, value);
            }
            return eval_with_loop(body_node, &arm_env, loop_state);
        }
    }

    Err(EvalError::NativeError(format!(
        "match: no pattern matched value {scrutinee}"
    )))
}

/// Try to match a pattern against a value, collecting bindings on success.
fn match_pattern(pattern: &Pattern, value: &Value, bindings: &mut Vec<(Rc<str>, Value)>) -> bool {
    match pattern {
        Pattern::Wildcard => true,

        Pattern::Var(name) => {
            bindings.push((Rc::from(name.as_str()), value.clone()));
            true
        }

        Pattern::Literal(atom) => match_literal(atom, value),

        Pattern::Constructor { name, args } => match value {
            Value::Adt { ctor, fields, .. } => {
                if ctor.as_ref() != name.as_str() {
                    return false;
                }
                if fields.len() != args.len() {
                    return false;
                }
                for (sub_pat, field_val) in args.iter().zip(fields.iter()) {
                    if !match_pattern(sub_pat, field_val, bindings) {
                        return false;
                    }
                }
                true
            }
            _ => false,
        },

        Pattern::Record { fields } => match value {
            Value::Map(entries) => {
                for (field_name, sub_pat) in fields {
                    let key = Value::Keyword {
                        ns: None,
                        name: Rc::from(field_name.as_str()),
                    };
                    match entries.get(&key) {
                        Some(val) => {
                            if !match_pattern(sub_pat, val, bindings) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }
                true
            }
            _ => false,
        },

        Pattern::Tuple(pats) => match value {
            Value::Vec(items) => {
                if items.len() != pats.len() {
                    return false;
                }
                for (sub_pat, item) in pats.iter().zip(items.iter()) {
                    if !match_pattern(sub_pat, item, bindings) {
                        return false;
                    }
                }
                true
            }
            _ => false,
        },

        Pattern::Or(alternatives) => {
            for alt in alternatives {
                let mut alt_bindings = Vec::new();
                if match_pattern(alt, value, &mut alt_bindings) {
                    bindings.extend(alt_bindings);
                    return true;
                }
            }
            false
        }

        Pattern::As {
            pattern: inner,
            name,
        } => {
            if match_pattern(inner, value, bindings) {
                bindings.push((Rc::from(name.as_str()), value.clone()));
                true
            } else {
                false
            }
        }
    }
}

/// Match a literal pattern atom against a runtime value.
fn match_literal(atom: &Atom, value: &Value) -> bool {
    match (atom, value) {
        (Atom::Int { value: n, .. }, Value::Int(v)) => *n as i64 == *v,
        (Atom::Float { value: n, .. }, Value::Float(v)) => *n == *v,
        (Atom::Bool(b), Value::Bool(v)) => b == v,
        (Atom::Str(s), Value::Str(v)) => s.as_str() == v.as_ref(),
        (Atom::Char(c), Value::Char(v)) => c == v,
        (Atom::Unit, Value::Unit) => true,
        (
            Atom::Keyword {
                ns: kns,
                name: kname,
            },
            Value::Keyword {
                ns: vns,
                name: vname,
            },
        ) => kns.as_deref() == vns.as_deref() && kname.as_str() == vname.as_ref(),
        _ => false,
    }
}

/// `(and e1 e2 ...)` — short-circuit boolean AND.
/// Evaluates left to right; stops at the first `false` without evaluating the rest.
fn eval_and<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() < 2 {
        return Err(EvalError::Arity);
    }
    for expr in &items[1..] {
        let val = match eval_with_loop(expr, env, loop_state)? {
            EvalReturn::Value(v) => v,
            recur @ EvalReturn::Recur(_) => return Ok(recur),
        };
        match val {
            Value::Bool(false) => return Ok(EvalReturn::Value(Value::Bool(false))),
            Value::Bool(true) => {}
            _ => return Err(EvalError::InvalidConditionType),
        }
    }
    Ok(EvalReturn::Value(Value::Bool(true)))
}

/// `(or e1 e2 ...)` — short-circuit boolean OR.
/// Evaluates left to right; stops at the first `true` without evaluating the rest.
fn eval_or<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() < 2 {
        return Err(EvalError::Arity);
    }
    for expr in &items[1..] {
        let val = match eval_with_loop(expr, env, loop_state)? {
            EvalReturn::Value(v) => v,
            recur @ EvalReturn::Recur(_) => return Ok(recur),
        };
        match val {
            Value::Bool(true) => return Ok(EvalReturn::Value(Value::Bool(true))),
            Value::Bool(false) => {}
            _ => return Err(EvalError::InvalidConditionType),
        }
    }
    Ok(EvalReturn::Value(Value::Bool(false)))
}

/// `(cond test1 expr1 test2 expr2 ... :else default)`
fn eval_cond<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    // Must have at least (cond test expr)
    let clauses = &items[1..];
    if clauses.is_empty() || !clauses.len().is_multiple_of(2) {
        return Err(EvalError::Arity);
    }

    for pair in clauses.chunks_exact(2) {
        let (test_node, body_node) = (&pair[0], &pair[1]);

        // Check for :else keyword — always matches
        if let NodeKind::Atom(Atom::Keyword { ns: None, name }) = &test_node.kind
            && name == "else"
        {
            return eval_with_loop(body_node, env, loop_state);
        }

        let test_val = match eval_with_loop(test_node, env, loop_state)? {
            EvalReturn::Value(v) => v,
            recur @ EvalReturn::Recur(_) => return Ok(recur),
        };
        match test_val {
            Value::Bool(true) => return eval_with_loop(body_node, env, loop_state),
            Value::Bool(false) => {}
            _ => return Err(EvalError::InvalidConditionType),
        }
    }

    // No clause matched — return Unit
    Ok(EvalReturn::Value(Value::Unit))
}

/// `(is expr)` or `(is expr "message")` — power-assert assertion.
///
/// Evaluates `expr`. On success (Bool true), returns Unit. On failure, produces
/// Generate a diff hint for two values when they differ.
///
/// Returns an extra annotation string (prefixed with `\n`) for string, Vec, and Map comparisons,
/// showing where the values diverge to aid debugging.
fn diff_hint(lhs: &Value, rhs: &Value) -> String {
    match (lhs, rhs) {
        (Value::Str(a), Value::Str(b)) => {
            // Find first differing character position
            let a_chars: Vec<char> = a.chars().collect();
            let b_chars: Vec<char> = b.chars().collect();
            let pos = a_chars.iter().zip(b_chars.iter()).position(|(x, y)| x != y)
                .unwrap_or_else(|| a_chars.len().min(b_chars.len()));
            if a.len() != b.len() || pos < a_chars.len() {
                format!("\n  diff: strings differ at char {pos}")
            } else {
                String::new()
            }
        }
        (Value::Vec(a), Value::Vec(b)) => {
            if a.len() != b.len() {
                format!("\n  diff: vec lengths differ ({} vs {})", a.len(), b.len())
            } else {
                let pos = a.iter().zip(b.iter()).position(|(x, y)| x != y);
                match pos {
                    Some(i) => format!("\n  diff: element at index {i} differs ({} vs {})", a[i], b[i]),
                    None => String::new(),
                }
            }
        }
        (Value::Map(a), Value::Map(b)) => {
            // Find keys present in one but not the other, or with different values
            let mut diffs: Vec<String> = Vec::new();
            for (k, av) in a.iter() {
                match b.get(k) {
                    None => diffs.push(format!("  key {} only in left", k)),
                    Some(bv) if av != bv => diffs.push(format!("  key {}: {} vs {}", k, av, bv)),
                    _ => {}
                }
            }
            for (k, _) in b.iter() {
                if a.get(k).is_none() {
                    diffs.push(format!("  key {} only in right", k));
                }
            }
            if diffs.is_empty() {
                String::new()
            } else {
                format!("\n  diff:\n{}", diffs.join("\n"))
            }
        }
        _ => String::new(),
    }
}

/// a rich error message by analyzing the expression's AST.
///
/// Recognized forms:
/// - `(= a b)`, `(not= a b)`, `(< a b)`, `(> a b)`, `(<= a b)`, `(>= a b)` →
///   evaluates both sides and reports them as `left` / `right`
/// - `(pred x)` — single-arg predicate → reports predicate name and value
/// - Any other form → reports the expression text and boolean result
fn eval_is<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    // items[0] = "is", items[1] = expr, items[2] = optional message
    let expr = items.get(1).ok_or(EvalError::Arity)?;
    let user_msg: Option<String> = items.get(2).and_then(|n| {
        if let NodeKind::Atom(Atom::Str(s)) = &n.kind {
            Some(s.clone())
        } else {
            None
        }
    });

    let expr_text = format!("{expr}");
    let prefix = match &user_msg {
        Some(msg) => format!("FAIL: {msg}\n  (is {expr_text})"),
        None => format!("assertion failed: (is {expr_text})"),
    };

    // Check for binary comparison forms: (op lhs rhs)
    if let NodeKind::List(inner) = &expr.kind
        && let Some(Atom::Symbol { ns: None, name: op_name }) = inner.first().and_then(|n| match &n.kind {
            NodeKind::Atom(a) => Some(a),
            _ => None,
        })
    {
            match op_name.as_str() {
                "=" | "not=" | "<" | ">" | "<=" | ">=" if inner.len() == 3 => {
                    let lhs_node = &inner[1];
                    let rhs_node = &inner[2];
                    let lhs_text = format!("{lhs_node}");
                    let rhs_text = format!("{rhs_node}");

                    let lhs = match eval_with_loop(lhs_node, env, loop_state)? {
                        EvalReturn::Value(v) => v,
                        r @ EvalReturn::Recur(_) => return Ok(r),
                    };
                    let rhs = match eval_with_loop(rhs_node, env, loop_state)? {
                        EvalReturn::Value(v) => v,
                        r @ EvalReturn::Recur(_) => return Ok(r),
                    };

                    // Evaluate the full expression with the captured values
                    let result = match eval_with_loop(expr, env, loop_state)? {
                        EvalReturn::Value(Value::Bool(b)) => b,
                        EvalReturn::Value(_) => {
                            return Err(EvalError::NativeError(format!(
                                "{prefix}\n  (is) expression must return Bool"
                            )));
                        }
                        r @ EvalReturn::Recur(_) => return Ok(r),
                    };

                    if result {
                        return Ok(EvalReturn::Value(Value::Unit));
                    }

                    let diff_info = diff_hint(&lhs, &rhs);
                    return Err(EvalError::NativeError(format!(
                        "{prefix}\n  {lhs_text}: {lhs}\n  {rhs_text}: {rhs}{diff_info}"
                    )));
                }
                // 1-arg predicate: (pred val)
                _ if inner.len() == 2 => {
                    let val_node = &inner[1];
                    let val_text = format!("{val_node}");
                    let val = match eval_with_loop(val_node, env, loop_state)? {
                        EvalReturn::Value(v) => v,
                        r @ EvalReturn::Recur(_) => return Ok(r),
                    };

                    let result = match eval_with_loop(expr, env, loop_state)? {
                        EvalReturn::Value(Value::Bool(b)) => b,
                        EvalReturn::Value(_) => {
                            return Err(EvalError::NativeError(format!(
                                "{prefix}\n  (is) expression must return Bool"
                            )));
                        }
                        r @ EvalReturn::Recur(_) => return Ok(r),
                    };

                    if result {
                        return Ok(EvalReturn::Value(Value::Unit));
                    }

                    return Err(EvalError::NativeError(format!(
                        "{prefix}\n  {val_text}: {val}  (expected {op_name} to be true)"
                    )));
                }
                _ => {}
            }
    }

    // Generic fallback
    let val = match eval_with_loop(expr, env, loop_state)? {
        EvalReturn::Value(v) => v,
        r @ EvalReturn::Recur(_) => return Ok(r),
    };
    match val {
        Value::Bool(true) => Ok(EvalReturn::Value(Value::Unit)),
        Value::Bool(false) => Err(EvalError::NativeError(prefix)),
        _ => Err(EvalError::NativeError(format!(
            "{prefix}\n  (is) expression must return Bool, got {val}"
        ))),
    }
}

/// `(deftest "name" body...)` — register a test with the test runner.
///
/// Expands to `(test/register! "name" (fn [] body...))` at runtime, respecting
/// `:skip`, `:focus`, `:tags`, `:timeout`, and `:flaky` keyword metadata (spec §6.1–6.2).
fn eval_deftest(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    // items[0] = "deftest", items[1] = name string, items[2..] = metadata + body
    let name_node = items.get(1).ok_or(EvalError::Arity)?;
    let name = match &name_node.kind {
        NodeKind::Atom(Atom::Str(s)) => s.clone(),
        _ => {
            return Err(EvalError::NativeError(
                "`deftest` first argument must be a string name".to_string(),
            ));
        }
    };

    // Parse optional metadata flags
    let mut idx = 2usize;
    let mut skip_reason: Option<String> = None;
    let mut is_focused = false;
    let mut tag_list: Vec<String> = Vec::new();

    while idx < items.len() {
        if let NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) = &items[idx].kind {
            match kw.as_str() {
                "skip" => {
                    idx += 1;
                    let reason = if idx < items.len() {
                        if let NodeKind::Atom(Atom::Str(s)) = &items[idx].kind {
                            let s = s.clone();
                            idx += 1;
                            s
                        } else {
                            "skipped".to_string()
                        }
                    } else {
                        "skipped".to_string()
                    };
                    skip_reason = Some(reason);
                }
                "focus" => {
                    is_focused = true;
                    idx += 1;
                }
                "tags" => {
                    idx += 1;
                    if let Some(tags_node) = items.get(idx)
                        && let NodeKind::Vector(tag_nodes) = &tags_node.kind
                    {
                        for tag_node in tag_nodes.iter() {
                            match &tag_node.kind {
                                NodeKind::Atom(Atom::Keyword { name: tag, .. }) => {
                                    tag_list.push(tag.clone());
                                }
                                NodeKind::Atom(Atom::Str(s)) => {
                                    tag_list.push(s.clone());
                                }
                                NodeKind::Atom(Atom::Symbol { name, .. }) => {
                                    tag_list.push(name.clone());
                                }
                                _ => {}
                            }
                        }
                        idx += 1;
                    }
                }
                "timeout" | "flaky" => {
                    idx += 2; // skip the keyword and its value
                }
                _ => break,
            }
        } else {
            break;
        }
    }

    let body_nodes = &items[idx..];

    // Look up test/register! in the env
    let register_fn = env.get_qualified("test", "register!").ok_or_else(|| {
        EvalError::NativeError("`deftest` requires `test/register!` in scope".to_string())
    })?;

    // Build the body thunk
    let thunk = if let Some(reason) = skip_reason {
        // :skip — thunk just calls test/skip
        let skip_fn = env.get_qualified("test", "skip").ok_or_else(|| {
            EvalError::NativeError("`deftest` :skip requires `test/skip` in scope".to_string())
        })?;
        let reason_val = Value::Str(Rc::from(reason.as_str()));
        let skip_fn_clone = skip_fn.clone();
        Value::NativeClosure {
            name: Rc::from("<skipped>"),
            f: Rc::new(move |_args| nexl_runtime::call_value(&skip_fn_clone, std::slice::from_ref(&reason_val))),
        }
    } else if body_nodes.is_empty() {
        return Err(EvalError::NativeError(
            "`deftest` requires at least one body expression".to_string(),
        ));
    } else {
        // Normal thunk: close over a snapshot of the env and body nodes, plus lifecycle hooks
        let env_clone = Rc::clone(env);
        let body: Vec<Node> = body_nodes.to_vec();
        // Snapshot lifecycle hooks at registration time so they're captured in the closure
        let setup_hooks = nexl_stdlib::test::setup_snapshot();
        let teardown_hooks = nexl_stdlib::test::teardown_snapshot();
        Value::NativeClosure {
            name: Rc::from(name.as_str()),
            f: Rc::new(move |_args| {
                // Run all setup hooks (outermost first)
                for hook in &setup_hooks {
                    nexl_runtime::call_value(hook, &[]).map_err(|e| format!("setup: {e}"))?;
                }
                // Run body; capture error but still run teardown
                let mut last = Value::Unit;
                let mut body_err: Option<String> = None;
                for node in &body {
                    match crate::eval::eval(node, &env_clone) {
                        Ok(v) => last = v,
                        Err(e) => {
                            body_err = Some(format!("{e}"));
                            break;
                        }
                    }
                }
                // Run all teardown hooks (innermost first)
                for hook in teardown_hooks.iter().rev() {
                    let _ = nexl_runtime::call_value(hook, &[]);
                }
                match body_err {
                    Some(e) => Err(e),
                    None => Ok(last),
                }
            }),
        }
    };

    // Prepend the current describe path to the test name (spec §7.1)
    let full_name = format!("{}{name}", nexl_stdlib::test::describe_prefix());

    // If :focus, register this test name so the CLI can run only focused tests
    if is_focused {
        nexl_stdlib::test::focus_push(full_name.clone());
    }

    // If :tags, register tags for this test so the CLI can filter by tag
    if !tag_list.is_empty() {
        nexl_stdlib::test::tags_register(full_name.clone(), tag_list);
    }

    // Call test/register!("full-name", thunk)
    nexl_runtime::call_value(
        &register_fn,
        &[Value::Str(Rc::from(full_name.as_str())), thunk],
    )
    .map_err(EvalError::NativeError)?;

    Ok(EvalReturn::Value(Value::Unit))
}

/// `(describe "label" body...)` — group tests under a scoped name prefix (spec §7.1).
///
/// Pushes `label` onto the describe stack before evaluating `body`, then pops it.
/// Tests registered inside body via `deftest` will have their names prefixed with
/// the full describe path, e.g. `"Calculator > addition > test name"`.
fn eval_describe<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    // items[0] = "describe", items[1] = label, items[2..] = body
    let label_node = items.get(1).ok_or(EvalError::Arity)?;
    let label = match &label_node.kind {
        NodeKind::Atom(Atom::Str(s)) => s.clone(),
        _ => {
            return Err(EvalError::NativeError(
                "`describe` first argument must be a string label".to_string(),
            ));
        }
    };

    let body_nodes = &items[2..];
    if body_nodes.is_empty() {
        return Err(EvalError::Arity);
    }

    // Parse optional :let clause — binds local fixtures for all tests in this describe
    let (body_start, describe_env) =
        if let Some(NodeKind::Atom(Atom::Keyword { ns: None, name: kw })) =
            body_nodes.first().map(|n| &n.kind)
            && kw == "let"
        {
            let bindings_node = body_nodes.get(1).ok_or_else(|| {
                EvalError::NativeError("`describe` :let requires a binding vector".to_string())
            })?;
            let binding_pairs = match &bindings_node.kind {
                NodeKind::Vector(v) => v,
                _ => {
                    return Err(EvalError::NativeError(
                        "`describe` :let bindings must be a vector".to_string(),
                    ));
                }
            };
            if binding_pairs.len() % 2 != 0 {
                return Err(EvalError::NativeError(
                    "`describe` :let bindings must have even number of elements".to_string(),
                ));
            }
            let let_env = Rc::new(Env::child(Rc::clone(env)));
            let mut i = 0;
            while i < binding_pairs.len() {
                let name = match &binding_pairs[i].kind {
                    NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
                    _ => {
                        return Err(EvalError::NativeError(
                            "`describe` :let binding target must be a symbol".to_string(),
                        ));
                    }
                };
                let val = match eval_with_loop(&binding_pairs[i + 1], &let_env, loop_state)? {
                    EvalReturn::Value(v) => v,
                    r @ EvalReturn::Recur(_) => return Ok(r),
                };
                let_env.define(Rc::from(name.as_str()), val);
                i += 2;
            }
            (2, let_env)
        } else {
            (0, Rc::clone(env))
        };

    nexl_stdlib::test::describe_push(label);
    // Track lifecycle hook stack depth so we can pop hooks added within this scope
    let setup_depth = nexl_stdlib::test::setup_snapshot().len();
    let teardown_depth = nexl_stdlib::test::teardown_snapshot().len();

    let mut last = EvalReturn::Value(Value::Unit);
    let mut error: Option<EvalError> = None;

    for node in &body_nodes[body_start..] {
        match eval_with_loop(node, &describe_env, loop_state) {
            Ok(v) => last = v,
            Err(e) => {
                error = Some(e);
                break;
            }
        }
    }

    nexl_stdlib::test::describe_pop();
    // Pop any setup/teardown hooks that were registered in this scope
    let current_setup = nexl_stdlib::test::setup_snapshot().len();
    let current_teardown = nexl_stdlib::test::teardown_snapshot().len();
    for _ in setup_depth..current_setup {
        nexl_stdlib::test::setup_pop();
    }
    for _ in teardown_depth..current_teardown {
        nexl_stdlib::test::teardown_pop();
    }

    match error {
        Some(e) => Err(e),
        None => Ok(last),
    }
}

/// `(throws? body...)` / `(throws? ErrorType body...)` / `(throws? ErrorType pattern body...)`
///
/// Asserts that evaluating the body raises an error (spec §5).
///
/// Forms:
/// - `(throws? body...)` — any error
/// - `(throws? ErrorType body...)` — error message contains ErrorType name
/// - `(throws? ErrorType "pattern" body...)` — message contains type name and pattern
fn eval_throws_q<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() < 2 {
        return Err(EvalError::Arity);
    }

    // Determine if items[1] is an ErrorType symbol (uppercase) or part of body
    let mut body_start = 1usize;
    let mut error_type: Option<String> = None;
    let mut message_pattern: Option<String> = None;

    if let NodeKind::Atom(Atom::Symbol { ns: None, name: sym_name }) = &items[1].kind
        && sym_name.starts_with(|c: char| c.is_uppercase())
    {
        error_type = Some(sym_name.clone());
        body_start = 2;

        // Check for optional message pattern string
        if let Some(next) = items.get(2)
            && let NodeKind::Atom(Atom::Str(s)) = &next.kind
        {
            message_pattern = Some(s.clone());
            body_start = 3;
        }
    }

    let body_nodes = &items[body_start..];
    if body_nodes.is_empty() {
        return Err(EvalError::Arity);
    }

    // Evaluate each body form; collect the first error
    let mut error_msg: Option<String> = None;
    for node in body_nodes {
        if let Err(e) = eval_with_loop(node, env, loop_state) {
            error_msg = Some(format!("{e}"));
            break;
        }
    }

    match error_msg {
        None => Err(EvalError::NativeError(
            "throws?: expected an exception but none was thrown".to_string(),
        )),
        Some(msg) => {
            // Check error_type filter
            if let Some(type_name) = &error_type
                && !msg.to_lowercase().contains(&type_name.to_lowercase())
            {
                return Err(EvalError::NativeError(format!(
                    "throws?: expected error of type `{type_name}` but got: {msg}"
                )));
            }
            // Check message pattern filter
            if let Some(pattern) = &message_pattern
                && !msg.contains(pattern.as_str())
            {
                return Err(EvalError::NativeError(format!(
                    "throws?: expected error message to contain {pattern:?} but got: {msg}"
                )));
            }
            Ok(EvalReturn::Value(Value::Unit))
        }
    }
}

/// `(setup thunk)` / `(teardown thunk)` / `(setup-all thunk)` / `(teardown-all thunk)` — lifecycle hooks.
///
/// Registers the thunk as a lifecycle hook for the current describe scope (spec §7.4).
/// - `setup`: thunk runs before each test in the scope
/// - `teardown`: thunk runs after each test in the scope
/// - `setup-all`: thunk runs once before all tests in the scope
/// - `teardown-all`: thunk runs once after all tests in the scope
fn eval_lifecycle_hook<'a>(
    hook_name: &str,
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() < 2 {
        return Err(EvalError::NativeError(format!(
            "`{hook_name}` requires a thunk argument"
        )));
    }
    let thunk_node = &items[1];
    let thunk = match eval_with_loop(thunk_node, env, loop_state)? {
        EvalReturn::Value(v) => v,
        r @ EvalReturn::Recur(_) => return Ok(r),
    };
    match hook_name {
        "setup" => nexl_stdlib::test::setup_push(thunk),
        "teardown" => nexl_stdlib::test::teardown_push(thunk),
        "setup-all" => nexl_stdlib::test::setup_all_push(thunk),
        "teardown-all" => nexl_stdlib::test::teardown_all_push(thunk),
        _ => unreachable!("unexpected lifecycle hook name: {hook_name}"),
    }
    Ok(EvalReturn::Value(Value::Unit))
}

/// `(is-match pattern expr [:when guard] body...)` — pattern-matching assertion (spec §7.2).
///
/// Evaluates `expr`, matches it against `pattern`. On failure: raises an error with diagnostics.
/// On success: binds variables from the pattern, evaluates optional `:when` guard, then `body`.
/// If `:when` guard evaluates to false, the assertion fails.
/// If no body is provided, a successful match passes silently (returns Unit).
fn eval_is_match<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    // (is-match pattern expr [:when guard] body...)
    if items.len() < 3 {
        return Err(EvalError::NativeError(
            "`is-match` requires at least (is-match pattern expr)".to_string(),
        ));
    }

    let pat_node = &items[1];
    let expr_node = &items[2];

    let pattern =
        parse_pattern(pat_node).map_err(|e| EvalError::NativeError(format!("is-match: {e}")))?;

    let value = match eval_with_loop(expr_node, env, loop_state)? {
        EvalReturn::Value(v) => v,
        r @ EvalReturn::Recur(_) => return Ok(r),
    };

    let mut bindings: Vec<(Rc<str>, Value)> = Vec::new();
    if !match_pattern(&pattern, &value, &mut bindings) {
        return Err(EvalError::NativeError(format!(
            "is-match: value {value} did not match pattern {pat_node}"
        )));
    }

    // Build child env with pattern bindings
    let match_env = Rc::new(Env::child(Rc::clone(env)));
    for (name, val) in bindings {
        match_env.define(name, val);
    }

    // Parse :when guard if present
    let mut body_start = 3usize;
    if let Some(kw_node) = items.get(3)
        && let NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) = &kw_node.kind
        && kw.as_str() == "when"
    {
            let guard_node = items.get(4).ok_or_else(|| {
                EvalError::NativeError("`is-match` :when requires a guard expression".to_string())
            })?;
            let guard_val = match eval_with_loop(guard_node, &match_env, loop_state)? {
                EvalReturn::Value(v) => v,
                r @ EvalReturn::Recur(_) => return Ok(r),
            };
            match guard_val {
                Value::Bool(true) => {}
                Value::Bool(false) => {
                    return Err(EvalError::NativeError(format!(
                        "is-match: :when guard failed for {value}"
                    )));
                }
                other => {
                    return Err(EvalError::NativeError(format!(
                        "is-match: :when guard must return Bool, got {other}"
                    )));
                }
            }
        body_start = 5;
    }

    // Evaluate optional body forms
    let body_nodes = &items[body_start..];
    if body_nodes.is_empty() {
        return Ok(EvalReturn::Value(Value::Unit));
    }

    let mut last = EvalReturn::Value(Value::Unit);
    for node in body_nodes {
        last = eval_with_loop(node, &match_env, loop_state)?;
    }
    Ok(last)
}

/// `(deftype TypeName [params...] | Ctor1 | (Ctor2 arg) ...)`
/// Registers ADT constructors into the environment.
fn eval_deftype(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    // (deftype Name ...)
    if items.len() < 3 {
        return Err(EvalError::Arity);
    }

    let type_name = match &items[1].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => return Err(EvalError::InvalidBindingTarget),
    };

    // Skip optional type params [a b ...] and :derive [...]
    let mut i = 2;
    // Skip type params
    if i < items.len()
        && let NodeKind::Vector(_) = &items[i].kind
    {
        i += 1;
    }
    // Skip :derive clause
    if i + 1 < items.len()
        && let NodeKind::Atom(Atom::Keyword { ns: None, name }) = &items[i].kind
        && name == "derive"
    {
        i += 2; // skip :derive and [Show Eq ...]
    }

    // Parse constructors: expect | Ctor | (Ctor arg...) ...
    while i < items.len() {
        // Expect a `|` separator
        match &items[i].kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "|" => {
                i += 1;
            }
            NodeKind::Map(_) => {
                // Record type body: `{:field Type ...}`
                // Register a constructor that wraps a single map argument.
                let type_name_rc: Rc<str> = Rc::from(type_name.as_str());
                let ctor_fn = Value::NativeClosure {
                    name: Rc::clone(&type_name_rc),
                    f: {
                        let tn = Rc::clone(&type_name_rc);
                        Rc::new(move |args: &[Value]| {
                            if args.len() != 1 {
                                return Err(format!(
                                    "`{tn}` record constructor expects 1 map argument, got {}",
                                    args.len()
                                ));
                            }
                            match &args[0] {
                                Value::Map(_) => Ok(args[0].clone()),
                                other => Err(format!(
                                    "`{tn}` record constructor expects a map, got {other}"
                                )),
                            }
                        })
                    },
                };
                env.define(type_name.clone(), ctor_fn);
                break;
            }
            _ => {
                break;
            }
        }

        if i >= items.len() {
            return Err(EvalError::Arity);
        }

        match &items[i].kind {
            // Nullary constructor: `Red`
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => {
                let ctor_name = name.clone();
                let value = Value::Adt {
                    type_name: Rc::from(type_name.as_str()),
                    ctor: Rc::from(ctor_name.as_str()),
                    fields: Rc::new(vec![]),
                };
                env.define(ctor_name, value);
            }
            // N-ary constructor: `(Some a)` or `(Branch a left right)`
            NodeKind::List(ctor_items) => {
                if ctor_items.is_empty() {
                    return Err(EvalError::Arity);
                }
                let ctor_name = match &ctor_items[0].kind {
                    NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
                    _ => return Err(EvalError::InvalidBindingTarget),
                };
                let arity = ctor_items.len() - 1;

                if arity == 0 {
                    // Nullary in list form: (None)
                    let value = Value::Adt {
                        type_name: Rc::from(type_name.as_str()),
                        ctor: Rc::from(ctor_name.as_str()),
                        fields: Rc::new(vec![]),
                    };
                    env.define(ctor_name, value);
                } else {
                    // Create a constructor function
                    let type_name_rc: Rc<str> = Rc::from(type_name.as_str());
                    let ctor_name_rc: Rc<str> = Rc::from(ctor_name.as_str());
                    let ctor_fn = Value::NativeClosure {
                        name: Rc::clone(&ctor_name_rc),
                        f: {
                            let tn = Rc::clone(&type_name_rc);
                            let cn = Rc::clone(&ctor_name_rc);
                            let expected_arity = arity;
                            Rc::new(move |args: &[Value]| {
                                if args.len() != expected_arity {
                                    return Err(format!(
                                        "`{cn}` expects {expected_arity} argument(s), got {}",
                                        args.len()
                                    ));
                                }
                                Ok(Value::Adt {
                                    type_name: Rc::clone(&tn),
                                    ctor: Rc::clone(&cn),
                                    fields: Rc::new(args.to_vec()),
                                })
                            })
                        },
                    };
                    env.define(ctor_name, ctor_fn);
                }
            }
            _ => return Err(EvalError::InvalidBindingTarget),
        }
        i += 1;
    }

    Ok(EvalReturn::Value(Value::Unit))
}

/// Evaluate a `(defhandler Name [params?] Effect (op [args] body) ...)` form.
///
/// Parses the handler declaration and binds it in the environment as a
/// `Value::Handler`. Parameterized handlers store their param names for
/// later instantiation when called via `(handle [(HandlerName args)] body)`.
fn eval_defhandler(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    let decl = parse_defhandler_decl(items).map_err(|e| {
        EvalError::NativeError(format!("defhandler: {}", e.description))
    })?;

    let handler_def = HandlerDef {
        name: Rc::from(decl.name.as_str()),
        params: decl.params.iter().map(|p| Rc::from(p.as_str())).collect(),
        effects: decl.effects,
        built_ops: vec![],
    };

    env.define(
        decl.name.as_str(),
        Value::Handler(Rc::new(handler_def)),
    );

    Ok(EvalReturn::Value(Value::Unit))
}

/// Evaluate a `(handle [...] body...)` form (spec §6.4–§6.5, §6.10).
///
/// Supports three kinds of handler specifications in the vector:
/// 1. Inline: `[Effect (op [args] body)]` — operations defined inline
/// 2. Named: `[HandlerName]` — reference to a `defhandler` definition
/// 3. Parameterized: `[(HandlerName args)]` — named handler with args
///
/// For each effect in the handler, binds the operations as functions in a
/// child environment, then evaluates the body forms in that scope.
fn eval_handle<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() < 3 {
        return Err(EvalError::NativeError(
            "handle form requires a handler vector and body".into(),
        ));
    }

    let handler_vec = match &items[1].kind {
        NodeKind::Vector(elems) => elems,
        _ => {
            return Err(EvalError::NativeError(
                "handle form requires a vector of effect handlers".into(),
            ));
        }
    };

    // Create a child environment for the handler scope
    let handler_env = Rc::new(Env::child(Rc::clone(env)));

    // Determine whether this is a named handler reference or inline handlers.
    // For parameterized handlers, param_env has the handler params bound.
    let (effects, built_ops, param_env) = resolve_handler_effects(handler_vec, env)?;

    // The env used by handler operations: includes params if parameterized
    let ops_env = param_env.as_ref().unwrap_or(env);

    // Bind each effect's operations in the handler environment
    install_handler_effects(&effects, &built_ops, &handler_env, ops_env);

    // Evaluate body forms in the handler environment
    let body = &items[2..];
    let mut result = Value::Unit;
    for node in body {
        match eval_with_loop(node, &handler_env, loop_state)? {
            EvalReturn::Value(v) => result = v,
            recur @ EvalReturn::Recur(_) => return Ok(recur),
        }
    }

    Ok(EvalReturn::Value(result))
}

/// Result of resolving a handler vector: AST-based effects, pre-built ops, optional param env.
type HandlerResolution = (Vec<HandledEffect>, Vec<BuiltHandlerEffect>, Option<Rc<Env>>);

/// Resolve the handler vector into a list of [`HandledEffect`]s.
///
/// Returns `(effects, optional_param_env)`:
/// - `effects` — the handler's effect implementations
/// - `param_env` — if parameterized, an env with handler params bound
///
/// Handles three cases:
/// - Single uppercase symbol that resolves to a Handler value → named handler
/// - List form `(HandlerName args...)` → parameterized named handler
/// - Arbitrary expression evaluating to `Value::Handler` → dynamic handler
/// - Inline effect operations → parse normally
///
/// Returns `(ast_effects, built_ops, optional_param_env)`.
fn resolve_handler_effects(
    handler_vec: &[Node],
    env: &Rc<Env>,
) -> Result<HandlerResolution, EvalError> {
    if handler_vec.is_empty() {
        return Err(EvalError::NativeError(
            "handle vector must list at least one effect".into(),
        ));
    }

    // Check if the first element is a named handler reference
    // Case 1: [HandlerName] — single symbol that resolves to a Handler
    if handler_vec.len() == 1 {
        if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &handler_vec[0].kind
            && let Some(Value::Handler(h)) = env.get(name)
        {
            return Ok((h.effects.clone(), h.built_ops.clone(), None));
        }
        // Case 2: [(HandlerName args)] — parameterized handler in list
        if let NodeKind::List(call_items) = &handler_vec[0].kind
            && !call_items.is_empty()
            && let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &call_items[0].kind
            && let Some(Value::Handler(h)) = env.get(name)
        {
            // Evaluate the arguments and bind them to the handler's params
            let param_env = Rc::new(Env::child(Rc::clone(env)));
            let arg_nodes = &call_items[1..];
            if arg_nodes.len() != h.params.len() {
                return Err(EvalError::NativeError(format!(
                    "parameterized handler `{}` expects {} argument(s), got {}",
                    h.name,
                    h.params.len(),
                    arg_nodes.len()
                )));
            }
            for (param, arg_node) in h.params.iter().zip(arg_nodes.iter()) {
                let val = eval(arg_node, env)?;
                param_env.define(Rc::clone(param), val);
            }
            return Ok((h.effects.clone(), h.built_ops.clone(), Some(param_env)));
        }
        // Case 3: arbitrary expression evaluating to Value::Handler (e.g. `(:handler log)`)
        if let Ok(Value::Handler(h)) = eval(&handler_vec[0], env) {
            return Ok((h.effects.clone(), h.built_ops.clone(), None));
        }
    }

    // Check if first element resolves to a named handler (in a multi-element vector)
    if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &handler_vec[0].kind
        && let Some(Value::Handler(h)) = env.get(name)
    {
        return Ok((h.effects.clone(), h.built_ops.clone(), None));
    }

    // Fall through to inline handler parsing
    let decl = parse_handle_form(
        &std::iter::once(Node::atom(
            Atom::Symbol { ns: None, name: "handle".into() },
            meta::Span::synthetic(),
        ))
        .chain(std::iter::once(Node::new(
            NodeKind::Vector(handler_vec.to_vec()),
            meta::Span::synthetic(),
        )))
        .chain(std::iter::once(Node::atom(Atom::Unit, meta::Span::synthetic())))
        .collect::<Vec<_>>(),
    )
    .map_err(|e| EvalError::NativeError(format!("handle: {}", e.description)))?;

    Ok((decl.effects, vec![], None))
}

/// Install handler effects into the environment by binding operations as functions.
///
/// For each effect, binds each operation as:
/// - Unqualified: `op-name` → function
/// - Qualified: `EffectName/op-name` → function (via module alias)
///
/// When `built_ops` is non-empty (call-log-wrapped handlers), uses pre-built
/// Value functions directly instead of building from AST nodes.
fn install_handler_effects(
    effects: &[HandledEffect],
    built_ops: &[BuiltHandlerEffect],
    handler_env: &Rc<Env>,
    parent_env: &Rc<Env>,
) {
    use std::collections::HashMap;

    // If pre-built ops are present, use them directly (skips AST-based effects)
    if !built_ops.is_empty() {
        for effect in built_ops {
            let mut module_exports: HashMap<Rc<str>, Value> = HashMap::new();
            for (op_name, op_fn) in &effect.ops {
                handler_env.define(op_name.as_str(), op_fn.clone());
                module_exports.insert(Rc::from(op_name.as_str()), op_fn.clone());
            }
            handler_env.define_module_alias(effect.name.as_str(), Rc::new(module_exports));
        }
        return;
    }

    for effect in effects {
        let mut module_exports: HashMap<Rc<str>, Value> = HashMap::new();

        for op in &effect.operations {
            let op_fn = build_handler_op_fn(op, parent_env);

            // Bind unqualified name
            handler_env.define(op.name.as_str(), op_fn.clone());

            // Collect for qualified access
            module_exports.insert(Rc::from(op.name.as_str()), op_fn);
        }

        // Register as module alias for qualified access (e.g., Log/info)
        handler_env.define_module_alias(
            effect.name.as_str(),
            Rc::new(module_exports),
        );
    }
}

/// Build a function value from a handler operation definition.
///
/// For simple (non-continuation) ops, creates a closure that evaluates the
/// op body with params bound to arguments.
/// For continuation ops, creates a closure that also binds `resume`.
fn build_handler_op_fn(op: &HandledOp, env: &Rc<Env>) -> Value {
    let params: Vec<Rc<str>> = op.params.iter().map(|p| Rc::from(p.as_str())).collect();
    let body = op.body.clone();
    let has_resume = op.has_resume;
    let env = env.clone();

    Value::NativeClosure {
        name: Rc::from(op.name.as_str()),
        f: Rc::new(move |args: &[Value]| {
            let op_env = Rc::new(Env::child(Rc::clone(&env)));

            if has_resume {
                // For continuation form, bind `resume` as an identity function
                // (simple one-shot resume: returns the value as the effect result)
                let resume_fn = Value::NativeClosure {
                    name: Rc::from("resume"),
                    f: Rc::new(|args: &[Value]| {
                        if args.len() != 1 {
                            return Err("resume expects exactly 1 argument".into());
                        }
                        Ok(args[0].clone())
                    }),
                };
                op_env.define("resume", resume_fn);
            }

            // Bind operation parameters
            if args.len() != params.len() {
                return Err(format!(
                    "handler operation expects {} argument(s), got {}",
                    params.len(),
                    args.len()
                ));
            }
            for (param, arg) in params.iter().zip(args.iter()) {
                op_env.define(Rc::clone(param), arg.clone());
            }

            // Evaluate body
            let mut result = Value::Unit;
            for node in &body {
                result = crate::eval::eval(node, &op_env)
                    .map_err(|e| format!("handler operation error: {e}"))?;
            }
            Ok(result)
        }),
    }
}

fn eval_fn(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    if items.len() < 3 {
        return Err(EvalError::Arity);
    }

    let params_node = &items[1];
    let params_nodes = match &params_node.kind {
        NodeKind::Vector(items) => items,
        _ => return Err(EvalError::Arity),
    };

    let mut params: Vec<Rc<str>> = Vec::new();
    let mut rest: Option<Rc<str>> = None;
    let mut variadic = false;

    let mut iter = params_nodes.iter().peekable();
    while let Some(param) = iter.next() {
        match &param.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "&" => {
                variadic = true;
                let rest_node = iter.next().ok_or(EvalError::Arity)?;
                match &rest_node.kind {
                    NodeKind::Atom(Atom::Symbol { ns: None, name }) => {
                        rest = Some(Rc::from(name.as_str()));
                    }
                    _ => return Err(EvalError::InvalidBindingTarget),
                }
                if iter.peek().is_some() {
                    return Err(EvalError::Arity);
                }
                break;
            }
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => {
                params.push(Rc::from(name.as_str()));
            }
            _ => return Err(EvalError::InvalidBindingTarget),
        }
    }

    let arity: u32 = params.len() as u32;

    let func = Function {
        name: None,
        params,
        rest,
        arity,
        variadic,
        captures: env.capture_closure(),
        module_captures: env.capture_modules(),
        body: items[2..].to_vec(),
        requires: vec![],
        ensures: vec![],
    };

    Ok(EvalReturn::Value(Value::Function(Rc::new(func))))
}

/// Register `:examples` from a `defn` as test cases in the test registry (spec §4.2.1).
///
/// Called only in test mode. Each `{:in [...] :out v}` map produces a named test thunk.
fn register_contract_examples(
    fn_name: &str,
    fn_val: &Value,
    examples: &[Node],
    env: &Rc<Env>,
) -> Result<(), EvalError> {
    for (idx, example_node) in examples.iter().enumerate() {
        let NodeKind::Map(pairs) = &example_node.kind else { continue };

        let mut in_node: Option<&Node> = None;
        let mut out_node: Option<&Node> = None;
        for (k, v) in pairs {
            match &k.kind {
                NodeKind::Atom(Atom::Keyword { ns: None, name }) if &**name == "in" => {
                    in_node = Some(v);
                }
                NodeKind::Atom(Atom::Keyword { ns: None, name }) if &**name == "out" => {
                    out_node = Some(v);
                }
                _ => {}
            }
        }
        let (in_node, out_node) = match (in_node, out_node) {
            (Some(i), Some(o)) => (i, o),
            _ => continue,
        };

        // Evaluate :in args
        let args: Vec<Value> = match &in_node.kind {
            NodeKind::Vector(arg_nodes) => arg_nodes
                .iter()
                .map(|n| {
                    eval_with_loop(n, env, None).and_then(|r| match r {
                        EvalReturn::Value(v) => Ok(v),
                        EvalReturn::Recur(_) => Err(EvalError::Arity),
                    })
                })
                .collect::<Result<_, _>>()?,
            _ => continue,
        };

        // Evaluate :out expected
        let expected = match eval_with_loop(out_node, env, None)? {
            EvalReturn::Value(v) => v,
            EvalReturn::Recur(_) => continue,
        };

        let test_name = format!("{fn_name} example {}", idx + 1);
        let fn_clone = fn_val.clone();
        let name_clone = test_name.clone();
        let thunk = Value::NativeClosure {
            name: Rc::from(test_name.as_str()),
            f: Rc::new(move |_| {
                let actual = nexl_runtime::call_value(&fn_clone, &args)?;
                if actual == expected {
                    Ok(Value::Unit)
                } else {
                    Err(format!("{name_clone}: expected {expected}, got {actual}"))
                }
            }),
        };
        nexl_stdlib::test::registry_push(test_name, thunk);
    }
    Ok(())
}

fn eval_defn(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    if items.len() < 4 {
        return Err(EvalError::Arity);
    }

    let name_node = &items[1];
    let name = match &name_node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => return Err(EvalError::InvalidBindingTarget),
    };

    // Optional docstring at position 2 when it's a Str literal
    let (params_idx, body_start) = match &items[2].kind {
        NodeKind::Atom(Atom::Str(_)) => (3, 4),
        _ => (2, 3),
    };

    if body_start > items.len() - 1 {
        return Err(EvalError::Arity);
    }

    // Scan for contract clauses (:requires, :ensures, :examples) before body expressions.
    let mut requires_nodes: Vec<Node> = vec![];
    let mut ensures_nodes: Vec<Node> = vec![];
    let mut examples_nodes: Vec<Node> = vec![];
    let mut actual_body_start = body_start;

    let mut scan = body_start;
    while scan + 1 < items.len() {
        match &items[scan].kind {
            NodeKind::Atom(Atom::Keyword { ns: None, name }) if name == "requires" => {
                if let NodeKind::Vector(exprs) = &items[scan + 1].kind {
                    requires_nodes = exprs.clone();
                    scan += 2;
                    actual_body_start = scan;
                } else {
                    break;
                }
            }
            NodeKind::Atom(Atom::Keyword { ns: None, name }) if name == "ensures" => {
                if let NodeKind::Vector(exprs) = &items[scan + 1].kind {
                    ensures_nodes = exprs.clone();
                    scan += 2;
                    actual_body_start = scan;
                } else {
                    break;
                }
            }
            NodeKind::Atom(Atom::Keyword { ns: None, name })
                if name == "examples" || name == "example" =>
            {
                // Capture example nodes for contract-driven testing; skip at dev-mode eval time.
                if let NodeKind::Vector(ex) = &items[scan + 1].kind {
                    examples_nodes = ex.clone();
                }
                scan += 2;
                actual_body_start = scan;
            }
            _ => break,
        }
    }

    if actual_body_start >= items.len() {
        return Err(EvalError::Arity);
    }

    // Build an equivalent (fn [params] body...) form using only the actual body items.
    let mut fn_items = Vec::new();
    fn_items.push(Node {
        kind: NodeKind::Atom(Atom::Symbol {
            ns: None,
            name: "fn".into(),
        }),
        span: items[0].span,
        leading_comments: vec![],
        trailing_comment: None,
    });
    fn_items.push(items[params_idx].clone());
    fn_items.extend_from_slice(&items[actual_body_start..]);

    let fn_value = eval_list(&fn_items, env, None)?;

    let fn_value_named = match fn_value {
        EvalReturn::Value(Value::Function(f)) => Value::Function(Rc::new(Function {
            name: Some(Rc::from(name.as_str())),
            params: f.params.clone(),
            rest: f.rest.clone(),
            arity: f.arity,
            variadic: f.variadic,
            captures: f.captures.clone(),
            module_captures: f.module_captures.clone(),
            body: f.body.clone(),
            requires: requires_nodes,
            ensures: ensures_nodes,
        })),
        EvalReturn::Value(other) => other,
        EvalReturn::Recur(vals) => return Ok(EvalReturn::Recur(vals)),
    };

    // In test mode, register :examples before moving fn/name into env (spec §4.2.1).
    if nexl_stdlib::test::is_test_mode() && !examples_nodes.is_empty() {
        register_contract_examples(&name, &fn_value_named, &examples_nodes, env)?;
    }

    env.define(name, fn_value_named);
    Ok(EvalReturn::Value(Value::Unit))
}

fn eval_loop(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    if items.len() < 3 {
        return Err(EvalError::Arity);
    }

    let bindings_node = &items[1];
    let bindings = match &bindings_node.kind {
        NodeKind::Vector(items) => items,
        _ => return Err(EvalError::Arity),
    };
    if !bindings.len().is_multiple_of(2) {
        return Err(EvalError::Arity);
    }

    let loop_env = Rc::new(Env::child(Rc::clone(env)));
    let mut names: Vec<Rc<str>> = Vec::new();

    for pair in bindings.chunks_exact(2) {
        let (name_node, value_node) = (&pair[0], &pair[1]);
        let name: Rc<str> = match &name_node.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => Rc::from(name.as_str()),
            _ => return Err(EvalError::InvalidBindingTarget),
        };
        let value = match eval_with_loop(value_node, &loop_env, None)? {
            EvalReturn::Value(v) => v,
            EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
        };
        loop_env.define(name.clone(), value);
        names.push(name);
    }

    let body = &items[2..];
    let frame = LoopFrame { names: &names };

    'lo: loop {
        let mut last = Value::Unit;
        for expr in body {
            match eval_with_loop(expr, &loop_env, Some(&frame))? {
                EvalReturn::Value(v) => last = v,
                EvalReturn::Recur(values) => {
                    if values.len() != names.len() {
                        return Err(EvalError::Arity);
                    }
                    for (name, val) in names.iter().zip(values.into_iter()) {
                        loop_env.define(name.clone(), val);
                    }
                    continue 'lo;
                }
            }
        }
        return Ok(EvalReturn::Value(last));
    }
}

fn eval_recur<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    let frame = match loop_state {
        Some(f) => f,
        None => return Err(EvalError::InvalidRecur),
    };

    if items.len() - 1 != frame.names.len() {
        return Err(EvalError::Arity);
    }

    let mut values = Vec::new();
    for arg in &items[1..] {
        let v = match eval_with_loop(arg, env, loop_state)? {
            EvalReturn::Value(v) => v,
            EvalReturn::Recur(vals) => return Ok(EvalReturn::Recur(vals)),
        };
        values.push(v);
    }

    Ok(EvalReturn::Recur(values))
}

fn eval_each(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    if items.len() < 3 {
        return Err(EvalError::Arity);
    }

    let bindings = match &items[1].kind {
        NodeKind::Vector(items) => items,
        _ => return Err(EvalError::Arity),
    };
    if bindings.len() != 2 {
        return Err(EvalError::Arity);
    }

    let binding_node = &bindings[0];
    let pattern = parse_pattern(binding_node)
        .map_err(|_| EvalError::InvalidBindingTarget)?;

    let coll_value = match eval_with_loop(&bindings[1], env, None)? {
        EvalReturn::Value(v) => v,
        EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
    };

    let mut iter_values: Vec<Value> = Vec::new();
    match coll_value {
        Value::Vec(items) => iter_values.extend(items.iter().cloned()),
        Value::Set(items) => iter_values.extend(items.iter().cloned()),
        Value::Map(entries) => iter_values.extend(entries.iter().map(|(_, v)| v.clone())),
        Value::Adt {
            type_name,
            ctor,
            fields,
        } if type_name.as_ref() == "Option" => match ctor.as_ref() {
            "None" => {}
            "Some" => {
                let value = fields
                    .first()
                    .ok_or_else(|| EvalError::NativeError("Option.Some missing field".into()))?;
                iter_values.push(value.clone());
            }
            _ => return Err(EvalError::NativeError("unknown Option constructor".into())),
        },
        other => {
            return Err(EvalError::NativeError(format!(
                "`each` expected Vec, Map, Set, or Option, got {}",
                other.type_name()
            )));
        }
    }

    for (idx, element) in iter_values.iter().enumerate() {
        let mut row_bindings: Vec<(Rc<str>, Value)> = Vec::new();
        if !match_pattern(&pattern, element, &mut row_bindings) {
            return Err(EvalError::NativeError(format!(
                "each row {idx}: value {element} did not match binding pattern {binding_node}"
            )));
        }
        let iter_env = Rc::new(Env::child(Rc::clone(env)));
        for (name, val) in row_bindings {
            iter_env.define(name, val);
        }
        for expr in &items[2..] {
            match eval_with_loop(expr, &iter_env, None).map_err(|e| {
                EvalError::NativeError(format!("each row {idx}: {e}"))
            })? {
                EvalReturn::Value(_) => {}
                EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
            }
        }
    }

    Ok(EvalReturn::Value(Value::Unit))
}

fn eval_times(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    if items.len() < 3 {
        return Err(EvalError::Arity);
    }

    let bindings = match &items[1].kind {
        NodeKind::Vector(items) => items,
        _ => return Err(EvalError::Arity),
    };
    if bindings.len() != 2 {
        return Err(EvalError::Arity);
    }

    let name = match &bindings[0].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => return Err(EvalError::InvalidBindingTarget),
    };

    let count_value = match eval_with_loop(&bindings[1], env, None)? {
        EvalReturn::Value(v) => v,
        EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
    };
    let count = match count_value {
        Value::Int(n) if n >= 0 => n,
        Value::Int(n) => {
            return Err(EvalError::NativeError(format!(
                "`times` count must be non-negative, got {n}"
            )));
        }
        other => {
            return Err(EvalError::NativeError(format!(
                "`times` expected Int count, got {}",
                other.type_name()
            )));
        }
    };

    for i in 0..count {
        let iter_env = Rc::new(Env::child(Rc::clone(env)));
        iter_env.define(name.clone(), Value::Int(i));
        for expr in &items[2..] {
            match eval_with_loop(expr, &iter_env, None)? {
                EvalReturn::Value(_) => {}
                EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
            }
        }
    }

    Ok(EvalReturn::Value(Value::Unit))
}

fn eval_for(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    if items.len() < 3 {
        return Err(EvalError::Arity);
    }

    let bindings = match &items[1].kind {
        NodeKind::Vector(items) => items,
        _ => return Err(EvalError::Arity),
    };

    let body = &items[2..];
    if body.is_empty() {
        return Err(EvalError::Arity);
    }

    let mut out = Vec::new();
    eval_for_bindings(bindings, 0, env, body, &mut out)?;
    Ok(EvalReturn::Value(Value::Vec(Rc::new(out))))
}

fn eval_for_bindings(
    bindings: &[Node],
    idx: usize,
    env: &Rc<Env>,
    body: &[Node],
    out: &mut Vec<Value>,
) -> Result<ForControl, EvalError> {
    if idx >= bindings.len() {
        let mut last = Value::Unit;
        for expr in body {
            match eval_with_loop(expr, env, None)? {
                EvalReturn::Value(v) => last = v,
                EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
            }
        }
        out.push(last);
        return Ok(ForControl::Continue);
    }

    match &bindings[idx].kind {
        NodeKind::Atom(Atom::Keyword { ns: None, name }) if name == "when" => {
            let cond_node = bindings.get(idx + 1).ok_or(EvalError::Arity)?;
            let cond_val = match eval_with_loop(cond_node, env, None)? {
                EvalReturn::Value(v) => v,
                EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
            };
            match cond_val {
                Value::Bool(true) => eval_for_bindings(bindings, idx + 2, env, body, out),
                Value::Bool(false) => Ok(ForControl::Continue),
                other => Err(EvalError::NativeError(format!(
                    "`for` :when expected Bool, got {}",
                    other.type_name()
                ))),
            }
        }
        NodeKind::Atom(Atom::Keyword { ns: None, name }) if name == "while" => {
            let cond_node = bindings.get(idx + 1).ok_or(EvalError::Arity)?;
            let cond_val = match eval_with_loop(cond_node, env, None)? {
                EvalReturn::Value(v) => v,
                EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
            };
            match cond_val {
                Value::Bool(true) => eval_for_bindings(bindings, idx + 2, env, body, out),
                Value::Bool(false) => Ok(ForControl::Break),
                other => Err(EvalError::NativeError(format!(
                    "`for` :while expected Bool, got {}",
                    other.type_name()
                ))),
            }
        }
        NodeKind::Atom(Atom::Keyword { ns: None, name }) if name == "let" => {
            let binding_node = bindings.get(idx + 1).ok_or(EvalError::Arity)?;
            let binding_vec = match &binding_node.kind {
                NodeKind::Vector(items) => items,
                _ => return Err(EvalError::Arity),
            };
            let let_env = eval_for_let_bindings(binding_vec, env)?;
            eval_for_bindings(bindings, idx + 2, &let_env, body, out)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => {
            let coll_node = bindings.get(idx + 1).ok_or(EvalError::Arity)?;
            let coll_val = match eval_with_loop(coll_node, env, None)? {
                EvalReturn::Value(v) => v,
                EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
            };

            let mut iter_values: Vec<Value> = Vec::new();
            match coll_val {
                Value::Vec(items) => iter_values.extend(items.iter().cloned()),
                Value::Set(items) => iter_values.extend(items.iter().cloned()),
                Value::Map(entries) => iter_values.extend(entries.iter().map(|(_, v)| v.clone())),
                Value::Adt {
                    type_name,
                    ctor,
                    fields,
                } if type_name.as_ref() == "Option" => match ctor.as_ref() {
                    "None" => {}
                    "Some" => {
                        let value = fields.first().ok_or_else(|| {
                            EvalError::NativeError("Option.Some missing field".into())
                        })?;
                        iter_values.push(value.clone());
                    }
                    _ => return Err(EvalError::NativeError("unknown Option constructor".into())),
                },
                other => {
                    return Err(EvalError::NativeError(format!(
                        "`for` expected Vec, Map, Set, or Option, got {}",
                        other.type_name()
                    )));
                }
            }

            for value in iter_values {
                let iter_env = Rc::new(Env::child(Rc::clone(env)));
                iter_env.define(name.clone(), value);
                let control = eval_for_bindings(bindings, idx + 2, &iter_env, body, out)?;
                if matches!(control, ForControl::Break) {
                    break;
                }
            }
            Ok(ForControl::Continue)
        }
        _ => Err(EvalError::Arity),
    }
}

fn eval_for_let_bindings(bindings: &[Node], env: &Rc<Env>) -> Result<Rc<Env>, EvalError> {
    if !bindings.len().is_multiple_of(2) {
        return Err(EvalError::Arity);
    }

    let child_env = Rc::new(Env::child(Rc::clone(env)));
    let mut i = 0;
    while i < bindings.len() {
        if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &bindings[i].kind
            && name == "mut"
        {
            i += 1;
        }

        let name_node = bindings.get(i).ok_or(EvalError::Arity)?;
        let name = match &name_node.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
            _ => return Err(EvalError::InvalidBindingTarget),
        };
        i += 1;

        let value_node = bindings.get(i).ok_or(EvalError::Arity)?;
        i += 1;

        let value = match eval_with_loop(value_node, &child_env, None)? {
            EvalReturn::Value(v) => v,
            EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
        };
        child_env.define(name, value);
    }

    Ok(child_env)
}

/// Keyword-as-function: look up a keyword in a map, returning the value
/// directly if found, or `None` (Option ADT) if missing.
fn keyword_lookup(
    ns: &Option<Rc<str>>,
    name: &Rc<str>,
    arg: &Value,
) -> Result<Value, EvalError> {
    if let Value::Map(entries) = arg {
        let kw = Value::Keyword {
            ns: ns.clone(),
            name: name.clone(),
        };
        for (key, value) in entries.iter() {
            if *key == kw {
                return Ok(value.clone());
            }
        }
        // Key not found — return None
        return Ok(Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("None"),
            fields: Rc::new(vec![]),
        });
    }
    Err(EvalError::NativeError(format!(
        "keyword `:{name}` called on non-map value: {arg}"
    )))
}

fn eval_apply<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    let head = &items[0];
    let callee = match eval_with_loop(head, env, loop_state)? {
        EvalReturn::Value(v) => v,
        EvalReturn::Recur(vals) => return Ok(EvalReturn::Recur(vals)),
    };

    // Dispatch native built-ins before the closure path.
    if let Value::NativeFunction(native) = &callee {
        let mut args = Vec::with_capacity(items.len() - 1);
        for arg_node in &items[1..] {
            match eval_with_loop(arg_node, env, loop_state)? {
                EvalReturn::Value(v) => args.push(v),
                recur @ EvalReturn::Recur(_) => return Ok(recur),
            }
        }
        let result = (native.f)(&args).map_err(EvalError::NativeError)?;
        return Ok(EvalReturn::Value(result));
    }

    // Dispatch native closures (stdlib HOFs like comp, partial, etc.).
    if let Value::NativeClosure { f, .. } = &callee {
        let mut args = Vec::with_capacity(items.len() - 1);
        for arg_node in &items[1..] {
            match eval_with_loop(arg_node, env, loop_state)? {
                EvalReturn::Value(v) => args.push(v),
                recur @ EvalReturn::Recur(_) => return Ok(recur),
            }
        }
        let result = f(&args).map_err(EvalError::NativeError)?;
        return Ok(EvalReturn::Value(result));
    }

    // Dispatch keyword-as-function: (:key map) → field lookup
    if let Value::Keyword { ns, name } = &callee {
        let arg_count = items.len() - 1;
        if arg_count != 1 {
            return Err(EvalError::Arity);
        }
        let arg = match eval_with_loop(&items[1], env, loop_state)? {
            EvalReturn::Value(v) => v,
            recur @ EvalReturn::Recur(_) => return Ok(recur),
        };
        return keyword_lookup(ns, name, &arg).map(EvalReturn::Value);
    }

    let Value::Function(func) = callee else {
        return Err(EvalError::InvalidCallable);
    };

    let required = func.arity as usize;
    let provided = items.len() - 1;

    if (!func.variadic && provided != required) || (func.variadic && provided < required) {
        return Err(EvalError::Arity);
    }

    let call_env = Rc::new(Env::new());

    // load captures
    for (name, value) in &func.captures {
        call_env.define(name.clone(), value.clone());
    }
    for (alias, exports) in &func.module_captures {
        call_env.define_module_alias(alias.clone(), Rc::clone(exports));
    }

    // Named functions can call themselves: bind the function under its own name
    // so recursive calls resolve correctly regardless of capture-snapshot timing.
    if let Some(self_name) = &func.name {
        call_env.define(self_name.clone(), Value::Function(Rc::clone(&func)));
    }

    // bind required params
    for (idx, param) in func.params.iter().enumerate() {
        let arg_val = match eval_with_loop(&items[idx + 1], env, loop_state)? {
            EvalReturn::Value(v) => v,
            EvalReturn::Recur(vals) => return Ok(EvalReturn::Recur(vals)),
        };
        call_env.define(param.clone(), arg_val);
    }

    // bind rest if variadic
    if func.variadic {
        if let Some(rest_name) = &func.rest {
            let mut rest_values = Vec::with_capacity(provided.saturating_sub(required));
            for arg_node in &items[required + 1..] {
                match eval_with_loop(arg_node, env, loop_state)? {
                    EvalReturn::Value(v) => rest_values.push(v),
                    EvalReturn::Recur(vals) => return Ok(EvalReturn::Recur(vals)),
                }
            }
            call_env.define(rest_name.clone(), Value::Vec(Rc::new(rest_values)));
        }
    } else if provided != required {
        return Err(EvalError::Arity);
    }

    // Check preconditions (spec §4.2.1: evaluated before body in dev mode).
    for req_expr in &func.requires {
        let cond = match eval_with_loop(req_expr, &call_env, None) {
            Ok(EvalReturn::Value(v)) => v,
            Ok(EvalReturn::Recur(_)) => return Err(EvalError::InvalidRecur),
            Err(e) => return Err(e),
        };
        match cond {
            Value::Bool(true) => {}
            Value::Bool(false) => return Err(EvalError::Panic("precondition failed".to_string())),
            _ => return Err(EvalError::InvalidConditionType),
        }
    }

    let mut last = Value::Unit;
    for expr in &func.body {
        match eval_with_loop(expr, &call_env, loop_state) {
            Ok(EvalReturn::Value(v)) => last = v,
            Ok(EvalReturn::Recur(vals)) => return Ok(EvalReturn::Recur(vals)),
            Err(EvalError::EarlyReturn(v)) => return Ok(EvalReturn::Value(v)),
            Err(e) => return Err(e),
        }
    }

    // Check postconditions (spec §4.2.1: evaluated after body; `result` bound to return value).
    if !func.ensures.is_empty() {
        let ensures_env = Rc::new(Env::child(Rc::clone(&call_env)));
        ensures_env.define("result", last.clone());
        for ens_expr in &func.ensures {
            let cond = match eval_with_loop(ens_expr, &ensures_env, None) {
                Ok(EvalReturn::Value(v)) => v,
                Ok(EvalReturn::Recur(_)) => return Err(EvalError::InvalidRecur),
                Err(e) => return Err(e),
            };
            match cond {
                Value::Bool(true) => {}
                Value::Bool(false) => {
                    return Err(EvalError::Panic("postcondition failed".to_string()));
                }
                _ => return Err(EvalError::InvalidConditionType),
            }
        }
    }

    Ok(EvalReturn::Value(last))
}

pub(crate) fn apply_value(callee: &Value, args: &[Value]) -> Result<Value, EvalError> {
    if let Value::NativeFunction(native) = callee {
        return (native.f)(args).map_err(EvalError::NativeError);
    }

    if let Value::NativeClosure { f, .. } = callee {
        return f(args).map_err(EvalError::NativeError);
    }

    // Dispatch keyword-as-function in apply_value too
    if let Value::Keyword { ns, name } = callee {
        if args.len() != 1 {
            return Err(EvalError::Arity);
        }
        return keyword_lookup(ns, name, &args[0]);
    }

    let Value::Function(func) = callee else {
        return Err(EvalError::InvalidCallable);
    };

    let required = func.arity as usize;
    let provided = args.len();

    if (!func.variadic && provided != required) || (func.variadic && provided < required) {
        return Err(EvalError::Arity);
    }

    let call_env = Rc::new(Env::new());

    for (name, value) in &func.captures {
        call_env.define(name.clone(), value.clone());
    }
    for (alias, exports) in &func.module_captures {
        call_env.define_module_alias(alias.clone(), Rc::clone(exports));
    }

    // Named functions can call themselves: bind the function under its own name
    // so recursive calls resolve correctly regardless of capture-snapshot timing.
    if let Some(self_name) = &func.name {
        call_env.define(self_name.clone(), callee.clone());
    }

    for (idx, param) in func.params.iter().enumerate() {
        let arg_val = args.get(idx).ok_or(EvalError::Arity)?.clone();
        call_env.define(param.clone(), arg_val);
    }

    if func.variadic
        && let Some(rest_name) = &func.rest
    {
        let rest_values: Vec<Value> = args[required..].to_vec();
        call_env.define(rest_name.clone(), Value::Vec(Rc::new(rest_values)));
    }

    for req_expr in &func.requires {
        let cond = match eval_with_loop(req_expr, &call_env, None) {
            Ok(EvalReturn::Value(v)) => v,
            Ok(EvalReturn::Recur(_)) => return Err(EvalError::InvalidRecur),
            Err(e) => return Err(e),
        };
        match cond {
            Value::Bool(true) => {}
            Value::Bool(false) => return Err(EvalError::Panic("precondition failed".to_string())),
            _ => return Err(EvalError::InvalidConditionType),
        }
    }

    let mut last = Value::Unit;
    for expr in &func.body {
        match eval_with_loop(expr, &call_env, None) {
            Ok(EvalReturn::Value(v)) => last = v,
            Ok(EvalReturn::Recur(_)) => return Err(EvalError::InvalidRecur),
            Err(EvalError::EarlyReturn(v)) => return Ok(v),
            Err(e) => return Err(e),
        }
    }

    if !func.ensures.is_empty() {
        let ensures_env = Rc::new(Env::child(Rc::clone(&call_env)));
        ensures_env.define("result", last.clone());
        for ens_expr in &func.ensures {
            let cond = match eval_with_loop(ens_expr, &ensures_env, None) {
                Ok(EvalReturn::Value(v)) => v,
                Ok(EvalReturn::Recur(_)) => return Err(EvalError::InvalidRecur),
                Err(e) => return Err(e),
            };
            match cond {
                Value::Bool(true) => {}
                Value::Bool(false) => {
                    return Err(EvalError::Panic("postcondition failed".to_string()));
                }
                _ => return Err(EvalError::InvalidConditionType),
            }
        }
    }

    Ok(last)
}

fn eval_set_bang(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    if items.len() != 3 {
        return Err(EvalError::Arity);
    }
    let name = match &items[1].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => return Err(EvalError::InvalidBindingTarget),
    };
    let value = match eval_with_loop(&items[2], env, None)? {
        EvalReturn::Value(v) => v,
        EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
    };
    env.set(&name, value)
        .map_err(|crate::EnvError::Unbound(n)| EvalError::UnboundSymbol(n))?;
    Ok(EvalReturn::Value(Value::Unit))
}

/// Evaluate `(panic "message")` — terminates with `EvalError::Panic`.
fn eval_panic<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() != 2 {
        return Err(EvalError::Arity);
    }
    let msg_val = match eval_with_loop(&items[1], env, loop_state)? {
        EvalReturn::Value(v) => v,
        EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
    };
    let msg = match msg_val {
        Value::Str(s) => s.to_string(),
        other => other.type_name().to_string(),
    };
    Err(EvalError::Panic(msg))
}

/// Evaluate `(assert! cond)` or `(assert! cond msg)`.
///
/// If `cond` evaluates to `true`, returns `unit`.
/// If `cond` evaluates to `false`, panics with the optional message or a default.
/// (spec §4.2.1)
fn eval_assert<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() < 2 || items.len() > 3 {
        return Err(EvalError::Arity);
    }
    let cond = match eval_with_loop(&items[1], env, loop_state)? {
        EvalReturn::Value(v) => v,
        EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
    };
    match cond {
        Value::Bool(true) => Ok(EvalReturn::Value(Value::Unit)),
        Value::Bool(false) => {
            let msg = if let Some(msg_node) = items.get(2) {
                match eval_with_loop(msg_node, env, loop_state)? {
                    EvalReturn::Value(Value::Str(s)) => s.to_string(),
                    EvalReturn::Value(other) => other.type_name().to_string(),
                    EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
                }
            } else {
                "assertion failed".to_string()
            };
            Err(EvalError::Panic(msg))
        }
        _ => Err(EvalError::InvalidConditionType),
    }
}

/// Evaluate `(assert-unreachable!)` or `(assert-unreachable! msg)`.
///
/// Always panics — used to mark code paths that should never be reached.
/// Typed as `Never` by the type checker. (spec §4.2.1)
fn eval_assert_unreachable<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() > 2 {
        return Err(EvalError::Arity);
    }
    let msg = if let Some(msg_node) = items.get(1) {
        match eval_with_loop(msg_node, env, loop_state)? {
            EvalReturn::Value(Value::Str(s)) => s.to_string(),
            EvalReturn::Value(other) => other.type_name().to_string(),
            EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
        }
    } else {
        "assert-unreachable! reached".to_string()
    };
    Err(EvalError::Panic(msg))
}

/// Evaluate the `?` postfix operator: `(? expr)` (spec §9.3).
///
/// - On `(Ok v)` / `(Some v)`: unwraps and returns `v`.
/// - On `(Err e)` / `None`: triggers a non-local early return via
///   `EvalError::EarlyReturn`, caught by the enclosing `eval_apply`.
fn eval_question<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() != 2 {
        return Err(EvalError::Arity);
    }
    let val = match eval_with_loop(&items[1], env, loop_state)? {
        EvalReturn::Value(v) => v,
        EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
    };
    match &val {
        Value::Adt {
            type_name,
            ctor,
            fields,
        } if type_name.as_ref() == "Result" => match ctor.as_ref() {
            "Ok" => {
                let inner = fields
                    .first()
                    .ok_or_else(|| EvalError::NativeError("Result.Ok missing field".into()))?
                    .clone();
                Ok(EvalReturn::Value(inner))
            }
            "Err" => Err(EvalError::EarlyReturn(val)),
            _ => Err(EvalError::NativeError(format!(
                "unknown Result constructor: {ctor}"
            ))),
        },
        Value::Adt {
            type_name,
            ctor,
            fields,
        } if type_name.as_ref() == "Option" => match ctor.as_ref() {
            "Some" => {
                let inner = fields
                    .first()
                    .ok_or_else(|| EvalError::NativeError("Option.Some missing field".into()))?
                    .clone();
                Ok(EvalReturn::Value(inner))
            }
            "None" => Err(EvalError::EarlyReturn(val)),
            _ => Err(EvalError::NativeError(format!(
                "unknown Option constructor: {ctor}"
            ))),
        },
        other => Err(EvalError::NativeError(format!(
            "? applied to non-Result/Option value: {}",
            other.type_name()
        ))),
    }
}

/// Evaluate a `(try body... (catch name catch-body...))` form (spec §9).
///
/// Desugars to a match on `Result`:
/// - `(Ok v)` → yields `v`
/// - `(Err e)` → evaluates `catch-body` with `name` bound to `e`
fn eval_try<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    let form = parse_try_form(items).map_err(|e| EvalError::NativeError(e.description))?;
    eval_try_form(&form, env, loop_state)
}

fn eval_try_form<'a>(
    form: &TryCatchForm,
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    // Evaluate body expressions; last result must be a Result ADT.
    let mut last = Value::Unit;
    for expr in &form.body {
        match eval_with_loop(expr, env, loop_state) {
            Ok(EvalReturn::Value(v)) => last = v,
            Ok(EvalReturn::Recur(vals)) => return Ok(EvalReturn::Recur(vals)),
            Err(EvalError::EarlyReturn(v)) => return Ok(EvalReturn::Value(v)),
            Err(e) => return Err(e),
        }
    }

    // Desugar: match last on Ok/Err.
    match last {
        Value::Adt {
            ref type_name,
            ref ctor,
            ref fields,
        } if type_name.as_ref() == "Result" => match ctor.as_ref() {
            "Ok" => {
                let inner = fields
                    .first()
                    .ok_or_else(|| EvalError::NativeError("Result.Ok missing field".into()))?
                    .clone();
                Ok(EvalReturn::Value(inner))
            }
            "Err" => {
                let inner = fields
                    .first()
                    .ok_or_else(|| EvalError::NativeError("Result.Err missing field".into()))?
                    .clone();
                let catch_env = Rc::new(Env::child(Rc::clone(env)));
                catch_env.define(form.catch_name.as_str(), inner);
                let mut catch_last = Value::Unit;
                for expr in &form.catch_body {
                    match eval_with_loop(expr, &catch_env, loop_state) {
                        Ok(EvalReturn::Value(v)) => catch_last = v,
                        Ok(EvalReturn::Recur(vals)) => return Ok(EvalReturn::Recur(vals)),
                        Err(EvalError::EarlyReturn(v)) => return Ok(EvalReturn::Value(v)),
                        Err(e) => return Err(e),
                    }
                }
                Ok(EvalReturn::Value(catch_last))
            }
            _ => Err(EvalError::NativeError(format!(
                "try: expected Result (Ok or Err), got constructor `{ctor}`"
            ))),
        },
        other => Err(EvalError::NativeError(format!(
            "try: body must produce a Result, got {}",
            other.type_name()
        ))),
    }
}

/// Evaluate a `(call-log HandlerName)` form.
///
/// Creates a recording-wrapped handler that logs every operation call.
/// Returns `{:handler wrapped-handler :calls (atom [])}` where each logged
/// call is `{:op :op-name :args [...] :returned value}`.
fn eval_call_log(
    items: &[Node],
    env: &Rc<Env>,
) -> Result<EvalReturn, EvalError> {
    if items.len() != 2 {
        return Err(EvalError::NativeError(
            "call-log requires exactly one argument (handler name or expression)".into(),
        ));
    }

    let handler_val = eval(&items[1], env)?;
    let h = match &handler_val {
        Value::Handler(h) => Rc::clone(h),
        other => {
            return Err(EvalError::NativeError(format!(
                "call-log: expected a Handler, got {}",
                other.type_name()
            )));
        }
    };

    use std::cell::RefCell;
    use nexl_runtime::BuiltHandlerEffect;

    // The shared calls atom: (atom [])
    let calls_inner: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::Vec(Rc::new(vec![]))));
    let calls_atom = Value::Atom(Rc::clone(&calls_inner));

    // Build wrapped effects — for each op, wrap with recording logic
    let mut built_effects: Vec<BuiltHandlerEffect> = Vec::new();

    // Process both AST-based effects and any existing built_ops
    let effect_names_and_ops: Vec<(String, Vec<(String, Value)>)> = if !h.built_ops.is_empty() {
        // Wrap existing built ops
        h.built_ops.iter().map(|be| {
            let ops = be.ops.iter().map(|(op_name, op_fn)| (op_name.clone(), op_fn.clone())).collect();
            (be.name.clone(), ops)
        }).collect()
    } else {
        // Build from AST effects
        h.effects.iter().map(|effect| {
            let ops = effect.operations.iter().map(|op| {
                let op_fn = build_handler_op_fn(op, env);
                (op.name.clone(), op_fn)
            }).collect();
            (effect.name.clone(), ops)
        }).collect()
    };

    for (effect_name, ops) in effect_names_and_ops {
        let mut wrapped_ops: Vec<(String, Value)> = Vec::new();
        for (op_name, original_fn) in ops {
            let calls_rc = Rc::clone(&calls_inner);
            let kw_name: Rc<str> = Rc::from(op_name.as_str());
            let wrapped_fn = Value::NativeClosure {
                name: Rc::from(op_name.as_str()),
                f: Rc::new(move |args: &[Value]| {
                    // Call original
                    let result = nexl_runtime::call_value(&original_fn, args)?;

                    // Record call: {:op :op-name :args [...] :returned result}
                    let kw = |s: &'static str| Value::Keyword { ns: None, name: Rc::from(s) };
                    let entry: Vec<(Value, Value)> = vec![
                        (kw("op"),       Value::Keyword { ns: None, name: Rc::clone(&kw_name) }),
                        (kw("args"),     Value::Vec(Rc::new(args.to_vec()))),
                        (kw("returned"), result.clone()),
                    ];
                    let entry_val = Value::Map(Rc::new(entry.into()));

                    // Append to calls atom
                    let mut calls = calls_rc.borrow_mut();
                    if let Value::Vec(ref vec_rc) = *calls {
                        let mut new_vec = (**vec_rc).clone();
                        new_vec.push(entry_val);
                        *calls = Value::Vec(Rc::new(new_vec));
                    }

                    Ok(result)
                }),
            };
            wrapped_ops.push((op_name, wrapped_fn));
        }
        built_effects.push(BuiltHandlerEffect { name: effect_name, ops: wrapped_ops });
    }

    // Create new HandlerDef with built_ops and empty AST effects
    let wrapped_handler_def = HandlerDef {
        name: Rc::from(format!("call-log({})", h.name).as_str()),
        params: vec![],
        effects: vec![],
        built_ops: built_effects,
    };

    // Return {:handler wrapped-handler :calls calls-atom}
    let kw = |s: &'static str| Value::Keyword { ns: None, name: Rc::from(s) };
    let result_pairs: Vec<(Value, Value)> = vec![
        (kw("handler"), Value::Handler(Rc::new(wrapped_handler_def))),
        (kw("calls"),   calls_atom),
    ];

    Ok(EvalReturn::Value(Value::Map(Rc::new(result_pairs.into()))))
}

/// Evaluate a `(submodule test name body...)` form (spec §8).
///
/// In test mode (`nexl_stdlib::test::is_test_mode()`), evaluates the body
/// in the current environment (giving access to all enclosing definitions,
/// including private ones). In non-test mode, the entire block is skipped.
fn eval_submodule<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    // Syntax: (submodule test <name> body...)
    if items.len() < 3 {
        return Err(EvalError::NativeError(
            "submodule requires: (submodule test <name> body...)".into(),
        ));
    }

    // Second element must be the atom `test`
    match &items[1].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "test" => {}
        _ => {
            return Err(EvalError::NativeError(
                "only `submodule test` is supported".into(),
            ));
        }
    }

    // In non-test mode, skip the entire block
    if !nexl_stdlib::test::is_test_mode() {
        return Ok(EvalReturn::Value(Value::Unit));
    }

    // Evaluate body forms in current env (same scope as enclosing module)
    // Items[2] is the name, Items[3..] is the body
    let body_start = if items.len() > 3 { 3 } else { 2 };
    let body = &items[body_start..];
    let mut result = Value::Unit;
    for node in body {
        match eval_with_loop(node, env, loop_state)? {
            EvalReturn::Value(v) => result = v,
            recur @ EvalReturn::Recur(_) => return Ok(recur),
        }
    }

    Ok(EvalReturn::Value(result))
}

/// Evaluate a `(check [name gen ...] opts... body...)` property test form (spec §12.1).
///
/// Runs the body with randomly generated values for each binding, `num-tests`
/// times (default 100). On failure, reports the seed and the failing values,
/// then attempts basic shrinking by re-trying with seed/2.
///
/// Options (keyword pairs before the body):
/// - `:num-tests N` — number of trials (default 100)
/// - `:seed N` — initial seed (default 42)
fn eval_check<'a>(
    items: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    // Syntax: (check [name gen name gen ...] opts... body...)
    if items.len() < 3 {
        return Err(EvalError::NativeError(
            "check requires: (check [name gen ...] body...)".into(),
        ));
    }

    // Parse bindings vector
    let bindings = match &items[1].kind {
        NodeKind::Vector(elems) => elems,
        _ => {
            return Err(EvalError::NativeError(
                "check: first argument must be a binding vector [name gen ...]".into(),
            ));
        }
    };
    if bindings.len() % 2 != 0 {
        return Err(EvalError::NativeError(
            "check: binding vector must have an even number of elements".into(),
        ));
    }

    // Collect (name, generator_value) pairs
    let mut binding_pairs: Vec<(Rc<str>, Value)> = Vec::new();
    let mut i = 0;
    while i < bindings.len() {
        let name = match &bindings[i].kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => Rc::from(name.as_str()),
            _ => {
                return Err(EvalError::NativeError(
                    "check: binding names must be symbols".into(),
                ));
            }
        };
        let generator = eval(&bindings[i + 1], env)?;
        binding_pairs.push((name, generator));
        i += 2;
    }

    // Parse optional keyword options and find body start
    let mut num_tests: i64 = 100;
    let mut seed: i64 = 42;
    let mut body_start = 2;
    let mut opt_idx = 2;
    while opt_idx < items.len() {
        match &items[opt_idx].kind {
            NodeKind::Atom(Atom::Keyword { ns: None, name }) if name == "num-tests" => {
                if opt_idx + 1 >= items.len() {
                    return Err(EvalError::NativeError("check: :num-tests requires a value".into()));
                }
                match eval(&items[opt_idx + 1], env)? {
                    Value::Int(n) => num_tests = n,
                    _ => return Err(EvalError::NativeError("check: :num-tests must be an Int".into())),
                }
                opt_idx += 2;
                body_start = opt_idx;
            }
            NodeKind::Atom(Atom::Keyword { ns: None, name }) if name == "seed" => {
                if opt_idx + 1 >= items.len() {
                    return Err(EvalError::NativeError("check: :seed requires a value".into()));
                }
                match eval(&items[opt_idx + 1], env)? {
                    Value::Int(n) => seed = n,
                    _ => return Err(EvalError::NativeError("check: :seed must be an Int".into())),
                }
                opt_idx += 2;
                body_start = opt_idx;
            }
            _ => break,
        }
    }

    let body = &items[body_start..];
    if body.is_empty() {
        return Err(EvalError::NativeError("check: requires a body".into()));
    }

    // Run the property num_tests times
    let mut cur_seed = seed;
    for trial in 0..num_tests {
        cur_seed = nexl_stdlib::gen_mod::lcg_next(cur_seed);
        let trial_env = Rc::new(Env::child(Rc::clone(env)));

        // Generate values for each binding
        let mut gen_seed = cur_seed;
        for (name, generator) in &binding_pairs {
            gen_seed = nexl_stdlib::gen_mod::lcg_next(gen_seed);
            let value = nexl_runtime::call_value(generator, &[Value::Int(gen_seed)])
                .map_err(|e| EvalError::NativeError(format!("check: generator error: {e}")))?;
            trial_env.define(Rc::clone(name), value);
        }

        // Run body assertions
        let body_result: Result<(), EvalError> = (|| {
            for node in body {
                eval_with_loop(node, &trial_env, loop_state)?;
            }
            Ok(())
        })();

        if let Err(e) = body_result {
            // Property falsified — attempt basic shrinking
            let shrunk = shrink_check(&binding_pairs, cur_seed, body, env, loop_state);
            let failing_vals: Vec<String> = binding_pairs
                .iter()
                .map(|(name, _)| {
                    let v = trial_env.get(name).unwrap_or(Value::Unit);
                    format!("{name} = {v}")
                })
                .collect();
            let shrunk_msg = match shrunk {
                Some(msg) => format!("\n  Shrunk to: {msg}"),
                None => String::new(),
            };
            return Err(EvalError::NativeError(format!(
                "check: property falsified after {} tests.\n  Failing input: {}{}\n  Error: {e}",
                trial + 1,
                failing_vals.join(", "),
                shrunk_msg,
            )));
        }
    }

    Ok(EvalReturn::Value(Value::Unit))
}

/// Try to find a simpler failing input by bisecting the seed.
///
/// Returns a description of the smaller failing case, or `None` if shrinking
/// didn't find a smaller failure.
fn shrink_check<'a>(
    binding_pairs: &[(Rc<str>, Value)],
    failing_seed: i64,
    body: &[Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Option<String> {
    let candidate_seeds: Vec<i64> = vec![failing_seed / 2, failing_seed / 4, 0, 1, -1];
    for s in candidate_seeds {
        let trial_env = Rc::new(Env::child(Rc::clone(env)));
        let mut gen_seed = s;
        for (name, generator) in binding_pairs {
            gen_seed = nexl_stdlib::gen_mod::lcg_next(gen_seed);
            if let Ok(value) = nexl_runtime::call_value(generator, &[Value::Int(gen_seed)]) {
                trial_env.define(Rc::clone(name), value);
            }
        }
        let still_fails = body
            .iter()
            .any(|node| eval_with_loop(node, &trial_env, loop_state).is_err());
        if still_fails {
            let vals: Vec<String> = binding_pairs
                .iter()
                .map(|(name, _)| {
                    let v = trial_env.get(name).unwrap_or(Value::Unit);
                    format!("{name} = {v}")
                })
                .collect();
            return Some(vals.join(", "));
        }
    }
    None
}

/// `(bench "name" body)` or `(bench "name" {:iterations N :warmup N} body...)` — benchmark form.
///
/// In bench mode (set by `nexl bench`): registers the body as a benchmark thunk.
/// Outside bench mode: evaluates the body and returns Unit (no-op for benchmarking).
fn eval_bench<'a>(
    items: &'a [Node],
    env: &Rc<Env>,
    loop_state: Option<&'a LoopFrame<'a>>,
) -> Result<EvalReturn, EvalError> {
    if items.len() < 3 {
        return Err(EvalError::NativeError(
            "`bench` requires at least 2 arguments: name and body".to_string(),
        ));
    }

    // Parse name (2nd item, index 1)
    let name = match &items[1].kind {
        NodeKind::Atom(Atom::Str(s)) => s.clone(),
        other => {
            return Err(EvalError::NativeError(format!(
                "`bench` name must be a string, got {other:?}"
            )))
        }
    };

    // Parse optional config map and determine body start
    let (warmup, iterations, body_start) = match &items[2].kind {
        NodeKind::Map(pairs) => {
            let mut warmup = 10usize;
            let mut iterations = 100usize;
            for (k, v) in pairs {
                let key = match &k.kind {
                    NodeKind::Atom(Atom::Keyword { name, .. }) => name.as_str(),
                    _ => continue,
                };
                let val = match eval_with_loop(v, env, loop_state)? {
                    EvalReturn::Value(Value::Int(n)) => n as usize,
                    _ => continue,
                };
                match key {
                    "warmup" => warmup = val,
                    "iterations" => iterations = val,
                    _ => {}
                }
            }
            (warmup, iterations, 3)
        }
        _ => (10, 100, 2),
    };

    let body_nodes = &items[body_start..];
    if body_nodes.is_empty() {
        return Err(EvalError::NativeError("`bench` requires a body".to_string()));
    }

    if nexl_stdlib::test::is_bench_mode() {
        // Register as benchmark thunk
        let body_nodes = body_nodes.to_vec();
        let env_clone = Rc::clone(env);
        let thunk = Value::NativeClosure {
            name: Rc::from(name.as_str()),
            f: Rc::new(move |_| {
                for node in &body_nodes {
                    eval(node, &env_clone).map_err(|e| format!("{e}"))?;
                }
                Ok(Value::Unit)
            }),
        };
        nexl_stdlib::test::bench_registry_push(name, thunk, warmup, iterations);
        Ok(EvalReturn::Value(Value::Unit))
    } else {
        // No-op: evaluate body and discard result
        let mut last = EvalReturn::Value(Value::Unit);
        for node in body_nodes {
            last = eval_with_loop(node, env, loop_state)?;
        }
        let _ = last;
        Ok(EvalReturn::Value(Value::Unit))
    }
}
