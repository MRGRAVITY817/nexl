use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use nexl_runtime::Value;
use thiserror::Error;

pub mod eval;

/// Lexical environment: a frame of bindings plus an optional parent link.
#[derive(Debug)]
pub struct Env {
    parent: Option<Rc<Env>>,
    bindings: RefCell<HashMap<Rc<str>, Value>>,
}

impl Env {
    /// Create a new root environment.
    pub fn new() -> Self {
        Self {
            parent: None,
            bindings: RefCell::new(HashMap::new()),
        }
    }

    /// Create a child environment that chains to `parent`.
    pub fn child(parent: Rc<Env>) -> Self {
        Self {
            parent: Some(parent),
            bindings: RefCell::new(HashMap::new()),
        }
    }

    /// Define or overwrite a binding in the current frame.
    pub fn define(&self, name: impl Into<Rc<str>>, value: Value) {
        self.bindings.borrow_mut().insert(name.into(), value);
    }

    /// Look up a binding, searching parents if needed.
    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(v) = self.bindings.borrow().get(name) {
            return Some(v.clone());
        }
        match &self.parent {
            Some(parent) => parent.get(name),
            None => None,
        }
    }

    /// Mutate an existing binding in the nearest frame where it appears.
    pub fn set(&self, name: &str, value: Value) -> Result<(), EnvError> {
        if let Some(slot) = self.bindings.borrow_mut().get_mut(name) {
            *slot = value;
            return Ok(());
        }
        if let Some(parent) = &self.parent {
            return parent.set(name, value);
        }
        Err(EnvError::Unbound(name.to_string()))
    }
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors produced by environment operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EnvError {
    /// Attempted to set an unbound name.
    #[error("unbound name: {0}")]
    Unbound(String),
}

