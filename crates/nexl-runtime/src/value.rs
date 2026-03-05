use indexmap::IndexMap;
use meta::Node;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::cell::RefCell;
use std::rc::Rc;

type ModuleExports = Rc<HashMap<Rc<str>, Value>>;

/// Type alias for a native closure's implementation.
pub type NativeClosureFn = Rc<dyn Fn(&[Value]) -> Result<Value, String>>;

// ---------------------------------------------------------------------------
// NexlMap — O(1) persistent map backed by IndexMap
// ---------------------------------------------------------------------------

/// A newtype wrapper around `Value` used exclusively as a map key.
///
/// Implements `Eq` and `Hash` so it can be stored in an `IndexMap`:
/// - Floats are compared and hashed **bitwise** (so `NaN == NaN` in this context).
/// - Functions / closures are compared and hashed by **pointer identity**.
///
/// This type is private to this module; public API is through [`NexlMap`].
#[derive(Clone, Debug)]
struct ValueKey(Value);

impl PartialEq for ValueKey {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
            _ => self.0 == other.0,
        }
    }
}

impl Eq for ValueKey {}

impl Hash for ValueKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(&self.0).hash(state);
        match &self.0 {
            Value::Int(n) => n.hash(state),
            Value::Float(f) => f.to_bits().hash(state),
            Value::Bool(b) => b.hash(state),
            Value::Str(s) => s.hash(state),
            Value::Unit => {}
            Value::Char(c) => c.hash(state),
            Value::Keyword { ns, name } => {
                ns.hash(state);
                name.hash(state);
            }
            Value::Symbol { ns, name } => {
                ns.hash(state);
                name.hash(state);
            }
            Value::Ratio(n, d) => {
                n.hash(state);
                d.hash(state);
            }
            Value::Vec(items) => {
                items.len().hash(state);
                for item in items.iter() {
                    ValueKey(item.clone()).hash(state);
                }
            }
            Value::Map(m) => {
                m.len().hash(state);
                // XOR hashes of entries for order-independence.
                let mut xor: u64 = 0;
                for (k, v) in m.iter() {
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    ValueKey(k.clone()).hash(&mut h);
                    ValueKey(v.clone()).hash(&mut h);
                    xor ^= h.finish();
                }
                xor.hash(state);
            }
            Value::Set(items) => {
                items.len().hash(state);
                // XOR hashes for order-independence.
                let mut xor: u64 = 0;
                for item in items.iter() {
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    ValueKey(item.clone()).hash(&mut h);
                    xor ^= h.finish();
                }
                xor.hash(state);
            }
            Value::Adt {
                type_name,
                ctor,
                fields,
            } => {
                type_name.hash(state);
                ctor.hash(state);
                for f in fields.iter() {
                    ValueKey(f.clone()).hash(state);
                }
            }
            Value::Function(f) => (Rc::as_ptr(f) as usize).hash(state),
            Value::NativeFunction(nf) => (nf.f as usize).hash(state),
            Value::NativeClosure { f, .. } => {
                (Rc::as_ptr(f) as *const () as usize).hash(state);
            }
            Value::Handler(h) => (Rc::as_ptr(h) as usize).hash(state),
            Value::Atom(cell) => (Rc::as_ptr(cell) as usize).hash(state),
        }
    }
}

/// A persistent, insertion-order-preserving map from [`Value`] keys to [`Value`] values.
///
/// Backed by an [`IndexMap`] for O(1) average-case lookup, put, remove, and contains,
/// while preserving insertion order for deterministic display output.
///
/// Map equality is **order-independent**: two maps with the same key–value pairs
/// are equal regardless of insertion order.
#[derive(Clone, Debug, Default)]
pub struct NexlMap {
    inner: IndexMap<ValueKey, Value>,
}

