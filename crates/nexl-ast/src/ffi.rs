//! FFI declaration types for C ABI interop (spec §15.3–§15.4).
//!
//! Parses `defextern` (C function import) and `defexport` (C-callable export)
//! declarations from raw AST nodes.

use crate::{Atom, Node, NodeKind};

// ---------------------------------------------------------------------------
// defextern declaration
// ---------------------------------------------------------------------------

/// A parsed `(defextern name : Type "c_name" ...)` declaration (spec §15.3).
///
/// Imports a C function with optional effect and safety annotations.
#[derive(Debug, Clone, PartialEq)]
pub struct DefExternDecl {
    /// Nexl function name.
    pub name: String,
    /// Type annotation (raw AST node, e.g. `(Fn [Float] -> Float)`).
    pub type_node: Node,
    /// C function name (string literal).
    pub c_name: String,
    /// Declared effects (`:performs [Effect ...]`). `None` means pure.
    pub performs: Option<Vec<String>>,
    /// Whether this is an unsafe extern (`:unsafe`).
    pub is_unsafe: bool,
}

// ---------------------------------------------------------------------------
// defexport declaration
// ---------------------------------------------------------------------------

/// A parsed `(defexport name : Type [params] body...)` declaration (spec §15.4).
///
/// Exports a Nexl function with a C-compatible ABI.
#[derive(Debug, Clone, PartialEq)]
pub struct DefExportDecl {
    /// Exported function name (used as C symbol name).
    pub name: String,
    /// Type annotation (raw AST node).
    pub type_node: Node,
    /// Parameter names.
    pub params: Vec<String>,
    /// Body expressions.
    pub body: Vec<Node>,
}

// ---------------------------------------------------------------------------
// deftype-opaque declaration
// ---------------------------------------------------------------------------

/// A parsed `(deftype-opaque Name Repr :drop drop-fn)` declaration (spec §15.3).
///
/// Wraps a C resource with an opaque type and automatic cleanup.
#[derive(Debug, Clone, PartialEq)]
pub struct DefTypeOpaqueDecl {
    /// The opaque type name (e.g. `"CHandle"`).
    pub name: String,
    /// The underlying representation type (e.g. `"Ptr"`).
    pub repr: String,
    /// Optional drop function name for automatic cleanup.
    pub drop_fn: Option<String>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// An error encountered while parsing an FFI declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfiParseError {
    /// Human-readable error description.
    pub description: String,
}

impl FfiParseError {
    /// Create a new error with the given description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

impl std::fmt::Display for FfiParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FFI parse error: {}", self.description)
    }
}

impl std::error::Error for FfiParseError {}

// ---------------------------------------------------------------------------
// Parsing — defextern
// ---------------------------------------------------------------------------

/// Parse a `(defextern name : Type "c_name" ...)` list into a [`DefExternDecl`].
///
/// `items` should be the full list contents including the `defextern` symbol.
pub fn parse_defextern_decl(items: &[Node]) -> Result<DefExternDecl, FfiParseError> {
    // items[0] = `defextern`
    // items[1] = name (symbol)
    // items[2] = `:` (keyword)
    // items[3] = type node
    // items[4] = "c_name" (string)
    // items[5..] = optional :performs [...] or :unsafe

    if items.len() < 2 {
        return Err(FfiParseError::new("defextern requires a name"));
    }

    let name = extract_plain_symbol(&items[1])?;

    if items.len() < 3 {
        return Err(FfiParseError::new(
            "defextern requires `:` before type annotation",
        ));
    }

    // Expect the colon
    match &items[2].kind {
        NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) if kw.is_empty() => {}
        NodeKind::Atom(Atom::Symbol { ns: None, name: s }) if s == ":" => {}
        _ => {
            return Err(FfiParseError::new(
                "expected `:` after name in defextern",
            ));
        }
    }

    if items.len() < 4 {
        return Err(FfiParseError::new("defextern requires a type annotation"));
    }

    let type_node = items[3].clone();

    if items.len() < 5 {
        return Err(FfiParseError::new(
            "defextern requires a C function name string",
        ));
    }

    let c_name = match &items[4].kind {
        NodeKind::Atom(Atom::Str(s)) => s.clone(),
        _ => {
            return Err(FfiParseError::new(
                "defextern C function name must be a string literal",
            ));
        }
    };

    // Parse optional annotations
    let mut performs: Option<Vec<String>> = None;
    let mut is_unsafe = false;

    let mut i = 5;
    while i < items.len() {
        match &items[i].kind {
            NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) if kw == "performs" => {
                i += 1;
                if i >= items.len() {
                    return Err(FfiParseError::new(
                        ":performs requires a vector argument",
                    ));
                }
                performs = Some(extract_symbol_vec(&items[i])?);
            }
            NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) if kw == "unsafe" => {
                is_unsafe = true;
            }
            _ => {
                return Err(FfiParseError::new(format!(
                    "unexpected form in defextern: {:?}",
                    items[i].kind
                )));
            }
        }
        i += 1;
    }

    Ok(DefExternDecl {
        name,
        type_node,
        c_name,
        performs,
        is_unsafe,
    })
}

