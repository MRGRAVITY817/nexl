//! Effect declaration types, parsed from raw AST nodes.
//!
//! Follows the same pattern as [`crate::module`]: semantic types plus
//! parsing functions that convert generic [`Node`] trees into structured
//! representations.

use crate::{Node, NodeKind, Atom};

// ---------------------------------------------------------------------------
// Effect declaration
// ---------------------------------------------------------------------------

/// A parsed `(defeffect Name ...)` declaration (spec §6.2).
#[derive(Debug, Clone, PartialEq)]
pub struct EffectDecl {
    /// Effect name, e.g. `"Console"`.
    pub name: String,
    /// Type parameters, e.g. `["a"]` for `(defeffect State [a] ...)`.
    pub type_params: Vec<String>,
    /// Operation declarations.
    pub operations: Vec<EffectOpDecl>,
}

/// A single operation within an effect declaration.
///
/// The type annotation is stored as a raw [`Node`] tree; the type system
/// interprets it in a later pass.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectOpDecl {
    /// Operation name, e.g. `"print"`.
    pub name: String,
    /// The raw type annotation node, e.g. `(Fn [Str] -> Unit)`.
    pub type_node: Node,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// An error encountered while parsing an effect declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectParseError {
    /// Human-readable error description.
    pub description: String,
}

impl EffectParseError {
    /// Create a new error with the given description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

impl std::fmt::Display for EffectParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "effect parse error: {}", self.description)
    }
}

impl std::error::Error for EffectParseError {}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a `(defeffect Name ...)` list into an [`EffectDecl`].
///
/// The caller is responsible for identifying that the list head is `defeffect`.
/// `items` should be the full list contents including the `defeffect` symbol.
pub fn parse_effect_decl(items: &[Node]) -> Result<EffectDecl, EffectParseError> {
    // items[0] = `defeffect`, items[1] = name, rest = optional type-params + operations
    if items.len() < 2 {
        return Err(EffectParseError::new(
            "defeffect requires a name",
        ));
    }

    let name = extract_plain_symbol(&items[1])?;

    let mut idx = 2;
    let mut type_params = Vec::new();
    let mut operations = Vec::new();

    // Optional type parameters: [a b ...]
    if idx < items.len()
        && let NodeKind::Vector(params) = &items[idx].kind
    {
        for p in params {
            type_params.push(extract_plain_symbol(p)?);
        }
        idx += 1;
    }

    // Remaining items are operation declarations: (op-name : Type)
    while idx < items.len() {
        operations.push(parse_op_decl(&items[idx])?);
        idx += 1;
    }

    Ok(EffectDecl {
        name,
        type_params,
        operations,
    })
}

