use meta::Node;
use std::rc::Rc;

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
    /// Body expressions to evaluate when called (in order).
    pub body: Vec<Node>,
}

/// A runtime value produced by the Nexl tree-walk interpreter.
///
/// This is distinct from the reader's `Atom` type: `Atom` is a *source-level*
/// representation with suffix annotations and raw text; `Value` is the
/// *evaluated* form that the interpreter operates on.
#[derive(Debug, Clone)]
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
    /// Persistent map value.
    Map(Rc<Vec<(Value, Value)>>),
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
            (Value::Map(a), Value::Map(b)) => multiset_eq(a, b),
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
            _ => false,
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
            body: vec![Node::atom(meta::Atom::Unit, meta::span::Span::synthetic())],
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
        let v = Value::Map(Rc::new(vec![
            (
                Value::Keyword {
                    ns: None,
                    name: Rc::from("a"),
                },
                Value::Int(1),
            ),
            (
                Value::Keyword {
                    ns: None,
                    name: Rc::from("b"),
                },
                Value::Int(2),
            ),
        ]));
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
        let a = Value::Map(Rc::new(vec![
            (
                Value::Keyword {
                    ns: None,
                    name: Rc::from("a"),
                },
                Value::Int(1),
            ),
            (
                Value::Keyword {
                    ns: None,
                    name: Rc::from("b"),
                },
                Value::Int(2),
            ),
        ]));
        let b = Value::Map(Rc::new(vec![
            (
                Value::Keyword {
                    ns: None,
                    name: Rc::from("b"),
                },
                Value::Int(2),
            ),
            (
                Value::Keyword {
                    ns: None,
                    name: Rc::from("a"),
                },
                Value::Int(1),
            ),
        ]));
        let c = Value::Map(Rc::new(vec![(
            Value::Keyword {
                ns: None,
                name: Rc::from("a"),
            },
            Value::Int(2),
        )]));
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
        assert_eq!(
            Value::Vec(Rc::new(vec![Value::Int(1)])).type_name(),
            "Vec"
        );
        assert_eq!(
            Value::Map(Rc::new(vec![(
                Value::Keyword {
                    ns: None,
                    name: Rc::from("a"),
                },
                Value::Int(1),
            )]))
            .type_name(),
            "Map"
        );
        assert_eq!(
            Value::Set(Rc::new(vec![Value::Int(1)])).type_name(),
            "Set"
        );
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
            body: vec![Node::atom(meta::Atom::Unit, meta::span::Span::synthetic())],
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
}
