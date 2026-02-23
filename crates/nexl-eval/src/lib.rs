use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use nexl_runtime::Value;
use thiserror::Error;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn int(n: i64) -> Value {
        Value::Int(n)
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
