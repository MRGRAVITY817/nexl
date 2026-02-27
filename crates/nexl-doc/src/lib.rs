//! `nexl-doc` — HTML documentation renderer for Nexl modules.

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
    /// Type signature string.
    pub signature: String,
    /// Optional docstring.
    pub docstring: Option<String>,
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
            out.push_str("      </article>\n");
        }
        out.push_str("    </section>\n");
    }

    out.push_str("  </main>\n");
    out.push_str("</body>\n");
    out.push_str("</html>\n");
    out
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
}