// ---------------------------------------------------------------------------
// Parsing — defexport
// ---------------------------------------------------------------------------

/// Parse a `(defexport name : Type [params] body...)` list into a [`DefExportDecl`].
///
/// `items` should be the full list contents including the `defexport` symbol.
pub fn parse_defexport_decl(items: &[Node]) -> Result<DefExportDecl, FfiParseError> {
    // items[0] = `defexport`
    // items[1] = name (symbol)
    // items[2] = `:` (symbol)
    // items[3] = type node
    // items[4] = [params] (vector)
    // items[5..] = body

    if items.len() < 2 {
        return Err(FfiParseError::new("defexport requires a name"));
    }

    let name = extract_plain_symbol(&items[1])?;

    if items.len() < 3 {
        return Err(FfiParseError::new(
            "defexport requires `:` before type annotation",
        ));
    }

    match &items[2].kind {
        NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) if kw.is_empty() => {}
        NodeKind::Atom(Atom::Symbol { ns: None, name: s }) if s == ":" => {}
        _ => {
            return Err(FfiParseError::new(
                "expected `:` after name in defexport",
            ));
        }
    }

    if items.len() < 4 {
        return Err(FfiParseError::new("defexport requires a type annotation"));
    }

    let type_node = items[3].clone();

    if items.len() < 5 {
        return Err(FfiParseError::new("defexport requires a parameter vector"));
    }

    let params = extract_symbol_vec(&items[4])?;
    let body = items[5..].to_vec();

    Ok(DefExportDecl {
        name,
        type_node,
        params,
        body,
    })
}

// ---------------------------------------------------------------------------
// Parsing — deftype-opaque
// ---------------------------------------------------------------------------

/// Parse a `(deftype-opaque Name Repr :drop drop-fn)` list into a
/// [`DefTypeOpaqueDecl`].
///
/// `items` should be the full list contents including the `deftype-opaque` symbol.
pub fn parse_deftype_opaque_decl(items: &[Node]) -> Result<DefTypeOpaqueDecl, FfiParseError> {
    // items[0] = `deftype-opaque`
    // items[1] = name (symbol, e.g. CHandle)
    // items[2] = repr (symbol, e.g. Ptr)
    // items[3] = :drop (optional keyword)
    // items[4] = drop-fn (symbol)

    if items.len() < 2 {
        return Err(FfiParseError::new("deftype-opaque requires a type name"));
    }

    let name = extract_plain_symbol(&items[1])?;

    if items.len() < 3 {
        return Err(FfiParseError::new(
            "deftype-opaque requires a representation type",
        ));
    }

    let repr = extract_plain_symbol(&items[2])?;

    let mut drop_fn = None;
    let mut i = 3;
    while i < items.len() {
        match &items[i].kind {
            NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) if kw == "drop" => {
                i += 1;
                if i >= items.len() {
                    return Err(FfiParseError::new(":drop requires a function name"));
                }
                drop_fn = Some(extract_plain_symbol(&items[i])?);
            }
            _ => {
                return Err(FfiParseError::new(format!(
                    "unexpected form in deftype-opaque: {:?}",
                    items[i].kind
                )));
            }
        }
        i += 1;
    }

    Ok(DefTypeOpaqueDecl {
        name,
        repr,
        drop_fn,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a plain (unqualified) symbol name from a node.
fn extract_plain_symbol(node: &Node) -> Result<String, FfiParseError> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => Ok(name.clone()),
        _ => Err(FfiParseError::new("expected a symbol")),
    }
}

