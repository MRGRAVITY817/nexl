use std::rc::Rc;

use meta::{Atom, Node, NodeKind, Pattern, TryCatchForm, parse_pattern, parse_try_form};
use nexl_runtime::{Value, value::Function};

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
            .ok_or_else(|| EvalError::UnboundSymbol(format!("{alias}/{name}")))?,
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
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "and" => {
            eval_and(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "or" => {
            eval_or(items, env, loop_state)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "cond" => {
            eval_cond(items, env, loop_state)
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
    Ok(EvalReturn::Value(Value::Map(Rc::new(values))))
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

    // Parse bindings with optional `mut` modifier: [mut? name value ...]
    let mut i = 0;
    while i < bindings.len() {
        // Consume optional `mut` modifier (noted but not enforced in M1).
        if let NodeKind::Atom(Atom::Symbol { ns: None, name }) = &bindings[i].kind
            && name == "mut"
        {
            i += 1;
        }

        // Binding name.
        let name_node = bindings.get(i).ok_or(EvalError::Arity)?;
        let name = match &name_node.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
            _ => return Err(EvalError::InvalidBindingTarget),
        };
        i += 1;

        // Binding value.
        let value_node = bindings.get(i).ok_or(EvalError::Arity)?;
        i += 1;

        let value = match eval_with_loop(value_node, &child_env, None)? {
            EvalReturn::Value(v) => v,
            EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
        };
        child_env.define(name, value);
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
                    let found = entries.iter().find(|(k, _)| *k == key);
                    match found {
                        Some((_, val)) => {
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
                // :examples are for documentation/testing tools, not dev-mode eval.
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

    let name = match &bindings[0].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => return Err(EvalError::InvalidBindingTarget),
    };

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

    for value in iter_values {
        let iter_env = Rc::new(Env::child(Rc::clone(env)));
        iter_env.define(name.clone(), value);
        for expr in &items[2..] {
            match eval_with_loop(expr, &iter_env, None)? {
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
