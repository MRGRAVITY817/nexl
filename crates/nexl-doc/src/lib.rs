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

/// Render a module documentation page as HTML.
pub fn render_module(doc: &ModuleDoc) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n");
    out.push_str("<html lang=\"en\">\n");
    out.push_str("<head>\n");
    out.push_str("  <meta charset=\"utf-8\">\n");
    out.push_str("  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("  <title>");
    out.push_str(&escape_html(&format!("{} — Nexl docs", doc.name)));
    out.push_str("</title>\n");
    out.push_str("</head>\n");
    out.push_str("<body>\n");
    out.push_str("  <main>\n");
    out.push_str("    <h1>");
    out.push_str(&escape_html(&doc.name));
    out.push_str("</h1>\n");

    if let Some(description) = &doc.description {
        out.push_str("    <p>");
        out.push_str(&escape_html(description));
        out.push_str("</p>\n");
    }

    if !doc.functions.is_empty() {
        out.push_str("    <section>\n");
        out.push_str("      <h2>Functions</h2>\n");
        for func in &doc.functions {
            out.push_str("      <article>\n");
            out.push_str("        <h3>");
            out.push_str(&escape_html(&func.name));
            out.push_str("</h3>\n");
            out.push_str("        <pre><code>");
            out.push_str(&escape_html(&func.signature));
            out.push_str("</code></pre>\n");
            if let Some(docstring) = &func.docstring {
                out.push_str("        <p>");
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

    out.push_str("  </main>\n");
    out.push_str("</body>\n");
    out.push_str("</html>\n");
    out
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
        NodeKind::Atom(Atom::Symbol { ns: None, name: head }) => head == name,
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
}