/// Extract a vector of plain symbol names from a `[...]` node.
fn extract_symbol_vec(node: &Node) -> Result<Vec<String>, FfiParseError> {
    match &node.kind {
        NodeKind::Vector(items) => {
            let mut names = Vec::with_capacity(items.len());
            for item in items {
                names.push(extract_plain_symbol(item)?);
            }
            Ok(names)
        }
        _ => Err(FfiParseError::new("expected a vector [...]")),
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

    fn str_lit(val: &str) -> Node {
        Node::atom(Atom::Str(val.to_string()), s())
    }

    fn list(items: Vec<Node>) -> Node {
        Node::new(NodeKind::List(items), s())
    }

    fn vec_node(items: Vec<Node>) -> Node {
        Node::new(NodeKind::Vector(items), s())
    }

    /// Build a simple `(Fn [A B] -> C)` type node.
    fn fn_type(params: &[&str], ret: &str) -> Node {
        let mut items = vec![sym("Fn"), vec_node(params.iter().map(|p| sym(p)).collect())];
        items.push(sym("->"));
        items.push(sym(ret));
        list(items)
    }

    // ── Test 1 ──

    #[test]
    fn test_parse_defextern_pure() {
        // (defextern sin : (Fn [Float] -> Float) "sin")
        let items = vec![
            sym("defextern"),
            sym("sin"),
            sym(":"),
            fn_type(&["Float"], "Float"),
            str_lit("sin"),
        ];
        let decl = parse_defextern_decl(&items).unwrap();
        assert_eq!(decl.name, "sin");
        assert_eq!(decl.c_name, "sin");
        assert_eq!(decl.performs, None);
        assert!(!decl.is_unsafe);
    }

    // ── Test 2 ──

    #[test]
    fn test_parse_defextern_with_performs() {
        // (defextern puts : (Fn [Str] -> Int32) "puts" :performs [Console])
        let items = vec![
            sym("defextern"),
            sym("puts"),
            sym(":"),
            fn_type(&["Str"], "Int32"),
            str_lit("puts"),
            kw("performs"),
            vec_node(vec![sym("Console")]),
        ];
        let decl = parse_defextern_decl(&items).unwrap();
        assert_eq!(decl.name, "puts");
        assert_eq!(decl.c_name, "puts");
        assert_eq!(decl.performs, Some(vec!["Console".to_string()]));
        assert!(!decl.is_unsafe);
    }

    // ── Test 3 ──

    #[test]
    fn test_parse_defextern_unsafe() {
        // (defextern malloc : (Fn [U64] -> Ptr) "malloc" :unsafe)
        let items = vec![
            sym("defextern"),
            sym("malloc"),
            sym(":"),
            fn_type(&["U64"], "Ptr"),
            str_lit("malloc"),
            kw("unsafe"),
        ];
        let decl = parse_defextern_decl(&items).unwrap();
        assert_eq!(decl.name, "malloc");
        assert_eq!(decl.c_name, "malloc");
        assert_eq!(decl.performs, None);
        assert!(decl.is_unsafe);
    }

    // ── Test 4 ──

    #[test]
    fn test_parse_defextern_error_missing_name() {
        let items = vec![sym("defextern")];
        let err = parse_defextern_decl(&items).unwrap_err();
        assert!(err.description.contains("requires a name"));
    }

    // ── Test 5 ──

    #[test]
    fn test_parse_defextern_error_missing_colon() {
        let items = vec![sym("defextern"), sym("sin")];
        let err = parse_defextern_decl(&items).unwrap_err();
        assert!(err.description.contains(":"));
    }

    // ── Test 6 ──

    #[test]
    fn test_parse_defextern_error_missing_type() {
        let items = vec![sym("defextern"), sym("sin"), sym(":")];
        let err = parse_defextern_decl(&items).unwrap_err();
        assert!(err.description.contains("type"));
    }

    // ── Test 7 ──

    #[test]
    fn test_parse_defextern_error_missing_c_name() {
        let items = vec![
            sym("defextern"),
            sym("sin"),
            sym(":"),
            fn_type(&["Float"], "Float"),
        ];
        let err = parse_defextern_decl(&items).unwrap_err();
        assert!(err.description.contains("C function name"));
    }

    // ── Test 8 ──

    #[test]
    fn test_parse_defextern_error_c_name_not_string() {
        let items = vec![
            sym("defextern"),
            sym("sin"),
            sym(":"),
            fn_type(&["Float"], "Float"),
            sym("sin"), // should be a string
        ];
        let err = parse_defextern_decl(&items).unwrap_err();
        assert!(err.description.contains("must be a string literal"));
    }

    // ── Test 9 ──

    #[test]
    fn test_defextern_decl_struct_fields() {
        let decl = DefExternDecl {
            name: "sin".to_string(),
            type_node: sym("Float"),
            c_name: "sin".to_string(),
            performs: Some(vec!["Math".to_string()]),
            is_unsafe: false,
        };
        assert_eq!(decl.name, "sin");
        assert_eq!(decl.c_name, "sin");
        assert!(decl.performs.is_some());
        assert!(!decl.is_unsafe);
    }

    // ====================================================================
    // defexport tests
    // ====================================================================

    // ── Test 10 (export) ──

    #[test]
    fn test_parse_defexport_basic() {
        // (defexport add_ints : (Fn [Int Int] -> Int) [a b] (+ a b))
        let items = vec![
            sym("defexport"),
            sym("add_ints"),
            sym(":"),
            fn_type(&["Int", "Int"], "Int"),
            vec_node(vec![sym("a"), sym("b")]),
            list(vec![sym("+"), sym("a"), sym("b")]),
        ];
        let decl = parse_defexport_decl(&items).unwrap();
        assert_eq!(decl.name, "add_ints");
        assert_eq!(decl.params, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(decl.body.len(), 1);
    }

    // ── Test 11 (export) ──

    #[test]
    fn test_parse_defexport_error_missing_name() {
        let items = vec![sym("defexport")];
        let err = parse_defexport_decl(&items).unwrap_err();
        assert!(err.description.contains("requires a name"));
    }

    // ── Test 12 (export) ──

    #[test]
    fn test_parse_defexport_error_missing_params() {
        let items = vec![
            sym("defexport"),
            sym("add"),
            sym(":"),
            fn_type(&["Int"], "Int"),
        ];
        let err = parse_defexport_decl(&items).unwrap_err();
        assert!(err.description.contains("parameter vector"));
    }

    // ── Test 13 (export) ──

    #[test]
    fn test_defexport_decl_struct_fields() {
        let decl = DefExportDecl {
            name: "add".to_string(),
            type_node: sym("Int"),
            params: vec!["a".to_string(), "b".to_string()],
            body: vec![sym("a")],
        };
        assert_eq!(decl.name, "add");
        assert_eq!(decl.params.len(), 2);
        assert_eq!(decl.body.len(), 1);
    }

    // ====================================================================
    // deftype-opaque tests
    // ====================================================================

    // ── Test 10 ──

    #[test]
    fn test_parse_deftype_opaque_with_drop() {
        // (deftype-opaque CHandle Ptr :drop free-handle)
        let items = vec![
            sym("deftype-opaque"),
            sym("CHandle"),
            sym("Ptr"),
            kw("drop"),
            sym("free-handle"),
        ];
        let decl = parse_deftype_opaque_decl(&items).unwrap();
        assert_eq!(decl.name, "CHandle");
        assert_eq!(decl.repr, "Ptr");
        assert_eq!(decl.drop_fn, Some("free-handle".to_string()));
    }

    // ── Test 11 ──

    #[test]
    fn test_parse_deftype_opaque_without_drop() {
        // (deftype-opaque RawBuffer Ptr)
        let items = vec![sym("deftype-opaque"), sym("RawBuffer"), sym("Ptr")];
        let decl = parse_deftype_opaque_decl(&items).unwrap();
        assert_eq!(decl.name, "RawBuffer");
        assert_eq!(decl.repr, "Ptr");
        assert_eq!(decl.drop_fn, None);
    }

    // ── Test 12 ──

    #[test]
    fn test_parse_deftype_opaque_error_missing_name() {
        let items = vec![sym("deftype-opaque")];
        let err = parse_deftype_opaque_decl(&items).unwrap_err();
        assert!(err.description.contains("requires a type name"));
    }

    // ── Test 13 ──

    #[test]
    fn test_parse_deftype_opaque_error_missing_repr() {
        let items = vec![sym("deftype-opaque"), sym("CHandle")];
        let err = parse_deftype_opaque_decl(&items).unwrap_err();
        assert!(err.description.contains("representation type"));
    }

    // ── Test 14 ──

    #[test]
    fn test_parse_deftype_opaque_error_drop_missing_fn() {
        let items = vec![
            sym("deftype-opaque"),
            sym("CHandle"),
            sym("Ptr"),
            kw("drop"),
        ];
        let err = parse_deftype_opaque_decl(&items).unwrap_err();
        assert!(err.description.contains(":drop requires a function name"));
    }
}
