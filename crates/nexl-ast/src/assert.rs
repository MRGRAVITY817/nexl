//! Assert form parsing for `assert!` and `assert-unreachable!`.

use crate::{Atom, Node, NodeKind};

// ---------------------------------------------------------------------------
// Assert forms
// ---------------------------------------------------------------------------

/// Parsed `assert!` and `assert-unreachable!` forms.
#[derive(Debug, Clone, PartialEq)]
pub enum AssertForm {
    /// `(assert! <condition> [message])`.
    Assert {
        /// The assertion condition expression.
        condition: Node,
        /// Optional message expression.
        message: Option<Node>,
    },
    /// `(assert-unreachable! [message])`.
    AssertUnreachable {
        /// Optional message expression.
        message: Option<Node>,
    },
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// An error encountered while parsing an assert form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertParseError {
    /// Human-readable error description.
    pub description: String,
}

impl AssertParseError {
    /// Create a new error with the given description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

impl std::fmt::Display for AssertParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "assert parse error: {}", self.description)
    }
}

impl std::error::Error for AssertParseError {}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse an `assert!` or `assert-unreachable!` list into an [`AssertForm`].
///
/// The caller should pass the full list items including the head symbol.
pub fn parse_assert_form(items: &[Node]) -> Result<AssertForm, AssertParseError> {
    let head = items
        .first()
        .ok_or_else(|| AssertParseError::new("assert form requires a head symbol"))?;

    let name = match &head.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.as_str(),
        _ => {
            return Err(AssertParseError::new(
                "assert form head must be an unqualified symbol",
            ));
        }
    };

    match name {
        "assert!" => {
            if items.len() < 2 || items.len() > 3 {
                return Err(AssertParseError::new(format!(
                    "assert! expects (assert! condition [message]), got {} elements",
                    items.len()
                )));
            }
            Ok(AssertForm::Assert {
                condition: items[1].clone(),
                message: items.get(2).cloned(),
            })
        }
        "assert-unreachable!" => {
            if items.len() > 2 {
                return Err(AssertParseError::new(format!(
                    "assert-unreachable! expects (assert-unreachable! [message]), got {} elements",
                    items.len()
                )));
            }
            Ok(AssertForm::AssertUnreachable {
                message: items.get(1).cloned(),
            })
        }
        _ => Err(AssertParseError::new(format!(
            "unsupported assert form `{name}`"
        ))),
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
        Node::new(
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: name.to_string(),
            }),
            s(),
        )
    }

    fn bool_node(value: bool) -> Node {
        Node::new(NodeKind::Atom(Atom::Bool(value)), s())
    }

    fn str_node(value: &str) -> Node {
        Node::new(NodeKind::Atom(Atom::Str(value.to_string())), s())
    }

    #[test]
    fn parse_assert_basic() {
        let items = vec![sym("assert!"), bool_node(true)];
        let form = parse_assert_form(&items).expect("parse failed");
        assert_eq!(
            form,
            AssertForm::Assert {
                condition: bool_node(true),
                message: None,
            }
        );
    }

    #[test]
    fn parse_assert_with_message() {
        let items = vec![sym("assert!"), bool_node(false), str_node("nope")];
        let form = parse_assert_form(&items).expect("parse failed");
        assert_eq!(
            form,
            AssertForm::Assert {
                condition: bool_node(false),
                message: Some(str_node("nope")),
            }
        );
    }

    #[test]
    fn parse_assert_unreachable_basic() {
        let items = vec![sym("assert-unreachable!")];
        let form = parse_assert_form(&items).expect("parse failed");
        assert_eq!(form, AssertForm::AssertUnreachable { message: None });
    }
}
