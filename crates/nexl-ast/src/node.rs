use crate::span::Span;

// ---------------------------------------------------------------------------
// Integer / float width suffixes
// ---------------------------------------------------------------------------

/// Width suffix on an integer literal, e.g. `i32` in `42i32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntSuffix {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
}

/// Width suffix on a float literal, e.g. `f32` in `3.14f32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloatSuffix {
    /// 32-bit single-precision (`f32`).
    F32,
    /// 64-bit double-precision (`f64`) — alias for the unsuffixed `Float` type.
    F64,
}

// ---------------------------------------------------------------------------
// Atom — leaf values
// ---------------------------------------------------------------------------

/// An atomic (leaf) value produced by the reader.
#[derive(Debug, Clone, PartialEq)]
pub enum Atom {
    /// Integer literal. `suffix` carries a width annotation when one was written.
    ///
    /// Stored as `i128` to accommodate the full range of both `i64` (negative values)
    /// and `u64` (large unsigned values) without loss. Range-checking against the
    /// suffix is deferred to the type-checking pass.
    Int {
        value: i128,
        suffix: Option<IntSuffix>,
    },

    /// Floating-point literal. `suffix` is `None` for unsuffixed literals (which
    /// default to `Float` / `f64`).
    Float {
        value: f64,
        suffix: Option<FloatSuffix>,
    },

    /// Exact rational number literal, e.g. `3/4`.
    ///
    /// Auto-simplified by the reader: `6/4` → `Ratio { numer: 3, denom: 2 }`.
    Ratio { numer: i64, denom: i64 },

    /// Boolean literal: `true` or `false`.
    Bool(bool),

    /// Unicode scalar-value literal, e.g. `\a`, `\newline`, `\u{1F600}`.
    Char(char),

    /// String literal (raw contents from the reader, with escape sequences already
    /// resolved). Interpolation spans `{...}` are retained as-is in this string and
    /// are resolved by a later compiler pass.
    Str(String),

    /// Keyword, e.g. `:status` or `:http/ok`.
    Keyword {
        /// Optional namespace, e.g. `http` in `:http/ok`.
        ns: Option<String>,
        name: String,
    },

    /// Symbol (an identifier reference), e.g. `add` or `my-module/my-fn`.
    Symbol {
        /// Optional qualifying namespace, e.g. `my-module` in `my-module/my-fn`.
        ns: Option<String>,
        name: String,
    },

    /// The `unit` literal — the sole value of the `Unit` type (ADR-001).
    Unit,
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

/// A single-line comment from the source file.
///
/// Stores the comment text after the leading `;`, not including the `;` itself
/// or the trailing newline. Attached to surrounding `Node`s so the pretty-printer
/// can reproduce them for round-trip formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment(pub String);

// ---------------------------------------------------------------------------
// Node
// ---------------------------------------------------------------------------

/// A Nexl reader AST node: a structural form, its source span, and any attached
/// comments needed for round-trip pretty-printing.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// The structural content of this node.
    pub kind: NodeKind,
    /// Byte range in the source file where this node begins and ends.
    pub span: Span,
    /// Comments that appear on lines immediately before this node.
    pub leading_comments: Vec<Comment>,
    /// An inline comment on the same source line as this node's last token.
    pub trailing_comment: Option<Comment>,
}

impl Node {
    /// Construct a node with no attached comments.
    pub fn new(kind: NodeKind, span: Span) -> Self {
        Self {
            kind,
            span,
            leading_comments: Vec::new(),
            trailing_comment: None,
        }
    }

    /// Convenience: wrap an `Atom` into a node.
    pub fn atom(atom: Atom, span: Span) -> Self {
        Self::new(NodeKind::Atom(atom), span)
    }
}

// ---------------------------------------------------------------------------
// NodeKind
// ---------------------------------------------------------------------------

