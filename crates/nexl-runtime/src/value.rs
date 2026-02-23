use std::rc::Rc;

/// A runtime value produced by the Nexl tree-walk interpreter.
///
/// This is distinct from the reader's `Atom` type: `Atom` is a *source-level*
/// representation with suffix annotations and raw text; `Value` is the
/// *evaluated* form that the interpreter operates on.
#[derive(Debug, Clone, PartialEq)]
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
    Keyword {
        ns: Option<Rc<str>>,
        name: Rc<str>,
    },
    /// Symbol (an identifier), e.g. `add` or `math/sqrt`.
    Symbol {
        ns: Option<Rc<str>>,
        name: Rc<str>,
    },
    /// Exact rational number, stored in lowest terms.
    Ratio(i64, i64),
}

impl Value {
    /// Return the name of this value's type, used in error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_)     => "Int",
            Value::Float(_)   => "Float",
            Value::Bool(_)    => "Bool",
            Value::Str(_)     => "Str",
            Value::Unit       => "Unit",
            Value::Char(_)    => "Char",
            Value::Keyword { .. } => "Keyword",
            Value::Symbol { .. }  => "Symbol",
            Value::Ratio(_, _)    => "Ratio",
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(n) => {
                if n.is_infinite() {
                    if *n > 0.0 { write!(f, "Infinity") } else { write!(f, "-Infinity") }
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
                    ' '  => Some("space"),
                    '\0' => Some("null"),
                    _    => None,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Value::Keyword { ns: None, name: Rc::from("k") }.type_name(),
            "Keyword"
        );
        assert_eq!(
            Value::Symbol { ns: None, name: Rc::from("s") }.type_name(),
            "Symbol"
        );
        assert_eq!(Value::Ratio(1, 2).type_name(), "Ratio");
    }

    #[test]
    fn value_ratio_display() {
        assert_eq!(Value::Ratio(1, 3).to_string(), "1/3");
    }

    #[test]
    fn value_keyword_bare() {
        let v = Value::Keyword { ns: None, name: Rc::from("foo") };
        assert_eq!(v.to_string(), ":foo");
    }

    #[test]
    fn value_keyword_namespaced() {
        let v = Value::Keyword { ns: Some(Rc::from("bar")), name: Rc::from("baz") };
        assert_eq!(v.to_string(), ":bar/baz");
    }

    #[test]
    fn value_symbol_bare() {
        let v = Value::Symbol { ns: None, name: Rc::from("add") };
        assert_eq!(v.to_string(), "add");
    }

    #[test]
    fn value_symbol_qualified() {
        let v = Value::Symbol { ns: Some(Rc::from("math")), name: Rc::from("sqrt") };
        assert_eq!(v.to_string(), "math/sqrt");
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
