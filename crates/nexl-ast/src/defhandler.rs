//! Named effect handler declaration types, parsed from raw AST nodes.
//!
//! `defhandler` (spec §6.10) declares a reusable, named effect handler that
//! can be installed via `handle [HandlerName]` instead of inline effect
//! implementations. Follows the same structure as `impl`: bare uppercase
//! symbols delimit effect sections.

use crate::handle::{HandledEffect, HandledOp};
use crate::node::Node;

// ---------------------------------------------------------------------------
// DefHandler declaration
// ---------------------------------------------------------------------------

/// A parsed `(defhandler Name ...)` declaration (spec §6.10).
#[derive(Debug, Clone, PartialEq)]
pub struct DefHandlerDecl {
    /// Handler name, e.g. `"ConsoleLog"`.
    pub name: String,
    /// Optional parameter names for parameterized handlers, e.g. `["config"]`.
    pub params: Vec<String>,
    /// Handled effects and their operation implementations.
    /// Reuses [`HandledEffect`] / [`HandledOp`] from the `handle` module.
    pub effects: Vec<HandledEffect>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// An error encountered while parsing a defhandler form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefHandlerParseError {
    /// Human-readable error description.
    pub description: String,
}

impl DefHandlerParseError {
    /// Create a new error with the given description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

impl std::fmt::Display for DefHandlerParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "defhandler parse error: {}", self.description)
    }
}

impl std::error::Error for DefHandlerParseError {}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a `(defhandler Name [params?] Effect (op [args] body) ...)` list
/// into a [`DefHandlerDecl`].
///
/// The caller is responsible for identifying that the list head is `defhandler`.
/// `items` should be the full list contents including the `defhandler` symbol.
pub fn parse_defhandler_decl(items: &[Node]) -> Result<DefHandlerDecl, DefHandlerParseError> {
    // Minimum: (defhandler Name Effect (op [args] body))
    if items.len() < 4 {
        return Err(DefHandlerParseError::new(
            "defhandler requires a name, at least one effect, and at least one operation",
        ));
    }

    // items[0] = "defhandler" symbol
    let name = extract_uppercase_symbol(&items[1]).map_err(|_| {
        DefHandlerParseError::new("defhandler name must be an uppercase symbol")
    })?;

    // Determine if items[2] is a params vector (lowercase contents) or an
    // effect section (uppercase symbol).
    let (params, effect_start) = match &items[2].kind {
        crate::NodeKind::Vector(elems) => {
            // Params vector — extract lowercase symbols
            let mut params = Vec::new();
            for elem in elems {
                let p = extract_plain_symbol(elem).map_err(|_| {
                    DefHandlerParseError::new(
                        "defhandler parameter list must contain only symbols",
                    )
                })?;
                params.push(p);
            }
            (params, 3)
        }
        _ => (Vec::new(), 2),
    };

    // Parse effect sections starting at `effect_start`
    let effects = parse_defhandler_effects(&items[effect_start..])?;

    if effects.is_empty() {
        return Err(DefHandlerParseError::new(
            "defhandler must implement at least one effect",
        ));
    }

    Ok(DefHandlerDecl {
        name,
        params,
        effects,
    })
}

