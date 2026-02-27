//! Component import/export declarations for WASM Component Model (spec §15.1).
//!
//! Follows the same pattern as [`crate::module`]: semantic types plus
//! parsing functions that convert generic [`Node`] trees into structured
//! representations.

use crate::{Atom, Node, NodeKind};

// ---------------------------------------------------------------------------
// Import component declaration
// ---------------------------------------------------------------------------

/// A parsed `(import-component "name" :as alias {...})` declaration (spec §15.1).
///
/// Imports a foreign WASM component with compile-time type verification.
/// The exports map associates names with type annotations (stored as raw AST
/// nodes for later interpretation by the type checker).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportComponentDecl {
    /// Component name, e.g. `"image-processing"`.
    pub component_name: String,
    /// Local alias for qualified access, e.g. `img`.
    pub alias: String,
    /// Exported symbols with their type annotations.
    /// Each entry is `(name, type_node)` where `type_node` is a raw AST node
    /// representing the type (e.g. `(Fn [Bytes Int Int] -> Bytes)`).
    pub exports: Vec<(String, Node)>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// An error encountered while parsing a component declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentParseError {
    /// Human-readable error description.
    pub description: String,
}

impl ComponentParseError {
    /// Create a new error with the given description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

impl std::fmt::Display for ComponentParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "component parse error: {}", self.description)
    }
}

impl std::error::Error for ComponentParseError {}

// ---------------------------------------------------------------------------
// Parsing — import-component
// ---------------------------------------------------------------------------

/// Parse an `(import-component "name" :as alias {...})` list into an
/// [`ImportComponentDecl`].
///
/// The caller is responsible for identifying that the list head is
/// `import-component`. `items` should be the full list contents including
/// the `import-component` symbol.
pub fn parse_import_component_decl(
    items: &[Node],
) -> Result<ImportComponentDecl, ComponentParseError> {
    // items[0] = `import-component`
    // items[1] = string literal (component name)
    // items[2] = :as keyword
    // items[3] = alias symbol
    // items[4] = exports map {...}

    if items.len() < 2 {
        return Err(ComponentParseError::new(
            "import-component requires a component name",
        ));
    }

    // Extract component name (must be a string literal)
    let component_name = match &items[1].kind {
        NodeKind::Atom(Atom::Str(s)) => s.clone(),
        _ => {
            return Err(ComponentParseError::new(
                "import-component name must be a string literal",
            ));
        }
    };

    // Expect :as keyword
    if items.len() < 3 {
        return Err(ComponentParseError::new(
            "import-component requires :as alias",
        ));
    }
    match &items[2].kind {
        NodeKind::Atom(Atom::Keyword { ns: None, name }) if name == "as" => {}
        _ => {
            return Err(ComponentParseError::new(
                "expected :as keyword after component name",
            ));
        }
    }

    // Extract alias
    if items.len() < 4 {
        return Err(ComponentParseError::new(
            "import-component :as requires an alias name",
        ));
    }
    let alias = match &items[3].kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.clone(),
        _ => {
            return Err(ComponentParseError::new(
                "import-component alias must be a symbol",
            ));
        }
    };

    // Extract exports map
    if items.len() < 5 {
        return Err(ComponentParseError::new(
            "import-component requires an exports map",
        ));
    }
    let exports = extract_exports_map(&items[4])?;

    Ok(ImportComponentDecl {
        component_name,
        alias,
        exports,
    })
}

// ---------------------------------------------------------------------------
// Export component declaration
// ---------------------------------------------------------------------------

/// A parsed `(export-component "name" {...})` declaration (spec §15.1).
///
/// Exports a Nexl module as a WASM component. The compiler generates a WIT
/// interface from this declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportComponentDecl {
    /// Component name, e.g. `"string-utils"`.
    pub component_name: String,
    /// Exported symbols with their type annotations.
    pub exports: Vec<(String, Node)>,
}

