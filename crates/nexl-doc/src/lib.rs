//! `nexl-doc` — HTML documentation renderer for Nexl modules.

use nexl_ast::{Atom, Comment, Node, NodeKind, PrettyPrinter, PrintConfig};
use nexl_infer::{Env, InferState};
use thiserror::Error;

/// Documentation for a single Nexl module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDoc {
    /// Fully-qualified module name.
    pub name: String,
    /// Optional module-level description.
    pub description: Option<String>,
    /// Documented functions in this module.
    pub functions: Vec<FunctionDoc>,
}

/// Documentation for a single function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDoc {
    /// Function name.
    pub name: String,
    /// Type signature string (includes effects when present).
    pub signature: String,
    /// Optional docstring.
    pub docstring: Option<String>,
    /// Requires contract clauses.
    pub requires: Vec<String>,
    /// Ensures contract clauses.
    pub ensures: Vec<String>,
    /// Examples contract clauses.
    pub examples: Vec<String>,
}

/// An HTML page rendered for a single module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModulePage {
    /// Module name.
    pub module: String,
    /// File name for the rendered page.
    pub filename: String,
    /// HTML contents.
    pub html: String,
}

/// Errors returned during documentation extraction.
#[derive(Debug, Error)]
pub enum DocError {
    /// Failed to parse source into forms.
    #[error("parse error: {0}")]
    Parse(String),
    /// Failed to infer a type signature.
    #[error("type error: {0}")]
    Type(String),
}

/// Extract documentation data from a Nexl source file.
pub fn extract_module_doc(source: &str) -> Result<ModuleDoc, DocError> {
    let nodes = nexl_reader::read(source, nexl_ast::FileId(0))
        .map_err(|diag| DocError::Parse(diag.to_string()))?;

    let name = module_name_from_nodes(&nodes).unwrap_or_else(|| "unknown".to_string());
    let description = module_description_from_nodes(&nodes);
    let printer = PrettyPrinter::new(PrintConfig::default());

    let mut env = Env::new();
    let mut state = InferState::new();
    let mut functions = Vec::new();

    for node in &nodes {
        let Some(defn) = extract_defn_doc(node, &printer) else {
            continue;
        };

        let (name, ty, new_env) = nexl_infer::infer_defn(&defn.node_for_infer, &env, &mut state)
            .map_err(|e| DocError::Type(e.to_string()))?;
        env = new_env;

        functions.push(FunctionDoc {
            name,
            signature: format!("{} : {ty}", defn.name),
            docstring: defn.docstring,
            requires: defn.requires,
            ensures: defn.ensures,
            examples: defn.examples,
        });
    }

    Ok(ModuleDoc {
        name,
        description,
        functions,
    })
}

/// Render HTML pages for a set of modules with cross-links.
pub fn render_module_pages(modules: &[ModuleDoc]) -> Vec<ModulePage> {
    let nav = modules
        .iter()
        .map(|doc| (doc.name.clone(), module_filename(&doc.name)))
        .collect::<Vec<_>>();

    modules
        .iter()
        .map(|doc| ModulePage {
            module: doc.name.clone(),
            filename: module_filename(&doc.name),
            html: render_module_with_nav(doc, &nav),
        })
        .collect()
}

/// Render a module documentation page as HTML.
pub fn render_module(doc: &ModuleDoc) -> String {
    render_module_with_nav(doc, &[])
}

