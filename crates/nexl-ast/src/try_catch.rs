//! Try/catch form parsing for Result-based error handling.

use crate::{Atom, Node, NodeKind};

// ---------------------------------------------------------------------------
// Try/catch forms
// ---------------------------------------------------------------------------

/// A parsed `try` form with a single `catch` clause.
#[derive(Debug, Clone, PartialEq)]
pub struct TryCatchForm {
    /// Body expressions evaluated inside the `try`.
    pub body: Vec<Node>,
    /// Bound error name for the `catch` clause.
    pub catch_name: String,
    /// Body expressions evaluated when a `Result` is `Err`.
    pub catch_body: Vec<Node>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// An error encountered while parsing a try/catch form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TryParseError {
    /// Human-readable error description.
    pub description: String,
}

impl TryParseError {
    /// Create a new error with the given description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

impl std::fmt::Display for TryParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "try parse error: {}", self.description)
    }
}

impl std::error::Error for TryParseError {}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a `(try ... (catch err ...))` list into a [`TryCatchForm`].
///
/// The caller is responsible for ensuring the list head is `try`.
pub fn parse_try_form(_items: &[Node]) -> Result<TryCatchForm, TryParseError> {
    let items = _items;
    if items.len() < 3 {
        return Err(TryParseError::new(
            "try form requires a body expression and a catch clause",
        ));
    }

    let catch_node = items.last().unwrap();
    let catch_items = match &catch_node.kind {
        NodeKind::List(items) => items,
        _ => {
            return Err(TryParseError::new(
                "try form requires a trailing (catch name body...) list",
            ));
        }
    };

    if catch_items.len() < 3 {
        return Err(TryParseError::new(
            "catch clause requires a name and body expression",
        ));
    }

    match &catch_items[0].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "catch" => {}
        _ => {
            return Err(TryParseError::new(
                "try form requires a (catch ...) clause as its last element",
            ));
        }
    }

    let catch_name = match &catch_items[1].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => {
            return Err(TryParseError::new(
                "catch clause name must be an unqualified symbol",
            ));
        }
    };

    let catch_body: Vec<Node> = catch_items[2..].to_vec();
    if catch_body.is_empty() {
        return Err(TryParseError::new(
            "catch clause requires a body expression",
        ));
    }

    let body: Vec<Node> = items[1..items.len() - 1].to_vec();
    if body.is_empty() {
        return Err(TryParseError::new(
            "try form requires at least one body expression",
        ));
    }

    Ok(TryCatchForm {
        body,
        catch_name,
        catch_body,
    })
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
        Node::new(
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: name.to_string(),
            }),
            s(),
        )
    }

    #[test]
    fn parse_try_catch_basic() {
        let catch = Node::new(
            NodeKind::List(vec![sym("catch"), sym("err"), sym("handle")]),
            s(),
        );
        let items = vec![sym("try"), sym("risky"), catch];
        let form = parse_try_form(&items).expect("parse failed");
        assert_eq!(form.body, vec![sym("risky")]);
        assert_eq!(form.catch_name, "err");
        assert_eq!(form.catch_body, vec![sym("handle")]);
    }
}
