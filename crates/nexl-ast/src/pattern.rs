//! Pattern AST nodes for `match` and destructuring.

use crate::{Atom, Node, NodeKind};

/// A pattern used in `match` arms and destructuring.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// `_` — matches any value.
    Wildcard,
    /// Variable binding (e.g. `x`).
    Var(String),
    /// Literal pattern (e.g. `42`, `"hi"`, `true`, `:ok`).
    Literal(Atom),
    /// Constructor pattern (e.g. `(Some x)`).
    Constructor { name: String, args: Vec<Pattern> },
    /// Record pattern with field patterns.
    Record { fields: Vec<(String, Pattern)> },
    /// Tuple pattern (e.g. `[a b]`).
    Tuple(Vec<Pattern>),
    /// OR pattern (e.g. `(or p1 p2)`).
    Or(Vec<Pattern>),
    /// Alias pattern (e.g. `p as x`).
    As { pattern: Box<Pattern>, name: String },
}

/// A pattern parsing error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError {
    /// Human-readable error description.
    pub description: String,
}

impl PatternError {
    /// Create a new pattern error with the given description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pattern error: {}", self.description)
    }
}

impl std::error::Error for PatternError {}

/// Parse an AST node appearing in a pattern position into a [`Pattern`].
pub fn parse_pattern(node: &Node) -> Result<Pattern, PatternError> {
    match &node.kind {
        NodeKind::Atom(atom) => parse_atom_pattern(atom),
        NodeKind::List(items) => parse_list_pattern(items),
        NodeKind::Vector(items) => {
            let mut pats = Vec::with_capacity(items.len());
            for item in items {
                pats.push(parse_pattern(item)?);
            }
            Ok(Pattern::Tuple(pats))
        }
        NodeKind::Map(entries) => {
            let mut fields = Vec::with_capacity(entries.len());
            for (key_node, val_node) in entries {
                let field = match &key_node.kind {
                    NodeKind::Atom(Atom::Keyword { ns: None, name }) => name.clone(),
                    _ => {
                        return Err(PatternError::new(
                            "record pattern keys must be unqualified keywords",
                        ));
                    }
                };
                let pat = parse_pattern(val_node)?;
                fields.push((field, pat));
            }
            Ok(Pattern::Record { fields })
        }
        _ => Err(PatternError::new("unsupported pattern form")),
    }
}

fn parse_atom_pattern(atom: &Atom) -> Result<Pattern, PatternError> {
    match atom {
        Atom::Symbol { ns: None, name } if name == "_" => Ok(Pattern::Wildcard),
        Atom::Symbol { ns: None, name } => Ok(Pattern::Var(name.clone())),
        Atom::Symbol { .. } => Err(PatternError::new("pattern variables must be unqualified")),
        Atom::Keyword { .. }
        | Atom::Int { .. }
        | Atom::Float { .. }
        | Atom::Ratio { .. }
        | Atom::Bool(_)
        | Atom::Char(_)
        | Atom::Str(_)
        | Atom::Unit => Ok(Pattern::Literal(atom.clone())),
    }
}

fn parse_list_pattern(items: &[Node]) -> Result<Pattern, PatternError> {
    if items.is_empty() {
        return Err(PatternError::new("empty list is not a valid pattern"));
    }
    let head = match &items[0].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.as_str(),
        _ => return Err(PatternError::new("pattern list head must be a symbol")),
    };

    if head == "|" {
        if items.len() < 3 {
            return Err(PatternError::new(
                "or-pattern requires at least two alternatives",
            ));
        }
        let mut pats = Vec::with_capacity(items.len() - 1);
        for item in &items[1..] {
            pats.push(parse_pattern(item)?);
        }
        return Ok(Pattern::Or(pats));
    }

    let mut args = Vec::with_capacity(items.len().saturating_sub(1));
    for item in &items[1..] {
        args.push(parse_pattern(item)?);
    }
    Ok(Pattern::Constructor {
        name: head.to_string(),
        args,
    })
}

#[cfg(test)]
mod tests {
    use super::{Pattern, parse_pattern};
    use crate::{Atom, Node, NodeKind, Span};

