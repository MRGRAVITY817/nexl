//! Effect handler declaration types, parsed from raw AST nodes.
//!
//! Follows the same pattern as [`crate::effect`]: semantic types plus
//! parsing functions that convert generic [`Node`] trees into structured
//! representations.

use crate::{Atom, Node, NodeKind};

// ---------------------------------------------------------------------------
// Handle declaration
// ---------------------------------------------------------------------------

/// A parsed `(handle [...effects...] body...)` form (spec §6.4–§6.5).
#[derive(Debug, Clone, PartialEq)]
pub struct HandleDecl {
    /// Handled effects and their operation implementations.
    pub effects: Vec<HandledEffect>,
    /// Body expressions evaluated under the handler.
    pub body: Vec<Node>,
}

/// A single handled effect in a `handle` form.
#[derive(Debug, Clone, PartialEq)]
pub struct HandledEffect {
    /// Effect name, e.g. `"Console"`.
    pub name: String,
    /// Operation implementations for this effect.
    pub operations: Vec<HandledOp>,
}

/// A single operation implementation inside a handler.
#[derive(Debug, Clone, PartialEq)]
pub struct HandledOp {
    /// Operation name, e.g. `"print"`.
    pub name: String,
    /// Operation parameters (excluding `resume`).
    pub params: Vec<String>,
    /// Whether this op uses continuation form (`resume` first param).
    pub has_resume: bool,
    /// Operation body expressions.
    pub body: Vec<Node>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// An error encountered while parsing a handle form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandleParseError {
    /// Human-readable error description.
    pub description: String,
}

impl HandleParseError {
    /// Create a new error with the given description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

impl std::fmt::Display for HandleParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "handle parse error: {}", self.description)
    }
}

impl std::error::Error for HandleParseError {}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a `(handle [...effects...] body...)` list into a [`HandleDecl`].
///
/// The caller is responsible for identifying that the list head is `handle`.
/// `items` should be the full list contents including the `handle` symbol.
pub fn parse_handle_form(items: &[Node]) -> Result<HandleDecl, HandleParseError> {
    if items.len() < 3 {
        return Err(HandleParseError::new(
            "handle form requires a handler vector and body",
        ));
    }

    let effects = match &items[1].kind {
        NodeKind::Vector(elems) => parse_handle_effects(elems)?,
        _ => {
            return Err(HandleParseError::new(
                "handle form requires a vector of effect handlers",
            ));
        }
    };

    let body: Vec<Node> = items[2..].to_vec();

    Ok(HandleDecl { effects, body })
}

fn parse_handle_effects(items: &[Node]) -> Result<Vec<HandledEffect>, HandleParseError> {
    if items.is_empty() {
        return Err(HandleParseError::new(
            "handle vector must list at least one effect",
        ));
    }

    let mut effects = Vec::new();
    let mut idx = 0;
    while idx < items.len() {
        let effect_name = extract_plain_symbol(&items[idx])?;
        idx += 1;
        let mut operations = Vec::new();
        while idx < items.len() {
            match &items[idx].kind {
                NodeKind::Atom(Atom::Symbol { ns: None, .. }) => break,
                NodeKind::List(_) => {
                    operations.push(parse_handle_op(&items[idx])?);
                    idx += 1;
                }
                _ => {
                    return Err(HandleParseError::new(
                        "expected an operation declaration list after effect name",
                    ));
                }
            }
        }
        if operations.is_empty() {
            return Err(HandleParseError::new(format!(
                "effect `{effect_name}` requires at least one operation",
            )));
        }
        effects.push(HandledEffect {
            name: effect_name,
            operations,
        });
    }

    Ok(effects)
}

