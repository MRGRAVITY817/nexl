use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use nexl_runtime::Value;
use thiserror::Error;

pub(crate) type ModuleExports = Rc<HashMap<Rc<str>, Value>>;
pub(crate) type ModuleAliasMap = HashMap<Rc<str>, ModuleExports>;

pub mod eval;
pub mod modules;
pub mod repl;
pub mod stdlib;

pub use modules::{ModuleSource, eval_modules, parse_module_source};

/// Lexical environment: a frame of bindings plus an optional parent link.
#[derive(Debug)]
pub struct Env {
    parent: Option<Rc<Env>>,
    bindings: RefCell<HashMap<Rc<str>, Value>>,
    modules: RefCell<ModuleAliasMap>,
}

impl Env {
    /// Create a new root environment.
    pub fn new() -> Self {
        Self {
            parent: None,
            bindings: RefCell::new(HashMap::new()),
            modules: RefCell::new(HashMap::new()),
        }
    }

    /// Create a child environment that chains to `parent`.
    pub fn child(parent: Rc<Env>) -> Self {
        Self {
            parent: Some(parent),
            bindings: RefCell::new(HashMap::new()),
            modules: RefCell::new(HashMap::new()),
        }
    }

    /// Define or overwrite a binding in the current frame.
    pub fn define(&self, name: impl Into<Rc<str>>, value: Value) {
        self.bindings.borrow_mut().insert(name.into(), value);
    }

    /// Define a module alias mapped to its exported bindings.
    pub fn define_module_alias(&self, alias: impl Into<Rc<str>>, exports: ModuleExports) {
        self.modules.borrow_mut().insert(alias.into(), exports);
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

    /// Look up a qualified name `alias/name` via imported module aliases.
    pub fn get_qualified(&self, alias: &str, name: &str) -> Option<Value> {
        if let Some(exports) = self.modules.borrow().get(alias)
            && let Some(v) = exports.get(name)
        {
            return Some(v.clone());
        }
        match &self.parent {
            Some(parent) => parent.get_qualified(alias, name),
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

    /// Snapshot module aliases for closure capture (nearest wins).
    pub fn capture_modules(&self) -> Vec<(Rc<str>, ModuleExports)> {
        let mut map = HashMap::new();
        self.fill_module_map(&mut map);
        map.into_iter().collect()
    }

    fn fill_module_map(&self, map: &mut ModuleAliasMap) {
        if let Some(parent) = &self.parent {
            parent.fill_module_map(map);
        }
        for (k, v) in self.modules.borrow().iter() {
            map.insert(k.clone(), Rc::clone(v));
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
#[derive(Debug, Error, PartialEq)]
pub enum EvalError {
    /// Referenced an unbound symbol.
    #[error("unbound symbol: {0}")]
    UnboundSymbol(String),
    /// Unsupported feature placeholder.
    #[error("unsupported qualified symbol: {0}")]
    UnsupportedQualifiedSymbol(String),
    /// Module declaration missing at the top of a file.
    #[error("missing module declaration")]
    MissingModuleDecl,
    /// Module declaration or import could not be parsed.
    #[error("module parse error: {0}")]
    ModuleParse(String),
    /// Module graph construction or ordering failed.
    #[error("module graph error: {0}")]
    ModuleGraph(String),
    /// An import referenced an unknown module.
    #[error("unknown module: {0}")]
    UnknownModule(String),
    /// An import referenced a name not exported by its module.
    #[error("`{name}` is not exported by module `{module}`")]
    ImportNotExported { module: String, name: String },
    /// An export list referenced a name not defined in the module.
    #[error("module `{module}` does not define export `{name}`")]
    MissingExport { module: String, name: String },
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
    /// Explicit `(panic "message")` — unrecoverable termination (spec §9.4).
    #[error("panic: {0}")]
    Panic(String),
    /// Non-local early return signal from the `?` operator (spec §9.3).
    ///
    /// Produced when `?` is applied to an `Err` or `None` value. Caught by
    /// `eval_apply` at the enclosing function boundary and converted into that
    /// function's return value.
    #[error("early return")]
    EarlyReturn(Value),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::eval;
    use meta::{self, Atom, Node, NodeKind};
    use nexl_reader::read;

    fn int(n: i64) -> Value {
        Value::Int(n)
    }

    /// Parse `src` and evaluate all top-level forms in `env`, returning the last result.
    fn eval_forms(src: &str, env: &Rc<Env>) -> Result<Value, EvalError> {
        let nodes = read(src, meta::FileId::SYNTHETIC).expect("parse error in test");
        let mut last = Value::Unit;
        for node in &nodes {
            last = eval(node, env)?;
        }
        Ok(last)
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

    fn map(entries: Vec<(Node, Node)>) -> Node {
        Node {
            kind: NodeKind::Map(entries),
            span: meta::span::Span::synthetic(),
            leading_comments: vec![],
            trailing_comment: None,
        }
    }

    fn set(items: Vec<Node>) -> Node {
        Node {
            kind: NodeKind::Set(items),
            span: meta::span::Span::synthetic(),
            leading_comments: vec![],
            trailing_comment: None,
        }
    }

    #[test]
    fn eval_vector_literal_ints() {
        let env = Rc::new(Env::new());
        let expr = vector(vec![
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
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]))
        );
    }

    #[test]
    fn eval_vector_literal_evaluates_elements() {
        let env = Rc::new(Env::new());
        env.define("x", Value::Int(10));
        let expr = vector(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
            lit(Atom::Int {
                value: 2,
                suffix: None,
            }),
        ]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![Value::Int(10), Value::Int(2)]))
        );
    }

    #[test]
    fn eval_vector_literal_empty() {
        let env = Rc::new(Env::new());
        let expr = vector(vec![]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Vec(Rc::new(vec![])));
    }

    #[test]
    fn eval_map_literal_keywords() {
        let env = Rc::new(Env::new());
        let expr = map(vec![
            (
                lit(Atom::Keyword {
                    ns: None,
                    name: "a".into(),
                }),
                lit(Atom::Int {
                    value: 1,
                    suffix: None,
                }),
            ),
            (
                lit(Atom::Keyword {
                    ns: None,
                    name: "b".into(),
                }),
                lit(Atom::Int {
                    value: 2,
                    suffix: None,
                }),
            ),
        ]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(
            result,
            Value::Map(Rc::new(
                vec![
                    (kw("a"), Value::Int(1)),
                    (kw("b"), Value::Int(2)),
                ]
                .into()
            ))
        );
    }

    #[test]
    fn eval_map_literal_evaluates_entries() {
        let env = Rc::new(Env::new());
        env.define(
            "k",
            Value::Keyword {
                ns: None,
                name: Rc::from("status"),
            },
        );
        env.define("v", Value::Int(10));
        let expr = map(vec![(
            lit(Atom::Symbol {
                ns: None,
                name: "k".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "v".into(),
            }),
        )]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(
            result,
            Value::Map(Rc::new(vec![(kw("status"), Value::Int(10))].into()))
        );
    }

    #[test]
    fn eval_map_literal_empty() {
        let env = Rc::new(Env::new());
        let expr = map(vec![]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Map(Rc::new(vec![].into())));
    }

    #[test]
    fn eval_set_literal_ints() {
        let env = Rc::new(Env::new());
        let expr = set(vec![
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
        assert_eq!(
            result,
            Value::Set(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]))
        );
    }

    #[test]
    fn eval_set_literal_evaluates_elements() {
        let env = Rc::new(Env::new());
        env.define("x", Value::Int(9));
        let expr = set(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "x".into(),
            }),
            lit(Atom::Int {
                value: 1,
                suffix: None,
            }),
        ]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(
            result,
            Value::Set(Rc::new(vec![Value::Int(9), Value::Int(1)]))
        );
    }

    #[test]
    fn eval_set_literal_empty() {
        let env = Rc::new(Env::new());
        let expr = set(vec![]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Set(Rc::new(vec![])));
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
    fn let_errors_when_literal_pattern_mismatches() {
        // `(let [1 2] 0)` — literal pattern `1` against value `2` fails.
        // Since let now supports refutable patterns (spec §4.12), this is a
        // pattern-mismatch error, not InvalidBindingTarget.
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
        assert!(
            matches!(&err, EvalError::NativeError(msg) if msg.contains("pattern did not match")),
            "expected pattern mismatch, got: {err:?}"
        );
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

    fn option_none() -> Value {
        Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("None"),
            fields: Rc::new(vec![]),
        }
    }

    fn option_some(value: Value) -> Value {
        Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("Some"),
            fields: Rc::new(vec![value]),
        }
    }

    fn kw(name: &str) -> Value {
        Value::Keyword {
            ns: None,
            name: Rc::from(name),
        }
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

    // --- collection builtin tests (Vec) ---

    #[test]
    fn vec_get_in_bounds_returns_some() {
        assert_eq!(
            eval_str("(get [1 2 3] 1)").unwrap(),
            option_some(Value::Int(2))
        );
    }

    #[test]
    fn vec_get_out_of_bounds_returns_none() {
        assert_eq!(eval_str("(get [1 2 3] 3)").unwrap(), option_none());
    }

    #[test]
    fn vec_put_updates_index() {
        assert_eq!(
            eval_str("(put [1 2 3] 1 9)").unwrap(),
            Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(9), Value::Int(3)]))
        );
    }

