use crate::node::{Atom, FloatSuffix, IntSuffix, Node, NodeKind};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the AST pretty-printer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintConfig {
    /// Number of spaces per indentation level (default: 2).
    ///
    /// M0 uses flat (single-line) output; this field is reserved for multi-line
    /// mode in a later milestone.
    pub indent_width: usize,
}

impl Default for PrintConfig {
    fn default() -> Self {
        Self { indent_width: 2 }
    }
}

// ---------------------------------------------------------------------------
// PrettyPrinter
// ---------------------------------------------------------------------------

/// S-expression pretty-printer for [`Node`] trees.
///
/// Produces a canonical text representation that preserves leading and trailing
/// comments for round-trip formatting. M0 output is flat (single-line); nested
/// indentation is planned for a later milestone.
pub struct PrettyPrinter {
    /// Printer configuration (indent width, etc.). Reserved for multi-line
    /// mode; M0 always produces flat output.
    #[allow(dead_code)]
    config: PrintConfig,
}

impl PrettyPrinter {
    /// Create a printer with the given configuration.
    pub fn new(config: PrintConfig) -> Self {
        Self { config }
    }

    /// Create a printer with default configuration.
    pub fn default_config() -> Self {
        Self::new(PrintConfig::default())
    }

    /// Pretty-print `node` into a [`String`].
    pub fn print(&self, node: &Node) -> String {
        let mut out = String::new();
        self.write_node(node, &mut out);
        out
    }

    fn write_node(&self, node: &Node, out: &mut String) {
        // Leading comments — each on its own line before the node.
        for comment in &node.leading_comments {
            out.push(';');
            out.push_str(&comment.0);
            out.push('\n');
        }

        self.write_kind(&node.kind, out);

        // Trailing comment — inline after the node.
        if let Some(comment) = &node.trailing_comment {
            out.push_str(" ;");
            out.push_str(&comment.0);
        }
    }

    fn write_kind(&self, kind: &NodeKind, out: &mut String) {
        match kind {
            NodeKind::Atom(atom) => self.write_atom(atom, out),
            NodeKind::List(nodes) => {
                out.push('(');
                write_sep(nodes, out, |n, o| self.write_node(n, o));
                out.push(')');
            }
            NodeKind::Vector(nodes) => {
                out.push('[');
                write_sep(nodes, out, |n, o| self.write_node(n, o));
                out.push(']');
            }
            NodeKind::Map(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    self.write_node(k, out);
                    out.push(' ');
                    self.write_node(v, out);
                }
                out.push('}');
            }
            NodeKind::Set(nodes) => {
                out.push_str("#{");
                write_sep(nodes, out, |n, o| self.write_node(n, o));
                out.push('}');
            }
            NodeKind::Quote(inner) => {
                out.push('\'');
                self.write_node(inner, out);
            }
            NodeKind::Deref(inner) => {
                out.push('@');
                self.write_node(inner, out);
            }
            NodeKind::Discard(inner) => {
                out.push_str("#_ ");
                self.write_node(inner, out);
            }
            NodeKind::Quasiquote(inner) => {
                out.push('`');
                self.write_node(inner, out);
            }
            NodeKind::Unquote(inner) => {
                out.push('~');
                self.write_node(inner, out);
            }
            NodeKind::UnquoteSplice(inner) => {
                out.push_str("~@");
                self.write_node(inner, out);
            }
        }
    }

    fn write_atom(&self, atom: &Atom, out: &mut String) {
        match atom {
            Atom::Int { value, suffix } => {
                out.push_str(&value.to_string());
                if let Some(s) = suffix {
                    out.push_str(int_suffix_str(*s));
                }
            }
            Atom::Float { value, suffix } => {
                out.push_str(&format_float(*value));
                if let Some(s) = suffix {
                    out.push_str(float_suffix_str(*s));
                }
            }
            Atom::Ratio { numer, denom } => {
                out.push_str(&numer.to_string());
                out.push('/');
                out.push_str(&denom.to_string());
            }
            Atom::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Atom::Unit => out.push_str("unit"),
            Atom::Char(c) => write_char(*c, out),
            Atom::Str(s) => write_str(s, out),
            Atom::Keyword { ns, name } => {
                out.push(':');
                if let Some(ns) = ns {
                    out.push_str(ns);
                    out.push('/');
                }
                out.push_str(name);
            }
            Atom::Symbol { ns, name } => {
                if let Some(ns) = ns {
                    out.push_str(ns);
                    out.push('/');
                }
                out.push_str(name);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// std::fmt::Display
// ---------------------------------------------------------------------------

impl std::fmt::Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = PrettyPrinter::default_config().print(self);
        f.write_str(&s)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn write_sep<T>(items: &[T], out: &mut String, mut write: impl FnMut(&T, &mut String)) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        write(item, out);
    }
}

fn int_suffix_str(s: IntSuffix) -> &'static str {
    match s {
        IntSuffix::I8 => "i8",
        IntSuffix::I16 => "i16",
        IntSuffix::I32 => "i32",
        IntSuffix::I64 => "i64",
        IntSuffix::U8 => "u8",
        IntSuffix::U16 => "u16",
        IntSuffix::U32 => "u32",
        IntSuffix::U64 => "u64",
    }
}

