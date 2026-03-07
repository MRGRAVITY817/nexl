use std::cell::Cell;
use std::rc::Rc;

use meta::{
    Atom, HandledEffect, HandledOp, Node, NodeKind, Pattern, TryCatchForm,
    parse_defhandler_decl, parse_handle_form, parse_pattern, parse_try_form,
};
use nexl_runtime::{BuiltHandlerEffect, Value, value::Function, value::HandlerDef};

use crate::{Env, EvalError};

/// Maximum Nexl call-stack depth before returning a `StackOverflow` error.
const MAX_CALL_DEPTH: usize = 10_000;

thread_local! {
    /// Tracks the current Nexl call depth to prevent unbounded recursion.
    static CALL_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// RAII guard that increments the call depth on construction and decrements on drop.
struct CallDepthGuard;

impl CallDepthGuard {
    /// Try to enter one call level. Returns `Err(StackOverflow)` if the limit is reached.
    fn enter() -> Result<Self, EvalError> {
        let depth = CALL_DEPTH.with(|d| d.get());
        if depth >= MAX_CALL_DEPTH {
            return Err(EvalError::StackOverflow);
        }
        CALL_DEPTH.with(|d| d.set(depth + 1));
        Ok(CallDepthGuard)
    }
}

impl Drop for CallDepthGuard {
    fn drop(&mut self) {
        CALL_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

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
        // is, throws?, is-match, setup/teardown, describe, deftest — deleted in M27 Phase 2-5 (now macros in test.nx)
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "call-log" => {
            eval_call_log(items, env)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "submodule" => {
            eval_submodule(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "load" => {
            eval_load(items, env)
        }
        // check, snap-file! — deleted in M27 Phase 6-7 (now defn/macro in test.nx)
        // bench — deleted in M27 Phase 3 (now a macro in test.nx)
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

// eval_is + diff_hint deleted in M27 Phase 5 — is is now a defmacro-syntax in test.nx

// eval_deftest deleted in M27 Phase 4 — now a defmacro-syntax in test.nx

// eval_describe deleted in M27 Phase 3 — now a defmacro-syntax in test.nx

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
        module_frame: env.get_module_frame(),
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

/// Parse `>>> expr` / expected pairs from a docstring (spec §14.2).
///
/// Returns a `Vec<(input_src, expected_src)>` for each `>>> expr` line followed
/// by a non-blank expected line.
pub(crate) fn parse_doctests(docstring: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut lines = docstring.lines().peekable();
    while let Some(line) = lines.next() {
        if let Some(expr) = line.trim().strip_prefix(">>> ") {
            // Skip blank lines before expected output
            let mut expected_opt = None;
            for next_line in lines.by_ref() {
                let t = next_line.trim();
                if t.is_empty() {
                    continue;
                }
                if !t.starts_with(">>>") {
                    expected_opt = Some(t.to_string());
                }
                break;
            }
            if let Some(expected) = expected_opt {
                pairs.push((expr.to_string(), expected));
            }
        }
    }
    pairs
}

/// Register doctest examples extracted from a `defn` docstring (spec §14.2).
///
/// Each `>>> expr` / expected pair becomes a test named `"<fn-name> doctest N"`.
/// The thunk evaluates `expr` in `env` (which will have `fn-name` bound by runtime)
/// and compares the result to the evaluated expected value.
fn register_doctest_examples(fn_name: &str, docstring: &str, env: &Rc<Env>) {
    let pairs = parse_doctests(docstring);
    for (idx, (input_src, expected_src)) in pairs.into_iter().enumerate() {
        let test_name = format!("{fn_name} doctest {}", idx + 1);
        let env_clone = Rc::clone(env);
        let name_clone = test_name.clone();
        let thunk = Value::NativeClosure {
            name: Rc::from(test_name.as_str()),
            f: Rc::new(move |_| {
                let in_nodes = nexl_reader::read(&input_src, meta::FileId::SYNTHETIC)
                    .map_err(|e| format!("doctest parse error in `{name_clone}`: {e:?}"))?;
                let in_node = in_nodes
                    .into_iter()
                    .last()
                    .ok_or_else(|| format!("doctest `{name_clone}`: empty input"))?;
                let actual = eval(&in_node, &env_clone)
                    .map_err(|e| format!("doctest eval error in `{name_clone}`: {e}"))?;

                let exp_nodes = nexl_reader::read(&expected_src, meta::FileId::SYNTHETIC)
                    .map_err(|e| format!("doctest expected parse error in `{name_clone}`: {e:?}"))?;
                let exp_node = exp_nodes
                    .into_iter()
                    .last()
                    .ok_or_else(|| format!("doctest `{name_clone}`: empty expected"))?;
                let expected = eval(&exp_node, &env_clone)
                    .map_err(|e| format!("doctest expected eval error in `{name_clone}`: {e}"))?;

                if actual == expected {
                    Ok(Value::Unit)
                } else {
                    Err(format!(
                        "doctest `{name_clone}` failed.\n  input:    {input_src}\n  expected: {expected}\n  actual:   {actual}"
                    ))
                }
            }),
        };
        nexl_stdlib::test::registry_push(test_name, thunk);
    }
}

fn eval_defn(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    if items.len() < 4 {
        return Err(EvalError::Arity);
    }

    let name_node = &items[1];
    let (ns_opt, name) = match &name_node.kind {
        NodeKind::Atom(Atom::Symbol { ns, name }) => (ns.clone(), name.clone()),
        _ => return Err(EvalError::InvalidBindingTarget),
    };
    let debug_name: Rc<str> = match &ns_opt {
        None => Rc::from(name.as_str()),
        Some(ns) => Rc::from(format!("{ns}/{name}").as_str()),
    };

    // Optional docstring at position 2 when it's a Str literal
    let (params_idx, body_start, docstring) = match &items[2].kind {
        NodeKind::Atom(Atom::Str(s)) => (3, 4, Some(s.clone())),
        _ => (2, 3, None),
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
            name: Some(Rc::clone(&debug_name)),
            params: f.params.clone(),
            rest: f.rest.clone(),
            arity: f.arity,
            variadic: f.variadic,
            captures: f.captures.clone(),
            module_captures: f.module_captures.clone(),
            module_frame: f.module_frame.clone(),
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
    // In test mode, register docstring `>>>` examples (spec §14.2).
    if nexl_stdlib::test::is_test_mode()
        && let Some(ref doc) = docstring
    {
        register_doctest_examples(&name, doc, env);
    }

    match ns_opt {
        None => env.define(name, fn_value_named),
        Some(ns) => env.add_to_module_alias(&ns, name.as_str(), fn_value_named),
    }
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

    // Guard against unbounded recursion before allocating the call environment.
    let _depth_guard = CallDepthGuard::enter()?;

    let required = func.arity as usize;
    let provided = items.len() - 1;

    if (!func.variadic && provided != required) || (func.variadic && provided < required) {
        return Err(EvalError::Arity);
    }

    let call_env = Rc::new(Env::new());

    // Seed from module frame first (lowest priority): gives access to all
    // module-level siblings, enabling mutual recursion between top-level defns.
    if let Some(frame) = &func.module_frame {
        for (name, value) in frame.borrow().iter() {
            call_env.define(name.clone(), value.clone());
        }
    }

    // load captures (override module frame — captures are more specific)
    for (name, value) in &func.captures {
        call_env.define(name.clone(), value.clone());
    }
    for (alias, exports) in &func.module_captures {
        call_env.define_module_alias(alias.clone(), Rc::clone(exports));
    }

    // Named functions can call themselves: bind the function under its own name
    // so recursive calls resolve correctly regardless of capture-snapshot timing.
    // For qualified names like "iter/from-vec", also register in the module alias
    // so inner closures can resolve the qualified symbol (iter/from-vec) via
    // get_qualified(), which only looks in module aliases, not plain bindings.
    if let Some(self_name) = &func.name {
        call_env.define(self_name.clone(), Value::Function(Rc::clone(&func)));
        if let Some(slash) = self_name.find('/') {
            let alias = &self_name[..slash];
            let name = &self_name[slash + 1..];
            call_env.add_to_module_alias(alias, name, Value::Function(Rc::clone(&func)));
        }
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
    for expr in func.body.iter() {
        match eval_with_loop(expr, &call_env, loop_state) {
            Ok(EvalReturn::Value(v)) => { last = v; }
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

    // Guard against unbounded recursion before allocating the call environment.
    let _depth_guard = CallDepthGuard::enter()?;

    let required = func.arity as usize;
    let provided = args.len();

    if (!func.variadic && provided != required) || (func.variadic && provided < required) {
        return Err(EvalError::Arity);
    }

    let call_env = Rc::new(Env::new());

    // Seed from module frame (lowest priority) for mutual recursion.
    if let Some(frame) = &func.module_frame {
        for (name, value) in frame.borrow().iter() {
            call_env.define(name.clone(), value.clone());
        }
    }

    for (name, value) in &func.captures {
        call_env.define(name.clone(), value.clone());
    }
    for (alias, exports) in &func.module_captures {
        call_env.define_module_alias(alias.clone(), Rc::clone(exports));
    }

    // Named functions can call themselves: bind the function under its own name.
    // Also register in module alias for qualified self-references (e.g. iter/from-vec).
    if let Some(self_name) = &func.name {
        call_env.define(self_name.clone(), callee.clone());
        if let Some(slash) = self_name.find('/') {
            let alias = &self_name[..slash];
            let name = &self_name[slash + 1..];
            call_env.add_to_module_alias(alias, name, callee.clone());
        }
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

// eval_check, shrink_check, eval_snap_file deleted in M27 Phase 6-7 — now defn/macro in test.nx
// eval_bench deleted in M27 Phase 3 — now a defmacro-syntax in test.nx

/// Evaluate `(load "path/to/file.nx")` — read, parse, macro-expand, and evaluate
/// a Nexl source file in the current environment.
///
/// This is the classic Lisp `load` primitive. Definitions (`def`, `defn`, etc.)
/// in the loaded file become available in the caller's environment, making it
/// useful for test files that need to load source modules.
fn eval_load(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
    if items.len() != 2 {
        return Err(EvalError::NativeError(
            "`load` requires exactly 1 argument (a file path string)".into(),
        ));
    }
    let path_val = eval(&items[1], env)?;
    let path_str = match &path_val {
        Value::Str(s) => s.clone(),
        _ => {
            return Err(EvalError::NativeError(
                "`load` argument must be a string".into(),
            ))
        }
    };

    let source = std::fs::read_to_string(path_str.as_ref()).map_err(|e| {
        EvalError::NativeError(format!("load: cannot read {:?}: {e}", path_str.as_ref()))
    })?;

    let nodes = nexl_reader::read(&source, meta::FileId::SYNTHETIC).map_err(|diag| {
        EvalError::NativeError(format!("load: parse error in {:?}: {diag}", path_str.as_ref()))
    })?;

    let mut expander = nexl_macros::Expander::new();
    let expanded = expander.expand_forms(&nodes).map_err(|e| {
        EvalError::NativeError(format!("load: macro error in {:?}: {e}", path_str.as_ref()))
    })?;

    for node in &expanded {
        eval(node, env)?;
    }

    Ok(EvalReturn::Value(Value::Unit))
}
