use std::rc::Rc;

use meta::{Atom, Node, NodeKind};
use nexl_runtime::Value;

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