fn float_suffix_str(s: FloatSuffix) -> &'static str {
    match s {
        FloatSuffix::F32 => "f32",
        FloatSuffix::F64 => "f64",
    }
}

/// Format a float value, ensuring a decimal point is always present.
fn format_float(v: f64) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 {
            "Inf".to_string()
        } else {
            "-Inf".to_string()
        };
    }
    let s = format!("{v}");
    // Ensure there is a decimal point so the output is recognised as a float.
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// Emit the Nexl source representation of a character literal.
fn write_char(c: char, out: &mut String) {
    out.push('\\');
    match c {
        ' ' => out.push_str("space"),
        '\n' => out.push_str("newline"),
        '\t' => out.push_str("tab"),
        '\r' => out.push_str("return"),
        c if c.is_ascii_graphic() => out.push(c),
        c => {
            let code = c as u32;
            out.push_str(&format!("u{{{code:X}}}"));
        }
    }
}

/// Emit a double-quoted string literal, re-escaping special characters.
fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::Span;

    fn sp() -> Span {
        Span::synthetic()
    }

    fn pp() -> PrettyPrinter {
        PrettyPrinter::default_config()
    }

    fn atom_node(atom: Atom) -> Node {
        Node::atom(atom, sp())
    }

    // ── 1. print_int_unsuffixed ───────────────────────────────────────────
    #[test]
    fn print_int_unsuffixed() {
        let node = atom_node(Atom::Int {
            value: 42,
            suffix: None,
        });
        assert_eq!(pp().print(&node), "42");
    }

    // ── 2. print_int_i32_suffix ───────────────────────────────────────────
    #[test]
    fn print_int_i32_suffix() {
        let node = atom_node(Atom::Int {
            value: 42,
            suffix: Some(IntSuffix::I32),
        });
        assert_eq!(pp().print(&node), "42i32");
    }

    // ── 3. print_int_u8_suffix ────────────────────────────────────────────
    #[test]
    fn print_int_u8_suffix() {
        let node = atom_node(Atom::Int {
            value: 255,
            suffix: Some(IntSuffix::U8),
        });
        assert_eq!(pp().print(&node), "255u8");
    }

    // ── 4. print_int_negative ─────────────────────────────────────────────
    #[test]
    fn print_int_negative() {
        let node = atom_node(Atom::Int {
            value: -7,
            suffix: None,
        });
        assert_eq!(pp().print(&node), "-7");
    }

    // ── 5. print_float_unsuffixed ─────────────────────────────────────────
    #[test]
    fn print_float_unsuffixed() {
        let node = atom_node(Atom::Float {
            value: 2.5,
            suffix: None,
        });
        assert_eq!(pp().print(&node), "2.5");
    }

    // ── 6. print_float_f32_suffix ─────────────────────────────────────────
    #[test]
    fn print_float_f32_suffix() {
        let node = atom_node(Atom::Float {
            value: 2.5,
            suffix: Some(FloatSuffix::F32),
        });
        assert_eq!(pp().print(&node), "2.5f32");
    }

    // ── 7. print_ratio ────────────────────────────────────────────────────
    #[test]
    fn print_ratio() {
        let node = atom_node(Atom::Ratio { numer: 3, denom: 4 });
        assert_eq!(pp().print(&node), "3/4");
    }

    // ── 8. print_bool_true ────────────────────────────────────────────────
    #[test]
    fn print_bool_true() {
        let node = atom_node(Atom::Bool(true));
        assert_eq!(pp().print(&node), "true");
    }

    // ── 9. print_bool_false ───────────────────────────────────────────────
    #[test]
    fn print_bool_false() {
        let node = atom_node(Atom::Bool(false));
        assert_eq!(pp().print(&node), "false");
    }

    // ── 10. print_unit ────────────────────────────────────────────────────
    #[test]
    fn print_unit() {
        let node = atom_node(Atom::Unit);
        assert_eq!(pp().print(&node), "unit");
    }

    // ── 11. print_char_letter ─────────────────────────────────────────────
    #[test]
    fn print_char_letter() {
        let node = atom_node(Atom::Char('a'));
        assert_eq!(pp().print(&node), "\\a");
    }

    // ── 12. print_char_named_space ────────────────────────────────────────
    #[test]
    fn print_char_named_space() {
        let node = atom_node(Atom::Char(' '));
        assert_eq!(pp().print(&node), "\\space");
    }

    // ── 13. print_char_named_newline ──────────────────────────────────────
    #[test]
    fn print_char_named_newline() {
        let node = atom_node(Atom::Char('\n'));
        assert_eq!(pp().print(&node), "\\newline");
    }

    // ── 14. print_char_named_tab ──────────────────────────────────────────
    #[test]
    fn print_char_named_tab() {
        let node = atom_node(Atom::Char('\t'));
        assert_eq!(pp().print(&node), "\\tab");
    }

    // ── 15. print_char_unicode ────────────────────────────────────────────
    #[test]
    fn print_char_unicode() {
        let node = atom_node(Atom::Char('\u{1F600}'));
        assert_eq!(pp().print(&node), "\\u{1F600}");
    }

    // ── 16. print_string_simple ───────────────────────────────────────────
    #[test]
    fn print_string_simple() {
        let node = atom_node(Atom::Str("hello".to_string()));
        assert_eq!(pp().print(&node), "\"hello\"");
    }

    // ── 17. print_string_escape_newline ───────────────────────────────────
    #[test]
    fn print_string_escape_newline() {
        let node = atom_node(Atom::Str("a\nb".to_string()));
        assert_eq!(pp().print(&node), "\"a\\nb\"");
    }

    // ── 18. print_string_escape_backslash ─────────────────────────────────
    #[test]
    fn print_string_escape_backslash() {
        let node = atom_node(Atom::Str("a\\b".to_string()));
        assert_eq!(pp().print(&node), "\"a\\\\b\"");
    }

    // ── 19. print_string_escape_quote ─────────────────────────────────────
    #[test]
    fn print_string_escape_quote() {
        let node = atom_node(Atom::Str("say \"hi\"".to_string()));
        assert_eq!(pp().print(&node), "\"say \\\"hi\\\"\"");
    }

    // ── 20. print_keyword_bare ────────────────────────────────────────────
    #[test]
    fn print_keyword_bare() {
        let node = atom_node(Atom::Keyword {
            ns: None,
            name: "status".to_string(),
        });
        assert_eq!(pp().print(&node), ":status");
    }

    // ── 21. print_keyword_namespaced ──────────────────────────────────────
    #[test]
    fn print_keyword_namespaced() {
        let node = atom_node(Atom::Keyword {
            ns: Some("http".to_string()),
            name: "ok".to_string(),
        });
        assert_eq!(pp().print(&node), ":http/ok");
    }

    // ── 22. print_symbol_bare ─────────────────────────────────────────────
    #[test]
    fn print_symbol_bare() {
        let node = atom_node(Atom::Symbol {
            ns: None,
            name: "add".to_string(),
        });
        assert_eq!(pp().print(&node), "add");
    }

    // ── 23. print_symbol_qualified ────────────────────────────────────────
    #[test]
    fn print_symbol_qualified() {
        let node = atom_node(Atom::Symbol {
            ns: Some("math".to_string()),
            name: "sqrt".to_string(),
        });
        assert_eq!(pp().print(&node), "math/sqrt");
    }

    // ── 24. print_list_empty ──────────────────────────────────────────────
    #[test]
    fn print_list_empty() {
        let node = Node::new(NodeKind::List(vec![]), sp());
        assert_eq!(pp().print(&node), "()");
    }

    // ── 25. print_list_atoms ──────────────────────────────────────────────
    #[test]
    fn print_list_atoms() {
        let plus = atom_node(Atom::Symbol {
            ns: None,
            name: "+".to_string(),
        });
        let one = atom_node(Atom::Int {
            value: 1,
            suffix: None,
        });
        let two = atom_node(Atom::Int {
            value: 2,
            suffix: None,
        });
        let node = Node::new(NodeKind::List(vec![plus, one, two]), sp());
        assert_eq!(pp().print(&node), "(+ 1 2)");
    }

    // ── 26. print_vector_empty ────────────────────────────────────────────
    #[test]
    fn print_vector_empty() {
        let node = Node::new(NodeKind::Vector(vec![]), sp());
        assert_eq!(pp().print(&node), "[]");
    }

    // ── 27. print_vector_atoms ────────────────────────────────────────────
    #[test]
    fn print_vector_atoms() {
        let items: Vec<Node> = (1i128..=3)
            .map(|v| {
                atom_node(Atom::Int {
                    value: v,
                    suffix: None,
                })
            })
            .collect();
        let node = Node::new(NodeKind::Vector(items), sp());
        assert_eq!(pp().print(&node), "[1 2 3]");
    }

    // ── 28. print_map_empty ───────────────────────────────────────────────
    #[test]
    fn print_map_empty() {
        let node = Node::new(NodeKind::Map(vec![]), sp());
        assert_eq!(pp().print(&node), "{}");
    }

    // ── 29. print_map_one_pair ────────────────────────────────────────────
    #[test]
    fn print_map_one_pair() {
        let k = atom_node(Atom::Keyword {
            ns: None,
            name: "a".to_string(),
        });
        let v = atom_node(Atom::Int {
            value: 1,
            suffix: None,
        });
        let node = Node::new(NodeKind::Map(vec![(k, v)]), sp());
        assert_eq!(pp().print(&node), "{:a 1}");
    }

    // ── 30. print_set_empty ───────────────────────────────────────────────
    #[test]
    fn print_set_empty() {
        let node = Node::new(NodeKind::Set(vec![]), sp());
        assert_eq!(pp().print(&node), "#{}");
    }

    // ── 31. print_set_atoms ───────────────────────────────────────────────
    #[test]
    fn print_set_atoms() {
        let one = atom_node(Atom::Int {
            value: 1,
            suffix: None,
        });
        let two = atom_node(Atom::Int {
            value: 2,
            suffix: None,
        });
        let node = Node::new(NodeKind::Set(vec![one, two]), sp());
        assert_eq!(pp().print(&node), "#{1 2}");
    }

    // ── 32. print_nested_list ─────────────────────────────────────────────
    #[test]
    fn print_nested_list() {
        let plus = atom_node(Atom::Symbol {
            ns: None,
            name: "+".to_string(),
        });
        let one = atom_node(Atom::Int {
            value: 1,
            suffix: None,
        });
        let two = atom_node(Atom::Int {
            value: 2,
            suffix: None,
        });
        let inner = Node::new(NodeKind::List(vec![plus, one, two]), sp());
        let three = atom_node(Atom::Int {
            value: 3,
            suffix: None,
        });
        let outer = Node::new(NodeKind::List(vec![inner, three]), sp());
        assert_eq!(pp().print(&outer), "((+ 1 2) 3)");
    }

    // ── 33. print_quote ───────────────────────────────────────────────────
    #[test]
    fn print_quote() {
        let x = atom_node(Atom::Symbol {
            ns: None,
            name: "x".to_string(),
        });
        let node = Node::new(NodeKind::Quote(Box::new(x)), sp());
        assert_eq!(pp().print(&node), "'x");
    }

    // ── 34. print_deref ───────────────────────────────────────────────────
    #[test]
    fn print_deref() {
        let counter = atom_node(Atom::Symbol {
            ns: None,
            name: "counter".to_string(),
        });
        let node = Node::new(NodeKind::Deref(Box::new(counter)), sp());
        assert_eq!(pp().print(&node), "@counter");
    }

    // ── 35. print_discard ─────────────────────────────────────────────────
    #[test]
    fn print_discard() {
        let x = atom_node(Atom::Symbol {
            ns: None,
            name: "x".to_string(),
        });
        let node = Node::new(NodeKind::Discard(Box::new(x)), sp());
        assert_eq!(pp().print(&node), "#_ x");
    }

    // ── 36. print_leading_comment ─────────────────────────────────────────
    #[test]
    fn print_leading_comment() {
        use crate::node::Comment;
        let mut node = atom_node(Atom::Int {
            value: 1,
            suffix: None,
        });
        node.leading_comments
            .push(Comment(" a comment".to_string()));
        assert_eq!(pp().print(&node), "; a comment\n1");
    }

    // ── 37. print_trailing_comment ────────────────────────────────────────
    #[test]
    fn print_trailing_comment() {
        use crate::node::Comment;
        let mut node = atom_node(Atom::Int {
            value: 42,
            suffix: None,
        });
        node.trailing_comment = Some(Comment(" the answer".to_string()));
        assert_eq!(pp().print(&node), "42 ; the answer");
    }

    // ── 38. print_config_indent_width ─────────────────────────────────────
    #[test]
    fn print_config_indent_width() {
        let pp4 = PrettyPrinter::new(PrintConfig { indent_width: 4 });
        let node = atom_node(Atom::Bool(true));
        // M0 uses flat output regardless of indent_width; just verify no panic.
        assert_eq!(pp4.print(&node), "true");
    }

    // Test 39 (roundtrip_simple_list) lives in nexl-reader to avoid a
    // circular crate dependency.
}
