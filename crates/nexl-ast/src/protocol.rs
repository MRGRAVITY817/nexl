//! Protocol declaration types, parsed from raw AST nodes.
//!
//! Follows the same pattern as [`crate::effect`]: semantic types plus
//! parsing functions that convert generic [`Node`] trees into structured
//! representations.

use crate::{Atom, Node, NodeKind};

// ---------------------------------------------------------------------------
// Protocol declaration
// ---------------------------------------------------------------------------

/// A parsed `(defprotocol Name ...)` declaration (spec §5.11).
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolDecl {
    /// Protocol name, e.g. `"Show"`.
    pub name: String,
    /// Optional documentation string.
    pub doc: Option<String>,
    /// Type parameters, e.g. `["a"]` for `(defprotocol Foldable [a] ...)`.
    pub type_params: Vec<String>,
    /// Protocols this one extends, e.g. `["Eq"]` for `:extends [Eq]`.
    pub extends: Vec<String>,
    /// Operation declarations.
    pub operations: Vec<ProtocolOpDecl>,
}

/// A single operation within a protocol declaration.
///
/// The type annotation is stored as a raw [`Node`] tree; the type system
/// interprets it in a later pass.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolOpDecl {
    /// Operation name, e.g. `"show"`.
    pub name: String,
    /// The raw type annotation node, e.g. `(Fn [Self] -> Str)`.
    pub type_node: Node,
    /// Optional default implementation body (raw AST).
    pub default_body: Option<Node>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// An error encountered while parsing a protocol declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolParseError {
    /// Human-readable error description.
    pub description: String,
}

impl ProtocolParseError {
    /// Create a new error with the given description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

impl std::fmt::Display for ProtocolParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "protocol parse error: {}", self.description)
    }
}

impl std::error::Error for ProtocolParseError {}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a `(defprotocol Name ...)` list into a [`ProtocolDecl`].
///
/// The caller is responsible for identifying that the list head is `defprotocol`.
/// `items` should be the full list contents including the `defprotocol` symbol.
pub fn parse_protocol_decl(items: &[Node]) -> Result<ProtocolDecl, ProtocolParseError> {
    // items[0] = `defprotocol`, items[1] = name, rest = optional doc, type-params, :extends, operations
    if items.len() < 2 {
        return Err(ProtocolParseError::new("defprotocol requires a name"));
    }

    let name = extract_plain_symbol(&items[1])?;

    let mut idx = 2;
    let mut doc = None;
    let mut type_params = Vec::new();
    let mut extends = Vec::new();
    let mut operations = Vec::new();

    // Optional documentation string
    if idx < items.len()
        && let NodeKind::Atom(Atom::Str(s)) = &items[idx].kind
    {
        doc = Some(s.clone());
        idx += 1;
    }

    // Optional type parameters: [a b ...]
    if idx < items.len()
        && let NodeKind::Vector(params) = &items[idx].kind
    {
        for p in params {
            type_params.push(extract_plain_symbol(p)?);
        }
        idx += 1;
    }

    // Optional :extends [Proto1 Proto2 ...]
    if idx + 1 < items.len()
        && let NodeKind::Atom(Atom::Keyword {
            ns: None,
            name: kw_name,
        }) = &items[idx].kind
        && kw_name == "extends"
    {
        idx += 1;
        if let NodeKind::Vector(protos) = &items[idx].kind {
            for p in protos {
                extends.push(extract_plain_symbol(p)?);
            }
            idx += 1;
        } else {
            return Err(ProtocolParseError::new(
                ":extends must be followed by a vector of protocol names",
            ));
        }
    }

    // Remaining items are operation declarations
    while idx < items.len() {
        operations.push(parse_protocol_op(&items[idx])?);
        idx += 1;
    }

    Ok(ProtocolDecl {
        name,
        doc,
        type_params,
        extends,
        operations,
    })
}