/// Parse the effect sections of a defhandler: `Effect (op ...) (op ...) Effect2 (op ...) ...`
fn parse_defhandler_effects(
    items: &[Node],
) -> Result<Vec<HandledEffect>, DefHandlerParseError> {
    if items.is_empty() {
        return Err(DefHandlerParseError::new(
            "defhandler must implement at least one effect",
        ));
    }

    let mut effects = Vec::new();
    let mut idx = 0;

    while idx < items.len() {
        // Expect uppercase symbol = effect name
        let effect_name = extract_uppercase_symbol(&items[idx]).map_err(|_| {
            DefHandlerParseError::new(format!(
                "expected an uppercase effect name, got {:?}",
                items[idx].kind
            ))
        })?;
        idx += 1;

        // Collect operation lists until the next uppercase symbol or end
        let mut operations = Vec::new();
        while idx < items.len() {
            // If it's an uppercase symbol, it's the start of the next effect section
            if is_uppercase_symbol(&items[idx]) {
                break;
            }
            operations.push(parse_defhandler_op(&items[idx])?);
            idx += 1;
        }

        if operations.is_empty() {
            return Err(DefHandlerParseError::new(format!(
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

/// Parse a single operation: `(op-name [params...] body...)`
fn parse_defhandler_op(node: &Node) -> Result<HandledOp, DefHandlerParseError> {
    let items = match &node.kind {
        crate::NodeKind::List(items) => items,
        _ => {
            return Err(DefHandlerParseError::new(
                "expected an operation list (op-name [params...] body...)",
            ));
        }
    };

    if items.len() < 3 {
        return Err(DefHandlerParseError::new(
            "operation requires a name, params, and body",
        ));
    }

    let name = extract_plain_symbol(&items[0]).map_err(|_| {
        DefHandlerParseError::new("operation name must be a symbol")
    })?;

    let params_nodes = match &items[1].kind {
        crate::NodeKind::Vector(items) => items,
        _ => {
            return Err(DefHandlerParseError::new(
                "operation parameters must be a vector",
            ));
        }
    };

    let mut params = Vec::new();
    let mut has_resume = false;
    for (i, node) in params_nodes.iter().enumerate() {
        let param = extract_plain_symbol(node).map_err(|_| {
            DefHandlerParseError::new("operation parameter must be a symbol")
        })?;
        if param == "resume" {
            if i == 0 {
                has_resume = true;
                continue;
            }
            return Err(DefHandlerParseError::new(
                "`resume` must be the first parameter in a handler operation",
            ));
        }
        params.push(param);
    }

    let body: Vec<Node> = items[2..].to_vec();
    if body.is_empty() {
        return Err(DefHandlerParseError::new(
            "operation requires a body expression",
        ));
    }

    Ok(HandledOp {
        name,
        params,
        has_resume,
        body,
    })
}

/// Extract an uppercase symbol name (e.g., `Console`, `Log`).
fn extract_uppercase_symbol(node: &Node) -> Result<String, ()> {
    match &node.kind {
        crate::NodeKind::Atom(crate::Atom::Symbol { ns: None, name })
            if name.starts_with(|c: char| c.is_uppercase()) =>
        {
            Ok(name.clone())
        }
        _ => Err(()),
    }
}

/// Check if a node is an uppercase symbol (for lookahead).
fn is_uppercase_symbol(node: &Node) -> bool {
    matches!(
        &node.kind,
        crate::NodeKind::Atom(crate::Atom::Symbol { ns: None, name })
            if name.starts_with(|c: char| c.is_uppercase())
    )
}

/// Extract a plain symbol name (any case).
fn extract_plain_symbol(node: &Node) -> Result<String, ()> {
    match &node.kind {
        crate::NodeKind::Atom(crate::Atom::Symbol { ns: None, name }) => Ok(name.clone()),
        _ => Err(()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::Span;
    use crate::{Atom, NodeKind};

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

    fn list(children: Vec<Node>) -> Node {
        Node::new(NodeKind::List(children), s())
    }

    fn vec_node(children: Vec<Node>) -> Node {
        Node::new(NodeKind::Vector(children), s())
    }

    // ── Test 1: Simple single-effect handler ──
    #[test]
    fn parse_defhandler_simple() {
        // (defhandler ConsoleLog Log (info [msg] (println msg)) (warn [msg] (println msg)))
        let items = vec![
            sym("defhandler"),
            sym("ConsoleLog"),
            sym("Log"),
            list(vec![
                sym("info"),
                vec_node(vec![sym("msg")]),
                list(vec![sym("println"), sym("msg")]),
            ]),
            list(vec![
                sym("warn"),
                vec_node(vec![sym("msg")]),
                list(vec![sym("println"), sym("msg")]),
            ]),
        ];
        let decl = parse_defhandler_decl(&items).expect("parse failed");
        assert_eq!(decl.name, "ConsoleLog");
        assert!(decl.params.is_empty());
        assert_eq!(decl.effects.len(), 1);
        assert_eq!(decl.effects[0].name, "Log");
        assert_eq!(decl.effects[0].operations.len(), 2);
        assert_eq!(decl.effects[0].operations[0].name, "info");
        assert_eq!(decl.effects[0].operations[1].name, "warn");
        assert!(!decl.effects[0].operations[0].has_resume);
    }

    // ── Test 2: Continuation form (resume) ──
    #[test]
    fn parse_defhandler_continuation() {
        // (defhandler TimestampLog Log (info [resume msg] (resume unit)))
        let items = vec![
            sym("defhandler"),
            sym("TimestampLog"),
            sym("Log"),
            list(vec![
                sym("info"),
                vec_node(vec![sym("resume"), sym("msg")]),
                list(vec![sym("resume"), sym("unit")]),
            ]),
        ];
        let decl = parse_defhandler_decl(&items).expect("parse failed");
        let op = &decl.effects[0].operations[0];
        assert!(op.has_resume);
        assert_eq!(op.params, vec!["msg".to_string()]);
    }

    // ── Test 3: Parameterized handler ──
    #[test]
    fn parse_defhandler_parameterized() {
        // (defhandler JsonLog [config] Log (info [msg] body))
        let items = vec![
            sym("defhandler"),
            sym("JsonLog"),
            vec_node(vec![sym("config")]),
            sym("Log"),
            list(vec![
                sym("info"),
                vec_node(vec![sym("msg")]),
                sym("body"),
            ]),
        ];
        let decl = parse_defhandler_decl(&items).expect("parse failed");
        assert_eq!(decl.name, "JsonLog");
        assert_eq!(decl.params, vec!["config".to_string()]);
        assert_eq!(decl.effects.len(), 1);
        assert_eq!(decl.effects[0].name, "Log");
    }

    // ── Test 4: Multi-effect handler ──
    #[test]
    fn parse_defhandler_multi_effect() {
        // (defhandler ProductionStack
        //   Db (query [sql params] body) (exec! [sql params] body)
        //   Log (info [msg] body) (warn [msg] body))
        let items = vec![
            sym("defhandler"),
            sym("ProductionStack"),
            sym("Db"),
            list(vec![
                sym("query"),
                vec_node(vec![sym("sql"), sym("params")]),
                sym("body"),
            ]),
            list(vec![
                sym("exec!"),
                vec_node(vec![sym("sql"), sym("params")]),
                sym("body"),
            ]),
            sym("Log"),
            list(vec![
                sym("info"),
                vec_node(vec![sym("msg")]),
                sym("body"),
            ]),
            list(vec![
                sym("warn"),
                vec_node(vec![sym("msg")]),
                sym("body"),
            ]),
        ];
        let decl = parse_defhandler_decl(&items).expect("parse failed");
        assert_eq!(decl.name, "ProductionStack");
        assert!(decl.params.is_empty());
        assert_eq!(decl.effects.len(), 2);
        assert_eq!(decl.effects[0].name, "Db");
        assert_eq!(decl.effects[0].operations.len(), 2);
        assert_eq!(decl.effects[1].name, "Log");
        assert_eq!(decl.effects[1].operations.len(), 2);
    }

    // ── Test 5: Parameterized + multi-effect ──
    #[test]
    fn parse_defhandler_parameterized_multi_effect() {
        // (defhandler ConfiguredStack [db-path log-level]
        //   Db (query [sql params] body)
        //   Log (info [msg] body))
        let items = vec![
            sym("defhandler"),
            sym("ConfiguredStack"),
            vec_node(vec![sym("db-path"), sym("log-level")]),
            sym("Db"),
            list(vec![
                sym("query"),
                vec_node(vec![sym("sql"), sym("params")]),
                sym("body"),
            ]),
            sym("Log"),
            list(vec![
                sym("info"),
                vec_node(vec![sym("msg")]),
                sym("body"),
            ]),
        ];
        let decl = parse_defhandler_decl(&items).expect("parse failed");
        assert_eq!(decl.name, "ConfiguredStack");
        assert_eq!(
            decl.params,
            vec!["db-path".to_string(), "log-level".to_string()]
        );
        assert_eq!(decl.effects.len(), 2);
    }

    // ── Test 6: Missing name is error ──
    #[test]
    fn parse_defhandler_missing_name_is_error() {
        // (defhandler) — too few items
        let items = vec![sym("defhandler")];
        assert!(parse_defhandler_decl(&items).is_err());
    }

    // ── Test 7: Missing effect is error ──
    #[test]
    fn parse_defhandler_missing_effect_is_error() {
        // (defhandler MyHandler) — name but no effects
        let items = vec![sym("defhandler"), sym("MyHandler")];
        assert!(parse_defhandler_decl(&items).is_err());
    }

    // ── Test 8: Empty effect section is error ──
    #[test]
    fn parse_defhandler_empty_effect_section_is_error() {
        // (defhandler MyHandler Log) — effect name but no operations
        let items = vec![sym("defhandler"), sym("MyHandler"), sym("Log")];
        assert!(parse_defhandler_decl(&items).is_err());
    }

    // ── Test 9: Resume not first is error ──
    #[test]
    fn parse_defhandler_resume_not_first_is_error() {
        // (defhandler MyHandler Log (info [msg resume] body))
        let items = vec![
            sym("defhandler"),
            sym("MyHandler"),
            sym("Log"),
            list(vec![
                sym("info"),
                vec_node(vec![sym("msg"), sym("resume")]),
                sym("body"),
            ]),
        ];
        let err = parse_defhandler_decl(&items).unwrap_err();
        assert!(err.description.contains("resume"));
    }
}