fn parse_handle_op(node: &Node) -> Result<HandledOp, HandleParseError> {
    let items = match &node.kind {
        NodeKind::List(items) => items,
        _ => {
            return Err(HandleParseError::new(
                "expected an operation handler list (op-name [params...] body...)",
            ));
        }
    };

    if items.len() < 3 {
        return Err(HandleParseError::new(
            "operation handler requires a name, params, and body",
        ));
    }

    let name = extract_plain_symbol(&items[0])?;

    let params_nodes = match &items[1].kind {
        NodeKind::Vector(items) => items,
        _ => {
            return Err(HandleParseError::new(
                "operation handler parameters must be a vector",
            ));
        }
    };

    let mut params = Vec::new();
    let mut has_resume = false;
    for (idx, node) in params_nodes.iter().enumerate() {
        let param = extract_plain_symbol(node)?;
        if param == "resume" {
            if idx == 0 {
                has_resume = true;
                continue;
            }
            return Err(HandleParseError::new(
                "`resume` must be the first parameter in a handler operation",
            ));
        }
        params.push(param);
    }

    let body: Vec<Node> = items[2..].to_vec();
    if body.is_empty() {
        return Err(HandleParseError::new(
            "operation handler requires a body expression",
        ));
    }

    Ok(HandledOp {
        name,
        params,
        has_resume,
        body,
    })
}

fn extract_plain_symbol(node: &Node) -> Result<String, HandleParseError> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => Ok(name.clone()),
        _ => Err(HandleParseError::new("expected a symbol")),
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
        Node::atom(Atom::Symbol { ns: None, name: name.to_string() }, s())
    }

    fn list(children: Vec<Node>) -> Node {
        Node::new(NodeKind::List(children), s())
    }

    fn vec_node(children: Vec<Node>) -> Node {
        Node::new(NodeKind::Vector(children), s())
    }

    // ── Test 1 ──
    #[test]
    fn parse_handle_single_effect_simple_op() {
        // (handle [Console (print [s] (do s))] (do 1))
        let handler_vec = vec_node(vec![
            sym("Console"),
            list(vec![
                sym("print"),
                vec_node(vec![sym("s")]),
                list(vec![sym("do"), sym("s")]),
            ]),
        ]);
        let items = vec![sym("handle"), handler_vec, list(vec![sym("do"), sym("1")])];
        let decl = parse_handle_form(&items).expect("parse failed");
        assert_eq!(decl.effects.len(), 1);
        let effect = &decl.effects[0];
        assert_eq!(effect.name, "Console");
        assert_eq!(effect.operations.len(), 1);
        let op = &effect.operations[0];
        assert_eq!(op.name, "print");
        assert_eq!(op.params, vec!["s".to_string()]);
        assert!(!op.has_resume);
        assert_eq!(op.body.len(), 1);
    }

    // ── Test 2 ──
    #[test]
    fn parse_handle_continuation_op() {
        // (handle [Console (print [resume s] (resume s))] (do 1))
        let handler_vec = vec_node(vec![
            sym("Console"),
            list(vec![
                sym("print"),
                vec_node(vec![sym("resume"), sym("s")]),
                list(vec![sym("resume"), sym("s")]),
            ]),
        ]);
        let items = vec![sym("handle"), handler_vec, list(vec![sym("do"), sym("1")])];
        let decl = parse_handle_form(&items).expect("parse failed");
        let op = &decl.effects[0].operations[0];
        assert!(op.has_resume);
        assert_eq!(op.params, vec!["s".to_string()]);
    }

    // ── Test 3 ──
    #[test]
    fn parse_handle_multiple_effects() {
        // (handle [Console (print [s] s) Log (info [msg] msg)] (do 1))
        let handler_vec = vec_node(vec![
            sym("Console"),
            list(vec![sym("print"), vec_node(vec![sym("s")]), sym("s")]),
            sym("Log"),
            list(vec![sym("info"), vec_node(vec![sym("msg")]), sym("msg")]),
        ]);
        let items = vec![sym("handle"), handler_vec, list(vec![sym("do"), sym("1")])];
        let decl = parse_handle_form(&items).expect("parse failed");
        assert_eq!(decl.effects.len(), 2);
        assert_eq!(decl.effects[0].name, "Console");
        assert_eq!(decl.effects[1].name, "Log");
    }

    // ── Test 4 ──
    #[test]
    fn parse_handle_resume_not_first_is_error() {
        // (handle [Console (print [s resume] s)] (do 1))
        let handler_vec = vec_node(vec![
            sym("Console"),
            list(vec![sym("print"), vec_node(vec![sym("s"), sym("resume")]), sym("s")]),
        ]);
        let items = vec![sym("handle"), handler_vec, list(vec![sym("do"), sym("1")])];
        let err = parse_handle_form(&items).unwrap_err();
        assert!(err.description.contains("resume"));
    }
}