// ---------------------------------------------------------------------------
// Parsing — export-component
// ---------------------------------------------------------------------------

/// Parse an `(export-component "name" {...})` list into an
/// [`ExportComponentDecl`].
///
/// `items` should be the full list contents including the `export-component`
/// symbol.
pub fn parse_export_component_decl(
    items: &[Node],
) -> Result<ExportComponentDecl, ComponentParseError> {
    // items[0] = `export-component`
    // items[1] = string literal (component name)
    // items[2] = exports map {...}

    if items.len() < 2 {
        return Err(ComponentParseError::new(
            "export-component requires a component name",
        ));
    }

    let component_name = match &items[1].kind {
        NodeKind::Atom(Atom::Str(s)) => s.clone(),
        _ => {
            return Err(ComponentParseError::new(
                "export-component name must be a string literal",
            ));
        }
    };

    if items.len() < 3 {
        return Err(ComponentParseError::new(
            "export-component requires an exports map",
        ));
    }

    let exports = extract_exports_map(&items[2])?;

    Ok(ExportComponentDecl {
        component_name,
        exports,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a map of `{:name TypeNode ...}` into `Vec<(String, Node)>`.
fn extract_exports_map(node: &Node) -> Result<Vec<(String, Node)>, ComponentParseError> {
    match &node.kind {
        NodeKind::Map(pairs) => {
            let mut exports = Vec::with_capacity(pairs.len());
            for (key, val) in pairs {
                let name = match &key.kind {
                    NodeKind::Atom(Atom::Keyword { ns: None, name }) => name.clone(),
                    _ => {
                        return Err(ComponentParseError::new("export map keys must be keywords"));
                    }
                };
                exports.push((name, val.clone()));
            }
            Ok(exports)
        }
        _ => Err(ComponentParseError::new(
            "import-component exports must be a map {...}",
        )),
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

    /// Build a symbol node.
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
    fn kw(name: &str) -> Node {
        Node::atom(
            Atom::Keyword {
                ns: None,
                name: name.to_string(),
            },
            s(),
        )
    }

    /// Build a string literal node.
    fn str_lit(val: &str) -> Node {
        Node::atom(Atom::Str(val.to_string()), s())
    }

    /// Build a list node.
    fn list(items: Vec<Node>) -> Node {
        Node::new(NodeKind::List(items), s())
    }

    /// Build a map node from (keyword, node) pairs.
    fn map(pairs: Vec<(&str, Node)>) -> Node {
        Node::new(
            NodeKind::Map(pairs.into_iter().map(|(k, v)| (kw(k), v)).collect()),
            s(),
        )
    }

    /// Build a vector node.
    fn vec_node(items: Vec<Node>) -> Node {
        Node::new(NodeKind::Vector(items), s())
    }

    /// Build a simple `(Fn [A B] -> C)` type node for testing.
    fn fn_type(params: &[&str], ret: &str) -> Node {
        let mut items = vec![sym("Fn"), vec_node(params.iter().map(|p| sym(p)).collect())];
        items.push(sym("->"));
        items.push(sym(ret));
        list(items)
    }

    // ── Test 1 ──

    #[test]
    fn test_parse_import_component_basic() {
        // (import-component "image-processing" :as img
        //   {:resize (Fn [Bytes Int Int] -> Bytes)
        //    :blur   (Fn [Bytes Float] -> Bytes)})
        let items = vec![
            sym("import-component"),
            str_lit("image-processing"),
            kw("as"),
            sym("img"),
            map(vec![
                ("resize", fn_type(&["Bytes", "Int", "Int"], "Bytes")),
                ("blur", fn_type(&["Bytes", "Float"], "Bytes")),
            ]),
        ];
        let decl = parse_import_component_decl(&items).unwrap();
        assert_eq!(decl.component_name, "image-processing");
        assert_eq!(decl.alias, "img");
        assert_eq!(decl.exports.len(), 2);
        assert_eq!(decl.exports[0].0, "resize");
        assert_eq!(decl.exports[1].0, "blur");
    }

    // ── Test 2 ──

    #[test]
    fn test_parse_import_component_single_export() {
        // (import-component "math" :as m {:sin (Fn [Float] -> Float)})
        let items = vec![
            sym("import-component"),
            str_lit("math"),
            kw("as"),
            sym("m"),
            map(vec![("sin", fn_type(&["Float"], "Float"))]),
        ];
        let decl = parse_import_component_decl(&items).unwrap();
        assert_eq!(decl.component_name, "math");
        assert_eq!(decl.alias, "m");
        assert_eq!(decl.exports.len(), 1);
        assert_eq!(decl.exports[0].0, "sin");
    }

    // ── Test 3 ──

    #[test]
    fn test_parse_import_component_with_resource() {
        // (import-component "database" :as db
        //   {:Connection (Resource {:open (Fn [Str] -> Connection)})})
        let resource_methods = map(vec![("open", fn_type(&["Str"], "Connection"))]);
        let resource_type = list(vec![sym("Resource"), resource_methods]);

        let items = vec![
            sym("import-component"),
            str_lit("database"),
            kw("as"),
            sym("db"),
            map(vec![("Connection", resource_type)]),
        ];
        let decl = parse_import_component_decl(&items).unwrap();
        assert_eq!(decl.component_name, "database");
        assert_eq!(decl.alias, "db");
        assert_eq!(decl.exports.len(), 1);
        assert_eq!(decl.exports[0].0, "Connection");
        // The Resource(...) node is stored as raw AST for type checker
        match &decl.exports[0].1.kind {
            NodeKind::List(items) => {
                assert_eq!(items.len(), 2);
                match &items[0].kind {
                    NodeKind::Atom(Atom::Symbol { name, .. }) => assert_eq!(name, "Resource"),
                    other => panic!("expected Resource symbol, got {:?}", other),
                }
            }
            other => panic!("expected list node, got {:?}", other),
        }
    }

    // ── Test 4 ──

    #[test]
    fn test_parse_import_component_error_missing_name() {
        // (import-component)
        let items = vec![sym("import-component")];
        let err = parse_import_component_decl(&items).unwrap_err();
        assert!(err.description.contains("requires a component name"));
    }

    // ── Test 5 ──

    #[test]
    fn test_parse_import_component_error_name_not_string() {
        // (import-component my-lib :as m {})
        let items = vec![
            sym("import-component"),
            sym("my-lib"),
            kw("as"),
            sym("m"),
            map(vec![]),
        ];
        let err = parse_import_component_decl(&items).unwrap_err();
        assert!(err.description.contains("must be a string literal"));
    }

    // ── Test 6 ──

    #[test]
    fn test_parse_import_component_error_missing_as() {
        // (import-component "lib")
        let items = vec![sym("import-component"), str_lit("lib")];
        let err = parse_import_component_decl(&items).unwrap_err();
        assert!(err.description.contains(":as"));
    }

    // ── Test 7 ──

    #[test]
    fn test_parse_import_component_error_missing_alias() {
        // (import-component "lib" :as)
        let items = vec![sym("import-component"), str_lit("lib"), kw("as")];
        let err = parse_import_component_decl(&items).unwrap_err();
        assert!(err.description.contains("alias"));
    }

    // ── Test 8 ──

    #[test]
    fn test_parse_import_component_error_missing_exports() {
        // (import-component "lib" :as l)
        let items = vec![sym("import-component"), str_lit("lib"), kw("as"), sym("l")];
        let err = parse_import_component_decl(&items).unwrap_err();
        assert!(err.description.contains("exports map"));
    }

    // ── Test 9 ──

    #[test]
    fn test_parse_import_component_error_exports_not_map() {
        // (import-component "lib" :as l [resize blur])
        let items = vec![
            sym("import-component"),
            str_lit("lib"),
            kw("as"),
            sym("l"),
            vec_node(vec![sym("resize"), sym("blur")]),
        ];
        let err = parse_import_component_decl(&items).unwrap_err();
        assert!(err.description.contains("must be a map"));
    }

    // ── Test 10 ──

    #[test]
    fn test_import_component_decl_struct_fields() {
        let decl = ImportComponentDecl {
            component_name: "my-component".to_string(),
            alias: "mc".to_string(),
            exports: vec![
                ("foo".to_string(), sym("Int")),
                ("bar".to_string(), sym("Str")),
            ],
        };
        assert_eq!(decl.component_name, "my-component");
        assert_eq!(decl.alias, "mc");
        assert_eq!(decl.exports.len(), 2);
        assert_eq!(decl.exports[0].0, "foo");
        assert_eq!(decl.exports[1].0, "bar");
    }

    // ====================================================================
    // export-component tests
    // ====================================================================

    // ── Test 11 ──

    #[test]
    fn test_parse_export_component_basic() {
        // (export-component "string-utils"
        //   {:reverse-words (Fn [Str] -> Str)
        //    :word-count    (Fn [Str] -> Int)})
        let items = vec![
            sym("export-component"),
            str_lit("string-utils"),
            map(vec![
                ("reverse-words", fn_type(&["Str"], "Str")),
                ("word-count", fn_type(&["Str"], "Int")),
            ]),
        ];
        let decl = parse_export_component_decl(&items).unwrap();
        assert_eq!(decl.component_name, "string-utils");
        assert_eq!(decl.exports.len(), 2);
        assert_eq!(decl.exports[0].0, "reverse-words");
        assert_eq!(decl.exports[1].0, "word-count");
    }

    // ── Test 12 ──

    #[test]
    fn test_parse_export_component_single_export() {
        let items = vec![
            sym("export-component"),
            str_lit("counter"),
            map(vec![("increment", fn_type(&["Int"], "Int"))]),
        ];
        let decl = parse_export_component_decl(&items).unwrap();
        assert_eq!(decl.component_name, "counter");
        assert_eq!(decl.exports.len(), 1);
    }

    // ── Test 13 ──

    #[test]
    fn test_parse_export_component_error_missing_name() {
        let items = vec![sym("export-component")];
        let err = parse_export_component_decl(&items).unwrap_err();
        assert!(err.description.contains("requires a component name"));
    }

    // ── Test 14 ──

    #[test]
    fn test_parse_export_component_error_name_not_string() {
        let items = vec![sym("export-component"), sym("not-a-string"), map(vec![])];
        let err = parse_export_component_decl(&items).unwrap_err();
        assert!(err.description.contains("must be a string literal"));
    }

    // ── Test 15 ──

    #[test]
    fn test_parse_export_component_error_missing_exports() {
        let items = vec![sym("export-component"), str_lit("lib")];
        let err = parse_export_component_decl(&items).unwrap_err();
        assert!(err.description.contains("exports map"));
    }

    // ── Test 16 ──

    #[test]
    fn test_parse_export_component_error_exports_not_map() {
        let items = vec![
            sym("export-component"),
            str_lit("lib"),
            vec_node(vec![sym("a")]),
        ];
        let err = parse_export_component_decl(&items).unwrap_err();
        assert!(err.description.contains("must be a map"));
    }

    // ── Test 17 ──

    #[test]
    fn test_export_component_decl_struct_fields() {
        let decl = ExportComponentDecl {
            component_name: "my-lib".to_string(),
            exports: vec![
                ("foo".to_string(), sym("Int")),
                ("bar".to_string(), sym("Str")),
            ],
        };
        assert_eq!(decl.component_name, "my-lib");
        assert_eq!(decl.exports.len(), 2);
        assert_eq!(decl.exports[0].0, "foo");
        assert_eq!(decl.exports[1].0, "bar");
    }
}