/// Errors produced while evaluating a node.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum EvalError {
    /// Referenced an unbound symbol.
    #[error("unbound symbol: {0}")]
    UnboundSymbol(String),
    /// Unsupported feature placeholder.
    #[error("unsupported qualified symbol: {0}")]
    UnsupportedQualifiedSymbol(String),
    /// `def`/`let` target was not a symbol.
    #[error("invalid binding target")]
    InvalidBindingTarget,
    /// Wrong arity for a special form.
    #[error("wrong number of arguments")]
    Arity,
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta::{Atom, Node, NodeKind};
    use crate::eval::eval;

    fn int(n: i64) -> Value {
        Value::Int(n)
    }

    fn lit(atom: Atom) -> Node {
        Node { kind: NodeKind::Atom(atom), span: meta::span::Span::synthetic(), leading_comments: vec![], trailing_comment: None }
    }

    // --- eval atom tests ---

    #[test]
    fn eval_int_literal() {
        let env = Env::new();
        let node = lit(Atom::Int { value: 42, suffix: None });
        let v = eval(&node, &env).unwrap();
        assert_eq!(v, Value::Int(42));
    }

    #[test]
    fn eval_float_literal() {
        let env = Env::new();
        let node = lit(Atom::Float { value: 2.5, suffix: None });
        let v = eval(&node, &env).unwrap();
        assert_eq!(v, Value::Float(2.5));
    }

    #[test]
    fn eval_ratio_literal_simplified() {
        let env = Env::new();
        let node = lit(Atom::Ratio { numer: 1, denom: 3 });
        let v = eval(&node, &env).unwrap();
        assert_eq!(v, Value::Ratio(1, 3));
    }

    #[test]
    fn eval_bool_true_false() {
        let env = Env::new();
        let t = lit(Atom::Bool(true));
        let f = lit(Atom::Bool(false));
        assert_eq!(eval(&t, &env).unwrap(), Value::Bool(true));
        assert_eq!(eval(&f, &env).unwrap(), Value::Bool(false));
    }

    #[test]
    fn eval_char_literal() {
        let env = Env::new();
        let node = lit(Atom::Char('a'));
        assert_eq!(eval(&node, &env).unwrap(), Value::Char('a'));
    }

    #[test]
    fn eval_str_literal() {
        let env = Env::new();
        let node = lit(Atom::Str("hello".to_string()));
        assert_eq!(eval(&node, &env).unwrap(), Value::Str(Rc::from("hello")));
    }

    #[test]
    fn eval_unit_literal() {
        let env = Env::new();
        let node = lit(Atom::Unit);
        assert_eq!(eval(&node, &env).unwrap(), Value::Unit);
    }

    #[test]
    fn eval_keyword_literal_bare() {
        let env = Env::new();
        let node = lit(Atom::Keyword { ns: None, name: "foo".to_string() });
        let v = eval(&node, &env).unwrap();
        assert_eq!(v, Value::Keyword { ns: None, name: Rc::from("foo") });
    }

    #[test]
    fn eval_keyword_literal_ns() {
        let env = Env::new();
        let node = lit(Atom::Keyword { ns: Some("http".to_string()), name: "ok".to_string() });
        let v = eval(&node, &env).unwrap();
        assert_eq!(v, Value::Keyword { ns: Some(Rc::from("http")), name: Rc::from("ok") });
    }

    #[test]
    fn eval_symbol_lookup_local() {
        let env = Env::new();
        env.define("x", int(7));
        let node = lit(Atom::Symbol { ns: None, name: "x".to_string() });
        assert_eq!(eval(&node, &env).unwrap(), Value::Int(7));
    }

    #[test]
    fn eval_symbol_lookup_parent() {
        let parent = Rc::new(Env::new());
        parent.define("x", int(9));
        let child = Env::child(parent.clone());
        let node = lit(Atom::Symbol { ns: None, name: "x".to_string() });
        assert_eq!(eval(&node, &child).unwrap(), Value::Int(9));
    }

    #[test]
    fn eval_symbol_unbound_error() {
        let env = Env::new();
        let node = lit(Atom::Symbol { ns: None, name: "missing".to_string() });
        let err = eval(&node, &env).unwrap_err();
        assert_eq!(err, EvalError::UnboundSymbol("missing".into()));
    }

    #[test]
    fn eval_does_not_mutate_env_on_literal() {
        let env = Env::new();
        let before = env.get("x");
        let node = lit(Atom::Int { value: 1, suffix: None });
        let _ = eval(&node, &env).unwrap();
        assert_eq!(env.get("x"), before);
    }

    #[test]
    fn eval_preserves_ratio_signs() {
        let env = Env::new();
        let node = lit(Atom::Ratio { numer: -1, denom: 4 });
        assert_eq!(eval(&node, &env).unwrap(), Value::Ratio(-1, 4));
    }

    // --- def form tests ---

    fn list(items: Vec<Node>) -> Node {
        Node { kind: NodeKind::List(items), span: meta::span::Span::synthetic(), leading_comments: vec![], trailing_comment: None }
    }

    #[test]
    fn def_binds_in_current_env() {
        let env = Env::new();
        let expr = list(vec![
            lit(Atom::Symbol { ns: None, name: "def".into() }),
            lit(Atom::Symbol { ns: None, name: "x".into() }),
            lit(Atom::Int { value: 3, suffix: None }),
        ]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Unit);
        assert_eq!(env.get("x"), Some(Value::Int(3)));
    }

    #[test]
    fn def_overwrites_existing_local() {
        let env = Env::new();
        env.define("x", Value::Int(1));
        let expr = list(vec![
            lit(Atom::Symbol { ns: None, name: "def".into() }),
            lit(Atom::Symbol { ns: None, name: "x".into() }),
            lit(Atom::Int { value: 5, suffix: None }),
        ]);
        eval(&expr, &env).unwrap();
        assert_eq!(env.get("x"), Some(Value::Int(5)));
    }

    #[test]
    fn def_does_not_touch_parent() {
        let parent = Rc::new(Env::new());
        parent.define("x", Value::Int(1));
        let child = Env::child(parent.clone());

        let expr = list(vec![
            lit(Atom::Symbol { ns: None, name: "def".into() }),
            lit(Atom::Symbol { ns: None, name: "x".into() }),
            lit(Atom::Int { value: 7, suffix: None }),
        ]);
        eval(&expr, &child).unwrap();

        assert_eq!(child.get("x"), Some(Value::Int(7)));
        assert_eq!(parent.get("x"), Some(Value::Int(1)));
    }

    #[test]
    fn def_returns_unit() {
        let env = Env::new();
        let expr = list(vec![
            lit(Atom::Symbol { ns: None, name: "def".into() }),
            lit(Atom::Symbol { ns: None, name: "x".into() }),
            lit(Atom::Int { value: 1, suffix: None }),
        ]);
        let v = eval(&expr, &env).unwrap();
        assert_eq!(v, Value::Unit);
    }

    #[test]
    fn def_eval_order_value_first() {
        let env = Env::new();
        env.define("y", Value::Int(2));
        let expr = list(vec![
            lit(Atom::Symbol { ns: None, name: "def".into() }),
            lit(Atom::Symbol { ns: None, name: "x".into() }),
            lit(Atom::Symbol { ns: None, name: "y".into() }),
        ]);
        let _ = eval(&expr, &env).unwrap();
        assert_eq!(env.get("x"), Some(Value::Int(2)));
    }

    #[test]
    fn def_error_on_symbol_arity() {
        let env = Env::new();
        let expr = list(vec![lit(Atom::Symbol { ns: None, name: "def".into() })]);
        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
        assert_eq!(env.get("x"), None);
    }

    #[test]
    fn def_error_on_non_symbol_name() {
        let env = Env::new();
        let expr = list(vec![
            lit(Atom::Symbol { ns: None, name: "def".into() }),
            lit(Atom::Int { value: 1, suffix: None }),
            lit(Atom::Int { value: 2, suffix: None }),
        ]);
        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::InvalidBindingTarget);
        assert_eq!(env.get("x"), None);
    }

    #[test]
    fn def_error_on_namespace_symbol() {
        let env = Env::new();
        let expr = list(vec![
            lit(Atom::Symbol { ns: None, name: "def".into() }),
            lit(Atom::Symbol { ns: Some("ns".into()), name: "x".into() }),
            lit(Atom::Int { value: 1, suffix: None }),
        ]);
        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::InvalidBindingTarget);
        assert_eq!(env.get("x"), None);
    }

    #[test]
    fn lookup_local_binding() {
        let env = Env::new();
        env.define("x", int(1));
        assert_eq!(env.get("x"), Some(int(1)));
    }

    #[test]
    fn lookup_parent_binding() {
        let parent = Rc::new(Env::new());
        parent.define("x", int(2));

        let child = Env::child(parent.clone());
        assert_eq!(child.get("x"), Some(int(2)));
    }

    #[test]
    fn shadowing_prefers_local() {
        let parent = Rc::new(Env::new());
        parent.define("x", int(2));

        let child = Env::child(parent.clone());
        child.define("x", int(5));

        assert_eq!(child.get("x"), Some(int(5)));
        assert_eq!(parent.get("x"), Some(int(2)));
    }

    #[test]
    fn set_updates_local() {
        let env = Env::new();
        env.define("x", int(1));

        env.set("x", int(3)).unwrap();
        assert_eq!(env.get("x"), Some(int(3)));
    }

    #[test]
    fn set_updates_parent() {
        let parent = Rc::new(Env::new());
        parent.define("x", int(1));

        let child = Env::child(parent.clone());
        child.set("x", int(9)).unwrap();

        assert_eq!(parent.get("x"), Some(int(9)));
        assert_eq!(child.get("x"), Some(int(9)));
    }

    #[test]
    fn set_errors_unbound() {
        let env = Env::new();
        let err = env.set("missing", int(1)).unwrap_err();
        assert_eq!(err, EnvError::Unbound("missing".to_string()));
    }

    #[test]
    fn define_overwrites_local() {
        let env = Env::new();
        env.define("x", int(1));
        env.define("x", int(2));

        assert_eq!(env.get("x"), Some(int(2)));
    }

    #[test]
    fn captures_are_independent() {
        let parent = Rc::new(Env::new());
        parent.define("p", int(1));

        let child = Env::child(parent.clone());
        child.define("c", int(2));

        assert_eq!(parent.get("p"), Some(int(1)));
        assert_eq!(child.get("p"), Some(int(1)));
        assert_eq!(child.get("c"), Some(int(2)));
        assert_eq!(parent.get("c"), None);
    }
}