impl NexlMap {
    /// Creates an empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a map from an iterator of `(key, value)` pairs.
    ///
    /// If a key appears more than once the **last** value wins (matching map-literal semantics).
    pub fn from_pairs(pairs: impl IntoIterator<Item = (Value, Value)>) -> Self {
        let mut inner = IndexMap::new();
        for (k, v) in pairs {
            inner.insert(ValueKey(k), v);
        }
        NexlMap { inner }
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if the map has no entries.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Looks up `key`, returning `Some(&value)` on a hit or `None` on a miss.
    pub fn get(&self, key: &Value) -> Option<&Value> {
        // SAFETY: ValueKey is repr-transparent over Value in terms of storage;
        // we borrow it here only for the lookup, not for ownership.
        self.inner.get(&ValueKey(key.clone()))
    }

    /// Returns `true` if `key` is present.
    pub fn contains(&self, key: &Value) -> bool {
        self.inner.contains_key(&ValueKey(key.clone()))
    }

    /// Returns a new map with `key` mapped to `value`.
    ///
    /// If `key` already exists its value is replaced **in place** (preserving its
    /// original insertion position).
    pub fn put(&self, key: Value, value: Value) -> Self {
        let mut inner = self.inner.clone();
        inner.insert(ValueKey(key), value);
        NexlMap { inner }
    }

    /// Returns a new map with `key` removed. If `key` is absent the original map
    /// is returned unchanged (cloned).
    pub fn remove(&self, key: &Value) -> Self {
        let mut inner = self.inner.clone();
        inner.shift_remove(&ValueKey(key.clone()));
        NexlMap { inner }
    }

    /// Iterates over `(key, value)` references in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&Value, &Value)> {
        self.inner.iter().map(|(k, v)| (&k.0, v))
    }

    /// Iterates over keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &Value> {
        self.inner.keys().map(|k| &k.0)
    }

    /// Iterates over values in insertion order.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.inner.values()
    }
}

impl PartialEq for NexlMap {
    /// Order-independent equality: two maps are equal iff they have the same key–value pairs.
    fn eq(&self, other: &Self) -> bool {
        if self.inner.len() != other.inner.len() {
            return false;
        }
        for (k, v) in &self.inner {
            match other.inner.get(k) {
                Some(ov) => {
                    if v != ov {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }
}

impl From<Vec<(Value, Value)>> for NexlMap {
    fn from(pairs: Vec<(Value, Value)>) -> Self {
        NexlMap::from_pairs(pairs)
    }
}

/// A built-in function implemented natively in Rust.
///
/// Uses a raw function pointer (not a trait object) so that `NativeFn`
/// implements `PartialEq` via pointer equality without heap allocation.
#[derive(Debug, Clone)]
pub struct NativeFn {
    /// Canonical name, used in `Display` and error messages.
    pub name: &'static str,
    /// The implementation. Receives the already-evaluated arguments and returns
    /// a [`Value`] or a runtime error message.
    pub f: fn(&[Value]) -> Result<Value, String>,
}

impl PartialEq for NativeFn {
    fn eq(&self, other: &Self) -> bool {
        (self.f as usize) == (other.f as usize)
    }
}

/// A runtime function closure.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    /// Optional name (present for `defn`, absent for anonymous `fn`).
    pub name: Option<Rc<str>>,
    /// Positional parameter names (required args).
    pub params: Vec<Rc<str>>,
    /// Optional variadic rest parameter name.
    pub rest: Option<Rc<str>>,
    /// Required positional parameter count.
    pub arity: u32,
    /// Whether the function accepts a variadic rest parameter.
    pub variadic: bool,
    /// Captured bindings from the defining environment (name, value).
    pub captures: Vec<(Rc<str>, Value)>,
    /// Captured module aliases (alias, exports).
    pub module_captures: Vec<(Rc<str>, ModuleExports)>,
    /// Body expressions to evaluate when called (in order).
    pub body: Vec<Node>,
    /// Precondition expressions (`:requires` clause). Checked before body in dev mode (spec §4.2.1).
    pub requires: Vec<Node>,
    /// Postcondition expressions (`:ensures` clause). Checked after body; `result` is bound (spec §4.2.1).
    pub ensures: Vec<Node>,
}

/// A single pre-built handler operation for `call-log`-wrapped handlers.
///
/// Used in [`HandlerDef::built_ops`] to hold recording-wrapped op functions
/// without requiring AST nodes.
#[derive(Clone)]
pub struct BuiltHandlerEffect {
    /// Effect name, e.g. `"Console"`.
    pub name: String,
    /// Operation name → pre-built function value.
    pub ops: Vec<(String, Value)>,
}

impl std::fmt::Debug for BuiltHandlerEffect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuiltHandlerEffect")
            .field("name", &self.name)
            .field("ops", &self.ops.iter().map(|(k, _)| k).collect::<Vec<_>>())
            .finish()
    }
}

impl PartialEq for BuiltHandlerEffect {
    fn eq(&self, _other: &Self) -> bool {
        false // pre-built effects are not structurally comparable
    }
}

/// A named effect handler definition (spec §6.10 `defhandler`).
///
/// Stored as a `Value::Handler` in the environment. When installed via
/// `(handle [HandlerName] body)`, the effect implementations are used to
/// intercept effect operations within the body.
#[derive(Debug, Clone, PartialEq)]
pub struct HandlerDef {
    /// Handler name, e.g. `"ConsoleLog"`.
    pub name: Rc<str>,
    /// Parameter names for parameterized handlers (empty if non-parameterized).
    pub params: Vec<Rc<str>>,
    /// Effect implementations — effect name + operation bodies (AST-based).
    pub effects: Vec<meta::HandledEffect>,
    /// Pre-built operation functions for `call-log`-wrapped handlers.
    ///
    /// When non-empty, `install_handler_effects` uses these Values directly
    /// instead of building functions from the AST-based `effects` field.
    pub built_ops: Vec<BuiltHandlerEffect>,
}

/// A runtime value produced by the Nexl tree-walk interpreter.
///
/// This is distinct from the reader's `Atom` type: `Atom` is a *source-level*
/// representation with suffix annotations and raw text; `Value` is the
/// *evaluated* form that the interpreter operates on.
#[derive(Clone)]
pub enum Value {
    /// 64-bit signed integer.
    Int(i64),
    /// 64-bit double-precision float.
    Float(f64),
    /// Boolean.
    Bool(bool),
    /// Immutable, reference-counted UTF-8 string.
    Str(Rc<str>),
    /// The sole value of the `Unit` type (ADR-001).
    Unit,
    /// Unicode scalar value.
    Char(char),
    /// Keyword, e.g. `:foo` or `:bar/baz`.
    Keyword { ns: Option<Rc<str>>, name: Rc<str> },
    /// Symbol (an identifier), e.g. `add` or `math/sqrt`.
    Symbol { ns: Option<Rc<str>>, name: Rc<str> },
    /// Exact rational number, stored in lowest terms.
    Ratio(i64, i64),

