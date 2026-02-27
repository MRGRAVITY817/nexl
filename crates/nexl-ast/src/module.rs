//! Module and import declaration types, parsed from raw AST nodes.
//!
//! Follows the same pattern as [`crate::pattern`]: semantic types plus
//! parsing functions that convert generic [`Node`] trees into structured
//! representations.

use crate::{Atom, Node, NodeKind};

// ---------------------------------------------------------------------------
// Module declaration
// ---------------------------------------------------------------------------

/// A parsed `(module name ...)` declaration (spec §8.1).
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDecl {
    /// Dotted module name, e.g. `"my-app.server"`.
    pub name: String,
    /// Exported symbols (`:exports [...]`). `None` means export everything.
    pub exports: Option<Vec<String>>,
    /// Declared effect capabilities (`:performs [...]`). `None` means infer.
    pub performs: Option<Vec<String>>,
    /// Imports declared via `:imports [[mod :as alias] ...]`.
    pub imports: Vec<ImportDecl>,
}

// ---------------------------------------------------------------------------
// Import declaration
// ---------------------------------------------------------------------------

/// A parsed `(import ...)` declaration (spec §8.2).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    /// Dotted module path, e.g. `"my-lib.http"`.
    pub module_path: String,
    /// How the import is brought into scope.
    pub kind: ImportKind,
}

/// How an imported module's names are made available.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportKind {
    /// `(import mod :as alias)` — qualified access via alias.
    Alias(String),
    /// `(import mod :refer [a b c])` — selective unqualified import.
    Refer(Vec<String>),
    /// `(import mod :exclude [a b c])` — import all but these names.
    Exclude(Vec<String>),
    /// `(import mod :rename {old new})` — rename imported symbols.
    Rename(Vec<(String, String)>),
    /// `(import mod)` — import all exports unqualified.
    All,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// An error encountered while parsing a module or import declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleParseError {
    /// Human-readable error description.
    pub description: String,
}

impl ModuleParseError {
    /// Create a new error with the given description.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
        }
    }
}

impl std::fmt::Display for ModuleParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "module parse error: {}", self.description)
    }
}

impl std::error::Error for ModuleParseError {}

// ---------------------------------------------------------------------------
// Parsing — module
// ---------------------------------------------------------------------------

/// Parse a `(module name ...)` list into a [`ModuleDecl`].
///
/// The caller is responsible for identifying that the list head is `module`.
/// `items` should be the full list contents including the `module` symbol.
pub fn parse_module_decl(items: &[Node]) -> Result<ModuleDecl, ModuleParseError> {
    // items[0] = `module` symbol, items[1] = name, rest = keyword options
    if items.len() < 2 {
        return Err(ModuleParseError::new("module declaration requires a name"));
    }

    let name = extract_symbol_or_dotted_name(&items[1])?;

    let mut exports: Option<Vec<String>> = None;
    let mut performs: Option<Vec<String>> = None;
    let mut imports: Vec<ImportDecl> = Vec::new();

    // Parse keyword options: :exports [...], :performs [...], :imports [[...]]
    let mut i = 2;
    while i < items.len() {
        match &items[i].kind {
            NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) if kw == "exports" => {
                i += 1;
                if i >= items.len() {
                    return Err(ModuleParseError::new(":exports requires a vector argument"));
                }
                exports = Some(extract_symbol_vec(&items[i])?);
            }
            NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) if kw == "performs" => {
                i += 1;
                if i >= items.len() {
                    return Err(ModuleParseError::new(
                        ":performs requires a vector argument",
                    ));
                }
                performs = Some(extract_symbol_vec(&items[i])?);
            }
            NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) if kw == "imports" => {
                i += 1;
                if i >= items.len() {
                    return Err(ModuleParseError::new(":imports requires a vector argument"));
                }
                imports = parse_import_specs(&items[i])?;
            }
            _ => {
                return Err(ModuleParseError::new(format!(
                    "unexpected form in module declaration: {:?}",
                    items[i].kind
                )));
            }
        }
        i += 1;
    }

    Ok(ModuleDecl {
        name,
        exports,
        performs,
        imports,
    })
}

// ---------------------------------------------------------------------------
// Parsing — import
// ---------------------------------------------------------------------------

