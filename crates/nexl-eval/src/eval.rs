use std::rc::Rc;

use meta::{Atom, Node, NodeKind};
use nexl_runtime::Value;

use crate::{Env, EvalError};

/// Evaluate a Nexl AST node within the given environment.
pub fn eval(node: &Node, env: &Env) -> Result<Value, EvalError> {
    match &node.kind {
        NodeKind::Atom(atom) => eval_atom(atom, env),
        NodeKind::List(items) => eval_list(items, env),
        _ => todo!("non-atom evaluation not yet implemented"),
    }
}

fn eval_atom(atom: &Atom, env: &Env) -> Result<Value, EvalError> {
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

fn eval_list(items: &[Node], env: &Env) -> Result<Value, EvalError> {
    if items.is_empty() {
        return Err(EvalError::Arity);
    }
    let head = &items[0];
    match &head.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "def" => eval_def(items, env),
        NodeKind::Atom(Atom::Symbol { ns: Some(_), name }) => Err(EvalError::UnsupportedQualifiedSymbol(name.clone())),
        _ => todo!("function application not yet implemented"),
    }
}

fn eval_def(items: &[Node], env: &Env) -> Result<Value, EvalError> {
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