/// The structural kind of a [`Node`].
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// A leaf value.
    Atom(Atom),

    // --- Compound forms ---
    /// S-expression list, e.g. `(f x y)`.
    List(Vec<Node>),
    /// Vector literal, e.g. `[1 2 3]`.
    Vector(Vec<Node>),
    /// Map literal, e.g. `{:a 1 :b 2}`.
    ///
    /// The reader enforces an even number of forms; stored here as key–value pairs.
    Map(Vec<(Node, Node)>),
    /// Set literal, e.g. `#{1 2 3}`.
    Set(Vec<Node>),

    // --- Reader macro expansions ---
    /// `'x` — sugar for `(quote x)`.
    Quote(Box<Node>),
    /// `@x` — sugar for `(deref x)`.
    Deref(Box<Node>),
    /// `#_ x` — discard reader macro.
    ///
    /// The discarded form is **not** included in semantic analysis, but is retained
    /// in the tree so that tooling and the pretty-printer can reproduce it faithfully.
    Discard(Box<Node>),
    /// `` `x `` — quasiquote prefix; sugar for `(quasiquote x)` (spec §D.2).
    Quasiquote(Box<Node>),
    /// `~x` — unquote prefix inside a quasiquote; sugar for `(unquote x)` (spec §D.2).
    Unquote(Box<Node>),
    /// `~@x` — unquote-splice prefix; sugar for `(unquote-splice x)` (spec §D.2).
    UnquoteSplice(Box<Node>),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::{FileId, Span};

    fn dummy_span() -> Span {
        Span::synthetic()
    }

    fn file_span(start: u32, len: u32) -> Span {
        Span::new(FileId(0), start, len)
    }

    // --- Atom construction ---

    #[test]
    fn atom_int_unsuffixed() {
        let a = Atom::Int {
            value: 42,
            suffix: None,
        };
        assert_eq!(
            a,
            Atom::Int {
                value: 42,
                suffix: None
            }
        );
    }

    #[test]
    fn atom_int_with_suffix() {
        let a = Atom::Int {
            value: 255,
            suffix: Some(IntSuffix::U8),
        };
        match a {
            Atom::Int {
                value,
                suffix: Some(IntSuffix::U8),
            } => assert_eq!(value, 255),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn atom_int_negative_fits_i128() {
        let a = Atom::Int {
            value: i64::MIN as i128,
            suffix: None,
        };
        assert_eq!(
            a,
            Atom::Int {
                value: -9223372036854775808,
                suffix: None
            }
        );
    }

    #[test]
    fn atom_float_unsuffixed() {
        let a = Atom::Float {
            value: 2.5,
            suffix: None,
        };
        match a {
            Atom::Float {
                value,
                suffix: None,
            } => assert!((value - 2.5).abs() < 1e-10),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn atom_float_f32_suffix() {
        let a = Atom::Float {
            value: 2.5,
            suffix: Some(FloatSuffix::F32),
        };
        assert_eq!(
            a,
            Atom::Float {
                value: 2.5,
                suffix: Some(FloatSuffix::F32)
            }
        );
    }

    #[test]
    fn atom_ratio() {
        let a = Atom::Ratio { numer: 1, denom: 3 };
        match a {
            Atom::Ratio { numer: 1, denom: 3 } => {}
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn atom_bool_true_false() {
        assert_eq!(Atom::Bool(true), Atom::Bool(true));
        assert_ne!(Atom::Bool(true), Atom::Bool(false));
    }

    #[test]
    fn atom_char() {
        assert_eq!(Atom::Char('a'), Atom::Char('a'));
        assert_eq!(Atom::Char('😀'), Atom::Char('\u{1F600}'));
    }

    #[test]
    fn atom_str() {
        let a = Atom::Str("hello {name}!".to_string());
        match &a {
            Atom::Str(s) => assert_eq!(s, "hello {name}!"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn atom_keyword_bare() {
        let a = Atom::Keyword {
            ns: None,
            name: "status".to_string(),
        };
        match &a {
            Atom::Keyword { ns: None, name } => assert_eq!(name, "status"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn atom_keyword_namespaced() {
        let a = Atom::Keyword {
            ns: Some("http".to_string()),
            name: "ok".to_string(),
        };
        match &a {
            Atom::Keyword { ns: Some(ns), name } => {
                assert_eq!(ns, "http");
                assert_eq!(name, "ok");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn atom_symbol_bare() {
        let a = Atom::Symbol {
            ns: None,
            name: "add".to_string(),
        };
        match &a {
            Atom::Symbol { ns: None, name } => assert_eq!(name, "add"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn atom_symbol_qualified() {
        let a = Atom::Symbol {
            ns: Some("math".to_string()),
            name: "sqrt".to_string(),
        };
        match &a {
            Atom::Symbol { ns: Some(ns), name } => {
                assert_eq!(ns, "math");
                assert_eq!(name, "sqrt");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn atom_unit() {
        assert_eq!(Atom::Unit, Atom::Unit);
    }

    // --- Node construction ---

    #[test]
    fn node_new_has_no_comments() {
        let n = Node::new(NodeKind::Atom(Atom::Unit), dummy_span());
        assert!(n.leading_comments.is_empty());
        assert!(n.trailing_comment.is_none());
    }

    #[test]
    fn node_atom_convenience() {
        let span = file_span(0, 4);
        let n = Node::atom(Atom::Unit, span);
        assert_eq!(n.span, span);
        assert_eq!(n.kind, NodeKind::Atom(Atom::Unit));
    }

    #[test]
    fn node_span_is_stored() {
        let span = file_span(10, 5);
        let n = Node::new(NodeKind::Atom(Atom::Bool(true)), span);
        assert_eq!(n.span, span);
    }

    // --- NodeKind variants ---

    #[test]
    fn node_kind_list() {
        let inner = Node::atom(
            Atom::Int {
                value: 1,
                suffix: None,
            },
            dummy_span(),
        );
        let list = NodeKind::List(vec![inner]);
        match list {
            NodeKind::List(items) => assert_eq!(items.len(), 1),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn node_kind_vector() {
        let v = NodeKind::Vector(vec![]);
        assert_eq!(v, NodeKind::Vector(vec![]));
    }

    #[test]
    fn node_kind_map_stores_pairs() {
        let key = Node::atom(
            Atom::Keyword {
                ns: None,
                name: "a".into(),
            },
            dummy_span(),
        );
        let val = Node::atom(
            Atom::Int {
                value: 1,
                suffix: None,
            },
            dummy_span(),
        );
        let map = NodeKind::Map(vec![(key, val)]);
        match map {
            NodeKind::Map(pairs) => assert_eq!(pairs.len(), 1),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn node_kind_set() {
        let s = NodeKind::Set(vec![]);
        assert_eq!(s, NodeKind::Set(vec![]));
    }

    #[test]
    fn node_kind_quote() {
        let inner = Node::atom(
            Atom::Symbol {
                ns: None,
                name: "x".into(),
            },
            dummy_span(),
        );
        let q = NodeKind::Quote(Box::new(inner));
        match q {
            NodeKind::Quote(n) => assert_eq!(
                n.kind,
                NodeKind::Atom(Atom::Symbol {
                    ns: None,
                    name: "x".into()
                })
            ),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn node_kind_deref() {
        let inner = Node::atom(
            Atom::Symbol {
                ns: None,
                name: "counter".into(),
            },
            dummy_span(),
        );
        let d = NodeKind::Deref(Box::new(inner));
        match d {
            NodeKind::Deref(_) => {}
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn node_kind_discard_retains_form() {
        let inner = Node::atom(Atom::Bool(false), dummy_span());
        let d = NodeKind::Discard(Box::new(inner.clone()));
        match d {
            NodeKind::Discard(n) => assert_eq!(*n, inner),
            _ => panic!("wrong variant"),
        }
    }

    // --- Comment attachment ---

    #[test]
    fn node_leading_comments() {
        let mut n = Node::new(NodeKind::Atom(Atom::Unit), dummy_span());
        n.leading_comments
            .push(Comment("top-level function".to_string()));
        assert_eq!(n.leading_comments.len(), 1);
        assert_eq!(n.leading_comments[0].0, "top-level function");
    }

    #[test]
    fn node_trailing_comment() {
        let mut n = Node::atom(
            Atom::Int {
                value: 42,
                suffix: None,
            },
            dummy_span(),
        );
        n.trailing_comment = Some(Comment("the answer".to_string()));
        assert_eq!(n.trailing_comment.as_ref().unwrap().0, "the answer");
    }
}