    #[test]
    fn vec_append_and_count() {
        assert_eq!(
            eval_str("(append [1 2] 3)").unwrap(),
            Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]))
        );
        assert_eq!(eval_str("(count [1 2 3])").unwrap(), Value::Int(3));
    }

    #[test]
    fn vec_first_rest_last_non_empty() {
        assert_eq!(
            eval_str("(first [1 2 3])").unwrap(),
            option_some(Value::Int(1))
        );
        assert_eq!(
            eval_str("(rest [1 2 3])").unwrap(),
            Value::Vec(Rc::new(vec![Value::Int(2), Value::Int(3)]))
        );
        assert_eq!(
            eval_str("(last [1 2 3])").unwrap(),
            option_some(Value::Int(3))
        );
    }

    #[test]
    fn vec_first_rest_last_empty() {
        assert_eq!(eval_str("(first [])").unwrap(), option_none());
        assert_eq!(eval_str("(rest [])").unwrap(), Value::Vec(Rc::new(vec![])));
        assert_eq!(eval_str("(last [])").unwrap(), option_none());
    }

    #[test]
    fn vec_slice_basic() {
        assert_eq!(
            eval_str("(slice [1 2 3 4] 1 3)").unwrap(),
            Value::Vec(Rc::new(vec![Value::Int(2), Value::Int(3)]))
        );
    }

    // --- collection builtin tests (Map) ---

    #[test]
    fn map_get_present_and_missing() {
        assert_eq!(
            eval_str(r#"(get {:a 1 :b 2} :a)"#).unwrap(),
            option_some(Value::Int(1))
        );
        assert_eq!(eval_str(r#"(get {:a 1 :b 2} :c)"#).unwrap(), option_none());
    }

    #[test]
    fn map_put_updates_and_appends() {
        assert_eq!(
            eval_str(r#"(put {:a 1 :b 2} :a 9)"#).unwrap(),
            Value::Map(Rc::new(
                vec![(kw("a"), Value::Int(9)), (kw("b"), Value::Int(2))].into()
            ))
        );
        assert_eq!(
            eval_str(r#"(put {:a 1 :b 2} :c 3)"#).unwrap(),
            Value::Map(Rc::new(
                vec![
                    (kw("a"), Value::Int(1)),
                    (kw("b"), Value::Int(2)),
                    (kw("c"), Value::Int(3)),
                ]
                .into()
            ))
        );
    }

    #[test]
    fn map_remove_existing_and_missing() {
        assert_eq!(
            eval_str(r#"(remove {:a 1 :b 2} :a)"#).unwrap(),
            Value::Map(Rc::new(vec![(kw("b"), Value::Int(2))].into()))
        );
        assert_eq!(
            eval_str(r#"(remove {:a 1 :b 2} :c)"#).unwrap(),
            Value::Map(Rc::new(
                vec![(kw("a"), Value::Int(1)), (kw("b"), Value::Int(2))].into()
            ))
        );
    }

    #[test]
    fn map_keys_vals_entries() {
        assert_eq!(
            eval_str(r#"(keys {:a 1 :b 2})"#).unwrap(),
            Value::Vec(Rc::new(vec![kw("a"), kw("b")]))
        );
        assert_eq!(
            eval_str(r#"(vals {:a 1 :b 2})"#).unwrap(),
            Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2)]))
        );
        assert_eq!(
            eval_str(r#"(entries {:a 1 :b 2})"#).unwrap(),
            Value::Vec(Rc::new(vec![
                Value::Vec(Rc::new(vec![kw("a"), Value::Int(1)])),
                Value::Vec(Rc::new(vec![kw("b"), Value::Int(2)])),
            ]))
        );
    }

    #[test]
    fn map_contains_and_count() {
        assert_eq!(
            eval_str(r#"(contains? {:a 1 :b 2} :a)"#).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            eval_str(r#"(contains? {:a 1 :b 2} :c)"#).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(eval_str(r#"(count {:a 1 :b 2})"#).unwrap(), Value::Int(2));
    }

    // --- collection builtin tests (Set) ---

    #[test]
    fn set_add_and_remove() {
        assert_eq!(
            eval_str(r#"(add #{1 2} 3)"#).unwrap(),
            Value::Set(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]))
        );
        assert_eq!(
            eval_str(r#"(add #{1 2} 2)"#).unwrap(),
            Value::Set(Rc::new(vec![Value::Int(1), Value::Int(2)]))
        );
        assert_eq!(
            eval_str(r#"(remove #{1 2 3} 2)"#).unwrap(),
            Value::Set(Rc::new(vec![Value::Int(1), Value::Int(3)]))
        );
        assert_eq!(
            eval_str(r#"(remove #{1 2 3} 4)"#).unwrap(),
            Value::Set(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]))
        );
    }

    #[test]
    fn set_contains_and_count() {
        assert_eq!(
            eval_str(r#"(contains? #{1 2} 1)"#).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            eval_str(r#"(contains? #{1 2} 3)"#).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(eval_str(r#"(count #{1 2 3})"#).unwrap(), Value::Int(3));
    }

    #[test]
    fn set_union_intersection_difference() {
        assert_eq!(
            eval_str(r#"(union #{1 2} #{2 3})"#).unwrap(),
            Value::Set(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]))
        );
        assert_eq!(
            eval_str(r#"(intersection #{1 2} #{2 3})"#).unwrap(),
            Value::Set(Rc::new(vec![Value::Int(2)]))
        );
        assert_eq!(
            eval_str(r#"(difference #{1 2} #{2 3})"#).unwrap(),
            Value::Set(Rc::new(vec![Value::Int(1)]))
        );
    }

    // --- sequence operation tests ---

    #[test]
    fn seq_map_filter_reduce_vec() {
        assert_eq!(
            eval_str("(map (fn [x] (+ x 1)) [1 2 3])").unwrap(),
            Value::Vec(Rc::new(vec![Value::Int(2), Value::Int(3), Value::Int(4)]))
        );
        assert_eq!(
            eval_str("(filter (fn [x] (> x 1)) [1 2 3])").unwrap(),
            Value::Vec(Rc::new(vec![Value::Int(2), Value::Int(3)]))
        );
        assert_eq!(
            eval_str("(reduce (fn [acc x] (+ acc x)) 0 [1 2 3])").unwrap(),
            Value::Int(6)
        );
    }

    #[test]
    fn seq_map_filter_reduce_map() {
        assert_eq!(
            eval_str(r#"(map (fn [x] (+ x 1)) {:a 1 :b 2})"#).unwrap(),
            Value::Map(Rc::new(
                vec![(kw("a"), Value::Int(2)), (kw("b"), Value::Int(3))].into()
            ))
        );
        assert_eq!(
            eval_str(r#"(filter (fn [x] (> x 1)) {:a 1 :b 2})"#).unwrap(),
            Value::Map(Rc::new(vec![(kw("b"), Value::Int(2))].into()))
        );
        assert_eq!(
            eval_str(r#"(reduce (fn [acc x] (+ acc x)) 0 {:a 1 :b 2})"#).unwrap(),
            Value::Int(3)
        );
    }

    #[test]
    fn seq_map_filter_reduce_set() {
        assert_eq!(
            eval_str(r#"(map (fn [x] 1) #{1 2 3})"#).unwrap(),
            Value::Set(Rc::new(vec![Value::Int(1)]))
        );
        assert_eq!(
            eval_str(r#"(filter (fn [x] (> x 1)) #{1 2 3})"#).unwrap(),
            Value::Set(Rc::new(vec![Value::Int(2), Value::Int(3)]))
        );
        assert_eq!(
            eval_str(r#"(reduce (fn [acc x] (+ acc x)) 0 #{1 2 3})"#).unwrap(),
            Value::Int(6)
        );
    }

    #[test]
    fn seq_map_filter_reduce_option() {
        assert_eq!(
            eval_str("(map (fn [x] (+ x 1)) (Some 1))").unwrap(),
            option_some(Value::Int(2))
        );
        assert_eq!(
            eval_str("(map (fn [x] (+ x 1)) None)").unwrap(),
            option_none()
        );
        assert_eq!(
            eval_str("(filter (fn [x] (> x 1)) (Some 2))").unwrap(),
            option_some(Value::Int(2))
        );
        assert_eq!(
            eval_str("(filter (fn [x] (> x 1)) (Some 1))").unwrap(),
            option_none()
        );
        assert_eq!(
            eval_str("(reduce (fn [acc x] (+ acc x)) 0 (Some 3))").unwrap(),
            Value::Int(3)
        );
        assert_eq!(
            eval_str("(reduce (fn [acc x] (+ acc x)) 0 None)").unwrap(),
            Value::Int(0)
        );
    }

    #[test]
    fn each_iterates_vec_for_side_effects() {
        let src = r#"
            (let [mut acc 0]
              (each [x [1 2 3]]
                (set! acc (+ acc x)))
              acc)
        "#;
        assert_eq!(eval_str(src).unwrap(), Value::Int(6));
    }

    #[test]
    fn each_iterates_map_values_for_side_effects() {
        let src = r#"
            (let [mut acc 0]
              (each [x {:a 1 :b 2}]
                (set! acc (+ acc x)))
              acc)
        "#;
        assert_eq!(eval_str(src).unwrap(), Value::Int(3));
    }

    #[test]
    fn times_iterates_from_zero() {
        let src = r#"
            (let [mut acc 0]
              (times [i 3]
                (set! acc (+ acc i)))
              acc)
        "#;
        assert_eq!(eval_str(src).unwrap(), Value::Int(3));
    }

    #[test]
    fn for_with_when_and_nested_bindings() {
        assert_eq!(
            eval_str(r#"(for [x [1 2] y [10 20] :when (= x 2)] [x y])"#).unwrap(),
            Value::Vec(Rc::new(vec![
                Value::Vec(Rc::new(vec![Value::Int(2), Value::Int(10)])),
                Value::Vec(Rc::new(vec![Value::Int(2), Value::Int(20)])),
            ]))
        );
    }

    #[test]
    fn for_with_let_when_while() {
        assert_eq!(
            eval_str(r#"(for [x [1 2 3] :let [y (+ x 1)] :when (> y 2) :while (< y 4)] y)"#)
                .unwrap(),
            Value::Vec(Rc::new(vec![Value::Int(3)]))
        );
    }

    #[test]
    fn for_bang_returns_vec() {
        assert_eq!(
            eval_str(r#"(for! [x [1 2]] (+ x 1))"#).unwrap(),
            Value::Vec(Rc::new(vec![Value::Int(2), Value::Int(3)]))
        );
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

    // --- let-else tests (spec §4.12) ---

    #[test]
    fn let_else_some_matches_value() {
        // (let [(Some x) (Some 42) | 0] x) → 42
        // Pattern succeeds: x is bound to 42, body runs.
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(let [(Some x) (Some 42) | 0] x)", &env).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn let_else_none_uses_fallback() {
        // (let [(Some x) None | 0] 99) → 0
        // Pattern fails: fallback 0 is returned, body 99 is never evaluated.
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(let [(Some x) None | 0] 99)", &env).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    #[test]
    fn let_else_ok_matches() {
        // (let [(Ok n) (Ok 7) | -1] n) → 7
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(let [(Ok n) (Ok 7) | -1] n)", &env).unwrap();
        assert_eq!(result, Value::Int(7));
    }

    #[test]
    fn let_else_err_uses_fallback() {
        // (let [(Ok n) (Err "oops") | -1] n) → -1
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(let [(Ok n) (Err \"oops\") | -1] n)", &env).unwrap();
        assert_eq!(result, Value::Int(-1));
    }

    #[test]
    fn let_else_multi_binding_second_fails() {
        // Both (Some x) and (Ok y) bindings present; second fails.
        // First succeeds (x=1), second (Ok y) on (Err "bad") fails → fallback -1 returned.
        let env = crate::stdlib::standard_env();
        let result = eval_forms(
            "(let [(Some x) (Some 1) | 99
                   (Ok y)   (Err \"bad\") | -1]
               (+ x y))",
            &env,
        )
        .unwrap();
        assert_eq!(result, Value::Int(-1));
    }

    #[test]
    fn let_else_multi_binding_first_fails() {
        // First binding (Some x) on None fails → fallback 99 returned immediately;
        // second binding and body are never evaluated (spec §4.12 evaluation semantics).
        let env = crate::stdlib::standard_env();
        let result = eval_forms(
            "(let [(Some x) None     | 99
                   (Ok y)   (Ok 2)   | -1]
               (+ x y))",
            &env,
        )
        .unwrap();
        assert_eq!(result, Value::Int(99));
    }

    // --- panic tests ---

    #[test]
    fn test_panic_with_string_message() {
        // (panic "oops") should terminate with EvalError::Panic containing "oops"
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "panic".into(),
            }),
            lit(Atom::Str("oops".into())),
        ]);
        let result = eval(&expr, &env);
        assert_eq!(result, Err(EvalError::Panic("oops".into())));
    }

    #[test]
    fn test_panic_produces_eval_error() {
        // (panic "broken") returns EvalError::Panic, NOT NativeError
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "panic".into(),
            }),
            lit(Atom::Str("broken".into())),
        ]);
        let result = eval(&expr, &env);
        assert!(result.is_err());
        match result.unwrap_err() {
            EvalError::Panic(msg) => assert_eq!(msg, "broken"),
            other => panic!("expected EvalError::Panic, got: {other:?}"),
        }
    }

    #[test]
    fn test_panic_in_branch() {
        // (if false (panic "unreachable") 42) should NOT panic
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "if".into(),
            }),
            lit(Atom::Bool(false)),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "panic".into(),
                }),
                lit(Atom::Str("unreachable".into())),
            ]),
            lit(Atom::Int {
                value: 42,
                suffix: None,
            }),
        ]);
        let result = eval(&expr, &env).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn test_panic_message_contains_source_info() {
        // (panic "bad state") error message includes "bad state"
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "panic".into(),
            }),
            lit(Atom::Str("bad state".into())),
        ]);
        let result = eval(&expr, &env);
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("bad state"),
            "error message should contain the panic text, got: {msg}"
        );
    }

    #[test]
    fn test_panic_halts_do_block() {
        // (do 1 (panic "stop") 3) — never reaches 3, returns Panic error
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
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "panic".into(),
                }),
                lit(Atom::Str("stop".into())),
            ]),
            lit(Atom::Int {
                value: 3,
                suffix: None,
            }),
        ]);
        let result = eval(&expr, &env);
        assert_eq!(result, Err(EvalError::Panic("stop".into())));
    }

    #[test]
    fn test_panic_no_args_is_arity_error() {
        // (panic) with no message is an arity error
        let env = Rc::new(Env::new());
        let expr = list(vec![lit(Atom::Symbol {
            ns: None,
            name: "panic".into(),
        })]);
        let result = eval(&expr, &env);
        assert_eq!(result, Err(EvalError::Arity));
    }

    #[test]
    fn test_panic_too_many_args_is_arity_error() {
        // (panic "a" "b") is an arity error
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "panic".into(),
            }),
            lit(Atom::Str("a".into())),
            lit(Atom::Str("b".into())),
        ]);
        let result = eval(&expr, &env);
        assert_eq!(result, Err(EvalError::Arity));
    }

    // --- assert! / assert-unreachable! tests ---

    // Test 1: (assert! true) → Value::Unit  (spec §4.2.1)
    #[test]
    fn assert_true_returns_unit() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "assert!".into(),
            }),
            lit(Atom::Bool(true)),
        ]);
        let result = eval(&expr, &env);
        assert_eq!(result, Ok(Value::Unit));
    }

    // Test 2: (assert! false) → EvalError::Panic  (spec §4.2.1)
    #[test]
    fn assert_false_panics() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "assert!".into(),
            }),
            lit(Atom::Bool(false)),
        ]);
        let result = eval(&expr, &env);
        assert!(matches!(result, Err(EvalError::Panic(_))));
    }

    // Test 3: (assert! false "boom") → EvalError::Panic("boom")  (spec §4.2.1)
    #[test]
    fn assert_false_with_message_panics_with_message() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "assert!".into(),
            }),
            lit(Atom::Bool(false)),
            lit(Atom::Str("boom".into())),
        ]);
        let result = eval(&expr, &env);
        assert_eq!(result, Err(EvalError::Panic("boom".into())));
    }

    // Test 4: (assert! true "boom") → Value::Unit  (condition passes, message irrelevant)
    #[test]
    fn assert_true_with_message_does_not_panic() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "assert!".into(),
            }),
            lit(Atom::Bool(true)),
            lit(Atom::Str("boom".into())),
        ]);
        let result = eval(&expr, &env);
        assert_eq!(result, Ok(Value::Unit));
    }

    // Test 5: (assert-unreachable!) → EvalError::Panic  (spec §4.2.1: always panics)
    #[test]
    fn assert_unreachable_always_panics() {
        let env = Rc::new(Env::new());
        let expr = list(vec![lit(Atom::Symbol {
            ns: None,
            name: "assert-unreachable!".into(),
        })]);
        let result = eval(&expr, &env);
        assert!(matches!(result, Err(EvalError::Panic(_))));
    }

    // Test 6: (assert-unreachable! "never here") → EvalError::Panic("never here")
    #[test]
    fn assert_unreachable_with_message_includes_message() {
        let env = Rc::new(Env::new());
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "assert-unreachable!".into(),
            }),
            lit(Atom::Str("never here".into())),
        ]);
        let result = eval(&expr, &env);
        assert_eq!(result, Err(EvalError::Panic("never here".into())));
    }

    // Test 7: (assert!) with no condition → EvalError::Arity
    #[test]
    fn assert_wrong_arity_is_error() {
        let env = Rc::new(Env::new());
        let expr = list(vec![lit(Atom::Symbol {
            ns: None,
            name: "assert!".into(),
        })]);
        let result = eval(&expr, &env);
        assert_eq!(result, Err(EvalError::Arity));
    }

    // --- ? operator evaluation tests (spec §9.3) ---

    fn ok_val(inner: Value) -> Value {
        Value::Adt {
            type_name: Rc::from("Result"),
            ctor: Rc::from("Ok"),
            fields: Rc::new(vec![inner]),
        }
    }

    fn err_val(inner: Value) -> Value {
        Value::Adt {
            type_name: Rc::from("Result"),
            ctor: Rc::from("Err"),
            fields: Rc::new(vec![inner]),
        }
    }

    fn some_val(inner: Value) -> Value {
        Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("Some"),
            fields: Rc::new(vec![inner]),
        }
    }

    fn none_val() -> Value {
        Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("None"),
            fields: Rc::new(vec![]),
        }
    }

    // Test 1: (? ok-val) → unwraps inner value (spec §9.3: "If (Ok v), produces v")
    #[test]
    fn question_ok_result_unwraps_inner() {
        let env = Rc::new(Env::new());
        env.define("ok-val", ok_val(Value::Int(42)));
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "?".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "ok-val".into(),
            }),
        ]);
        let result = eval(&expr, &env);
        assert_eq!(result, Ok(Value::Int(42)));
    }

    // Test 2: ? on Err inside fn → function returns Err value (spec §9.3: "returns (Err e)")
    // Red: EarlyReturn bubbles out as Err rather than being caught as function return.
    #[test]
    fn question_err_result_early_returns_from_fn() {
        let env = Rc::new(Env::new());
        env.define("err-val", err_val(Value::Str(Rc::from("boom"))));
        // ((fn [] (? err-val)))
        let expr = list(vec![list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            vector(vec![]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "?".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "err-val".into(),
                }),
            ]),
        ])]);
        let result = eval(&expr, &env);
        assert_eq!(result, Ok(err_val(Value::Str(Rc::from("boom")))));
    }

    // Test 3: (? some-val) → unwraps inner value (spec §9.3 Option: "If (Some v), produces v")
    #[test]
    fn question_some_option_unwraps_inner() {
        let env = Rc::new(Env::new());
        env.define("some-val", some_val(Value::Int(7)));
        let expr = list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "?".into(),
            }),
            lit(Atom::Symbol {
                ns: None,
                name: "some-val".into(),
            }),
        ]);
        let result = eval(&expr, &env);
        assert_eq!(result, Ok(Value::Int(7)));
    }

    // Test 4: ? on None inside fn → function returns None (spec §9.3: "returns None")
    #[test]
    fn question_none_option_early_returns_from_fn() {
        let env = Rc::new(Env::new());
        env.define("none-val", none_val());
        // ((fn [] (? none-val)))
        let expr = list(vec![list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            vector(vec![]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "?".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "none-val".into(),
                }),
            ]),
        ])]);
        let result = eval(&expr, &env);
        assert_eq!(result, Ok(none_val()));
    }

    // Test 5: ? on Err skips subsequent body expressions (spec §9.3: "no remaining code executes")
    #[test]
    fn question_err_skips_subsequent_body_exprs() {
        let env = Rc::new(Env::new());
        env.define("err-val", err_val(Value::Int(99)));
        // ((fn [] (? err-val) (panic "reached")))  — panic must not fire
        let expr = list(vec![list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            vector(vec![]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "?".into(),
                }),
                lit(Atom::Symbol {
                    ns: None,
                    name: "err-val".into(),
                }),
            ]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "panic".into(),
                }),
                lit(Atom::Str("reached".into())),
            ]),
        ])]);
        let result = eval(&expr, &env);
        assert_eq!(result, Ok(err_val(Value::Int(99))));
    }

    // Test 6: ? on Ok unwraps into let binding and is returned (spec §9.3: usage in let)
    #[test]
    fn question_ok_result_bound_and_used() {
        let env = Rc::new(Env::new());
        env.define("ok-val", ok_val(Value::Int(42)));
        // ((fn [] (let [x (? ok-val)] x)))
        let expr = list(vec![list(vec![
            lit(Atom::Symbol {
                ns: None,
                name: "fn".into(),
            }),
            vector(vec![]),
            list(vec![
                lit(Atom::Symbol {
                    ns: None,
                    name: "let".into(),
                }),
                vector(vec![
                    lit(Atom::Symbol {
                        ns: None,
                        name: "x".into(),
                    }),
                    list(vec![
                        lit(Atom::Symbol {
                            ns: None,
                            name: "?".into(),
                        }),
                        lit(Atom::Symbol {
                            ns: None,
                            name: "ok-val".into(),
                        }),
                    ]),
                ]),
                lit(Atom::Symbol {
                    ns: None,
                    name: "x".into(),
                }),
            ]),
        ])]);
        let result = eval(&expr, &env);
        assert_eq!(result, Ok(Value::Int(42)));
    }

    // --- contract enforcement tests (spec §4.2.1) ---

    // Test 1: :requires [x] with x=true → body runs, returns 42
    #[test]
    fn contracts_requires_passes_allows_body() {
        let env = Rc::new(Env::new());
        // (defn f [x] :requires [x] 42)  then  (f true)
        eval_forms("(defn f [x] :requires [x] 42)", &env).unwrap();
        let result = eval_forms("(f true)", &env);
        assert_eq!(result, Ok(Value::Int(42)));
    }

    // Test 2: :requires [x] with x=false → EvalError::Panic (spec §4.2.1: "panic on failure")
    #[test]
    fn contracts_requires_fails_panics() {
        let env = Rc::new(Env::new());
        eval_forms("(defn f [x] :requires [x] 42)", &env).unwrap();
        let result = eval_forms("(f false)", &env);
        assert!(matches!(result, Err(EvalError::Panic(_))));
    }

    // Test 3: :ensures [result] with body=true → returns true (spec §4.2.1: postcondition passes)
    #[test]
    fn contracts_ensures_passes_returns_result() {
        let env = Rc::new(Env::new());
        // Body returns `true`; :ensures [result] checks `result` which is true → OK
        eval_forms("(defn g [] :ensures [result] true)", &env).unwrap();
        let result = eval_forms("(g)", &env);
        assert_eq!(result, Ok(Value::Bool(true)));
    }

    // Test 4: :ensures [result] with body=false → EvalError::Panic (spec §4.2.1: postcondition fails)
    #[test]
    fn contracts_ensures_fails_panics() {
        let env = Rc::new(Env::new());
        // Body returns `false`; :ensures [result] checks `result` which is false → panic
        eval_forms("(defn g [] :ensures [result] false)", &env).unwrap();
        let result = eval_forms("(g)", &env);
        assert!(matches!(result, Err(EvalError::Panic(_))));
    }

    // Test 5: multiple :requires clauses — all must pass (spec §4.2.1 example with 2 clauses)
    #[test]
    fn contracts_multiple_requires_all_checked() {
        let env = Rc::new(Env::new());
        // :requires [a b]; call with a=true b=false → second clause fails
        eval_forms("(defn f [a b] :requires [a b] 42)", &env).unwrap();
        let result = eval_forms("(f true false)", &env);
        assert!(matches!(result, Err(EvalError::Panic(_))));
    }

    // Test 6: :examples clause is parsed but not checked at eval time
    #[test]
    fn contracts_examples_clause_is_skipped() {
        let env = Rc::new(Env::new());
        eval_forms("(defn f [x] :examples [] x)", &env).unwrap();
        let result = eval_forms("(f 99)", &env);
        assert_eq!(result, Ok(Value::Int(99)));
    }

    // --- try/catch evaluation tests (spec §9: desugars to match on Result) ---

    fn result_env() -> Rc<Env> {
        crate::stdlib::standard_env()
    }

    // Test 1: Ok branch unwraps inner value; catch is not run (spec §9)
    #[test]
    fn try_catch_ok_unwraps_value() {
        let env = result_env();
        // (try (Ok 42) (catch e 0))  → 42
        let result = eval_forms("(try (Ok 42) (catch e 0))", &env);
        assert_eq!(result, Ok(Value::Int(42)));
    }

    // Test 2: Err branch runs catch body; catch name bound to inner error value (spec §9)
    #[test]
    fn try_catch_err_runs_catch_with_inner() {
        let env = result_env();
        // (try (Err "boom") (catch e e))  → "boom"  (e = inner value, not the Err ADT)
        let result = eval_forms(r#"(try (Err "boom") (catch e e))"#, &env);
        assert_eq!(result, Ok(Value::Str(Rc::from("boom"))));
    }

    // Test 3: catch body receives the numeric inner value (confirms e = inner)
    #[test]
    fn try_catch_catch_body_uses_error_value() {
        let env = result_env();
        // (try (Err 99) (catch n n))  → 99
        let result = eval_forms("(try (Err 99) (catch n n))", &env);
        assert_eq!(result, Ok(Value::Int(99)));
    }

    // Test 4: multiple body exprs; last one is the Result (spec §9)
    #[test]
    fn try_catch_multi_body_last_is_result() {
        let env = result_env();
        // (try 1 (Ok 2) (catch e e))  → 2  (first body expr is 1, last is (Ok 2))
        let result = eval_forms("(try 1 (Ok 2) (catch e e))", &env);
        assert_eq!(result, Ok(Value::Int(2)));
    }

    // Test 5: malformed try (missing catch) → error
    #[test]
    fn try_catch_malformed_missing_catch_errors() {
        let env = result_env();
        // (try (Ok 1))  → error (needs catch clause)
        let result = eval_forms("(try (Ok 1))", &env);
        assert!(matches!(result, Err(EvalError::NativeError(_))));
    }

    // ── Self-recursive defn ──────────────────────────────────────────────────

    #[test]
    fn eval_self_recursive_defn() {
        // (defn fact [n] (if (<= n 1) 1 (* n (fact (- n 1)))))
        // fact must be visible inside its own body at call time.
        let env = crate::stdlib::standard_env();
        eval_forms("(defn fact [n] (if (<= n 1) 1 (* n (fact (- n 1)))))", &env)
            .expect("defn should succeed");
        let result = eval_forms("(fact 5)", &env).expect("fact 5 should succeed");
        assert_eq!(result, Value::Int(120));
    }

    #[test]
    fn eval_recursive_defn_fibonacci() {
        // Classic double-recursive fib — exercises multiple recursive calls.
        let env = crate::stdlib::standard_env();
        eval_forms(
            "(defn fib [n] (if (<= n 1) n (+ (fib (- n 1)) (fib (- n 2)))))",
            &env,
        )
        .expect("defn should succeed");
        let result = eval_forms("(fib 10)", &env).expect("fib 10 should succeed");
        assert_eq!(result, Value::Int(55));
    }

    // --- stdlib module qualified access ---

    #[test]
    fn test_stdlib_module_qualified_access() {
        let result = eval_str("(core/identity 42)").expect("core/identity should work");
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn test_core_identity_preserves_type() {
        assert_eq!(
            eval_str(r#"(core/identity "hello")"#).unwrap(),
            Value::Str(Rc::from("hello"))
        );
        assert_eq!(eval_str("(core/identity true)").unwrap(), Value::Bool(true));
        assert_eq!(
            eval_str("(core/identity [1 2 3])").unwrap(),
            Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]))
        );
    }

    #[test]
    fn test_core_comp() {
        let env = crate::stdlib::standard_env();
        // (defn inc [x] (+ x 1))
        // (defn double [x] (* x 2))
        // ((core/comp inc double) 3) => (inc (double 3)) => (inc 6) => 7
        eval_forms("(defn inc [x] (+ x 1))", &env).unwrap();
        eval_forms("(defn double [x] (* x 2))", &env).unwrap();
        let result = eval_forms("((core/comp inc double) 3)", &env).unwrap();
        assert_eq!(result, Value::Int(7));
    }

    #[test]
    fn test_core_partial() {
        // ((core/partial + 1) 2) => (+ 1 2) => 3
        let result = eval_str("((core/partial + 1) 2)").unwrap();
        assert_eq!(result, Value::Int(3));
    }

    #[test]
    fn test_core_constantly() {
        // ((core/constantly 42) "ignored") => 42
        let result = eval_str(r#"((core/constantly 42) "ignored")"#).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn test_core_constantly_ignores_args() {
        let result = eval_str(r#"((core/constantly 42) 1 2 3)"#).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn test_core_apply() {
        // (core/apply + [1 2 3]) => 6
        let result = eval_str("(core/apply + [1 2 3])").unwrap();
        assert_eq!(result, Value::Int(6));
    }

    #[test]
    fn test_core_apply_with_leading_args() {
        // (core/apply + 1 [2 3]) => (+ 1 2 3) => 6
        let result = eval_str("(core/apply + 1 [2 3])").unwrap();
        assert_eq!(result, Value::Int(6));
    }

    #[test]
    fn test_core_juxt() {
        // ((core/juxt first last) [1 2 3]) => [(Some 1) (Some 3)]
        let result = eval_str("((core/juxt first last) [1 2 3])").unwrap();
        let expected = Value::Vec(Rc::new(vec![
            option_some(Value::Int(1)),
            option_some(Value::Int(3)),
        ]));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_str_module_qualified_access() {
        let result = eval_str(r#"(str/upper "hello")"#).unwrap();
        assert_eq!(result, Value::Str(Rc::from("HELLO")));
    }

    #[test]
    fn test_str_split_via_eval() {
        let result = eval_str(r#"(str/split "a,b,c" ",")"#).unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![
                Value::Str(Rc::from("a")),
                Value::Str(Rc::from("b")),
                Value::Str(Rc::from("c")),
            ]))
        );
    }

    // --- end-to-end stdlib integration ---

    #[test]
    fn test_e2e_stdlib_pipeline_simple() {
        // Verify qualified access works from within a defn body
        let env = crate::stdlib::standard_env();
        eval_forms(r#"(defn up [s] (str/upper s))"#, &env).unwrap();
        let result = eval_forms(r#"(up "hello")"#, &env).unwrap();
        assert_eq!(result, Value::Str(Rc::from("HELLO")));
    }

    #[test]
    fn test_e2e_stdlib_pipeline() {
        // A realistic pipeline using multiple stdlib modules:
        // Split a CSV line, uppercase each field, join with " | "
        let env = crate::stdlib::standard_env();
        eval_forms(
            r#"(defn format-row [line]
                 (str/join " | "
                   (map (fn [s] (str/upper (str/trim s)))
                        (str/split line ","))))"#,
            &env,
        )
        .unwrap();
        let result = eval_forms(r#"(format-row " alice , bob , carol ")"#, &env).unwrap();
        assert_eq!(result, Value::Str(Rc::from("ALICE | BOB | CAROL")));
    }

    #[test]
    fn test_e2e_math_with_core() {
        // Use core/comp with math functions
        let env = crate::stdlib::standard_env();
        // compose abs and floor: first abs then floor
        let result = eval_forms("((core/comp math/floor math/abs) -3.7)", &env).unwrap();
        assert_eq!(result, Value::Float(3.0));
    }

    #[test]
    fn test_e2e_json_roundtrip() {
        let env = crate::stdlib::standard_env();
        // Create a map, stringify it, parse it back
        eval_forms(r#"(def data {:name "nexl" :version 1})"#, &env).unwrap();
        eval_forms(r#"(def json-str (json/stringify data))"#, &env).unwrap();
        let result = eval_forms(r#"(json/parse json-str)"#, &env).unwrap();
        // Should be (Ok {...})
        match result {
            Value::Adt { ctor, .. } => assert_eq!(ctor.as_ref(), "Ok"),
            _ => panic!("expected Result Ok"),
        }
    }

    #[test]
    fn test_e2e_conv_pipeline() {
        let env = crate::stdlib::standard_env();
        // Convert string to int, check it's Some
        eval_forms(r#"(def result (conv/->int "42"))"#, &env).unwrap();
        let result = eval_forms(r#"result"#, &env).unwrap();
        assert_eq!(result, option_some(Value::Int(42)));
    }

    // -----------------------------------------------------------------------
    // M19: Variadic rest args
    // -----------------------------------------------------------------------

    #[test]
    fn variadic_rest_collects_extra_args() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(defn f [x & rest] rest) (f 1 2 3 4)", &env).unwrap();
        assert_eq!(result, Value::Vec(Rc::new(vec![int(2), int(3), int(4)])));
    }

    #[test]
    fn variadic_rest_empty_when_no_extras() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(defn f [x & rest] rest) (f 1)", &env).unwrap();
        assert_eq!(result, Value::Vec(Rc::new(vec![])));
    }

    #[test]
    fn variadic_rest_works_with_apply_value() {
        let env = crate::stdlib::standard_env();
        // Use map with a variadic fn — map calls apply_value internally
        let result = eval_forms("(defn my-list [& items] items) (my-list 10 20 30)", &env).unwrap();
        assert_eq!(result, Value::Vec(Rc::new(vec![int(10), int(20), int(30)])));
    }

    // -----------------------------------------------------------------------
    // M19: Short-circuit and/or
    // -----------------------------------------------------------------------

    #[test]
    fn and_short_circuits_on_false() {
        let env = crate::stdlib::standard_env();
        // (and false (panic "boom")) should return false without panicking
        let result = eval_forms(r#"(and false (panic "boom"))"#, &env).unwrap();
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn and_evaluates_all_true() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(and true true true)", &env).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn and_variadic_stops_at_first_false() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms(r#"(and true false (panic "boom"))"#, &env).unwrap();
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn or_short_circuits_on_true() {
        let env = crate::stdlib::standard_env();
        // (or true (panic "boom")) should return true without panicking
        let result = eval_forms(r#"(or true (panic "boom"))"#, &env).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn or_evaluates_all_false() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(or false false false)", &env).unwrap();
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn or_variadic_stops_at_first_true() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms(r#"(or false true (panic "boom"))"#, &env).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn and_two_args_basic() {
        let env = crate::stdlib::standard_env();
        assert_eq!(
            eval_forms("(and true true)", &env).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            eval_forms("(and true false)", &env).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            eval_forms("(and false true)", &env).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn or_two_args_basic() {
        let env = crate::stdlib::standard_env();
        assert_eq!(
            eval_forms("(or false false)", &env).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            eval_forms("(or true false)", &env).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            eval_forms("(or false true)", &env).unwrap(),
            Value::Bool(true)
        );
    }

    // -----------------------------------------------------------------------
    // M19: cond form
    // -----------------------------------------------------------------------

    #[test]
    fn cond_first_true_branch() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(cond true 1 true 2)", &env).unwrap();
        assert_eq!(result, int(1));
    }

    #[test]
    fn cond_second_branch() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(cond false 1 true 2)", &env).unwrap();
        assert_eq!(result, int(2));
    }

    #[test]
    fn cond_else_branch() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(cond false 1 false 2 :else 3)", &env).unwrap();
        assert_eq!(result, int(3));
    }

    #[test]
    fn cond_no_match_returns_unit() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(cond false 1 false 2)", &env).unwrap();
        assert_eq!(result, Value::Unit);
    }

    #[test]
    fn cond_short_circuits() {
        let env = crate::stdlib::standard_env();
        // Second test should not be evaluated since first matches
        let result = eval_forms(r#"(cond true 42 (panic "boom") 99)"#, &env).unwrap();
        assert_eq!(result, int(42));
    }

    #[test]
    fn variadic_rest_in_anonymous_fn() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("((fn [& args] args) 5 6 7)", &env).unwrap();
        assert_eq!(result, Value::Vec(Rc::new(vec![int(5), int(6), int(7)])));
    }

    // -----------------------------------------------------------------------
    // M19: match form
    // -----------------------------------------------------------------------

    #[test]
    fn match_literal_int() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms(
            r#"(match 42
               1 "one"
               42 "forty-two"
               _ "other")"#,
            &env,
        )
        .unwrap();
        assert_eq!(result, Value::Str(Rc::from("forty-two")));
    }

    #[test]
    fn match_literal_string() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms(
            r#"(match "hello"
               "world" 1
               "hello" 2
               _ 3)"#,
            &env,
        )
        .unwrap();
        assert_eq!(result, int(2));
    }

    #[test]
    fn match_literal_bool() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(match false true 1 false 2)", &env).unwrap();
        assert_eq!(result, int(2));
    }

    #[test]
    fn match_literal_keyword() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(match :ok :err 0 :ok 1 _ 2)", &env).unwrap();
        assert_eq!(result, int(1));
    }

    #[test]
    fn match_wildcard() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(match 999 _ 42)", &env).unwrap();
        assert_eq!(result, int(42));
    }

    #[test]
    fn match_binding_var() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(match 10 x (+ x 1))", &env).unwrap();
        assert_eq!(result, int(11));
    }

    #[test]
    fn match_constructor_some() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(match (Some 5) (None) 0 (Some x) x)", &env).unwrap();
        assert_eq!(result, int(5));
    }

    #[test]
    fn match_constructor_none() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(match None (None) 0 (Some x) x)", &env).unwrap();
        assert_eq!(result, int(0));
    }

    #[test]
    fn match_constructor_ok_err() {
        let env = crate::stdlib::standard_env();
        let ok_result = eval_forms("(match (Ok 42) (Ok v) v (Err e) 0)", &env).unwrap();
        assert_eq!(ok_result, int(42));

        let err_result = eval_forms(r#"(match (Err "bad") (Ok v) 0 (Err e) e)"#, &env).unwrap();
        assert_eq!(err_result, Value::Str(Rc::from("bad")));
    }

    #[test]
    fn match_tuple_pattern() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(match [1 2 3] [a b c] (+ a (+ b c)))", &env).unwrap();
        assert_eq!(result, int(6));
    }

    #[test]
    fn match_map_pattern() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms(
            "(match {:name \"alice\" :age 30} {:name n :age a} (str n \" is \" a))",
            &env,
        )
        .unwrap();
        assert_eq!(result, Value::Str(Rc::from("alice is 30")));
    }

    #[test]
    fn match_nested_constructor() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms(
            "(match (Some (Ok 7)) (Some (Ok v)) v (Some (Err e)) 0 (None) 0)",
            &env,
        )
        .unwrap();
        assert_eq!(result, int(7));
    }

    #[test]
    fn match_no_match_errors() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(match 42 1 \"one\" 2 \"two\")", &env);
        assert!(result.is_err());
    }

    #[test]
    fn match_unit_literal() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(match unit unit 1 _ 2)", &env).unwrap();
        assert_eq!(result, int(1));
    }

    #[test]
    fn match_or_pattern() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(match :b (| :a :b :c) 1 _ 2)", &env).unwrap();
        assert_eq!(result, int(1));
    }

    // -----------------------------------------------------------------------
    // M19: deftype
    // -----------------------------------------------------------------------

    #[test]
    fn deftype_nullary_constructors() {
        let env = crate::stdlib::standard_env();
        eval_forms("(deftype Color | Red | Green | Blue)", &env).unwrap();

        let red = eval_forms("Red", &env).unwrap();
        assert_eq!(
            red,
            Value::Adt {
                type_name: Rc::from("Color"),
                ctor: Rc::from("Red"),
                fields: Rc::new(vec![]),
            }
        );

        let green = eval_forms("Green", &env).unwrap();
        assert_eq!(
            green,
            Value::Adt {
                type_name: Rc::from("Color"),
                ctor: Rc::from("Green"),
                fields: Rc::new(vec![]),
            }
        );
    }

    #[test]
    fn deftype_parameterized_constructors() {
        let env = crate::stdlib::standard_env();
        eval_forms("(deftype MyOption [a] | MyNone | (MySome a))", &env).unwrap();

        let none = eval_forms("MyNone", &env).unwrap();
        assert_eq!(
            none,
            Value::Adt {
                type_name: Rc::from("MyOption"),
                ctor: Rc::from("MyNone"),
                fields: Rc::new(vec![]),
            }
        );

        let some_val = eval_forms("(MySome 42)", &env).unwrap();
        assert_eq!(
            some_val,
            Value::Adt {
                type_name: Rc::from("MyOption"),
                ctor: Rc::from("MySome"),
                fields: Rc::new(vec![int(42)]),
            }
        );
    }

    #[test]
    fn deftype_match_on_custom_adt() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms(
            r#"(deftype Shape | Circle | (Rect w h))
            (defn area [s]
              (match s
                (Circle) 3
                (Rect w h) (* w h)))
            (area (Rect 4 5))"#,
            &env,
        )
        .unwrap();
        assert_eq!(result, int(20));
    }

    #[test]
    fn deftype_multi_arg_constructor() {
        let env = crate::stdlib::standard_env();
        eval_forms("(deftype Tree [a] | Leaf | (Branch a left right))", &env).unwrap();

        let leaf = eval_forms("Leaf", &env).unwrap();
        assert_eq!(
            leaf,
            Value::Adt {
                type_name: Rc::from("Tree"),
                ctor: Rc::from("Leaf"),
                fields: Rc::new(vec![]),
            }
        );

        let branch = eval_forms("(Branch 1 Leaf Leaf)", &env).unwrap();
        assert_eq!(
            branch,
            Value::Adt {
                type_name: Rc::from("Tree"),
                ctor: Rc::from("Branch"),
                fields: Rc::new(vec![
                    int(1),
                    Value::Adt {
                        type_name: Rc::from("Tree"),
                        ctor: Rc::from("Leaf"),
                        fields: Rc::new(vec![]),
                    },
                    Value::Adt {
                        type_name: Rc::from("Tree"),
                        ctor: Rc::from("Leaf"),
                        fields: Rc::new(vec![]),
                    },
                ]),
            }
        );
    }

    // -- record deftype --
    #[test]
    fn deftype_record_constructor_creates_map() {
        let env = crate::stdlib::standard_env();
        eval_forms("(deftype Foo {:x Int :y Str})", &env).unwrap();
        let result = eval_forms(r#"(Foo {:x 1 :y "hi"})"#, &env).unwrap();
        // Record constructor should produce a Map value
        match &result {
            Value::Map(entries) => {
                assert_eq!(entries.len(), 2);
            }
            other => panic!("expected Map, got {other:?}"),
        }
    }

    #[test]
    fn deftype_record_get_field() {
        let env = crate::stdlib::standard_env();
        eval_forms("(deftype Foo {:x Int :y Str})", &env).unwrap();
        let result = eval_forms(r#"(get (Foo {:x 42 :y "hi"}) :x)?"#, &env).unwrap();
        assert_eq!(result, int(42));
    }

    #[test]
    fn deftype_record_constructor_arity() {
        let env = crate::stdlib::standard_env();
        eval_forms("(deftype Foo {:x Int :y Str})", &env).unwrap();
        // No args — should fail
        assert!(eval_forms("(Foo)", &env).is_err());
        // Two args — should fail
        assert!(eval_forms("(Foo 1 2)", &env).is_err());
    }

    // -- keyword as function --
    #[test]
    fn keyword_as_function_on_map() {
        let result = eval_str("(:x {:x 42 :y 0})").unwrap();
        assert_eq!(result, int(42));
    }

    #[test]
    fn keyword_as_function_missing_key() {
        let result = eval_str("(:z {:x 42})").unwrap();
        // Missing key returns None (Option ADT)
        assert_eq!(
            result,
            Value::Adt {
                type_name: Rc::from("Option"),
                ctor: Rc::from("None"),
                fields: Rc::new(vec![]),
            }
        );
    }

    #[test]
    fn keyword_as_function_arity() {
        // No args
        assert!(eval_str("(:x)").is_err());
        // Two args
        assert!(eval_str("(:x {:x 1} {:x 2})").is_err());
    }

    // ===================================================================
    // M22 — Collections & Algorithms
    // ===================================================================

    // -- sort --
    #[test]
    fn sort_ints() {
        assert_eq!(
            eval_str("(sort [3 1 2])").unwrap(),
            Value::Vec(Rc::new(vec![int(1), int(2), int(3)]))
        );
    }

    #[test]
    fn sort_strings() {
        assert_eq!(
            eval_str(r#"(sort ["banana" "apple" "cherry"])"#).unwrap(),
            Value::Vec(Rc::new(vec![
                Value::Str(Rc::from("apple")),
                Value::Str(Rc::from("banana")),
                Value::Str(Rc::from("cherry")),
            ]))
        );
    }

    #[test]
    fn sort_empty() {
        assert_eq!(eval_str("(sort [])").unwrap(), Value::Vec(Rc::new(vec![])));
    }

    #[test]
    fn sort_by_key() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(defn neg [x] (- 0 x))\n(sort-by neg [1 3 2])", &env).unwrap();
        assert_eq!(result, Value::Vec(Rc::new(vec![int(3), int(2), int(1)])));
    }

    // -- reverse --
    #[test]
    fn reverse_vec() {
        assert_eq!(
            eval_str("(reverse [1 2 3])").unwrap(),
            Value::Vec(Rc::new(vec![int(3), int(2), int(1)]))
        );
    }

    #[test]
    fn reverse_empty() {
        assert_eq!(
            eval_str("(reverse [])").unwrap(),
            Value::Vec(Rc::new(vec![]))
        );
    }

    // -- range --
    #[test]
    fn range_one_arg() {
        assert_eq!(
            eval_str("(range 5)").unwrap(),
            Value::Vec(Rc::new(vec![int(0), int(1), int(2), int(3), int(4)]))
        );
    }

    #[test]
    fn range_two_args() {
        assert_eq!(
            eval_str("(range 2 5)").unwrap(),
            Value::Vec(Rc::new(vec![int(2), int(3), int(4)]))
        );
    }

    #[test]
    fn range_three_args() {
        assert_eq!(
            eval_str("(range 0 10 3)").unwrap(),
            Value::Vec(Rc::new(vec![int(0), int(3), int(6), int(9)]))
        );
    }

    #[test]
    fn range_zero() {
        assert_eq!(eval_str("(range 0)").unwrap(), Value::Vec(Rc::new(vec![])));
    }

    // -- flat-map --
    #[test]
    fn flat_map_basic() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms("(defn dup [x] [x x])\n(flat-map dup [1 2 3])", &env).unwrap();
        assert_eq!(
            result,
            Value::Vec(Rc::new(vec![
                int(1),
                int(1),
                int(2),
                int(2),
                int(3),
                int(3)
            ]))
        );
    }

    // -- group-by --
    #[test]
    fn group_by_basic() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms(
            "(defn even? [x] (= (mod x 2) 0))\n(group-by even? [1 2 3 4 5])",
            &env,
        )
        .unwrap();
        // Result is a Map with keys false/true
        match &result {
            Value::Map(pairs) => {
                assert_eq!(pairs.len(), 2);
                let odds = pairs.get(&Value::Bool(false)).expect("key false");
                assert_eq!(*odds, Value::Vec(Rc::new(vec![int(1), int(3), int(5)])));
                let evens = pairs.get(&Value::Bool(true)).expect("key true");
                assert_eq!(*evens, Value::Vec(Rc::new(vec![int(2), int(4)])));
            }
            other => panic!("expected Map, got {other}"),
        }
    }

    // -- zip --
    #[test]
    fn zip_basic() {
        assert_eq!(
            eval_str(r#"(zip [1 2 3] ["a" "b" "c"])"#).unwrap(),
            Value::Vec(Rc::new(vec![
                Value::Vec(Rc::new(vec![int(1), Value::Str(Rc::from("a"))])),
                Value::Vec(Rc::new(vec![int(2), Value::Str(Rc::from("b"))])),
                Value::Vec(Rc::new(vec![int(3), Value::Str(Rc::from("c"))])),
            ]))
        );
    }

    #[test]
    fn zip_unequal_length() {
        assert_eq!(
            eval_str("(zip [1 2] [10 20 30])").unwrap(),
            Value::Vec(Rc::new(vec![
                Value::Vec(Rc::new(vec![int(1), int(10)])),
                Value::Vec(Rc::new(vec![int(2), int(20)])),
            ]))
        );
    }

    // -- take / drop --
    #[test]
    fn take_basic() {
        assert_eq!(
            eval_str("(take 2 [1 2 3 4])").unwrap(),
            Value::Vec(Rc::new(vec![int(1), int(2)]))
        );
    }

    #[test]
    fn drop_basic() {
        assert_eq!(
            eval_str("(drop 2 [1 2 3 4])").unwrap(),
            Value::Vec(Rc::new(vec![int(3), int(4)]))
        );
    }

    // -- take-while / drop-while --
    #[test]
    fn take_while_basic() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms(
            "(defn small? [x] (< x 3))\n(take-while small? [1 2 3 4 5])",
            &env,
        )
        .unwrap();
        assert_eq!(result, Value::Vec(Rc::new(vec![int(1), int(2)])));
    }

    #[test]
    fn drop_while_basic() {
        let env = crate::stdlib::standard_env();
        let result = eval_forms(
            "(defn small? [x] (< x 3))\n(drop-while small? [1 2 3 4 5])",
            &env,
        )
        .unwrap();
        assert_eq!(result, Value::Vec(Rc::new(vec![int(3), int(4), int(5)])));
    }

    // -- bitwise operations --
    #[test]
    fn bit_and_basic() {
        assert_eq!(eval_str("(bit-and 12 10)").unwrap(), int(8));
    }

    #[test]
    fn bit_or_basic() {
        assert_eq!(eval_str("(bit-or 12 10)").unwrap(), int(14));
    }

    #[test]
    fn bit_xor_basic() {
        assert_eq!(eval_str("(bit-xor 12 10)").unwrap(), int(6));
    }

    #[test]
    fn bit_not_basic() {
        assert_eq!(eval_str("(bit-not 0)").unwrap(), int(-1));
    }

    #[test]
    fn bit_shift_left_basic() {
        assert_eq!(eval_str("(bit-shift-left 1 4)").unwrap(), int(16));
    }

    #[test]
    fn bit_shift_right_basic() {
        assert_eq!(eval_str("(bit-shift-right 16 2)").unwrap(), int(4));
    }

    // -- str/format --
    #[test]
    fn str_format_basic() {
        assert_eq!(
            eval_str(r#"(str/format "Hello, {}!" "world")"#).unwrap(),
            Value::Str(Rc::from("Hello, world!"))
        );
    }

    #[test]
    fn str_format_multiple() {
        assert_eq!(
            eval_str(r#"(str/format "{} + {} = {}" 1 2 3)"#).unwrap(),
            Value::Str(Rc::from("1 + 2 = 3"))
        );
    }

    #[test]
    fn str_format_no_placeholders() {
        assert_eq!(
            eval_str(r#"(str/format "no placeholders")"#).unwrap(),
            Value::Str(Rc::from("no placeholders"))
        );
    }

    // ── defhandler tests ──

    #[test]
    fn eval_defhandler_simple() {
        let env = crate::stdlib::standard_env();
        let src = r#"
            (defhandler ConsoleLog
              Log
              (info [msg] (println msg)))
            ConsoleLog
        "#;
        let nodes = read(src, meta::FileId::SYNTHETIC).expect("parse error");
        let mut last = Value::Unit;
        for node in &nodes {
            last = eval::eval(node, &env).unwrap();
        }
        // ConsoleLog should resolve to a Handler value
        assert!(matches!(last, Value::Handler(_)));
        if let Value::Handler(h) = &last {
            assert_eq!(&*h.name, "ConsoleLog");
            assert!(h.params.is_empty());
            assert_eq!(h.effects.len(), 1);
            assert_eq!(h.effects[0].name, "Log");
            assert_eq!(h.effects[0].operations.len(), 1);
            assert_eq!(h.effects[0].operations[0].name, "info");
        }
    }

    #[test]
    fn eval_defhandler_parameterized() {
        let env = crate::stdlib::standard_env();
        let src = r#"
            (defhandler JsonLog [config]
              Log
              (info [msg] msg))
            JsonLog
        "#;
        let nodes = read(src, meta::FileId::SYNTHETIC).expect("parse error");
        let mut last = Value::Unit;
        for node in &nodes {
            last = eval::eval(node, &env).unwrap();
        }
        if let Value::Handler(h) = &last {
            assert_eq!(&*h.name, "JsonLog");
            assert_eq!(h.params.len(), 1);
            assert_eq!(&*h.params[0], "config");
        } else {
            panic!("expected Handler, got {last:?}");
        }
    }

    #[test]
    fn eval_defhandler_multi_effect() {
        let env = crate::stdlib::standard_env();
        let src = r#"
            (defhandler ProductionStack
              Db
              (query [sql] sql)
              Log
              (info [msg] msg))
            ProductionStack
        "#;
        let nodes = read(src, meta::FileId::SYNTHETIC).expect("parse error");
        let mut last = Value::Unit;
        for node in &nodes {
            last = eval::eval(node, &env).unwrap();
        }
        if let Value::Handler(h) = &last {
            assert_eq!(&*h.name, "ProductionStack");
            assert_eq!(h.effects.len(), 2);
            assert_eq!(h.effects[0].name, "Db");
            assert_eq!(h.effects[1].name, "Log");
        } else {
            panic!("expected Handler, got {last:?}");
        }
    }

    #[test]
    fn eval_defhandler_continuation() {
        let env = crate::stdlib::standard_env();
        let src = r#"
            (defhandler TimestampLog
              Log
              (info [resume msg] (resume unit)))
            TimestampLog
        "#;
        let nodes = read(src, meta::FileId::SYNTHETIC).expect("parse error");
        let mut last = Value::Unit;
        for node in &nodes {
            last = eval::eval(node, &env).unwrap();
        }
        if let Value::Handler(h) = &last {
            assert_eq!(&*h.name, "TimestampLog");
            assert!(h.effects[0].operations[0].has_resume);
        } else {
            panic!("expected Handler, got {last:?}");
        }
    }

    #[test]
    fn eval_defhandler_returns_unit() {
        let env = crate::stdlib::standard_env();
        let src = r#"
            (defhandler ConsoleLog
              Log
              (info [msg] msg))
        "#;
        let nodes = read(src, meta::FileId::SYNTHETIC).expect("parse error");
        let mut last = Value::Unit;
        for node in &nodes {
            last = eval::eval(node, &env).unwrap();
        }
        // defhandler itself returns Unit (like def/defn)
        assert_eq!(last, Value::Unit);
    }
}