/// Parse an `(import mod ...)` list into an [`ImportDecl`].
///
/// `items` should be the full list contents including the `import` symbol.
pub fn parse_import_decl(items: &[Node]) -> Result<ImportDecl, ModuleParseError> {
    // items[0] = `import` symbol, items[1] = module path, rest = options
    if items.len() < 2 {
        return Err(ModuleParseError::new(
            "import declaration requires a module path",
        ));
    }

    let module_path = extract_symbol_or_dotted_name(&items[1])?;
    parse_import_options(&module_path, &items[2..])
}

/// Shared keyword-option parsing for both `(import ...)` and `:imports` entries.
///
/// Given a module path and the remaining option nodes (`:as alias`, `:refer [...]`,
/// etc.), returns the corresponding [`ImportDecl`].
fn parse_import_options(
    module_path: &str,
    options: &[Node],
) -> Result<ImportDecl, ModuleParseError> {
    if options.is_empty() {
        return Ok(ImportDecl {
            module_path: module_path.to_string(),
            kind: ImportKind::All,
        });
    }

    match &options[0].kind {
        NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) if kw == "as" => {
            if options.len() < 2 {
                return Err(ModuleParseError::new(":as requires an alias name"));
            }
            let alias = extract_plain_symbol(&options[1])?;
            Ok(ImportDecl {
                module_path: module_path.to_string(),
                kind: ImportKind::Alias(alias),
            })
        }
        NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) if kw == "refer" => {
            if options.len() < 2 {
                return Err(ModuleParseError::new(":refer requires a vector of names"));
            }
            let names = extract_symbol_vec(&options[1])?;
            Ok(ImportDecl {
                module_path: module_path.to_string(),
                kind: ImportKind::Refer(names),
            })
        }
        NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) if kw == "exclude" => {
            if options.len() < 2 {
                return Err(ModuleParseError::new(":exclude requires a vector of names"));
            }
            let names = extract_symbol_vec(&options[1])?;
            Ok(ImportDecl {
                module_path: module_path.to_string(),
                kind: ImportKind::Exclude(names),
            })
        }
        NodeKind::Atom(Atom::Keyword { ns: None, name: kw }) if kw == "rename" => {
            if options.len() < 2 {
                return Err(ModuleParseError::new(":rename requires a map of names"));
            }
            let renames = extract_symbol_map(&options[1])?;
            Ok(ImportDecl {
                module_path: module_path.to_string(),
                kind: ImportKind::Rename(renames),
            })
        }
        _ => Err(ModuleParseError::new(format!(
            "unexpected import option: {:?}",
            options[0].kind
        ))),
    }
}

/// Parse the outer `:imports` vector: `[[mod :as alias] [mod2 :refer [a b]]]`.
fn parse_import_specs(node: &Node) -> Result<Vec<ImportDecl>, ModuleParseError> {
    match &node.kind {
        NodeKind::Vector(entries) => {
            let mut imports = Vec::with_capacity(entries.len());
            for entry in entries {
                imports.push(parse_import_spec(entry)?);
            }
            Ok(imports)
        }
        _ => Err(ModuleParseError::new(
            ":imports expects a vector of import specs",
        )),
    }
}

/// Parse a single `:imports` entry: `[mod :as alias]` or `[mod]`.
fn parse_import_spec(node: &Node) -> Result<ImportDecl, ModuleParseError> {
    match &node.kind {
        NodeKind::Vector(items) => {
            if items.is_empty() {
                return Err(ModuleParseError::new(":imports entry must not be empty"));
            }
            let module_path = extract_symbol_or_dotted_name(&items[0])?;
            parse_import_options(&module_path, &items[1..])
        }
        _ => Err(ModuleParseError::new(
            ":imports entry must be a vector like [mod :as alias]",
        )),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a plain (unqualified) symbol name from a node.
fn extract_plain_symbol(node: &Node) -> Result<String, ModuleParseError> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => Ok(name.clone()),
        _ => Err(ModuleParseError::new("expected a symbol")),
    }
}

/// Extract a symbol that may be a dotted module path.
///
/// The reader parses `my-app.server` as a single symbol token (dots are valid
/// in symbol characters). This function accepts both plain symbols and
/// dot-separated paths.
fn extract_symbol_or_dotted_name(node: &Node) -> Result<String, ModuleParseError> {
    match &node.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => Ok(name.clone()),
        _ => Err(ModuleParseError::new(
            "expected a module name (symbol or dotted path)",
        )),
    }
}