/// Parse a single protocol operation declaration.
///
/// Accepted forms:
/// - `(op-name : Type)` — operation without default
/// - `(op-name : Type :default body)` — operation with default implementation
fn parse_protocol_op(node: &Node) -> Result<ProtocolOpDecl, ProtocolParseError> {
    let items = match &node.kind {
        NodeKind::List(items) => items,
        _ => {
            return Err(ProtocolParseError::new(
                "expected an operation declaration list (op-name : Type)",
            ));
        }
    };

    if items.len() < 3 {
        return Err(ProtocolParseError::new(
            "operation declaration requires (op-name : Type)",
        ));
    }

    let op_name = extract_plain_symbol(&items[0])?;

    // items[1] must be the `:` separator
    match &items[1].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == ":" => {}
        _ => {
            return Err(ProtocolParseError::new(format!(
                "expected `:` after operation name `{op_name}`"
            )));
        }
    }

    let type_node = items[2].clone();

    // Check for optional :default body
    let mut default_body = None;
    if items.len() >= 5
        && let NodeKind::Atom(Atom::Keyword {
            ns: None,
            name: kw_name,
        }) = &items[3].kind
        && kw_name == "default"
    {
        default_body = Some(items[4].clone());
    }

    Ok(ProtocolOpDecl {
        name: op_name,
        type_node,
        default_body,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a plain (unqualified) symbol name from a node.
fn extract_plain_symbol(node: &Node) -> Result<String, ProtocolParseError> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => Ok(name.clone()),
        _ => Err(ProtocolParseError::new("expected a symbol")),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::Span;

    fn s() -> Span {
        Span::synthetic()
    }

    fn sym(name: &str) -> Node {
        Node::atom(
            Atom::Symbol {
                ns: None,
                name: name.to_string(),
            },
            s(),
        )
    }

    fn kw(name: &str) -> Node {
        Node::atom(
            Atom::Keyword {
                ns: None,
                name: name.to_string(),
            },
            s(),
        )
    }

    fn str_node(text: &str) -> Node {
        Node::atom(Atom::Str(text.to_string()), s())
    }

    fn colon() -> Node {
        sym(":")
    }

    fn list(children: Vec<Node>) -> Node {
        Node::new(NodeKind::List(children), s())
    }

    fn vec_node(children: Vec<Node>) -> Node {
        Node::new(NodeKind::Vector(children), s())
    }

    fn fn_type(args: Vec<Node>, ret: Node) -> Node {
        list(vec![sym("Fn"), vec_node(args), sym("->"), ret])
    }

    fn op(name: &str, type_ann: Node) -> Node {
        list(vec![sym(name), colon(), type_ann])
    }

    fn op_with_default(name: &str, type_ann: Node, default_body: Node) -> Node {
        list(vec![
            sym(name),
            colon(),
            type_ann,
            kw("default"),
            default_body,
        ])
    }

    // ── Test 7 ──

    #[test]
    fn test_parse_protocol_single_op() {
        // (defprotocol Show (show : (Fn [Self] -> Str)))
        let show_type = fn_type(vec![sym("Self")], sym("Str"));
        let items = vec![
            sym("defprotocol"),
            sym("Show"),
            op("show", show_type.clone()),
        ];
        let decl = parse_protocol_decl(&items).unwrap();
        assert_eq!(decl.name, "Show");
        assert!(decl.doc.is_none());
        assert!(decl.type_params.is_empty());
        assert!(decl.extends.is_empty());
        assert_eq!(decl.operations.len(), 1);
        assert_eq!(decl.operations[0].name, "show");
        assert_eq!(decl.operations[0].type_node, show_type);
        assert!(decl.operations[0].default_body.is_none());
    }

    // ── Test 8 ──

    #[test]
    fn test_parse_protocol_with_doc() {
        // (defprotocol Show "Convert a value to a string." (show : (Fn [Self] -> Str)))
        let show_type = fn_type(vec![sym("Self")], sym("Str"));
        let items = vec![
            sym("defprotocol"),
            sym("Show"),
            str_node("Convert a value to a string."),
            op("show", show_type),
        ];
        let decl = parse_protocol_decl(&items).unwrap();
        assert_eq!(decl.name, "Show");
        assert_eq!(decl.doc.as_deref(), Some("Convert a value to a string."));
        assert_eq!(decl.operations.len(), 1);
    }

    // ── Test 9 ──

    #[test]
    fn test_parse_protocol_with_extends() {
        // (defprotocol Ord :extends [Eq] (compare : (Fn [Self Self] -> Keyword)))
        let cmp_type = fn_type(vec![sym("Self"), sym("Self")], sym("Keyword"));
        let items = vec![
            sym("defprotocol"),
            sym("Ord"),
            kw("extends"),
            vec_node(vec![sym("Eq")]),
            op("compare", cmp_type),
        ];
        let decl = parse_protocol_decl(&items).unwrap();
        assert_eq!(decl.name, "Ord");
        assert_eq!(decl.extends, vec!["Eq".to_string()]);
        assert_eq!(decl.operations.len(), 1);
        assert_eq!(decl.operations[0].name, "compare");
    }

    // ── Test 10 ──

    #[test]
    fn test_parse_protocol_with_type_params() {
        // (defprotocol Foldable [a] (fold : (Fn [(Fn [b a] -> b) b Self] -> b)))
        let fold_type = fn_type(
            vec![
                fn_type(vec![sym("b"), sym("a")], sym("b")),
                sym("b"),
                sym("Self"),
            ],
            sym("b"),
        );
        let items = vec![
            sym("defprotocol"),
            sym("Foldable"),
            vec_node(vec![sym("a")]),
            op("fold", fold_type),
        ];
        let decl = parse_protocol_decl(&items).unwrap();
        assert_eq!(decl.name, "Foldable");
        assert_eq!(decl.type_params, vec!["a".to_string()]);
        assert_eq!(decl.operations.len(), 1);
        assert_eq!(decl.operations[0].name, "fold");
    }

    // ── Test 11 ──

    #[test]
    fn test_parse_protocol_op_with_default() {
        // (defprotocol Foldable [a]
        //   (fold : (Fn [Self] -> Int))
        //   (count : (Fn [Self] -> Int) :default (fn [self] (fold (fn [n _] (+ n 1)) 0 self))))
        let fold_type = fn_type(vec![sym("Self")], sym("Int"));
        let count_type = fn_type(vec![sym("Self")], sym("Int"));
        let default_body = list(vec![
            sym("fn"),
            vec_node(vec![sym("self")]),
            list(vec![
                sym("fold"),
                list(vec![
                    sym("fn"),
                    vec_node(vec![sym("n"), sym("_")]),
                    list(vec![sym("+"), sym("n"), sym("1")]),
                ]),
                sym("0"),
                sym("self"),
            ]),
        ]);
        let items = vec![
            sym("defprotocol"),
            sym("Foldable"),
            vec_node(vec![sym("a")]),
            op("fold", fold_type),
            op_with_default("count", count_type, default_body.clone()),
        ];
        let decl = parse_protocol_decl(&items).unwrap();
        assert_eq!(decl.operations.len(), 2);
        assert_eq!(decl.operations[0].name, "fold");
        assert!(decl.operations[0].default_body.is_none());
        assert_eq!(decl.operations[1].name, "count");
        assert_eq!(
            decl.operations[1].default_body.as_ref().unwrap(),
            &default_body
        );
    }

    // ── Test 12 ──

    #[test]
    fn test_parse_protocol_error_missing_name() {
        // (defprotocol) → error
        let items = vec![sym("defprotocol")];
        let err = parse_protocol_decl(&items).unwrap_err();
        assert!(err.description.contains("requires a name"));
    }

    // ── Test 13 ──

    #[test]
    fn test_parse_protocol_error_non_list_op() {
        // (defprotocol Foo bar) → error (bare symbol where list op expected)
        let items = vec![sym("defprotocol"), sym("Foo"), sym("bar")];
        let err = parse_protocol_decl(&items).unwrap_err();
        assert!(
            err.description
                .contains("expected an operation declaration list"),
            "got: {}",
            err.description
        );
    }
}