    /// Persistent vector value.
    Vec(Rc<Vec<Value>>),
    /// Persistent map value — O(1) lookup, insertion-order display.
    Map(Rc<NexlMap>),
    /// Persistent set value.
    Set(Rc<Vec<Value>>),
    /// Algebraic data type value (constructor + fields).
    Adt {
        /// The parent type name (e.g. "Option").
        type_name: Rc<str>,
        /// The constructor name (e.g. "Some", "None").
        ctor: Rc<str>,
        /// Constructor fields.
        fields: Rc<Vec<Value>>,
    },

    /// Function value — captures an environment and callable body.
    Function(Rc<Function>),

    /// A built-in (native Rust) function.
    NativeFunction(Rc<NativeFn>),

    /// A native closure — a Rust closure capturing runtime values.
    ///
    /// Used by stdlib higher-order functions like `comp`, `partial`, `constantly`
    /// that return new functions capturing their arguments.
    NativeClosure {
        /// Display name for error messages.
        name: Rc<str>,
        /// The closure implementation.
        f: NativeClosureFn,
    },

    /// A named effect handler definition (spec §6.10 `defhandler`).
    ///
    /// Stores the parsed handler structure for later installation via `handle`.
    Handler(Rc<HandlerDef>),

    /// A mutable reference cell (Clojure-style atom).
    ///
    /// `(atom val)` creates an atom. `(deref a)` reads it.
    /// `(reset! a v)` replaces the value. `(swap! a f)` applies f to current value.
    Atom(Rc<RefCell<Value>>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Unit, Value::Unit) => true,
            (Value::Char(a), Value::Char(b)) => a == b,
            (
                Value::Keyword {
                    ns: ans,
                    name: aname,
                },
                Value::Keyword {
                    ns: bns,
                    name: bname,
                },
            ) => ans == bns && aname == bname,
            (
                Value::Symbol {
                    ns: ans,
                    name: aname,
                },
                Value::Symbol {
                    ns: bns,
                    name: bname,
                },
            ) => ans == bns && aname == bname,
            (Value::Ratio(an, ad), Value::Ratio(bn, bd)) => an == bn && ad == bd,
            (Value::Vec(a), Value::Vec(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => multiset_eq(a, b),
            (
                Value::Adt {
                    type_name: a_type,
                    ctor: a_ctor,
                    fields: a_fields,
                },
                Value::Adt {
                    type_name: b_type,
                    ctor: b_ctor,
                    fields: b_fields,
                },
            ) => a_type == b_type && a_ctor == b_ctor && a_fields == b_fields,
            (Value::Function(a), Value::Function(b)) => Rc::ptr_eq(a, b),
            (Value::NativeFunction(a), Value::NativeFunction(b)) => a == b,
            (Value::NativeClosure { f: af, .. }, Value::NativeClosure { f: bf, .. }) => {
                Rc::ptr_eq(af, bf)
            }
            (Value::Handler(a), Value::Handler(b)) => Rc::ptr_eq(a, b),
            (Value::Atom(a), Value::Atom(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(n) => f.debug_tuple("Int").field(n).finish(),
            Value::Float(n) => f.debug_tuple("Float").field(n).finish(),
            Value::Bool(b) => f.debug_tuple("Bool").field(b).finish(),
            Value::Str(s) => f.debug_tuple("Str").field(s).finish(),
            Value::Unit => write!(f, "Unit"),
            Value::Char(c) => f.debug_tuple("Char").field(c).finish(),
            Value::Keyword { ns, name } => f
                .debug_struct("Keyword")
                .field("ns", ns)
                .field("name", name)
                .finish(),
            Value::Symbol { ns, name } => f
                .debug_struct("Symbol")
                .field("ns", ns)
                .field("name", name)
                .finish(),
            Value::Ratio(n, d) => f.debug_tuple("Ratio").field(n).field(d).finish(),
            Value::Vec(items) => f.debug_tuple("Vec").field(items).finish(),
            Value::Map(entries) => f.debug_tuple("Map").field(entries).finish(),
            Value::Set(items) => f.debug_tuple("Set").field(items).finish(),
            Value::Adt {
                type_name,
                ctor,
                fields,
            } => f
                .debug_struct("Adt")
                .field("type_name", type_name)
                .field("ctor", ctor)
                .field("fields", fields)
                .finish(),
            Value::Function(func) => {
                // Do NOT recurse into captures or module_captures — they can form Rc cycles
                // (e.g., result/map.module_captures["result"] contains result/map itself),
                // which would cause infinite Debug recursion and hang the process.
                f.debug_struct("Function")
                    .field("name", &func.name)
                    .field("arity", &func.arity)
                    .field("captures", &format!("[{} bindings]", func.captures.len()))
                    .field("modules", &format!("[{} aliases]", func.module_captures.len()))
                    .finish_non_exhaustive()
            }
            Value::NativeFunction(nf) => f.debug_tuple("NativeFunction").field(nf).finish(),
            Value::NativeClosure { name, .. } => f
                .debug_struct("NativeClosure")
                .field("name", name)
                .finish_non_exhaustive(),
            Value::Handler(h) => f
                .debug_struct("Handler")
                .field("name", &h.name)
                .finish_non_exhaustive(),
            Value::Atom(cell) => f
                .debug_struct("Atom")
                .field("value", &*cell.borrow())
                .finish(),
        }
    }
}

fn multiset_eq<T: PartialEq>(a: &[T], b: &[T]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut used = vec![false; b.len()];
    'outer: for item in a {
        for (idx, other) in b.iter().enumerate() {
            if !used[idx] && item == other {
                used[idx] = true;
                continue 'outer;
            }
        }
        return false;
    }
    true
}

impl Value {
    /// Return the name of this value's type, used in error messages.
    pub fn type_name(&self) -> &str {
        match self {
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::Bool(_) => "Bool",
            Value::Str(_) => "Str",
            Value::Unit => "Unit",
            Value::Char(_) => "Char",
            Value::Keyword { .. } => "Keyword",
            Value::Symbol { .. } => "Symbol",
            Value::Ratio(_, _) => "Ratio",
            Value::Vec(_) => "Vec",
            Value::Map(_) => "Map",
            Value::Set(_) => "Set",
            Value::Adt { type_name, .. } => type_name,
            Value::Function(_) => "Function",
            Value::NativeFunction(_) => "Function",
            Value::NativeClosure { .. } => "Function",
            Value::Handler(h) => &h.name,
            Value::Atom(_) => "Atom",
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(n) => {
                if n.is_infinite() {
                    if *n > 0.0 {
                        write!(f, "Infinity")
                    } else {
                        write!(f, "-Infinity")
                    }
                } else if n.is_nan() {
                    write!(f, "NaN")
                } else {
                    write!(f, "{n}")
                }
            }
            Value::Bool(b) => write!(f, "{b}"),
            Value::Str(s) => write!(f, "\"{s}\""),
            Value::Unit => write!(f, "unit"),
            Value::Char(c) => {
                let named = match c {
                    '\n' => Some("newline"),
                    '\t' => Some("tab"),
                    '\r' => Some("return"),
                    ' ' => Some("space"),
                    '\0' => Some("null"),
                    _ => None,
                };
                if let Some(name) = named {
                    write!(f, r"\{name}")
                } else if c.is_ascii() && !c.is_ascii_control() {
                    write!(f, r"\{c}")
                } else {
                    write!(f, r"\u{{{:X}}}", *c as u32)
                }
            }
            Value::Keyword { ns, name } => match ns {
                Some(ns) => write!(f, ":{ns}/{name}"),
                None => write!(f, ":{name}"),
            },
            Value::Symbol { ns, name } => match ns {
                Some(ns) => write!(f, "{ns}/{name}"),
                None => write!(f, "{name}"),
            },
            Value::Ratio(n, d) => write!(f, "{n}/{d}"),
            Value::Vec(items) => {
                write!(f, "[")?;
                for (idx, item) in items.iter().enumerate() {
                    if idx > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "]")
            }
            Value::Map(entries) => {
                write!(f, "{{")?;
                for (idx, (key, value)) in entries.iter().enumerate() {
                    if idx > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{key} {value}")?;
                }
                write!(f, "}}")
            }
            Value::Set(items) => {
                write!(f, "#{{")?;
                for (idx, item) in items.iter().enumerate() {
                    if idx > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{item}")?;
                }
                write!(f, "}}")
            }
            Value::Adt { ctor, fields, .. } => {
                if fields.is_empty() {
                    write!(f, "{ctor}")
                } else {
                    write!(f, "({ctor}")?;
                    for field in fields.iter() {
                        write!(f, " {field}")?;
                    }
                    write!(f, ")")
                }
            }
            Value::Function(func) => {
                let name = func.name.as_deref().unwrap_or("<anon>");
                if func.variadic {
                    write!(f, "fn {name}/{}/+", func.arity)
                } else {
                    write!(f, "fn {name}/{}", func.arity)
                }
            }
            Value::NativeFunction(native) => write!(f, "fn {}/native", native.name),
            Value::NativeClosure { name, .. } => write!(f, "fn {name}/closure"),
            Value::Handler(h) => {
                if h.params.is_empty() {
                    write!(f, "handler {}", h.name)
                } else {
                    write!(f, "handler {}/{}", h.name, h.params.len())
                }
            }
            Value::Atom(cell) => write!(f, "#<atom: {}>", cell.borrow()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fn_val(name: Option<&str>, arity: u32, variadic: bool) -> Value {
        Value::Function(Rc::new(Function {
            name: name.map(Rc::from),
            params: vec![],
            rest: None,
            arity,
            variadic,
            captures: vec![],
            module_captures: vec![],
            body: vec![Node::atom(meta::Atom::Unit, meta::span::Span::synthetic())],
            requires: vec![],
            ensures: vec![],
        }))
    }

    #[test]
    fn value_int_roundtrip() {
        assert_eq!(Value::Int(42).to_string(), "42");
    }

    #[test]
    fn value_int_negative() {
        assert_eq!(Value::Int(-1).to_string(), "-1");
    }

    #[test]
    fn value_float_display() {
        assert_eq!(Value::Float(2.5).to_string(), "2.5");
    }

    #[test]
    fn value_float_special() {
        assert_eq!(Value::Float(f64::INFINITY).to_string(), "Infinity");
    }

    #[test]
    fn value_bool_true() {
        assert_eq!(Value::Bool(true).to_string(), "true");
    }

    #[test]
    fn value_bool_false() {
        assert_eq!(Value::Bool(false).to_string(), "false");
    }

    #[test]
    fn value_unit_display() {
        assert_eq!(Value::Unit.to_string(), "unit");
    }

    #[test]
    fn value_char_ascii() {
        assert_eq!(Value::Char('a').to_string(), r"\a");
    }

    #[test]
    fn value_char_unicode() {
        assert_eq!(Value::Char('\u{1F600}').to_string(), r"\u{1F600}");
    }

    #[test]
    fn value_char_named() {
        assert_eq!(Value::Char('\n').to_string(), r"\newline");
    }

    #[test]
    fn value_str_display() {
        let v = Value::Str(Rc::from("hello"));
        assert_eq!(v.to_string(), r#""hello""#);
    }

    #[test]
    fn value_equality_same() {
        assert_eq!(Value::Int(1), Value::Int(1));
        assert_eq!(Value::Bool(true), Value::Bool(true));
        assert_eq!(Value::Unit, Value::Unit);
        assert_eq!(Value::Str(Rc::from("hi")), Value::Str(Rc::from("hi")));
    }

    #[test]
    fn value_equality_different() {
        assert_ne!(Value::Int(1), Value::Bool(true));
        assert_ne!(Value::Int(1), Value::Int(2));
        assert_ne!(Value::Str(Rc::from("a")), Value::Str(Rc::from("b")));
    }

    #[test]
    fn value_type_name_each_variant() {
        assert_eq!(Value::Int(0).type_name(), "Int");
        assert_eq!(Value::Float(0.0).type_name(), "Float");
        assert_eq!(Value::Bool(true).type_name(), "Bool");
        assert_eq!(Value::Str(Rc::from("")).type_name(), "Str");
        assert_eq!(Value::Unit.type_name(), "Unit");
        assert_eq!(Value::Char('a').type_name(), "Char");
        assert_eq!(
            Value::Keyword {
                ns: None,
                name: Rc::from("k")
            }
            .type_name(),
            "Keyword"
        );
        assert_eq!(
            Value::Symbol {
                ns: None,
                name: Rc::from("s")
            }
            .type_name(),
            "Symbol"
        );
        assert_eq!(Value::Ratio(1, 2).type_name(), "Ratio");
    }

    #[test]
    fn value_adt_display_none_some() {
        let none = Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("None"),
            fields: Rc::new(vec![]),
        };
        let some = Value::Adt {
            type_name: Rc::from("Option"),
            ctor: Rc::from("Some"),
            fields: Rc::new(vec![Value::Int(1)]),
        };

        assert_eq!(none.to_string(), "None");
        assert_eq!(some.to_string(), "(Some 1)");
    }

    #[test]
    fn value_ratio_display() {
        assert_eq!(Value::Ratio(1, 3).to_string(), "1/3");
    }

    #[test]
    fn value_vec_display() {
        let v = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        assert_eq!(v.to_string(), "[1 2 3]");
    }

    #[test]
    fn value_map_display() {
        let v = Value::Map(Rc::new(
            vec![
                (kw("a"), Value::Int(1)),
                (kw("b"), Value::Int(2)),
            ]
            .into(),
        ));
        assert_eq!(v.to_string(), "{:a 1 :b 2}");
    }

    #[test]
    fn value_set_display() {
        let v = Value::Set(Rc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        assert_eq!(v.to_string(), "#{1 2 3}");
    }

    #[test]
    fn value_vec_equality_structural() {
        let a = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2)]));
        let b = Value::Vec(Rc::new(vec![Value::Int(1), Value::Int(2)]));
        let c = Value::Vec(Rc::new(vec![Value::Int(2), Value::Int(1)]));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn value_map_equality_structural() {
        let a = Value::Map(Rc::new(
            vec![(kw("a"), Value::Int(1)), (kw("b"), Value::Int(2))].into(),
        ));
        let b = Value::Map(Rc::new(
            vec![(kw("b"), Value::Int(2)), (kw("a"), Value::Int(1))].into(),
        ));
        let c = Value::Map(Rc::new(vec![(kw("a"), Value::Int(2))].into()));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn value_set_equality_structural() {
        let a = Value::Set(Rc::new(vec![Value::Int(1), Value::Int(2)]));
        let b = Value::Set(Rc::new(vec![Value::Int(2), Value::Int(1)]));
        let c = Value::Set(Rc::new(vec![Value::Int(1), Value::Int(3)]));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn value_type_name_collections() {
        assert_eq!(Value::Vec(Rc::new(vec![Value::Int(1)])).type_name(), "Vec");
        assert_eq!(
            Value::Map(Rc::new(vec![(kw("a"), Value::Int(1))].into())).type_name(),
            "Map"
        );
        assert_eq!(Value::Set(Rc::new(vec![Value::Int(1)])).type_name(), "Set");
    }

    #[test]
    fn value_keyword_bare() {
        let v = Value::Keyword {
            ns: None,
            name: Rc::from("foo"),
        };
        assert_eq!(v.to_string(), ":foo");
    }

    #[test]
    fn value_keyword_namespaced() {
        let v = Value::Keyword {
            ns: Some(Rc::from("bar")),
            name: Rc::from("baz"),
        };
        assert_eq!(v.to_string(), ":bar/baz");
    }

    #[test]
    fn value_symbol_bare() {
        let v = Value::Symbol {
            ns: None,
            name: Rc::from("add"),
        };
        assert_eq!(v.to_string(), "add");
    }

    #[test]
    fn value_symbol_qualified() {
        let v = Value::Symbol {
            ns: Some(Rc::from("math")),
            name: Rc::from("sqrt"),
        };
        assert_eq!(v.to_string(), "math/sqrt");
    }

    #[test]
    fn function_display_named_arity() {
        let v = fn_val(Some("add"), 2, false);
        assert_eq!(v.to_string(), "fn add/2");
    }

    #[test]
    fn function_display_anonymous() {
        let v = fn_val(None, 1, false);
        assert_eq!(v.to_string(), "fn <anon>/1");
    }

    #[test]
    fn function_display_variadic() {
        let v = fn_val(Some("printf"), 1, true);
        assert_eq!(v.to_string(), "fn printf/1/+");
    }

    #[test]
    fn function_type_name_function() {
        let v = fn_val(Some("add"), 2, false);
        assert_eq!(v.type_name(), "Function");
    }

    #[test]
    fn function_equality_is_identity() {
        let f1 = fn_val(None, 1, false);
        let f1_clone = f1.clone();
        let f2 = fn_val(None, 1, false);

        assert_eq!(f1, f1_clone);
        assert_ne!(f1, f2);
    }

    #[test]
    fn function_captures_preserved() {
        let captured = [Value::Int(10), Value::Str(Rc::from("hi"))];
        let func = Value::Function(Rc::new(Function {
            name: Some(Rc::from("adder")),
            params: vec![],
            rest: None,
            arity: 1,
            variadic: false,
            captures: captured
                .iter()
                .enumerate()
                .map(|(i, v)| (Rc::from(format!("c{i}").as_str()), v.clone()))
                .collect(),
            module_captures: vec![],
            body: vec![Node::atom(meta::Atom::Unit, meta::span::Span::synthetic())],
            requires: vec![],
            ensures: vec![],
        }));

        let Value::Function(rc_func) = func else {
            panic!("not a function")
        };
        assert_eq!(rc_func.captures.len(), captured.len());
    }

    #[test]
    fn value_str_clone_is_cheap() {
        let v = Value::Str(Rc::from("hello"));
        let Value::Str(ref rc1) = v else { panic!() };
        let v2 = v.clone();
        let Value::Str(ref rc2) = v2 else { panic!() };
        assert!(Rc::ptr_eq(rc1, rc2));
    }

    // NexlMap tests

    fn kw(name: &str) -> Value {
        Value::Keyword {
            ns: None,
            name: Rc::from(name),
        }
    }

    #[test]
    fn nexlmap_new_is_empty() {
        let m = NexlMap::new();
        assert_eq!(m.len(), 0);
        assert!(m.is_empty());
    }

    #[test]
    fn nexlmap_from_pairs_keyword_lookup() {
        let m = NexlMap::from_pairs(vec![(kw("a"), Value::Int(1))]);
        assert_eq!(m.len(), 1);
        assert_eq!(m.get(&kw("a")), Some(&Value::Int(1)));
        assert_eq!(m.get(&kw("b")), None);
    }

    #[test]
    fn nexlmap_from_pairs_int_key() {
        let m = NexlMap::from_pairs(vec![(Value::Int(42), Value::Bool(true))]);
        assert_eq!(m.get(&Value::Int(42)), Some(&Value::Bool(true)));
        assert_eq!(m.get(&Value::Int(0)), None);
    }

    #[test]
    fn nexlmap_from_pairs_string_key() {
        let m = NexlMap::from_pairs(vec![(
            Value::Str(Rc::from("hello")),
            Value::Int(99),
        )]);
        assert_eq!(m.get(&Value::Str(Rc::from("hello"))), Some(&Value::Int(99)));
        assert_eq!(m.get(&Value::Str(Rc::from("world"))), None);
    }

    #[test]
    fn nexlmap_put_new_key() {
        let m = NexlMap::from_pairs(vec![(kw("a"), Value::Int(1))]);
        let m2 = m.put(kw("b"), Value::Int(2));
        assert_eq!(m2.len(), 2);
        assert_eq!(m2.get(&kw("b")), Some(&Value::Int(2)));
    }

    #[test]
    fn nexlmap_put_existing_key_replaces() {
        let m = NexlMap::from_pairs(vec![(kw("a"), Value::Int(1))]);
        let m2 = m.put(kw("a"), Value::Int(99));
        assert_eq!(m2.len(), 1);
        assert_eq!(m2.get(&kw("a")), Some(&Value::Int(99)));
    }

    #[test]
    fn nexlmap_put_is_persistent() {
        let m = NexlMap::from_pairs(vec![(kw("a"), Value::Int(1))]);
        let _m2 = m.put(kw("b"), Value::Int(2));
        // Original map unchanged
        assert_eq!(m.len(), 1);
        assert_eq!(m.get(&kw("b")), None);
    }

    #[test]
    fn nexlmap_remove_existing() {
        let m = NexlMap::from_pairs(vec![(kw("a"), Value::Int(1)), (kw("b"), Value::Int(2))]);
        let m2 = m.remove(&kw("a"));
        assert_eq!(m2.len(), 1);
        assert_eq!(m2.get(&kw("a")), None);
        assert_eq!(m2.get(&kw("b")), Some(&Value::Int(2)));
    }

    #[test]
    fn nexlmap_remove_missing_noop() {
        let m = NexlMap::from_pairs(vec![(kw("a"), Value::Int(1))]);
        let m2 = m.remove(&kw("z"));
        assert_eq!(m2.len(), 1);
        assert_eq!(m2.get(&kw("a")), Some(&Value::Int(1)));
    }

    #[test]
    fn nexlmap_contains() {
        let m = NexlMap::from_pairs(vec![(kw("a"), Value::Int(1))]);
        assert!(m.contains(&kw("a")));
        assert!(!m.contains(&kw("b")));
    }

    #[test]
    fn nexlmap_equality_order_independent() {
        let ab = NexlMap::from_pairs(vec![(kw("a"), Value::Int(1)), (kw("b"), Value::Int(2))]);
        let ba = NexlMap::from_pairs(vec![(kw("b"), Value::Int(2)), (kw("a"), Value::Int(1))]);
        assert_eq!(ab, ba);
    }

    #[test]
    fn nexlmap_inequality() {
        let m1 = NexlMap::from_pairs(vec![(kw("a"), Value::Int(1))]);
        let m2 = NexlMap::from_pairs(vec![(kw("a"), Value::Int(2))]);
        let m3 = NexlMap::from_pairs(vec![(kw("b"), Value::Int(1))]);
        assert_ne!(m1, m2);
        assert_ne!(m1, m3);
    }

    #[test]
    fn nexlmap_iter_insertion_order() {
        let m = NexlMap::from_pairs(vec![
            (kw("c"), Value::Int(3)),
            (kw("a"), Value::Int(1)),
            (kw("b"), Value::Int(2)),
        ]);
        let keys: Vec<&Value> = m.keys().collect();
        assert_eq!(keys, vec![&kw("c"), &kw("a"), &kw("b")]);
    }

    #[test]
    fn nexlmap_duplicate_key_last_wins() {
        let m = NexlMap::from_pairs(vec![
            (kw("a"), Value::Int(1)),
            (kw("a"), Value::Int(99)),
        ]);
        assert_eq!(m.len(), 1);
        assert_eq!(m.get(&kw("a")), Some(&Value::Int(99)));
    }
}
