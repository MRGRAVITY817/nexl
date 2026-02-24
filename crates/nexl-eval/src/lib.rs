use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use nexl_runtime::Value;
use thiserror::Error;

pub mod eval;
pub mod repl;
pub mod stdlib;

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

    /// Snapshot the full visible environment for closure capture (nearest wins).
    pub fn capture_closure(&self) -> Vec<(Rc<str>, Value)> {
        let mut map = HashMap::new();
        self.fill_closure_map(&mut map);
        map.into_iter().collect()
    }

    fn fill_closure_map(&self, map: &mut HashMap<Rc<str>, Value>) {
        if let Some(parent) = &self.parent {
            parent.fill_closure_map(map);
        }
        for (k, v) in self.bindings.borrow().iter() {
            map.insert(k.clone(), v.clone());
        }
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
    /// Attempted to call a non-function value.
    #[error("invalid callable")]
    InvalidCallable,
    /// `def`/`let` target was not a symbol.
    #[error("invalid binding target")]
    InvalidBindingTarget,
    /// Wrong arity for a special form.
    #[error("wrong number of arguments")]
    Arity,
    /// Condition to `if` was not Bool.
    #[error("condition must be Bool")]
    InvalidConditionType,
    /// `recur` used outside of a loop.
    #[error("recur outside loop")]
    InvalidRecur,
    /// A native built-in function signalled a runtime error.
    #[error("runtime error: {0}")]
    NativeError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::eval;
    use meta::{Atom, Node, NodeKind};

    fn int(n: i64) -> Value {
        Value::Int(n)
    }

    // --- function application tests ---

    #[test]
    fn apply_invokes_closure_body() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "fn".into(),
                }),
                vector(vec![lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                })]),
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
            ]),
            lit(Atom::Int {
                value: 5,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(5));
    }

    #[test]
    fn apply_passes_multiple_args() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "fn".into(),
                }),
                vector(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "x".into(),
                    }),
                    lit(Atom::Symbol {
                        ns: None,
                        name: "y".into(),
                    }),
                ]),
                lit(Atom::Symbol {
                    ns: None,
                    name: "y".into(),
                }),
            ]),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 3,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(3));
    }

    #[test]
    fn apply_arity_mismatch_too_few() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "fn".into(),
                }),
                vector(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "x".into(),
                    }),
                    lit(Atom::Symbol {
                        ns: None,
                        name: "y".into(),
                    }),
                ]),
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
            ]),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
    }

    #[test]
    fn apply_arity_mismatch_too_many() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "fn".into(),
                }),
                vector(vec![lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                })]),
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
            ]),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
    }

    #[test]
    fn apply_variadic_allows_extra() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "fn".into(),
                }),
                vector(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "x".into(),
                    }),
                    lit(Atom::Symbol {
                        ns: None,
                        name: "&".into(),
                    }),
                    lit(Atom::Symbol {
                        ns: None,
                        name: "r".into(),
                    }),
                ]),
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
            ]),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 3,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(1));
    }

    #[test]
    fn apply_closure_sees_captured() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 7,
                    suffix: None,
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "f".into(),
                }),
                list(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "fn".into(),
                    }),
                    vector(vec![]),
                    lit(Atom::Symbol {
                        ns: None,
                        name: "x".into(),
                    }),
                ]),
            ]),
            list(vec![lit(Atom::Symbol {
                ns: None,
                name: "f".into(),
            })]),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(7));
    }

    #[test]
    fn apply_non_function_head_error() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::InvalidCallable);
    }

    #[test]
    fn apply_head_evaluated_once() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "do".into(),
            }),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "def".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "counter".into(),
                }),
                lit(Atom::Int {
                    value: 0,
                    suffix: None,
                }),
            ]),
            list(vec![list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "do".into(),
                }),
                list(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "def".into(),
                    }),
                    lit(Atom::Symbol {
                        ns: None,
                        name: "counter".into(),
                    }),
                    lit(Atom::Int {
                        value: 1,
                        suffix: None,
                    }),
                ]),
                list(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "fn".into(),
                    }),
                    vector(vec![]),
                    lit(Atom::Symbol {
                        ns: None,
                        name: "counter".into(),
                    }),
                ]),
            ])]),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(1));
        assert_eq!(env.get("counter"), Some(Value::Int(1)));
    }

    // --- loop / recur tests ---

    #[test]
    fn loop_returns_initial_body_when_no_recur() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "loop".into(),
            }),
            vector(vec![]),
            lit(Atom::Int {
                value: 42,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn loop_recur_updates_bindings() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "loop".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "first".into(),
                }),
                lit(Atom::Bool(true)),
            ]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "if".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "first".into(),
                }),
                list(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "recur".into(),
                    }),
                    lit(Atom::Bool(false)),
                ]),
                lit(Atom::Int {
                    value: 99,
                    suffix: None,
                }),
            ]),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(99));
    }

    #[test]
    fn loop_recur_multiple_bindings() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "loop".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "flag".into(),
                }),
                lit(Atom::Bool(true)),
                lit(Atom::Symbol {
                    ns: None,
                    name: "val".into(),
                }),
                lit(Atom::Int {
                    value: 1,
                    suffix: None,
                }),
            ]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "if".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "flag".into(),
                }),
                list(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "recur".into(),
                    }),
                    lit(Atom::Bool(false)),
                    lit(Atom::Int {
                        value: 3,
                        suffix: None,
                    }),
                ]),
                lit(Atom::Symbol {
                    ns: None,
                    name: "val".into(),
                }),
            ]),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(3));
    }

    #[test]
    fn loop_recur_arity_mismatch_errors() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "loop".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 0,
                    suffix: None,
                }),
            ]),
            list(vec![lit(Atom::Symbol {
                ns: None,
                name: "recur".into(),
            })]),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
    }

    #[test]
    fn recur_outside_loop_errors() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "recur".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::InvalidRecur);
    }

    #[test]
    fn loop_bindings_shadow_outer_not_mutate() {
        let env = Rc::new(Env::new());
        env.define("x", Value::Int(1));
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "loop".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 2,
                    suffix: None,
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(2));
        assert_eq!(env.get("x"), Some(Value::Int(1)));
    }

    #[test]
    fn loop_allows_empty_bindings_and_body_runs() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "loop".into(),
            }),
            vector(vec![]),
            lit(Atom::Int {
                value: 7,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(7));
    }

    fn lit(atom: Atom) -> Node {
        Node {
            kind: NodeKind::Atom(atom),
            span: meta::span::Span::synthetic(),
            leading_comments: vec![],
            trailing_comment: None,
        }
    }

    // --- eval atom tests ---

    #[test]
    fn eval_int_literal() {
        let env = Rc::new(Env::new());
        let node = lit(Atom::Int {
            value: 42,
            suffix: None,
        });
        let v = eval(&node, &env).unwrap();
        assert_eq!(v, Value::Int(42));
    }

    #[test]
    fn eval_float_literal() {
        let env = Rc::new(Env::new());
        let node = lit(Atom::Float {
            value: 2.5,
            suffix: None,
        });
        let v = eval(&node, &env).unwrap();
        assert_eq!(v, Value::Float(2.5));
    }

    #[test]
    fn eval_ratio_literal_simplified() {
        let env = Rc::new(Env::new());
        let node = lit(Atom::Ratio { numer: 1, denom: 3 });
        let v = eval(&node, &env).unwrap();
        assert_eq!(v, Value::Ratio(1, 3));
    }

    #[test]
    fn eval_bool_true_false() {
        let env = Rc::new(Env::new());
        let t = lit(Atom::Bool(true));
        let f = lit(Atom::Bool(false));
        assert_eq!(eval(&t, &env).unwrap(), Value::Bool(true));
        assert_eq!(eval(&f, &env).unwrap(), Value::Bool(false));
    }

    #[test]
    fn eval_char_literal() {
        let env = Rc::new(Env::new());
        let node = lit(Atom::Char('a'));
        assert_eq!(eval(&node, &env).unwrap(), Value::Char('a'));
    }

    #[test]
    fn eval_str_literal() {
        let env = Rc::new(Env::new());
        let node = lit(Atom::Str("hello".to_string()));
        assert_eq!(eval(&node, &env).unwrap(), Value::Str(Rc::from("hello")));
    }

    #[test]
    fn eval_unit_literal() {
        let env = Rc::new(Env::new());
        let node = lit(Atom::Unit);
        assert_eq!(eval(&node, &env).unwrap(), Value::Unit);
    }

    #[test]
    fn eval_keyword_literal_bare() {
        let env = Rc::new(Env::new());
        let node = lit(Atom::Keyword {
            ns: None,
            name: "foo".to_string(),
        });
        let v = eval(&node, &env).unwrap();
        assert_eq!(
            v,
            Value::Keyword {
                ns: None,
                name: Rc::from("foo")
            }
        );
    }

    #[test]
    fn eval_keyword_literal_ns() {
        let env = Rc::new(Env::new());
        let node = lit(Atom::Keyword {
            ns: Some("http".to_string()),
            name: "ok".to_string(),
        });
        let v = eval(&node, &env).unwrap();
        assert_eq!(
            v,
            Value::Keyword {
                ns: Some(Rc::from("http")),
                name: Rc::from("ok")
            }
        );
    }

    #[test]
    fn eval_symbol_lookup_local() {
        let env = Rc::new(Env::new());
        env.define("x", int(7));
        let node = lit(Atom::Symbol {
            ns: None,
            name: "x".to_string(),
        });
        assert_eq!(eval(&node, &env).unwrap(), Value::Int(7));
    }

    #[test]
    fn eval_symbol_lookup_parent() {
        let parent = Rc::new(Env::new());
        parent.define("x", int(9));
        let child = Rc::new(Env::child(parent.clone()));
        let node = lit(Atom::Symbol {
            ns: None,
            name: "x".to_string(),
        });
        assert_eq!(eval(&node, &child).unwrap(), Value::Int(9));
    }

    #[test]
    fn eval_symbol_unbound_error() {
        let env = Rc::new(Env::new());
        let node = lit(Atom::Symbol {
            ns: None,
            name: "missing".to_string(),
        });
        let err = eval(&node, &env).unwrap_err();
        assert_eq!(err, EvalError::UnboundSymbol("missing".into()));
    }

    #[test]
    fn eval_does_not_mutate_env_on_literal() {
        let env = Rc::new(Env::new());
        let before = env.get("x");
        let node = lit(Atom::Int {
            value: 1,
            suffix: None,
        });
        let _ = eval(&node, &env).unwrap();
        assert_eq!(env.get("x"), before);
    }

    #[test]
    fn eval_preserves_ratio_signs() {
        let env = Rc::new(Env::new());
        let node = lit(Atom::Ratio {
            numer: -1,
            denom: 4,
        });
        assert_eq!(eval(&node, &env).unwrap(), Value::Ratio(-1, 4));
    }

    // --- def form tests ---

    fn list(items: Vec<Node>) -> Node {
        Node {
            kind: NodeKind::List(items),
            span: meta::span::Span::synthetic(),
            leading_comments: vec![],
            trailing_comment: None,
        }
    }

    fn vector(items: Vec<Node>) -> Node {
        Node {
            kind: NodeKind::Vector(items),
            span: meta::span::Span::synthetic(),
            leading_comments: vec![],
            trailing_comment: None,
        }
    }

    #[test]
    fn def_binds_in_current_env() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "def".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
            lit(Atom::Int {
                value: 3,
                suffix: None,
            }),
        ]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Unit);
        assert_eq!(env.get("x"), Some(Value::Int(3)));
    }

    #[test]
    fn def_overwrites_existing_local() {
        let env = Rc::new(Env::new());
        env.define("x", Value::Int(1));
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "def".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
            lit(Atom::Int {
                value: 5,
                suffix: None,
            }),
        ]);
        eval(&expr, &env).unwrap();
        assert_eq!(env.get("x"), Some(Value::Int(5)));
    }

    #[test]
    fn def_does_not_touch_parent() {
        let parent = Rc::new(Env::new());
        parent.define("x", Value::Int(1));
        let child = Rc::new(Env::child(parent.clone()));

        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "def".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
            lit(Atom::Int {
                value: 7,
                suffix: None,
            }),
        ]);
        eval(&expr, &child).unwrap();

        assert_eq!(child.get("x"), Some(Value::Int(7)));
        assert_eq!(parent.get("x"), Some(Value::Int(1)));
    }

    #[test]
    fn def_returns_unit() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "def".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
        ]);
        let v = eval(&expr, &env).unwrap();
        assert_eq!(v, Value::Unit);
    }

    #[test]
    fn def_eval_order_value_first() {
        let env = Rc::new(Env::new());
        env.define("y", Value::Int(2));
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "def".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "y".into(),
            }),
        ]);
        let _ = eval(&expr, &env).unwrap();
        assert_eq!(env.get("x"), Some(Value::Int(2)));
    }

    // --- let form tests ---

    #[test]
    fn let_returns_last_body_value() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 2,
                    suffix: None,
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "y".into(),
                }),
                lit(Atom::Int {
                    value: 3,
                    suffix: None,
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "y".into(),
            }),
            lit(Atom::Int {
                value: 99,
                suffix: None,
            }),
        ]);

        // Body should evaluate in order; result is last expression
        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(99));
    }

    #[test]
    fn let_bindings_are_sequential() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 2,
                    suffix: None,
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "y".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "y".into(),
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(2));
    }

    #[test]
    fn let_shadows_parent_only_locally() {
        let parent = Rc::new(Env::new());
        parent.define("x", Value::Int(1));

        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 5,
                    suffix: None,
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
        ]);

        let result = eval(&expr, &parent).unwrap();
        assert_eq!(result, Value::Int(5));
        assert_eq!(parent.get("x"), Some(Value::Int(1)));
    }

    #[test]
    fn let_no_leak_to_parent() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 10,
                    suffix: None,
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(10));
        assert_eq!(env.get("x"), None);
    }

    #[test]
    fn let_allows_empty_bindings() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![]),
            lit(Atom::Int {
                value: 7,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(7));
        assert_eq!(env.get("x"), None);
    }

    #[test]
    fn let_errors_on_non_vector_bindings_form() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
    }

    #[test]
    fn let_errors_on_odd_binding_pairs() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 1,
                    suffix: None,
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "y".into(),
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
    }

    #[test]
    fn let_errors_on_non_symbol_binding_target() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Int {
                    value: 1,
                    suffix: None,
                }),
                lit(Atom::Int {
                    value: 2,
                    suffix: None,
                }),
            ]),
            lit(Atom::Int {
                value: 0,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::InvalidBindingTarget);
    }

    // --- do form tests ---

    #[test]
    fn do_returns_last_expression() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "do".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 3,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(3));
    }

    #[test]
    fn do_evaluates_in_order() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "do".into(),
            }),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "def".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 7,
                    suffix: None,
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(7));
        assert_eq!(env.get("x"), Some(Value::Int(7)));
    }

    #[test]
    fn do_single_expression_passthrough() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "do".into(),
            }),
            lit(Atom::Int {
                value: 11,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(11));
    }

    #[test]
    fn do_allows_zero_body_is_error() {
        let env = Rc::new(Env::new());
        let expr = list(vec![lit(Atom::Symbol {
            ns: None,
            name: "do".into(),
        })]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
    }

    #[test]
    fn do_propagates_errors_early() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "do".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "missing".into(),
            }), // error
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "def".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 1,
                    suffix: None,
                }),
            ]),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::UnboundSymbol("missing".into()));
        assert_eq!(env.get("x"), None);
    }

    #[test]
    fn do_uses_same_scope() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "do".into(),
            }),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "def".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 4,
                    suffix: None,
                }),
            ]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "def".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 6,
                    suffix: None,
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(6));
        assert_eq!(env.get("x"), Some(Value::Int(6)));
    }

    #[test]
    fn do_ignores_intermediate_results() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "do".into(),
            }),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "def".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Int {
                    value: 1,
                    suffix: None,
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }), // 1
            lit(Atom::Int {
                value: 42,
                suffix: None,
            }), // should be result
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(42));
        assert_eq!(env.get("x"), Some(Value::Int(1))); // env state unchanged by final expr
    }

    // --- if form tests ---

    #[test]
    fn if_true_branch_taken() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "if".into(),
            }),
            lit(Atom::Bool(true)),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(1));
    }

    #[test]
    fn if_false_branch_taken() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "if".into(),
            }),
            lit(Atom::Bool(false)),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(2));
    }

    #[test]
    fn if_evaluates_condition_once() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "do".into(),
            }),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "def".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "count".into(),
                }),
                lit(Atom::Int {
                    value: 0,
                    suffix: None,
                }),
            ]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "if".into(),
                }),
                list(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "do".into(),
                    }),
                    list(vec![
                        lit(Atom::Symbol {
                            ns: None,
                            name: "def".into(),
                        }),
                        lit(Atom::Symbol {
                            ns: None,
                            name: "count".into(),
                        }),
                        lit(Atom::Int {
                            value: 1,
                            suffix: None,
                        }),
                    ]),
                    lit(Atom::Bool(true)),
                ]),
                lit(Atom::Int {
                    value: 10,
                    suffix: None,
                }),
                lit(Atom::Int {
                    value: 20,
                    suffix: None,
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "count".into(),
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(1)); // condition ran once
    }

    #[test]
    fn if_short_circuits_then_branch() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "if".into(),
            }),
            lit(Atom::Bool(true)),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "missing".into(),
            }), // should not be evaluated
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(1));
    }

    #[test]
    fn if_short_circuits_else_branch() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "if".into(),
            }),
            lit(Atom::Bool(false)),
            lit(Atom::Symbol {
                ns: None,
                name: "missing".into(),
            }), // should not be evaluated
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(2));
    }

    #[test]
    fn if_condition_must_be_bool_error() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "if".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 10,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 20,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::InvalidConditionType);
    }

    #[test]
    fn if_arity_error_on_missing_branch() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "if".into(),
            }),
            lit(Atom::Bool(true)),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
    }

    #[test]
    fn if_allows_non_bool_branches() {
        let env = Rc::new(Env::new());
        let expr1 = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "if".into(),
            }),
            lit(Atom::Bool(true)),
            lit(Atom::Str("yes".into())),
            lit(Atom::Int {
                value: 0,
                suffix: None,
            }),
        ]);
        let expr2 = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "if".into(),
            }),
            lit(Atom::Bool(false)),
            lit(Atom::Int {
                value: 0,
                suffix: None,
            }),
            lit(Atom::Str("no".into())),
        ]);

        let r1 = eval(&expr1, &env).unwrap();
        let r2 = eval(&expr2, &env).unwrap();
        assert_eq!(r1, Value::Str(Rc::from("yes")));
        assert_eq!(r2, Value::Str(Rc::from("no")));
    }

    // --- fn form tests ---

    #[test]
    fn fn_returns_function_value() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            vector(vec![lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            })]),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        match result {
            Value::Function(func) => {
                assert_eq!(func.name, None);
                assert_eq!(func.arity, 1);
                assert!(!func.variadic);
            }
            other => panic!("expected function, got {:?}", other),
        }
    }

    #[test]
    fn fn_empty_params_allowed() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            vector(vec![]),
            lit(Atom::Int {
                value: 42,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        match result {
            Value::Function(func) => {
                assert_eq!(func.arity, 0);
                assert!(!func.variadic);
            }
            other => panic!("expected function, got {:?}", other),
        }
    }

    #[test]
    fn fn_variadic_sets_flag() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "&".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "rest".into(),
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "rest".into(),
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        match result {
            Value::Function(func) => {
                assert_eq!(func.arity, 0);
                assert!(func.variadic);
            }
            other => panic!("expected function, got {:?}", other),
        }
    }

    #[test]
    fn fn_mixed_params_variadic_arity_counts_required() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "y".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "&".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "rest".into(),
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        match result {
            Value::Function(func) => {
                assert_eq!(func.arity, 2);
                assert!(func.variadic);
            }
            other => panic!("expected function, got {:?}", other),
        }
    }

    #[test]
    fn fn_captures_lexical_env_values() {
        let env = Rc::new(Env::new());
        env.define("x", Value::Int(10));

        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            vector(vec![]),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        match result {
            Value::Function(func) => {
                assert!(func.captures.contains(&(Rc::from("x"), Value::Int(10))));
            }
            other => panic!("expected function, got {:?}", other),
        }
    }

    #[test]
    fn fn_params_must_be_symbols() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            vector(vec![lit(Atom::Int {
                value: 1,
                suffix: None,
            })]),
            lit(Atom::Int {
                value: 0,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::InvalidBindingTarget);
    }

    #[test]
    fn fn_params_cannot_be_qualified() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            vector(vec![lit(Atom::Symbol {
                ns: Some("ns".into()),
                name: "x".into(),
            })]),
            lit(Atom::Int {
                value: 0,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::InvalidBindingTarget);
    }

    #[test]
    fn fn_param_list_must_be_vector() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 0,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
    }

    #[test]
    fn fn_requires_body_expr() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            vector(vec![lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            })]),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
    }

    // --- defn form tests ---

    #[test]
    fn defn_binds_function_in_env() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "defn".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "foo".into(),
            }),
            vector(vec![lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            })]),
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Unit);

        let binding = env.get("foo").expect("foo bound");
        match binding {
            Value::Function(func) => {
                assert_eq!(func.name, Some(Rc::from("foo")));
            }
            other => panic!("expected function, got {:?}", other),
        }
    }

    #[test]
    fn defn_returns_unit() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "defn".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "foo".into(),
            }),
            vector(vec![]),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Unit);
    }

    #[test]
    fn defn_overwrites_existing_binding_locally() {
        let env = Rc::new(Env::new());
        env.define("foo", Value::Int(1));

        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "defn".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "foo".into(),
            }),
            vector(vec![]),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
        ]);

        eval(&expr, &env).unwrap();
        let binding = env.get("foo").unwrap();
        match binding {
            Value::Function(func) => assert_eq!(func.name, Some(Rc::from("foo"))),
            other => panic!("expected function, got {:?}", other),
        }
    }

    #[test]
    fn defn_accepts_docstring_ignored_in_runtime() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "defn".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "foo".into(),
            }),
            lit(Atom::Str("doc".into())),
            vector(vec![]),
            lit(Atom::Int {
                value: 3,
                suffix: None,
            }),
        ]);

        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Unit);
        assert!(matches!(env.get("foo"), Some(Value::Function(_))));
    }

    #[test]
    fn defn_param_list_must_be_vector() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "defn".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "foo".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 0,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
    }

    #[test]
    fn defn_param_must_be_unqualified_symbol() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "defn".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "foo".into(),
            }),
            vector(vec![lit(Atom::Str("not-a-symbol".into()))]),
            lit(Atom::Int {
                value: 0,
                suffix: None,
            }),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::InvalidBindingTarget);
    }

    #[test]
    fn defn_requires_body_expr() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "defn".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "foo".into(),
            }),
            vector(vec![lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            })]),
        ]);

        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
    }

    #[test]
    fn defn_captures_lexical_env() {
        let env = Rc::new(Env::new());
        env.define("y", Value::Int(9));

        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "defn".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "foo".into(),
            }),
            vector(vec![]),
            lit(Atom::Symbol {
                ns: None,
                name: "y".into(),
            }),
        ]);

        eval(&expr, &env).unwrap();
        let binding = env.get("foo").unwrap();
        match binding {
            Value::Function(func) => {
                assert!(func.captures.contains(&(Rc::from("y"), Value::Int(9))))
            }
            other => panic!("expected function, got {:?}", other),
        }
    }

    #[test]
    fn def_error_on_symbol_arity() {
        let env = Rc::new(Env::new());
        let expr = list(vec![lit(Atom::Symbol {
            ns: None,
            name: "def".into(),
        })]);
        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::Arity);
        assert_eq!(env.get("x"), None);
    }

    #[test]
    fn def_error_on_non_symbol_name() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "def".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
        ]);
        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::InvalidBindingTarget);
        assert_eq!(env.get("x"), None);
    }

    #[test]
    fn def_error_on_namespace_symbol() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "def".into(),
            }),
            lit(Atom::Symbol {
                ns: Some("ns".into()),
                name: "x".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
        ]);
        let err = eval(&expr, &env).unwrap_err();
        assert_eq!(err, EvalError::InvalidBindingTarget);
        assert_eq!(env.get("x"), None);
    }

    #[test]
    fn lookup_local_binding() {
        let env = Rc::new(Env::new());
        env.define("x", int(1));
        assert_eq!(env.get("x"), Some(int(1)));
    }

    #[test]
    fn lookup_parent_binding() {
        let parent = Rc::new(Env::new());
        parent.define("x", int(2));

        let child = Rc::new(Env::child(parent.clone()));
        assert_eq!(child.get("x"), Some(int(2)));
    }

    #[test]
    fn shadowing_prefers_local() {
        let parent = Rc::new(Env::new());
        parent.define("x", int(2));

        let child = Rc::new(Env::child(parent.clone()));
        child.define("x", int(5));

        assert_eq!(child.get("x"), Some(int(5)));
        assert_eq!(parent.get("x"), Some(int(2)));
    }

    #[test]
    fn set_updates_local() {
        let env = Rc::new(Env::new());
        env.define("x", int(1));

        env.set("x", int(3)).unwrap();
        assert_eq!(env.get("x"), Some(int(3)));
    }

    #[test]
    fn set_updates_parent() {
        let parent = Rc::new(Env::new());
        parent.define("x", int(1));

        let child = Rc::new(Env::child(parent.clone()));
        child.set("x", int(9)).unwrap();

        assert_eq!(parent.get("x"), Some(int(9)));
        assert_eq!(child.get("x"), Some(int(9)));
    }

    #[test]
    fn set_errors_unbound() {
        let env = Rc::new(Env::new());
        let err = env.set("missing", int(1)).unwrap_err();
        assert_eq!(err, EnvError::Unbound("missing".to_string()));
    }

    #[test]
    fn define_overwrites_local() {
        let env = Rc::new(Env::new());
        env.define("x", int(1));
        env.define("x", int(2));

        assert_eq!(env.get("x"), Some(int(2)));
    }

    #[test]
    fn captures_are_independent() {
        let parent = Rc::new(Env::new());
        parent.define("p", int(1));

        let child = Rc::new(Env::child(parent.clone()));
        child.define("c", int(2));

        assert_eq!(parent.get("p"), Some(int(1)));
        assert_eq!(child.get("p"), Some(int(1)));
        assert_eq!(child.get("c"), Some(int(2)));
        assert_eq!(parent.get("c"), None);
    }

    // --- stdlib helpers ---

    /// Parse a Nexl snippet and evaluate it in the standard environment.
    fn eval_str(src: &str) -> Result<Value, EvalError> {
        let nodes = nexl_reader::read(src, meta::FileId::SYNTHETIC).expect("parse error");
        assert_eq!(nodes.len(), 1, "eval_str expects exactly one form");
        let env = crate::stdlib::standard_env();
        eval::eval(&nodes[0], &env)
    }

    // --- arithmetic builtin tests ---

    #[test]
    fn add_int_basic() {
        assert_eq!(eval_str("(+ 2 3)").unwrap(), Value::Int(5));
    }

    #[test]
    fn add_int_identity_zero_args() {
        assert_eq!(eval_str("(+)").unwrap(), Value::Int(0));
    }

    #[test]
    fn add_int_multi() {
        assert_eq!(eval_str("(+ 1 2 3)").unwrap(), Value::Int(6));
    }

    #[test]
    fn sub_int_binary() {
        assert_eq!(eval_str("(- 5 3)").unwrap(), Value::Int(2));
    }

    #[test]
    fn sub_int_unary_negation() {
        assert_eq!(eval_str("(- 7)").unwrap(), Value::Int(-7));
    }

    #[test]
    fn mul_int_basic() {
        assert_eq!(eval_str("(* 3 4)").unwrap(), Value::Int(12));
    }

    #[test]
    fn mul_int_identity_zero_args() {
        assert_eq!(eval_str("(*)").unwrap(), Value::Int(1));
    }

    #[test]
    fn div_int_exact() {
        assert_eq!(eval_str("(/ 10 2)").unwrap(), Value::Int(5));
    }

    #[test]
    fn div_int_by_zero_error() {
        assert!(matches!(
            eval_str("(/ 1 0)").unwrap_err(),
            EvalError::NativeError(_)
        ));
    }

    #[test]
    fn mod_int() {
        assert_eq!(eval_str("(mod 10 3)").unwrap(), Value::Int(1));
    }

    #[test]
    fn mod_int_by_zero_error() {
        assert!(matches!(
            eval_str("(mod 5 0)").unwrap_err(),
            EvalError::NativeError(_)
        ));
    }

    #[test]
    fn add_float_basic() {
        assert_eq!(eval_str("(+ 1.5 2.5)").unwrap(), Value::Float(4.0));
    }

    #[test]
    fn sub_float() {
        assert_eq!(eval_str("(- 3.0 1.5)").unwrap(), Value::Float(1.5));
    }

    #[test]
    fn mul_float() {
        assert_eq!(eval_str("(* 2.0 3.0)").unwrap(), Value::Float(6.0));
    }

    #[test]
    fn div_float() {
        assert_eq!(eval_str("(/ 7.0 2.0)").unwrap(), Value::Float(3.5));
    }

    #[test]
    fn type_mismatch_int_float_error() {
        // ADR-006: cross-type arithmetic is a runtime error in M1
        assert!(matches!(
            eval_str("(+ 1 2.0)").unwrap_err(),
            EvalError::NativeError(_)
        ));
    }

    // --- comparison builtin tests ---

    #[test]
    fn eq_int_equal() {
        assert_eq!(eval_str("(= 1 1)").unwrap(), Value::Bool(true));
    }

    #[test]
    fn eq_int_unequal() {
        assert_eq!(eval_str("(= 1 2)").unwrap(), Value::Bool(false));
    }

    #[test]
    fn eq_different_types() {
        // Int 1 and Float 1.0 are different Value variants — not equal.
        assert_eq!(eval_str("(= 1 1.0)").unwrap(), Value::Bool(false));
    }

    #[test]
    fn lt_int() {
        assert_eq!(eval_str("(< 1 2)").unwrap(), Value::Bool(true));
        assert_eq!(eval_str("(< 2 1)").unwrap(), Value::Bool(false));
    }

    #[test]
    fn gt_int() {
        assert_eq!(eval_str("(> 3 2)").unwrap(), Value::Bool(true));
    }

    #[test]
    fn le_int_boundary() {
        assert_eq!(eval_str("(<= 2 2)").unwrap(), Value::Bool(true));
        assert_eq!(eval_str("(<= 3 2)").unwrap(), Value::Bool(false));
    }

    #[test]
    fn ge_int_boundary() {
        assert_eq!(eval_str("(>= 3 3)").unwrap(), Value::Bool(true));
        assert_eq!(eval_str("(>= 2 3)").unwrap(), Value::Bool(false));
    }

    #[test]
    fn compare_floats() {
        assert_eq!(eval_str("(< 1.5 2.5)").unwrap(), Value::Bool(true));
        assert_eq!(eval_str("(> 2.5 1.5)").unwrap(), Value::Bool(true));
    }

    // --- logic builtin tests ---

    #[test]
    fn not_true() {
        assert_eq!(eval_str("(not true)").unwrap(), Value::Bool(false));
    }

    #[test]
    fn not_false() {
        assert_eq!(eval_str("(not false)").unwrap(), Value::Bool(true));
    }

    #[test]
    fn not_non_bool_error() {
        assert!(matches!(
            eval_str("(not 1)").unwrap_err(),
            EvalError::NativeError(_)
        ));
    }

    #[test]
    fn and_all_true() {
        assert_eq!(eval_str("(and true true)").unwrap(), Value::Bool(true));
    }

    #[test]
    fn and_one_false() {
        assert_eq!(eval_str("(and true false)").unwrap(), Value::Bool(false));
    }

    #[test]
    fn or_one_true() {
        assert_eq!(eval_str("(or false true)").unwrap(), Value::Bool(true));
    }

    #[test]
    fn or_all_false() {
        assert_eq!(eval_str("(or false false)").unwrap(), Value::Bool(false));
    }

    // --- string builtin tests ---

    #[test]
    fn str_concat() {
        assert_eq!(
            eval_str(r#"(str "hello" " " "world")"#).unwrap(),
            Value::Str(Rc::from("hello world"))
        );
    }

    #[test]
    fn str_coerce_int() {
        assert_eq!(eval_str("(str 42)").unwrap(), Value::Str(Rc::from("42")));
    }

    #[test]
    fn str_empty() {
        assert_eq!(eval_str("(str)").unwrap(), Value::Str(Rc::from("")));
    }

    #[test]
    fn count_str_ascii() {
        assert_eq!(eval_str(r#"(count "hello")"#).unwrap(), Value::Int(5));
    }

    #[test]
    fn count_str_unicode() {
        // "café" has 4 Unicode scalar values (spec §3.1)
        assert_eq!(eval_str(r#"(count "café")"#).unwrap(), Value::Int(4));
    }

    // --- integration test ---

    #[test]
    fn integration_fibonacci_10() {
        // Example from milestones.md:
        //   (defn fibonacci [n]
        //     (loop [i n, a 0, b 1]
        //       (if (= i 0) a (recur (- i 1) b (+ a b)))))
        //   (fibonacci 10)  ;; => 55
        let src = r#"
            (do
              (defn fibonacci [n]
                (loop [i n a 0 b 1]
                  (if (= i 0) a (recur (- i 1) b (+ a b)))))
              (fibonacci 10))
        "#;
        assert_eq!(eval_str(src).unwrap(), Value::Int(55));
    }

    // --- native function tests ---

    #[test]
    fn native_fn_value_is_callable() {
        // A NativeFunction bound in the env can be called like any function.
        let env = Rc::new(Env::new());
        env.define(
            "inc",
            Value::NativeFunction(Rc::new(nexl_runtime::NativeFn {
                name: "inc",
                f: |args| match args {
                    [Value::Int(n)] => Ok(Value::Int(n + 1)),
                    _ => Err("expected 1 Int".into()),
                },
            })),
        );

        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "inc".into(),
            }),
            lit(Atom::Int {
                value: 5,
                suffix: None,
            }),
        ]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(6));
    }

    // --- set! / let [mut ...] tests ---

    #[test]
    fn set_bang_returns_unit() {
        let env = Rc::new(Env::new());
        env.define("n", Value::Int(0));
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "set!".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "n".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
        ]);
        assert_eq!(eval(&expr, &env).unwrap(), Value::Unit);
    }

    #[test]
    fn set_bang_multiple_mutations() {
        // Last write wins.
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "mut".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "n".into(),
                }),
                lit(Atom::Int {
                    value: 0,
                    suffix: None,
                }),
            ]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "set!".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "n".into(),
                }),
                lit(Atom::Int {
                    value: 1,
                    suffix: None,
                }),
            ]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "set!".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "n".into(),
                }),
                lit(Atom::Int {
                    value: 2,
                    suffix: None,
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "n".into(),
            }),
        ]);
        assert_eq!(eval(&expr, &env).unwrap(), Value::Int(2));
    }

    #[test]
    fn set_bang_in_nested_do() {
        // Mutation from a nested `do` is visible in the outer let body.
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "mut".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "n".into(),
                }),
                lit(Atom::Int {
                    value: 0,
                    suffix: None,
                }),
            ]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "do".into(),
                }),
                list(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "set!".into(),
                    }),
                    lit(Atom::Symbol {
                        ns: None,
                        name: "n".into(),
                    }),
                    lit(Atom::Int {
                        value: 99,
                        suffix: None,
                    }),
                ]),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "n".into(),
            }),
        ]);
        assert_eq!(eval(&expr, &env).unwrap(), Value::Int(99));
    }

    #[test]
    fn set_bang_in_loop_body() {
        // set! inside loop updates a mut binding that outlives the loop.
        // (let [mut n 0]
        //   (loop [i false]
        //     (if i unit
        //       (do (set! n 7) (recur true))))
        //   n) → 7
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "mut".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "n".into(),
                }),
                lit(Atom::Int {
                    value: 0,
                    suffix: None,
                }),
            ]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "loop".into(),
                }),
                vector(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "done".into(),
                    }),
                    lit(Atom::Bool(false)),
                ]),
                list(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "if".into(),
                    }),
                    lit(Atom::Symbol {
                        ns: None,
                        name: "done".into(),
                    }),
                    lit(Atom::Unit),
                    list(vec![
                        lit(Atom::Symbol {
                            ns: None,
                            name: "do".into(),
                        }),
                        list(vec![
                            lit(Atom::Symbol {
                                ns: None,
                                name: "set!".into(),
                            }),
                            lit(Atom::Symbol {
                                ns: None,
                                name: "n".into(),
                            }),
                            lit(Atom::Int {
                                value: 7,
                                suffix: None,
                            }),
                        ]),
                        list(vec![
                            lit(Atom::Symbol {
                                ns: None,
                                name: "recur".into(),
                            }),
                            lit(Atom::Bool(true)),
                        ]),
                    ]),
                ]),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "n".into(),
            }),
        ]);
        assert_eq!(eval(&expr, &env).unwrap(), Value::Int(7));
    }

    #[test]
    fn set_bang_basic_mutation() {
        // (let [mut n 0] (set! n 5) n) → 5
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "mut".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "n".into(),
                }),
                lit(Atom::Int {
                    value: 0,
                    suffix: None,
                }),
            ]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "set!".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "n".into(),
                }),
                lit(Atom::Int {
                    value: 5,
                    suffix: None,
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "n".into(),
            }),
        ]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(5));
    }

    #[test]
    fn set_bang_error_on_unbound() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "set!".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "missing".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
        ]);
        assert_eq!(
            eval(&expr, &env).unwrap_err(),
            EvalError::UnboundSymbol("missing".into()),
        );
    }

    #[test]
    fn set_bang_error_arity_too_few() {
        // (set! n) — missing value
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "set!".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "n".into(),
            }),
        ]);
        assert_eq!(eval(&expr, &env).unwrap_err(), EvalError::Arity);
    }

    #[test]
    fn set_bang_error_arity_too_many() {
        // (set! n 1 2) — extra arg
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "set!".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "n".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
        ]);
        assert_eq!(eval(&expr, &env).unwrap_err(), EvalError::Arity);
    }

    #[test]
    fn set_bang_error_non_symbol_target() {
        // (set! 1 2) — literal as target
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "set!".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
        ]);
        assert_eq!(
            eval(&expr, &env).unwrap_err(),
            EvalError::InvalidBindingTarget
        );
    }

    #[test]
    fn let_mut_with_immutable_binding_can_still_be_set() {
        // In M1 (no type checker), set! works on plain let bindings too.
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "n".into(),
                }),
                lit(Atom::Int {
                    value: 0,
                    suffix: None,
                }),
            ]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "set!".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "n".into(),
                }),
                lit(Atom::Int {
                    value: 3,
                    suffix: None,
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "n".into(),
            }),
        ]);
        assert_eq!(eval(&expr, &env).unwrap(), Value::Int(3));
    }

    #[test]
    fn let_mut_keyword_accepted() {
        // (let [mut n 0] n) → 0
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "let".into(),
            }),
            vector(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "mut".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "n".into(),
                }),
                lit(Atom::Int {
                    value: 0,
                    suffix: None,
                }),
            ]),
            lit(Atom::Symbol {
                ns: None,
                name: "n".into(),
            }),
        ]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(0));
    }
}