    #[test]
    fn pattern_constructor_holds_args() {
        let pat = Pattern::Constructor {
            name: "Some".to_string(),
            args: vec![Pattern::Var("x".to_string())],
        };
        let expected = Pattern::Constructor {
            name: "Some".to_string(),
            args: vec![Pattern::Var("x".to_string())],
        };
        assert_eq!(pat, expected);
    }

    #[test]
    fn pattern_record_holds_fields() {
        let pat = Pattern::Record {
            fields: vec![
                ("x".to_string(), Pattern::Wildcard),
                (
                    "y".to_string(),
                    Pattern::Literal(Atom::Int {
                        value: 1,
                        suffix: None,
                    }),
                ),
            ],
        };
        let expected = Pattern::Record {
            fields: vec![
                ("x".to_string(), Pattern::Wildcard),
                (
                    "y".to_string(),
                    Pattern::Literal(Atom::Int {
                        value: 1,
                        suffix: None,
                    }),
                ),
            ],
        };
        assert_eq!(pat, expected);
    }

    fn sym(name: &str) -> Node {
        Node::atom(
            Atom::Symbol {
                ns: None,
                name: name.to_string(),
            },
            Span::synthetic(),
        )
    }

    fn kw(name: &str) -> Node {
        Node::atom(
            Atom::Keyword {
                ns: None,
                name: name.to_string(),
            },
            Span::synthetic(),
        )
    }

    // -- Parser Test 1 --
    #[test]
    fn parse_pattern_wildcard_var_literal() {
        let wildcard = parse_pattern(&sym("_")).unwrap();
        assert_eq!(wildcard, Pattern::Wildcard);

        let var = parse_pattern(&sym("x")).unwrap();
        assert_eq!(var, Pattern::Var("x".to_string()));

        let lit = parse_pattern(&Node::atom(
            Atom::Int {
                value: 42,
                suffix: None,
            },
            Span::synthetic(),
        ))
        .unwrap();
        assert_eq!(
            lit,
            Pattern::Literal(Atom::Int {
                value: 42,
                suffix: None
            })
        );
    }

    // -- Parser Test 2 --
    #[test]
    fn parse_pattern_constructor_list() {
        let node = Node::new(
            NodeKind::List(vec![sym("Some"), sym("x")]),
            Span::synthetic(),
        );
        let pat = parse_pattern(&node).unwrap();
        assert_eq!(
            pat,
            Pattern::Constructor {
                name: "Some".to_string(),
                args: vec![Pattern::Var("x".to_string())],
            }
        );
    }

    // -- Parser Test 3 --
    #[test]
    fn parse_pattern_tuple_vector() {
        let node = Node::new(
            NodeKind::Vector(vec![sym("a"), sym("b")]),
            Span::synthetic(),
        );
        let pat = parse_pattern(&node).unwrap();
        assert_eq!(
            pat,
            Pattern::Tuple(vec![
                Pattern::Var("a".to_string()),
                Pattern::Var("b".to_string())
            ]),
        );
    }

    // -- Parser Test 4 --
    #[test]
    fn parse_pattern_record_map() {
        let node = Node::new(
            NodeKind::Map(vec![
                (kw("x"), sym("x")),
                (
                    kw("y"),
                    Node::atom(
                        Atom::Int {
                            value: 1,
                            suffix: None,
                        },
                        Span::synthetic(),
                    ),
                ),
            ]),
            Span::synthetic(),
        );
        let pat = parse_pattern(&node).unwrap();
        assert_eq!(
            pat,
            Pattern::Record {
                fields: vec![
                    ("x".to_string(), Pattern::Var("x".to_string())),
                    (
                        "y".to_string(),
                        Pattern::Literal(Atom::Int {
                            value: 1,
                            suffix: None
                        })
                    ),
                ],
            }
        );
    }

    // -- Parser Test 5 --
    #[test]
    fn parse_pattern_or_list() {
        let node = Node::new(
            NodeKind::List(vec![sym("|"), kw("pending"), kw("processing")]),
            Span::synthetic(),
        );
        let pat = parse_pattern(&node).unwrap();
        assert_eq!(
            pat,
            Pattern::Or(vec![
                Pattern::Literal(Atom::Keyword {
                    ns: None,
                    name: "pending".to_string()
                }),
                Pattern::Literal(Atom::Keyword {
                    ns: None,
                    name: "processing".to_string()
                }),
            ]),
        );
    }
}