/// Generate an index page linking to all module pages.
pub fn render_index_page(modules: &[ModuleDoc]) -> ModulePage {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("  <meta charset=\"utf-8\">\n");
    out.push_str("  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("  <title>Nexl Documentation</title>\n");
    out.push_str(&format!("  <style>\n{NEXL_DOC_CSS}\n  </style>\n"));
    out.push_str("</head>\n<body>\n  <main>\n");
    out.push_str("    <h1>Nexl Documentation</h1>\n");
    out.push_str("    <section class=\"module-index\">\n");
    out.push_str("      <h2>Modules</h2>\n");
    out.push_str("      <ul>\n");
    for doc in modules {
        let href = module_filename(&doc.name);
        out.push_str("        <li><a href=\"");
        out.push_str(&escape_html(&href));
        out.push_str("\">");
        out.push_str(&escape_html(&doc.name));
        out.push_str("</a>");
        if let Some(desc) = &doc.description {
            out.push_str(" — ");
            out.push_str(&escape_html(desc));
        }
        out.push_str("</li>\n");
    }
    out.push_str("      </ul>\n    </section>\n");
    out.push_str("  </main>\n</body>\n</html>\n");
    ModulePage {
        module: "index".to_string(),
        filename: "index.html".to_string(),
        html: out,
    }
}

fn render_module_with_nav(doc: &ModuleDoc, nav: &[(String, String)]) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("  <meta charset=\"utf-8\">\n");
    out.push_str("  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("  <title>");
    out.push_str(&escape_html(&format!("{} — Nexl docs", doc.name)));
    out.push_str("</title>\n");
    out.push_str(&format!("  <style>\n{NEXL_DOC_CSS}\n  </style>\n"));
    out.push_str("</head>\n<body>\n");
    if !nav.is_empty() {
        out.push_str("  <nav class=\"sidebar\">\n");
        out.push_str("    <h2><a href=\"index.html\">Nexl Docs</a></h2>\n");
        out.push_str("    <h3>Modules</h3>\n");
        out.push_str("    <ul>\n");
        for (name, href) in nav {
            let active = if name == &doc.name { " class=\"active\"" } else { "" };
            out.push_str(&format!(
                "      <li{active}><a href=\"{href}\">{name}</a></li>\n",
                href = escape_html(href),
                name = escape_html(name),
            ));
        }
        out.push_str("    </ul>\n");
        if !doc.functions.is_empty() {
            out.push_str("    <h3>Functions</h3>\n");
            out.push_str("    <ul>\n");
            for func in &doc.functions {
                let anchor = fn_anchor(&func.name);
                out.push_str(&format!(
                    "      <li><a href=\"#{anchor}\">{name}</a></li>\n",
                    name = escape_html(&func.name),
                ));
            }
            out.push_str("    </ul>\n");
        }
        out.push_str("  </nav>\n");
    }
    out.push_str("  <main>\n");
    out.push_str("    <h1>");
    out.push_str(&escape_html(&doc.name));
    out.push_str("</h1>\n");

    if let Some(description) = &doc.description {
        out.push_str("    <p class=\"module-desc\">");
        out.push_str(&escape_html(description));
        out.push_str("</p>\n");
    }

    if !doc.functions.is_empty() {
        out.push_str("    <section>\n");
        out.push_str("      <h2>Functions</h2>\n");
        for func in &doc.functions {
            let anchor = fn_anchor(&func.name);
            out.push_str(&format!(
                "      <article id=\"{anchor}\">\n"
            ));
            out.push_str(&format!(
                "        <h3><a href=\"#{anchor}\">{name}</a></h3>\n",
                name = escape_html(&func.name),
            ));
            out.push_str("        <pre><code>");
            out.push_str(&escape_html(&func.signature));
            out.push_str("</code></pre>\n");
            if let Some(docstring) = &func.docstring {
                out.push_str("        <p class=\"docstring\">");
                out.push_str(&escape_html(docstring));
                out.push_str("</p>\n");
            }
            render_contract_list(&mut out, "Requires", &func.requires);
            render_contract_list(&mut out, "Ensures", &func.ensures);
            render_contract_list(&mut out, "Examples", &func.examples);
            out.push_str("      </article>\n");
        }
        out.push_str("    </section>\n");
    }

    out.push_str("  </main>\n</body>\n</html>\n");
    out
}

/// Convert a function name to an HTML anchor id.
fn fn_anchor(name: &str) -> String {
    name.replace('!', "").replace('?', "").replace('/', "-")
}

fn render_contract_list(out: &mut String, title: &str, clauses: &[String]) {
    if clauses.is_empty() {
        return;
    }
    out.push_str("        <h4>");
    out.push_str(&escape_html(title));
    out.push_str("</h4>\n");
    out.push_str("        <ul>\n");
    for clause in clauses {
        out.push_str("          <li>");
        out.push_str(&escape_html(clause));
        out.push_str("</li>\n");
    }
    out.push_str("        </ul>\n");
}

/// Minimal CSS for documentation pages.
const NEXL_DOC_CSS: &str = r#"
:root { --bg: #fff; --fg: #1a1a2e; --accent: #4361ee; --code-bg: #f4f4f8;
        --nav-bg: #f9f9fc; --border: #e0e0e8; }
@media (prefers-color-scheme: dark) {
  :root { --bg: #1a1a2e; --fg: #e0e0e8; --accent: #7b8cff; --code-bg: #232342;
          --nav-bg: #16162a; --border: #2a2a4a; }
}
* { margin: 0; padding: 0; box-sizing: border-box; }
body { font-family: system-ui, sans-serif; color: var(--fg); background: var(--bg);
       display: flex; line-height: 1.6; }
nav.sidebar { width: 240px; min-height: 100vh; background: var(--nav-bg);
              border-right: 1px solid var(--border); padding: 1rem; position: sticky;
              top: 0; overflow-y: auto; flex-shrink: 0; }
nav.sidebar h2 { font-size: 1rem; margin-bottom: 0.5rem; }
nav.sidebar h2 a { color: var(--accent); text-decoration: none; }
nav.sidebar h3 { font-size: 0.85rem; margin-top: 1rem; text-transform: uppercase;
                 letter-spacing: 0.05em; color: var(--fg); opacity: 0.6; }
nav.sidebar ul { list-style: none; }
nav.sidebar li { margin: 0.2rem 0; }
nav.sidebar li a { color: var(--fg); text-decoration: none; font-size: 0.9rem; }
nav.sidebar li a:hover { color: var(--accent); }
nav.sidebar li.active a { color: var(--accent); font-weight: 600; }
main { max-width: 48rem; padding: 2rem 3rem; flex: 1; }
h1 { font-size: 1.8rem; margin-bottom: 0.5rem; }
h2 { font-size: 1.3rem; margin-top: 2rem; margin-bottom: 0.5rem; border-bottom: 1px solid var(--border); padding-bottom: 0.3rem; }
h3 { font-size: 1.1rem; margin-top: 1.5rem; }
h3 a { color: var(--fg); text-decoration: none; }
h3 a:hover { color: var(--accent); }
article { margin-bottom: 1.5rem; padding-bottom: 1rem; border-bottom: 1px solid var(--border); }
pre { background: var(--code-bg); padding: 0.75rem 1rem; border-radius: 4px;
      overflow-x: auto; margin: 0.5rem 0; font-size: 0.9rem; }
code { font-family: "SF Mono", Menlo, Consolas, monospace; }
.module-desc { margin: 0.5rem 0 1rem; opacity: 0.8; }
.docstring { margin: 0.5rem 0; }
h4 { font-size: 0.9rem; margin-top: 0.5rem; }
ul { padding-left: 1.5rem; margin: 0.25rem 0; }
.module-index ul { list-style: disc; }
.module-index li { margin: 0.4rem 0; }
.module-index a { color: var(--accent); text-decoration: none; font-weight: 500; }
"#;

fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn module_filename(name: &str) -> String {
    let mut sanitized = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            '.' | '/' => sanitized.push('-'),
            _ => sanitized.push(ch),
        }
    }
    sanitized.push_str(".html");
    sanitized
}

struct DefnDocParts {
    name: String,
    docstring: Option<String>,
    requires: Vec<String>,
    ensures: Vec<String>,
    examples: Vec<String>,
    node_for_infer: Node,
}

fn extract_defn_doc(node: &Node, printer: &PrettyPrinter) -> Option<DefnDocParts> {
    if !list_head_is(node, "defn") {
        return None;
    }
    let NodeKind::List(items) = &node.kind else {
        return None;
    };
    let name = match items.get(1)?.kind.clone() {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => name,
        NodeKind::Atom(Atom::Symbol { ns: Some(ns), name }) => format!("{ns}/{name}"),
        _ => return None,
    };

    let mut idx = 2;
    let docstring = match items.get(idx) {
        Some(Node {
            kind: NodeKind::Atom(Atom::Str(text)),
            ..
        }) => {
            idx += 1;
            Some(text.clone())
        }
        _ => None,
    };

    let params = items.get(idx)?;
    if !matches!(params.kind, NodeKind::Vector(_)) {
        return None;
    }
    idx += 1;

    if items.get(idx).is_some_and(|node| {
        matches!(
            &node.kind,
            NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "->"
        )
    }) {
        items.get(idx + 1)?;
        idx += 2;
    }

    let mut requires = Vec::new();
    let mut ensures = Vec::new();
    let mut examples = Vec::new();

    while idx + 1 < items.len() {
        let clause = match &items[idx].kind {
            NodeKind::Atom(Atom::Keyword { ns: None, name }) => name.as_str(),
            _ => break,
        };
        let NodeKind::Vector(forms) = &items[idx + 1].kind else {
            break;
        };
        let rendered = forms.iter().map(|node| printer.print(node)).collect();
        match clause {
            "requires" => requires = rendered,
            "ensures" => ensures = rendered,
            "examples" | "example" => examples = rendered,
            _ => break,
        }
        idx += 2;
    }

    Some(DefnDocParts {
        name,
        docstring,
        requires,
        ensures,
        examples,
        node_for_infer: defn_node_for_infer(node),
    })
}

fn defn_node_for_infer(node: &Node) -> Node {
    let NodeKind::List(items) = &node.kind else {
        return node.clone();
    };
    let has_docstring = matches!(
        items.get(2),
        Some(Node {
            kind: NodeKind::Atom(Atom::Str(_)),
            ..
        })
    );
    if !has_docstring {
        return node.clone();
    }
    let mut stripped = items.clone();
    stripped.remove(2);
    Node {
        kind: NodeKind::List(stripped),
        span: node.span,
        leading_comments: node.leading_comments.clone(),
        trailing_comment: node.trailing_comment.clone(),
    }
}

fn module_name_from_nodes(nodes: &[Node]) -> Option<String> {
    let node = nodes.first()?;
    if !list_head_is(node, "module") {
        return None;
    }
    let NodeKind::List(items) = &node.kind else {
        return None;
    };
    match items.get(1)?.kind.clone() {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) => Some(name),
        NodeKind::Atom(Atom::Symbol { ns: Some(ns), name }) => Some(format!("{ns}/{name}")),
        _ => None,
    }
}