/// Extract a vector of plain symbol names from a `[...]` node.
fn extract_symbol_vec(node: &Node) -> Result<Vec<String>, ModuleParseError> {
    match &node.kind {
        NodeKind::Vector(items) => {
            let mut names = Vec::with_capacity(items.len());
            for item in items {
                names.push(extract_plain_symbol(item)?);
            }
            Ok(names)
        }
        _ => Err(ModuleParseError::new("expected a vector [...]")),
    }
}

/// Extract a map of plain symbol renames from a `{...}` node.
fn extract_symbol_map(node: &Node) -> Result<Vec<(String, String)>, ModuleParseError> {
    match &node.kind {
        NodeKind::Map(pairs) => {
            let mut renames = Vec::with_capacity(pairs.len());
            for (key, val) in pairs {
                let from = extract_plain_symbol(key)?;
                let to = extract_plain_symbol(val)?;
                renames.push((from, to));
            }
            Ok(renames)
        }
        _ => Err(ModuleParseError::new("expected a map {...}")),
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

    /// Build a vector node from symbol names.
    fn sym_vec(names: &[&str]) -> Node {
        Node::new(
            NodeKind::Vector(names.iter().map(|n| sym(n)).collect()),
            s(),
        )
    }

    // ── Test 1 ──

    #[test]
    fn test_parse_module_minimal() {
        // (module my-app.server)
        let items = vec![sym("module"), sym("my-app.server")];
        let decl = parse_module_decl(&items).unwrap();
        assert_eq!(decl.name, "my-app.server");
        assert_eq!(decl.exports, None);
        assert_eq!(decl.performs, None);
    }

    // ── Test 2 ──

    #[test]
    fn test_parse_module_with_exports() {
        // (module my-app.server :exports [start! stop! ServerConfig])
        let items = vec![
            sym("module"),
            sym("my-app.server"),
            kw("exports"),
            sym_vec(&["start!", "stop!", "ServerConfig"]),
        ];
        let decl = parse_module_decl(&items).unwrap();
        assert_eq!(decl.name, "my-app.server");
        assert_eq!(
            decl.exports,
            Some(vec![
                "start!".to_string(),
                "stop!".to_string(),
                "ServerConfig".to_string(),
            ])
        );
        assert_eq!(decl.performs, None);
    }

    // ── Test 3 ──

    #[test]
    fn test_parse_module_with_performs() {
        // (module my-app.server :performs [Net IO Log])
        let items = vec![
            sym("module"),
            sym("my-app.server"),
            kw("performs"),
            sym_vec(&["Net", "IO", "Log"]),
        ];
        let decl = parse_module_decl(&items).unwrap();
        assert_eq!(decl.name, "my-app.server");
        assert_eq!(decl.exports, None);
        assert_eq!(
            decl.performs,
            Some(vec!["Net".to_string(), "IO".to_string(), "Log".to_string(),])
        );
    }

    // ── Test 4 ──

    #[test]
    fn test_parse_module_full() {
        // (module my-app.server :performs [Net IO] :exports [start! stop!])
        let items = vec![
            sym("module"),
            sym("my-app.server"),
            kw("performs"),
            sym_vec(&["Net", "IO"]),
            kw("exports"),
            sym_vec(&["start!", "stop!"]),
        ];
        let decl = parse_module_decl(&items).unwrap();
        assert_eq!(decl.name, "my-app.server");
        assert_eq!(
            decl.exports,
            Some(vec!["start!".to_string(), "stop!".to_string()])
        );
        assert_eq!(
            decl.performs,
            Some(vec!["Net".to_string(), "IO".to_string()])
        );
    }

    // ── Test 5 ──

    #[test]
    fn test_parse_module_error_missing_name() {
        // (module) → error
        let items = vec![sym("module")];
        let err = parse_module_decl(&items).unwrap_err();
        assert!(err.description.contains("requires a name"));
    }

    // ── Test 6 ──

    #[test]
    fn test_parse_module_error_bad_name() {
        // (module 42) → error
        let items = vec![
            sym("module"),
            Node::atom(
                Atom::Int {
                    value: 42,
                    suffix: None,
                },
                s(),
            ),
        ];
        let err = parse_module_decl(&items).unwrap_err();
        assert!(err.description.contains("expected a module name"));
    }

    // ── Test 7 ──

    #[test]
    fn test_parse_import_as() {
        // (import foo.bar :as fb)
        let items = vec![sym("import"), sym("foo.bar"), kw("as"), sym("fb")];
        let decl = parse_import_decl(&items).unwrap();
        assert_eq!(decl.module_path, "foo.bar");
        assert_eq!(decl.kind, ImportKind::Alias("fb".to_string()));
    }

    // ── Test 8 ──

    #[test]
    fn test_parse_import_refer() {
        // (import foo.bar :refer [a b c])
        let items = vec![
            sym("import"),
            sym("foo.bar"),
            kw("refer"),
            sym_vec(&["a", "b", "c"]),
        ];
        let decl = parse_import_decl(&items).unwrap();
        assert_eq!(decl.module_path, "foo.bar");
        assert_eq!(
            decl.kind,
            ImportKind::Refer(vec!["a".to_string(), "b".to_string(), "c".to_string(),])
        );
    }

    // ── Test 9 ──

    #[test]
    fn test_parse_import_bare() {
        // (import foo.bar)
        let items = vec![sym("import"), sym("foo.bar")];
        let decl = parse_import_decl(&items).unwrap();
        assert_eq!(decl.module_path, "foo.bar");
        assert_eq!(decl.kind, ImportKind::All);
    }

    // ── Test 10 ──

    #[test]
    fn test_parse_import_error_missing_module() {
        // (import) → error
        let items = vec![sym("import")];
        let err = parse_import_decl(&items).unwrap_err();
        assert!(err.description.contains("requires a module path"));
    }

    // ── Test 11 ──

    #[test]
    fn test_parse_import_error_bad_module_name() {
        // (import 42) → error
        let items = vec![
            sym("import"),
            Node::atom(
                Atom::Int {
                    value: 42,
                    suffix: None,
                },
                s(),
            ),
        ];
        let err = parse_import_decl(&items).unwrap_err();
        assert!(err.description.contains("expected a module name"));
    }

    // ── Test 12 ──

    #[test]
    fn test_module_decl_struct_fields() {
        let decl = ModuleDecl {
            name: "my-app".to_string(),
            exports: Some(vec!["foo".to_string()]),
            performs: Some(vec!["IO".to_string()]),
            imports: vec![],
        };
        assert_eq!(decl.name, "my-app");
        assert_eq!(decl.exports.as_ref().unwrap().len(), 1);
        assert_eq!(decl.performs.as_ref().unwrap()[0], "IO");
        assert!(decl.imports.is_empty());
    }

    // ── Test 13 ──

    #[test]
    fn test_import_decl_struct_fields() {
        let decl = ImportDecl {
            module_path: "std.str".to_string(),
            kind: ImportKind::Alias("s".to_string()),
        };
        assert_eq!(decl.module_path, "std.str");
        assert_eq!(decl.kind, ImportKind::Alias("s".to_string()));

        let decl2 = ImportDecl {
            module_path: "std.io".to_string(),
            kind: ImportKind::All,
        };
        assert_eq!(decl2.kind, ImportKind::All);
    }

    // ── :imports tests ──

    // ── Test 14 ──

    #[test]
    fn parse_module_with_imports_as() {
        // (module todo.app :imports [[todo.model :as model]])
        let items = vec![
            sym("module"),
            sym("todo.app"),
            kw("imports"),
            Node::new(
                NodeKind::Vector(vec![Node::new(
                    NodeKind::Vector(vec![sym("todo.model"), kw("as"), sym("model")]),
                    s(),
                )]),
                s(),
            ),
        ];
        let decl = parse_module_decl(&items).unwrap();
        assert_eq!(decl.name, "todo.app");
        assert_eq!(decl.imports.len(), 1);
        assert_eq!(decl.imports[0].module_path, "todo.model");
        assert_eq!(decl.imports[0].kind, ImportKind::Alias("model".to_string()));
    }

    // ── Test 15 ──

    #[test]
    fn parse_module_with_imports_refer() {
        // (module todo.app :imports [[todo.model :refer [Task Priority]]])
        let items = vec![
            sym("module"),
            sym("todo.app"),
            kw("imports"),
            Node::new(
                NodeKind::Vector(vec![Node::new(
                    NodeKind::Vector(vec![
                        sym("todo.model"),
                        kw("refer"),
                        sym_vec(&["Task", "Priority"]),
                    ]),
                    s(),
                )]),
                s(),
            ),
        ];
        let decl = parse_module_decl(&items).unwrap();
        assert_eq!(decl.imports.len(), 1);
        assert_eq!(
            decl.imports[0].kind,
            ImportKind::Refer(vec!["Task".to_string(), "Priority".to_string()])
        );
    }

    // ── Test 16 ──

    #[test]
    fn parse_module_with_imports_bare() {
        // (module todo.app :imports [[todo.model]])
        let items = vec![
            sym("module"),
            sym("todo.app"),
            kw("imports"),
            Node::new(
                NodeKind::Vector(vec![Node::new(
                    NodeKind::Vector(vec![sym("todo.model")]),
                    s(),
                )]),
                s(),
            ),
        ];
        let decl = parse_module_decl(&items).unwrap();
        assert_eq!(decl.imports.len(), 1);
        assert_eq!(decl.imports[0].module_path, "todo.model");
        assert_eq!(decl.imports[0].kind, ImportKind::All);
    }

    // ── Test 17 ──

    #[test]
    fn parse_module_with_imports_multiple() {
        // (module todo.app :imports [[todo.model :as model]
        //                            [todo.storage :refer [save! load!]]
        //                            [todo.display]])
        let items = vec![
            sym("module"),
            sym("todo.app"),
            kw("imports"),
            Node::new(
                NodeKind::Vector(vec![
                    Node::new(
                        NodeKind::Vector(vec![sym("todo.model"), kw("as"), sym("model")]),
                        s(),
                    ),
                    Node::new(
                        NodeKind::Vector(vec![
                            sym("todo.storage"),
                            kw("refer"),
                            sym_vec(&["save!", "load!"]),
                        ]),
                        s(),
                    ),
                    Node::new(NodeKind::Vector(vec![sym("todo.display")]), s()),
                ]),
                s(),
            ),
        ];
        let decl = parse_module_decl(&items).unwrap();
        assert_eq!(decl.imports.len(), 3);
        assert_eq!(decl.imports[0].module_path, "todo.model");
        assert_eq!(decl.imports[0].kind, ImportKind::Alias("model".to_string()));
        assert_eq!(decl.imports[1].module_path, "todo.storage");
        assert_eq!(
            decl.imports[1].kind,
            ImportKind::Refer(vec!["save!".to_string(), "load!".to_string()])
        );
        assert_eq!(decl.imports[2].module_path, "todo.display");
        assert_eq!(decl.imports[2].kind, ImportKind::All);
    }

    // ── Test 18 ──

    #[test]
    fn parse_module_with_imports_and_exports() {
        // (module todo.app
        //   :exports [run!]
        //   :imports [[todo.model :as model]])
        let items = vec![
            sym("module"),
            sym("todo.app"),
            kw("exports"),
            sym_vec(&["run!"]),
            kw("imports"),
            Node::new(
                NodeKind::Vector(vec![Node::new(
                    NodeKind::Vector(vec![sym("todo.model"), kw("as"), sym("model")]),
                    s(),
                )]),
                s(),
            ),
        ];
        let decl = parse_module_decl(&items).unwrap();
        assert_eq!(decl.exports, Some(vec!["run!".to_string()]));
        assert_eq!(decl.imports.len(), 1);
        assert_eq!(decl.imports[0].module_path, "todo.model");
    }

    // ── Test 19 ──

    #[test]
    fn parse_module_imports_error_not_vector() {
        // (module todo.app :imports foo) → error
        let items = vec![sym("module"), sym("todo.app"), kw("imports"), sym("foo")];
        let err = parse_module_decl(&items).unwrap_err();
        assert!(err.description.contains("vector"));
    }

    // ── Test 20 ──

    #[test]
    fn parse_module_imports_error_entry_not_vector() {
        // (module todo.app :imports [foo]) → error (entry is symbol, not vector)
        let items = vec![
            sym("module"),
            sym("todo.app"),
            kw("imports"),
            Node::new(NodeKind::Vector(vec![sym("foo")]), s()),
        ];
        let err = parse_module_decl(&items).unwrap_err();
        assert!(err.description.contains("vector"));
    }

    // ── Test 21 ──

    #[test]
    fn parse_module_imports_error_empty_entry() {
        // (module todo.app :imports [[]]) → error
        let items = vec![
            sym("module"),
            sym("todo.app"),
            kw("imports"),
            Node::new(
                NodeKind::Vector(vec![Node::new(NodeKind::Vector(vec![]), s())]),
                s(),
            ),
        ];
        let err = parse_module_decl(&items).unwrap_err();
        assert!(err.description.contains("empty"));
    }
}
