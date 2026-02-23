use std::rc::Rc;

use meta::{Atom, Node, NodeKind};
use nexl_runtime::{value::Function, Value};

use crate::{Env, EvalError};

/// Evaluate a Nexl AST node within the given environment.
pub fn eval(node: &Node, env: &Rc<Env>) -> Result<Value, EvalError> {
    match &node.kind {
        NodeKind::Atom(atom) => eval_atom(atom, env),
        NodeKind::List(items) => eval_list(items, env),
        _ => todo!("non-atom evaluation not yet implemented"),
    }
}

fn eval_atom(atom: &Atom, env: &Rc<Env>) -> Result<Value, EvalError> {
    match atom {
        Atom::Int { value, .. } => Ok(Value::Int(*value as i64)),
        Atom::Float { value, .. } => Ok(Value::Float(*value)),
        Atom::Ratio { numer, denom } => Ok(Value::Ratio(*numer, *denom)),
        Atom::Bool(b) => Ok(Value::Bool(*b)),
        Atom::Char(c) => Ok(Value::Char(*c)),
        Atom::Str(s) => Ok(Value::Str(Rc::from(s.as_str()))),
        Atom::Unit => Ok(Value::Unit),
        Atom::Keyword { ns, name } => Ok(Value::Keyword {
            ns: ns.as_ref().map(|s| Rc::from(s.as_str())),
            name: Rc::from(name.as_str()),
        }),
        Atom::Symbol { ns: None, name } => env
            .get(name)
            .ok_or_else(|| EvalError::UnboundSymbol(name.clone())),
        Atom::Symbol { ns: Some(_), name } => Err(EvalError::UnsupportedQualifiedSymbol(name.clone())),
    }
}

fn eval_list(items: &[Node], env: &Rc<Env>) -> Result<Value, EvalError> {
    if items.is_empty() {
        return Err(EvalError::Arity);
    }
    let head = &items[0];
    match &head.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "def" => eval_def(items, env),
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "let" => eval_let(items, env),
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "do" => eval_do(items, env),
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "if" => eval_if(items, env),
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "fn" => eval_fn(items, env),
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "defn" => eval_defn(items, env),
        NodeKind::Atom(Atom::Symbol { ns: Some(_), name }) => Err(EvalError::UnsupportedQualifiedSymbol(name.clone())),
        _ => todo!("function application not yet implemented"),
    }
}

fn eval_def(items: &[Node], env: &Rc<Env>) -> Result<Value, EvalError> {
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
    Ok(Value::Unit)
}

fn eval_let(items: &[Node], env: &Rc<Env>) -> Result<Value, EvalError> {
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

    let child_env = Rc::new(Env::child(Rc::clone(env)));

    // evaluate bindings sequentially
    for pair in bindings.chunks_exact(2) {
        let (name_node, value_node) = (&pair[0], &pair[1]);
        let name = match &name_node.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
            _ => return Err(EvalError::InvalidBindingTarget),
        };
        let value = eval(value_node, &child_env)?;
        child_env.define(name, value);
    }

    // body expressions
    let mut last = Value::Unit;
    for expr in &items[2..] {
        last = eval(expr, &child_env)?;
    }
    Ok(last)
}

fn eval_do(items: &[Node], env: &Rc<Env>) -> Result<Value, EvalError> {
    if items.len() < 2 {
        return Err(EvalError::Arity);
    }

    let mut last = Value::Unit;
    for expr in &items[1..] {
        last = eval(expr, env)?;
    }
    Ok(last)
}

fn eval_if(items: &[Node], env: &Rc<Env>) -> Result<Value, EvalError> {
    if items.len() != 4 {
        return Err(EvalError::Arity);
    }

    let cond = eval(&items[1], env)?;
    let cond_bool = match cond {
        Value::Bool(b) => b,
        _ => return Err(EvalError::InvalidConditionType),
    };

    if cond_bool {
        eval(&items[2], env)
    } else {
        eval(&items[3], env)
    }
}

fn eval_fn(items: &[Node], env: &Rc<Env>) -> Result<Value, EvalError> {
    if items.len() < 3 {
        return Err(EvalError::Arity);
    }

    let params_node = &items[1];
    let params = match &params_node.kind {
        NodeKind::Vector(items) => items,
        _ => return Err(EvalError::Arity),
    };

    let mut arity: u32 = 0;
    let mut variadic = false;

    let mut iter = params.iter().peekable();
    while let Some(param) = iter.next() {
        match &param.kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "&" => {
                variadic = true;
                let rest = iter.next().ok_or(EvalError::Arity)?;
                match &rest.kind {
                    NodeKind::Atom(Atom::Symbol { ns: None, name: _ }) => { /* ok */ }
                    _ => return Err(EvalError::InvalidBindingTarget),
                }
                if iter.peek().is_some() {
                    return Err(EvalError::Arity);
                }
                break;
            }
            NodeKind::Atom(Atom::Symbol { ns: None, name: _ }) => {
                if variadic {
                    return Err(EvalError::Arity);
                }
                arity += 1;
            }
            _ => return Err(EvalError::InvalidBindingTarget),
        }
    }

    let func = Function {
        name: None,
        arity,
        variadic,
        captures: env.capture_values(),
    };

    Ok(Value::Function(Rc::new(func)))
}

fn eval_defn(items: &[Node], env: &Rc<Env>) -> Result<Value, EvalError> {
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
        kind: NodeKind::Atom(Atom::Symbol { ns: None, name: "fn".into() }),
        span: items[0].span,
        leading_comments: vec![],
        trailing_comment: None,
    });
    fn_items.push(items[params_idx].clone());
    fn_items.extend_from_slice(&items[body_start..]);

    let fn_value = eval_list(&fn_items, env)?;

    let fn_value_named = match fn_value {
        Value::Function(f) => Value::Function(Rc::new(Function {
            name: Some(Rc::from(name.as_str())),
            arity: f.arity,
            variadic: f.variadic,
            captures: f.captures.clone(),
        })),
        _ => fn_value,
    };

    env.define(name, fn_value_named);
    Ok(Value::Unit)
}
