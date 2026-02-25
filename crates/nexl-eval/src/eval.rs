use std::rc::Rc;

use meta::{Atom, Node, NodeKind};
use nexl_runtime::{Value, value::Function};

use crate::{Env, EvalError};

#[derive(Debug)]
enum EvalReturn {
    Value(Value),
    Recur(Vec<Value>),
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
        Atom::Symbol { ns: Some(_), name } => {
            return Err(EvalError::UnsupportedQualifiedSymbol(name.clone()));
        }
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
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "let" => eval_let(items, env),
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
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "each" => {
            eval_each(items, env)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "times" => {
            eval_times(items, env)
        }
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "set!" => {
            eval_set_bang(items, env)
        }
        NodeKind::Atom(Atom::Symbol { ns: Some(_), name }) => {
            Err(EvalError::UnsupportedQualifiedSymbol(name.clone()))
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

fn eval_let(items: &[Node], env: &Rc<Env>) -> Result<EvalReturn, EvalError> {
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

    // Body expressions.
    let mut last = Value::Unit;
    for expr in &items[2..] {
        match eval_with_loop(expr, &child_env, None)? {
            EvalReturn::Value(v) => last = v,
            EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
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
        body: items[2..].to_vec(),
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

    // Build an equivalent (fn [params] body...) form
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
    fn_items.extend_from_slice(&items[body_start..]);

    let fn_value = eval_list(&fn_items, env, None)?;

    let fn_value_named = match fn_value {
        EvalReturn::Value(Value::Function(f)) => Value::Function(Rc::new(Function {
            name: Some(Rc::from(name.as_str())),
            params: f.params.clone(),
            rest: f.rest.clone(),
            arity: f.arity,
            variadic: f.variadic,
            captures: f.captures.clone(),
            body: f.body.clone(),
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
    if bindings.len() % 2 != 0 {
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
            _ => {
                return Err(EvalError::NativeError(
                    "unknown Option constructor".into(),
                ))
            }
        },
        other => {
            return Err(EvalError::NativeError(format!(
                "`each` expected Vec, Map, Set, or Option, got {}",
                other.type_name()
            )))
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
            )))
        }
        other => {
            return Err(EvalError::NativeError(format!(
                "`times` expected Int count, got {}",
                other.type_name()
            )))
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
            // collect but we don't yet model vectors; bind Unit placeholder
            call_env.define(rest_name.clone(), Value::Unit);
        }
    } else if provided != required {
        return Err(EvalError::Arity);
    }

    let mut last = Value::Unit;
    for expr in &func.body {
        match eval_with_loop(expr, &call_env, loop_state)? {
            EvalReturn::Value(v) => last = v,
            EvalReturn::Recur(vals) => return Ok(EvalReturn::Recur(vals)),
        }
    }
    Ok(EvalReturn::Value(last))
}

pub(crate) fn apply_value(callee: &Value, args: &[Value]) -> Result<Value, EvalError> {
    if let Value::NativeFunction(native) = callee {
        return (native.f)(args).map_err(EvalError::NativeError);
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

    for (idx, param) in func.params.iter().enumerate() {
        let arg_val = args
            .get(idx)
            .ok_or(EvalError::Arity)?
            .clone();
        call_env.define(param.clone(), arg_val);
    }

    if func.variadic
        && let Some(rest_name) = &func.rest
    {
        call_env.define(rest_name.clone(), Value::Unit);
    }

    let mut last = Value::Unit;
    for expr in &func.body {
        match eval_with_loop(expr, &call_env, None)? {
            EvalReturn::Value(v) => last = v,
            EvalReturn::Recur(_) => return Err(EvalError::InvalidRecur),
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