/// Parse a single operation declaration `(op-name : Type)`.
fn parse_op_decl(node: &Node) -> Result<EffectOpDecl, EffectParseError> {
    let items = match &node.kind {
        NodeKind::List(items) => items,
        _ => {
            return Err(EffectParseError::new(
                "expected an operation declaration list (op-name : Type)",
            ));
        }
    };

    if items.len() < 3 {
        return Err(EffectParseError::new(
            "operation declaration requires (op-name : Type)",
        ));
    }

    let op_name = extract_plain_symbol(&items[0])?;

    // items[1] must be the `:` separator
    match &items[1].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == ":" => {}
        _ => {
            return Err(EffectParseError::new(format!(
                "expected `:` after operation name `{op_name}`"
            )));
        }
    }

    let type_node = items[2].clone();

    Ok(EffectOpDecl {
        name: op_name,
        type_node,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a plain (unqualified) symbol name from a node.
fn extract_plain_symbol(node: &Node) -> Result<String, EffectParseError> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => Ok(name.clone()),
        _ => Err(EffectParseError::new("expected a symbol")),
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

    /// Build a plain symbol node.
    fn sym(name: &str) -> Node {
        Node::atom(
            Atom::Symbol {
                ns: None,
                name: name.to_string(),
            },
            s(),
        )
    }

    /// Build a keyword node.
    #[allow(dead_code)]
    fn kw(name: &str) -> Node {
        Node::atom(
            Atom::Keyword {
                ns: None,
                name: name.to_string(),
            },
            s(),
        )
    }

    /// Build the colon separator (`:` is read as a symbol by the reader).
    fn colon() -> Node {
        sym(":")
    }

    /// Build a list node from children.
    fn list(children: Vec<Node>) -> Node {
        Node::new(NodeKind::List(children), s())
    }

    /// Build a vector node from children.
    fn vec_node(children: Vec<Node>) -> Node {
        Node::new(NodeKind::Vector(children), s())
    }

    /// Build a Fn type annotation: (Fn [args...] -> Ret)
    fn fn_type(args: Vec<Node>, ret: Node) -> Node {
        let mut children = vec![sym("Fn"), vec_node(args), sym("->"), ret];
        let _ = &mut children; // silence unused-mut if needed
        list(children)
    }

    /// Build an effect operation: (op-name : (Fn [...] -> Ret))
    fn op(name: &str, type_ann: Node) -> Node {
        list(vec![sym(name), colon(), type_ann])
    }

    // ── Test 1 ──

    #[test]
    fn test_parse_effect_single_op() {
        // (defeffect Console (print : (Fn [Str] -> Unit)))
        let print_type = fn_type(vec![sym("Str")], sym("Unit"));
        let items = vec![
            sym("defeffect"),
            sym("Console"),
            op("print", print_type.clone()),
        ];
        let decl = parse_effect_decl(&items).unwrap();
        assert_eq!(decl.name, "Console");
        assert!(decl.type_params.is_empty());
        assert_eq!(decl.operations.len(), 1);
        assert_eq!(decl.operations[0].name, "print");
        assert_eq!(decl.operations[0].type_node, print_type);
    }

    // ── Test 2 ──

    #[test]
    fn test_parse_effect_multiple_ops() {
        // (defeffect Console
        //   (print : (Fn [Str] -> Unit))
        //   (read-line : (Fn [] -> Str)))
        let print_type = fn_type(vec![sym("Str")], sym("Unit"));
        let read_type = fn_type(vec![], sym("Str"));
        let items = vec![
            sym("defeffect"),
            sym("Console"),
            op("print", print_type.clone()),
            op("read-line", read_type.clone()),
        ];
        let decl = parse_effect_decl(&items).unwrap();
        assert_eq!(decl.name, "Console");
        assert_eq!(decl.operations.len(), 2);
        assert_eq!(decl.operations[0].name, "print");
        assert_eq!(decl.operations[0].type_node, print_type);
        assert_eq!(decl.operations[1].name, "read-line");
        assert_eq!(decl.operations[1].type_node, read_type);
    }

    // ── Test 3 ──

    #[test]
    fn test_parse_effect_with_type_params() {
        // (defeffect State [a]
        //   (get-state : (Fn [] -> a))
        //   (put-state : (Fn [a] -> Unit)))
        let get_type = fn_type(vec![], sym("a"));
        let put_type = fn_type(vec![sym("a")], sym("Unit"));
        let items = vec![
            sym("defeffect"),
            sym("State"),
            vec_node(vec![sym("a")]),
            op("get-state", get_type.clone()),
            op("put-state", put_type.clone()),
        ];
        let decl = parse_effect_decl(&items).unwrap();
        assert_eq!(decl.name, "State");
        assert_eq!(decl.type_params, vec!["a".to_string()]);
        assert_eq!(decl.operations.len(), 2);
        assert_eq!(decl.operations[0].name, "get-state");
        assert_eq!(decl.operations[0].type_node, get_type);
        assert_eq!(decl.operations[1].name, "put-state");
        assert_eq!(decl.operations[1].type_node, put_type);
    }

    // ── Test 4 ──

    #[test]
    fn test_parse_effect_no_operations() {
        // (defeffect Empty)
        let items = vec![sym("defeffect"), sym("Empty")];
        let decl = parse_effect_decl(&items).unwrap();
        assert_eq!(decl.name, "Empty");
        assert!(decl.type_params.is_empty());
        assert!(decl.operations.is_empty());
    }

    // ── Test 5 ──

    #[test]
    fn test_parse_effect_error_missing_name() {
        // (defeffect) → error
        let items = vec![sym("defeffect")];
        let err = parse_effect_decl(&items).unwrap_err();
        assert!(err.description.contains("requires a name"));
    }

    // ── Test 6 ──

    #[test]
    fn test_parse_effect_error_non_symbol_name() {
        // (defeffect 42) → error
        let items = vec![
            sym("defeffect"),
            Node::atom(
                Atom::Int {
                    value: 42,
                    suffix: None,
                },
                s(),
            ),
        ];
        let err = parse_effect_decl(&items).unwrap_err();
        assert!(err.description.contains("expected a symbol"));
    }

    // ── Test 7 ──

    #[test]
    fn test_parse_effect_error_op_missing_colon() {
        // (defeffect Foo (bar x (Fn [] -> Unit))) — `x` instead of `:`
        let type_ann = fn_type(vec![], sym("Unit"));
        let bad_op = list(vec![sym("bar"), sym("x"), type_ann]);
        let items = vec![sym("defeffect"), sym("Foo"), bad_op];
        let err = parse_effect_decl(&items).unwrap_err();
        assert!(
            err.description.contains("expected `:`"),
            "got: {}",
            err.description
        );
    }

    // ── Test 8 ──

    #[test]
    fn test_parse_effect_error_op_missing_type() {
        // (defeffect Foo (bar :)) — colon but no type after it
        let bad_op = list(vec![sym("bar"), colon()]);
        let items = vec![sym("defeffect"), sym("Foo"), bad_op];
        let err = parse_effect_decl(&items).unwrap_err();
        assert!(
            err.description.contains("requires (op-name : Type)"),
            "got: {}",
            err.description
        );
    }

    // ── Test 9 ──

    #[test]
    fn test_parse_effect_error_non_list_op() {
        // (defeffect Foo bar) — bare symbol where a list op is expected
        let items = vec![sym("defeffect"), sym("Foo"), sym("bar")];
        let err = parse_effect_decl(&items).unwrap_err();
        assert!(
            err.description.contains("expected an operation declaration list"),
            "got: {}",
            err.description
        );
    }

    // ── Test 10 ──

    #[test]
    fn test_parse_effect_struct_fields() {
        let type_node = sym("SomeType");
        let op_decl = EffectOpDecl {
            name: "do-thing".to_string(),
            type_node: type_node.clone(),
        };
        assert_eq!(op_decl.name, "do-thing");
        assert_eq!(op_decl.type_node, type_node);

        let decl = EffectDecl {
            name: "MyEffect".to_string(),
            type_params: vec!["a".to_string(), "b".to_string()],
            operations: vec![op_decl],
        };
        assert_eq!(decl.name, "MyEffect");
        assert_eq!(decl.type_params, vec!["a", "b"]);
        assert_eq!(decl.operations.len(), 1);
        assert_eq!(decl.operations[0].name, "do-thing");
    }
}