fn module_description_from_nodes(nodes: &[Node]) -> Option<String> {
    let node = nodes.first()?;
    if node.leading_comments.is_empty() {
        return None;
    }
    let lines = node
        .leading_comments
        .iter()
        .map(|Comment(text)| text.trim().to_string())
        .collect::<Vec<_>>();
    Some(lines.join("\n"))
}

fn list_head_is(node: &Node, name: &str) -> bool {
    let NodeKind::List(items) = &node.kind else {
        return false;
    };
    let Some(first) = items.first() else {
        return false;
    };
    match &first.kind {
        NodeKind::Atom(Atom::Symbol {
            ns: None,
            name: head,
        }) => head == name,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_module_includes_header_and_description() {
        let doc = ModuleDoc {
            name: "math.core".to_string(),
            description: Some("Core math utilities".to_string()),
            functions: Vec::new(),
        };

        let html = render_module(&doc);
        assert!(html.contains("math.core"));
        assert!(html.contains("Core math utilities"));
        assert!(html.contains("<title>math.core — Nexl docs</title>"));
    }

    #[test]
    fn render_module_renders_functions() {
        let doc = ModuleDoc {
            name: "math.core".to_string(),
            description: None,
            functions: vec![FunctionDoc {
                name: "add".to_string(),
                signature: "(Fn [Int Int] -> Int)".to_string(),
                docstring: Some("Adds two integers".to_string()),
                requires: vec![],
                ensures: vec![],
                examples: vec![],
            }],
        };

        let html = render_module(&doc);
        assert!(html.contains("add"));
        assert!(html.contains("(Fn [Int Int] -&gt; Int)"));
        assert!(html.contains("Adds two integers"));
    }

    #[test]
    fn render_module_escapes_html() {
        let doc = ModuleDoc {
            name: "evil".to_string(),
            description: Some("<script>alert(1)</script>".to_string()),
            functions: Vec::new(),
        };

        let html = render_module(&doc);
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn extract_module_doc_collects_defn_details() {
        let source = r#";Module docs
(module math.core)

(defn ident "Identity" [x]
  :requires [true]
  :ensures [true]
  :examples [1]
  x)
"#;

        let doc = extract_module_doc(source).expect("extract doc");
        assert_eq!(doc.name, "math.core");
        assert_eq!(doc.description.as_deref(), Some("Module docs"));
        assert_eq!(doc.functions.len(), 1);

        let func = &doc.functions[0];
        assert_eq!(func.name, "ident");
        assert!(func.signature.contains("ident : (Fn"));
        assert_eq!(func.docstring.as_deref(), Some("Identity"));
        assert_eq!(func.requires, vec!["true"]);
        assert_eq!(func.ensures, vec!["true"]);
        assert_eq!(func.examples, vec!["1"]);
    }

    #[test]
    fn render_module_pages_include_cross_links() {
        let modules = vec![
            ModuleDoc {
                name: "alpha".to_string(),
                description: None,
                functions: Vec::new(),
            },
            ModuleDoc {
                name: "beta".to_string(),
                description: None,
                functions: Vec::new(),
            },
        ];

        let pages = render_module_pages(&modules);
        let alpha = pages
            .iter()
            .find(|page| page.module == "alpha")
            .expect("alpha page");
        assert!(alpha.html.contains("beta.html"));
        assert!(alpha.html.contains(">beta<"));
    }

    // ---- M25: Enhanced documentation ----

    #[test]
    fn rendered_html_includes_css() {
        let doc = ModuleDoc {
            name: "test".to_string(),
            description: None,
            functions: Vec::new(),
        };
        let html = render_module(&doc);
        assert!(html.contains("<style>"), "should include inline CSS");
        assert!(html.contains("--accent"), "CSS should have accent variable");
    }

    #[test]
    fn rendered_html_has_function_anchors() {
        let doc = ModuleDoc {
            name: "test".to_string(),
            description: None,
            functions: vec![FunctionDoc {
                name: "hello".to_string(),
                signature: "hello : (Fn [] -> Str)".to_string(),
                docstring: Some("Say hello".to_string()),
                requires: Vec::new(),
                ensures: Vec::new(),
                examples: Vec::new(),
            }],
        };
        let pages = render_module_pages(&[doc]);
        let page = &pages[0];
        assert!(page.html.contains("id=\"hello\""), "should have anchor id");
        assert!(page.html.contains("#hello"), "should have anchor link");
    }

    #[test]
    fn index_page_links_to_modules() {
        let modules = vec![
            ModuleDoc {
                name: "json".to_string(),
                description: Some("JSON handling".to_string()),
                functions: Vec::new(),
            },
            ModuleDoc {
                name: "http".to_string(),
                description: None,
                functions: Vec::new(),
            },
        ];
        let index = render_index_page(&modules);
        assert_eq!(index.filename, "index.html");
        assert!(index.html.contains("json.html"), "should link to json module");
        assert!(index.html.contains("http.html"), "should link to http module");
        assert!(index.html.contains("JSON handling"), "should include description");
    }

    #[test]
    fn sidebar_has_function_toc() {
        let doc = ModuleDoc {
            name: "math".to_string(),
            description: None,
            functions: vec![
                FunctionDoc {
                    name: "add".to_string(),
                    signature: "add : (Fn [Int Int] -> Int)".to_string(),
                    docstring: None,
                    requires: Vec::new(),
                    ensures: Vec::new(),
                    examples: Vec::new(),
                },
                FunctionDoc {
                    name: "sub".to_string(),
                    signature: "sub : (Fn [Int Int] -> Int)".to_string(),
                    docstring: None,
                    requires: Vec::new(),
                    ensures: Vec::new(),
                    examples: Vec::new(),
                },
            ],
        };
        let pages = render_module_pages(&[doc]);
        let html = &pages[0].html;
        // Sidebar should list functions for quick navigation.
        assert!(html.contains("#add"), "sidebar should link to add function");
        assert!(html.contains("#sub"), "sidebar should link to sub function");
    }
}
