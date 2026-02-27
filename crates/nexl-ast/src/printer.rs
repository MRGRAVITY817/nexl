use crate::node::{Atom, FloatSuffix, IntSuffix, Node, NodeKind};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the AST pretty-printer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintConfig {
    /// Number of spaces per indentation level (default: 2).
    pub indent_width: usize,
    /// Maximum line width before breaking to multi-line (default: 80).
    pub max_line_width: usize,
    /// Whether to vertically align columns in tabular forms (default: true).
    pub align_columns: bool,
}

impl Default for PrintConfig {
    fn default() -> Self {
        Self {
            indent_width: 2,
            max_line_width: 80,
            align_columns: true,
        }
    }
}

// ---------------------------------------------------------------------------
// PrettyPrinter
// ---------------------------------------------------------------------------

/// S-expression pretty-printer for [`Node`] trees.
///
/// Produces a canonical text representation that preserves leading and trailing
/// comments for round-trip formatting. Supports both flat (single-line) output
/// via [`print`](Self::print) and multi-line formatted output via
/// [`print_file`](Self::print_file).
pub struct PrettyPrinter {
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

    /// Pretty-print `node` into a [`String`] (flat, single-line).
    pub fn print(&self, node: &Node) -> String {
        let mut out = String::new();
        self.write_node(node, &mut out);
        out
    }

    /// Format a sequence of top-level forms as a complete file.
    ///
    /// Top-level forms are separated by blank lines. The output ends with a
    /// trailing newline.
    pub fn print_file(&self, nodes: &[Node]) -> String {
        let mut out = String::new();
        for (i, node) in nodes.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            self.write_node_indented(node, &mut out, 0, 0);
            out.push('\n');
        }
        out
    }

    /// Measure the flat (single-line) width of a node, ignoring comments.
    fn flat_len(&self, node: &Node) -> usize {
        if !node.leading_comments.is_empty() {
            return usize::MAX;
        }
        let base = self.flat_len_kind(&node.kind);
        if base == usize::MAX {
            return usize::MAX;
        }
        match &node.trailing_comment {
            Some(c) => base.saturating_add(2).saturating_add(c.0.len()), // " ;" + text
            None => base,
        }
    }

    fn flat_len_kind(&self, kind: &NodeKind) -> usize {
        match kind {
            NodeKind::Atom(atom) => flat_len_atom(atom),
            NodeKind::List(items) => flat_len_seq(items, self, '(', ')'),
            NodeKind::Vector(items) => flat_len_seq(items, self, '[', ']'),
            NodeKind::Set(items) => {
                // "#{" + items + "}"
                flat_len_seq(items, self, '#', '}').saturating_add(1)
            }
            NodeKind::Map(pairs) => {
                if pairs.is_empty() {
                    return 2; // "{}"
                }
                let mut len: usize = 2; // "{" + "}"
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        len = len.saturating_add(1); // space separator
                    }
                    len = len.saturating_add(self.flat_len(k));
                    len = len.saturating_add(1); // space between k and v
                    len = len.saturating_add(self.flat_len(v));
                }
                len
            }
            NodeKind::Quote(inner) => self.flat_len(inner).saturating_add(1),
            NodeKind::Deref(inner) => self.flat_len(inner).saturating_add(1),
            NodeKind::Discard(inner) => self.flat_len(inner).saturating_add(3),
            NodeKind::Quasiquote(inner) => self.flat_len(inner).saturating_add(1),
            NodeKind::Unquote(inner) => self.flat_len(inner).saturating_add(1),
            NodeKind::UnquoteSplice(inner) => self.flat_len(inner).saturating_add(2),
        }
    }

    // ── Indented output ─────────────────────────────────────────────────

    /// Write a node with indentation. `column` is the current column position.
    fn write_node_indented(&self, node: &Node, out: &mut String, indent: usize, column: usize) {
        let has_leading = !node.leading_comments.is_empty();

        // Leading comments — each on its own line at current indent.
        for comment in &node.leading_comments {
            push_indent(out, indent);
            out.push(';');
            out.push_str(&comment.0);
            out.push('\n');
        }

        // After leading comments, the node starts on a fresh line at `indent`.
        // So we measure the *kind* width against `indent`, not `column`.
        let effective_col = if has_leading { indent } else { column };
        let kind_len = self.flat_len_kind(&node.kind);
        let trailing_len = match &node.trailing_comment {
            Some(c) => 2 + c.0.len(), // " ;" + text
            None => 0,
        };
        let total = kind_len.saturating_add(trailing_len);

        if total != usize::MAX && effective_col.saturating_add(total) <= self.config.max_line_width
        {
            if has_leading {
                push_indent(out, indent);
            }
            self.write_kind(&node.kind, out);
        } else {
            if has_leading {
                push_indent(out, indent);
            }
            self.write_kind_indented(&node.kind, out, indent, effective_col);
        }

        if let Some(comment) = &node.trailing_comment {
            out.push_str(" ;");
            out.push_str(&comment.0);
        }
    }

    fn write_kind_indented(&self, kind: &NodeKind, out: &mut String, indent: usize, column: usize) {
        match kind {
            NodeKind::Atom(atom) => self.write_atom(atom, out),
            NodeKind::List(items) => {
                if items.is_empty() {
                    out.push_str("()");
                    return;
                }
                self.write_list_indented(items, out, indent, column);
            }
            NodeKind::Vector(items) => {
                self.write_collection_indented(items, out, indent, column, '[', ']');
            }
            NodeKind::Set(items) => {
                out.push('#');
                self.write_collection_indented(items, out, indent, column + 1, '{', '}');
            }
            NodeKind::Map(pairs) => {
                self.write_map_indented(pairs, out, indent, column);
            }
            NodeKind::Quote(inner) => {
                out.push('\'');
                self.write_node_indented(inner, out, indent, column + 1);
            }
            NodeKind::Deref(inner) => {
                out.push('@');
                self.write_node_indented(inner, out, indent, column + 1);
            }
            NodeKind::Discard(inner) => {
                out.push_str("#_ ");
                self.write_node_indented(inner, out, indent, column + 3);
            }
            NodeKind::Quasiquote(inner) => {
                out.push('`');
                self.write_node_indented(inner, out, indent, column + 1);
            }
            NodeKind::Unquote(inner) => {
                out.push('~');
                self.write_node_indented(inner, out, indent, column + 1);
            }
            NodeKind::UnquoteSplice(inner) => {
                out.push_str("~@");
                self.write_node_indented(inner, out, indent, column + 2);
            }
        }
    }

    /// Write a list form `(head ...)` with special-form-aware indentation.
    fn write_list_indented(&self, items: &[Node], out: &mut String, indent: usize, column: usize) {
        let head_name = match &items[0].kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => Some(name.as_str()),
            _ => None,
        };

        match head_name {
            Some(name) if is_body_indent_form(name) => {
                self.write_body_form(items, out, indent, column);
            }
            Some("let" | "loop") => {
                self.write_let_form(items, out, indent, column);
            }
            Some("if") => {
                self.write_if_form(items, out, indent, column);
            }
            Some("cond") => {
                self.write_cond_form(items, out, indent, column);
            }
            Some("match") => {
                self.write_match_form(items, out, indent, column);
            }
            _ => {
                self.write_call_form(items, out, indent, column);
            }
        }
    }

    /// Body-indent form: head + leading args on first line, body indented +2.
    /// e.g. `(defn name [params]\n  body)`
    fn write_body_form(&self, items: &[Node], out: &mut String, indent: usize, _column: usize) {
        let body_indent = indent + self.config.indent_width;
        out.push('(');

        // Find the split: everything before the first "body" item goes on line 1.
        // For defn: head name [params] goes on line 1, rest is body.
        // Generic approach: put head + args that fit on line 1, then body.
        let head_name = match &items[0].kind {
            NodeKind::Atom(Atom::Symbol { ns: None, name }) => name.as_str(),
            _ => "",
        };

        let leading_count = body_form_leading_count(head_name, items);

        // Write leading items on first line
        let mut col = indent + 1; // after '('
        for (i, item) in items[..leading_count].iter().enumerate() {
            if i > 0 {
                out.push(' ');
                col += 1;
            }
            self.write_node_indented(item, out, body_indent, col);
            col += self.flat_len(item).min(self.config.max_line_width);
        }

        // Write body items, each on its own line
        for item in &items[leading_count..] {
            out.push('\n');
            push_indent(out, body_indent);
            self.write_node_indented(item, out, body_indent, body_indent);
        }
        out.push(')');
    }

    /// Let/loop form: `(let [bindings]\n  body)`
    fn write_let_form(&self, items: &[Node], out: &mut String, indent: usize, _column: usize) {
        let body_indent = indent + self.config.indent_width;
        out.push('(');

        // Write "let"/"loop"
        self.write_node_indented(&items[0], out, indent + 1, indent + 1);

        if items.len() > 1 {
            out.push(' ');
            // The binding vector
            let binding_node = &items[1];
            let bind_col = indent + 1 + self.flat_len(&items[0]).min(40) + 1;

            // Try to write binding vector with alignment
            if let NodeKind::Vector(bindings) = &binding_node.kind {
                if bindings.len() >= 4 && bindings.len() % 2 == 0 && self.config.align_columns {
                    self.write_binding_vector_aligned(bindings, out, bind_col);
                } else {
                    self.write_node_indented(binding_node, out, body_indent, bind_col);
                }
            } else {
                self.write_node_indented(binding_node, out, body_indent, bind_col);
            }
        }

        // Body forms
        for item in items.iter().skip(2) {
            out.push('\n');
            push_indent(out, body_indent);
            self.write_node_indented(item, out, body_indent, body_indent);
        }
        out.push(')');
    }

    /// Write a binding vector with aligned name/value columns.
    fn write_binding_vector_aligned(&self, bindings: &[Node], out: &mut String, start_col: usize) {
        let pairs: Vec<(&Node, &Node)> = bindings
            .chunks(2)
            .filter_map(|chunk| {
                if chunk.len() == 2 {
                    Some((&chunk[0], &chunk[1]))
                } else {
                    None
                }
            })
            .collect();

        let max_name_width = pairs
            .iter()
            .map(|(name, _)| self.flat_len(name).min(40))
            .max()
            .unwrap_or(0);

        // Check if aligned version exceeds line width
        let max_val_width = pairs
            .iter()
            .map(|(_, val)| self.flat_len(val).min(self.config.max_line_width))
            .max()
            .unwrap_or(0);
        let aligned_width = start_col + 1 + max_name_width + 1 + max_val_width + 1;
        let use_alignment = aligned_width <= self.config.max_line_width;

        out.push('[');
        for (i, (name, val)) in pairs.iter().enumerate() {
            if i > 0 {
                out.push('\n');
                push_indent(out, start_col + 1);
            }
            let name_len = self.flat_len(name).min(40);
            self.write_node_indented(name, out, start_col + 1, start_col + 1);
            if use_alignment {
                // Pad to align
                for _ in name_len..max_name_width {
                    out.push(' ');
                }
            }
            out.push(' ');
            let val_col = if use_alignment {
                start_col + 1 + max_name_width + 1
            } else {
                start_col + 1 + name_len + 1
            };
            self.write_node_indented(val, out, start_col + 1, val_col);
        }
        out.push(']');
    }

    /// If form: `(if cond\n  then\n  else)`
    fn write_if_form(&self, items: &[Node], out: &mut String, indent: usize, _column: usize) {
        let body_indent = indent + self.config.indent_width;
        out.push('(');
        // "if"
        self.write_node_indented(&items[0], out, indent + 1, indent + 1);

        // condition on same line
        if items.len() > 1 {
            out.push(' ');
            let cond_col = indent + 1 + self.flat_len(&items[0]).min(10) + 1;
            self.write_node_indented(&items[1], out, body_indent, cond_col);
        }

        // then and else on their own lines
        for item in items.iter().skip(2) {
            out.push('\n');
            push_indent(out, body_indent);
            self.write_node_indented(item, out, body_indent, body_indent);
        }
        out.push(')');
    }

    /// Cond form: `(cond\n  test result\n  ...)`
    fn write_cond_form(&self, items: &[Node], out: &mut String, indent: usize, _column: usize) {
        let body_indent = indent + self.config.indent_width;
        out.push('(');
        self.write_node_indented(&items[0], out, indent + 1, indent + 1);

        let clauses: Vec<(&Node, &Node)> = items[1..]
            .chunks(2)
            .filter_map(|chunk| {
                if chunk.len() == 2 {
                    Some((&chunk[0], &chunk[1]))
                } else {
                    None
                }
            })
            .collect();

        if self.config.align_columns && clauses.len() >= 2 {
            let max_test_width = clauses
                .iter()
                .map(|(test, _)| self.flat_len(test).min(40))
                .max()
                .unwrap_or(0);
            let max_result_width = clauses
                .iter()
                .map(|(_, result)| self.flat_len(result).min(self.config.max_line_width))
                .max()
                .unwrap_or(0);
            let aligned_width = body_indent + max_test_width + 1 + max_result_width;
            let use_alignment = aligned_width <= self.config.max_line_width;

            for (test, result) in &clauses {
                out.push('\n');
                push_indent(out, body_indent);
                let test_len = self.flat_len(test).min(40);
                self.write_node_indented(test, out, body_indent, body_indent);
                if use_alignment {
                    for _ in test_len..max_test_width {
                        out.push(' ');
                    }
                }
                out.push(' ');
                let result_col = if use_alignment {
                    body_indent + max_test_width + 1
                } else {
                    body_indent + test_len + 1
                };
                self.write_node_indented(result, out, body_indent, result_col);
            }
        } else {
            // No alignment: just pairs on separate lines
            for chunk in items[1..].chunks(2) {
                out.push('\n');
                push_indent(out, body_indent);
                self.write_node_indented(&chunk[0], out, body_indent, body_indent);
                if chunk.len() > 1 {
                    out.push(' ');
                    let col = body_indent + self.flat_len(&chunk[0]).min(40) + 1;
                    self.write_node_indented(&chunk[1], out, body_indent, col);
                }
            }
        }
        out.push(')');
    }

    /// Match form: `(match expr\n  pattern body\n  ...)`
    fn write_match_form(&self, items: &[Node], out: &mut String, indent: usize, _column: usize) {
        let body_indent = indent + self.config.indent_width;
        out.push('(');
        // "match"
        self.write_node_indented(&items[0], out, indent + 1, indent + 1);

        // scrutinee on same line
        if items.len() > 1 {
            out.push(' ');
            let scr_col = indent + 1 + self.flat_len(&items[0]).min(10) + 1;
            self.write_node_indented(&items[1], out, body_indent, scr_col);
        }

        // Arms: pairs of pattern + body
        let arms: Vec<(&Node, &Node)> = items[2..]
            .chunks(2)
            .filter_map(|chunk| {
                if chunk.len() == 2 {
                    Some((&chunk[0], &chunk[1]))
                } else {
                    None
                }
            })
            .collect();

        if self.config.align_columns && arms.len() >= 2 {
            let max_pat_width = arms
                .iter()
                .map(|(pat, _)| self.flat_len(pat).min(40))
                .max()
                .unwrap_or(0);
            let max_body_width = arms
                .iter()
                .map(|(_, body)| self.flat_len(body).min(self.config.max_line_width))
                .max()
                .unwrap_or(0);
            let aligned_width = body_indent + max_pat_width + 1 + max_body_width;
            let use_alignment = aligned_width <= self.config.max_line_width;

            for (pat, body) in &arms {
                out.push('\n');
                push_indent(out, body_indent);
                let pat_len = self.flat_len(pat).min(40);
                self.write_node_indented(pat, out, body_indent, body_indent);
                if use_alignment {
                    for _ in pat_len..max_pat_width {
                        out.push(' ');
                    }
                }
                out.push(' ');
                let body_col = if use_alignment {
                    body_indent + max_pat_width + 1
                } else {
                    body_indent + pat_len + 1
                };
                self.write_node_indented(body, out, body_indent, body_col);
            }
        } else {
            for chunk in items[2..].chunks(2) {
                out.push('\n');
                push_indent(out, body_indent);
                self.write_node_indented(&chunk[0], out, body_indent, body_indent);
                if chunk.len() > 1 {
                    out.push(' ');
                    let col = body_indent + self.flat_len(&chunk[0]).min(40) + 1;
                    self.write_node_indented(&chunk[1], out, body_indent, col);
                }
            }
        }
        out.push(')');
    }

    /// Call-indent form: if fits → flat; else first arg on same line, rest
    /// aligned under first arg.
    fn write_call_form(&self, items: &[Node], out: &mut String, indent: usize, column: usize) {
        // Try flat first
        let flat = self.flat_len_kind(&NodeKind::List(items.to_vec()));
        if flat != usize::MAX && column.saturating_add(flat) <= self.config.max_line_width {
            out.push('(');
            write_sep(items, out, |n, o| self.write_node(n, o));
            out.push(')');
            return;
        }

        out.push('(');
        // Write head
        self.write_node_indented(&items[0], out, indent + 1, column + 1);

        if items.len() > 1 {
            out.push(' ');
            let first_arg_col = column + 1 + self.flat_len(&items[0]).min(40) + 1;
            // Write first arg on same line
            self.write_node_indented(&items[1], out, first_arg_col, first_arg_col);

            // Remaining args aligned under first arg
            for item in &items[2..] {
                out.push('\n');
                push_indent(out, first_arg_col);
                self.write_node_indented(item, out, first_arg_col, first_arg_col);
            }
        }
        out.push(')');
    }

    /// Write a vector or set multi-line: items indented +1 from bracket.
    fn write_collection_indented(
        &self,
        items: &[Node],
        out: &mut String,
        _indent: usize,
        column: usize,
        open: char,
        close: char,
    ) {
        // Try flat
        let flat = flat_len_seq(items, self, open, close);
        if flat != usize::MAX && column.saturating_add(flat) <= self.config.max_line_width {
            out.push(open);
            write_sep(items, out, |n, o| self.write_node(n, o));
            out.push(close);
            return;
        }

        let item_indent = column + 1; // +1 for the bracket
        out.push(open);
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push('\n');
                push_indent(out, item_indent);
            }
            self.write_node_indented(item, out, item_indent, item_indent);
        }
        out.push(close);
    }

    /// Write a map multi-line: one k-v pair per line, with column alignment.
    fn write_map_indented(
        &self,
        pairs: &[(Node, Node)],
        out: &mut String,
        _indent: usize,
        column: usize,
    ) {
        if pairs.is_empty() {
            out.push_str("{}");
            return;
        }

        // Try flat
        let flat = self.flat_len_kind(&NodeKind::Map(pairs.to_vec()));
        if flat != usize::MAX && column.saturating_add(flat) <= self.config.max_line_width {
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
            return;
        }

        let pair_indent = column + 1;
        out.push('{');

        if self.config.align_columns && pairs.len() >= 2 {
            let max_key_width = pairs
                .iter()
                .map(|(k, _)| self.flat_len(k).min(40))
                .max()
                .unwrap_or(0);
            let max_val_width = pairs
                .iter()
                .map(|(_, v)| self.flat_len(v).min(self.config.max_line_width))
                .max()
                .unwrap_or(0);
            let aligned_width = pair_indent + max_key_width + 1 + max_val_width + 1;
            let use_alignment = aligned_width <= self.config.max_line_width;

            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                    push_indent(out, pair_indent);
                }
                let key_len = self.flat_len(k).min(40);
                self.write_node_indented(k, out, pair_indent, pair_indent);
                if use_alignment {
                    for _ in key_len..max_key_width {
                        out.push(' ');
                    }
                }
                out.push(' ');
                let val_col = if use_alignment {
                    pair_indent + max_key_width + 1
                } else {
                    pair_indent + key_len + 1
                };
                self.write_node_indented(v, out, pair_indent, val_col);
            }
        } else {
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                    push_indent(out, pair_indent);
                }
                self.write_node_indented(k, out, pair_indent, pair_indent);
                out.push(' ');
                let val_col = pair_indent + self.flat_len(k).min(40) + 1;
                self.write_node_indented(v, out, pair_indent, val_col);
            }
        }
        out.push('}');
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

/// Measure the flat width of an atom.
fn flat_len_atom(atom: &Atom) -> usize {
    match atom {
        Atom::Int { value, suffix } => {
            let mut len = format!("{value}").len();
            if let Some(s) = suffix {
                len += int_suffix_str(*s).len();
            }
            len
        }
        Atom::Float { value, suffix } => {
            let mut len = format_float(*value).len();
            if let Some(s) = suffix {
                len += float_suffix_str(*s).len();
            }
            len
        }
        Atom::Ratio { numer, denom } => format!("{numer}").len() + 1 + format!("{denom}").len(),
        Atom::Bool(b) => {
            if *b {
                4
            } else {
                5
            }
        }
        Atom::Unit => 4,
        Atom::Char(c) => {
            // Approximation: named chars are longer
            match c {
                ' ' => 6,                       // \space
                '\n' => 8,                      // \newline
                '\t' => 4,                      // \tab
                '\r' => 7,                      // \return
                c if c.is_ascii_graphic() => 2, // \x
                _ => 8,                         // \u{XXXX} approximation
            }
        }
        Atom::Str(s) => {
            // 2 for quotes + escaped length
            let mut len = 2;
            for c in s.chars() {
                len += match c {
                    '\\' | '"' | '\n' | '\t' | '\r' => 2,
                    _ => 1,
                };
            }
            len
        }
        Atom::Keyword { ns, name } => {
            let mut len = 1 + name.len(); // ":" + name
            if let Some(ns) = ns {
                len += ns.len() + 1; // ns + "/"
            }
            len
        }
        Atom::Symbol { ns, name } => {
            let mut len = name.len();
            if let Some(ns) = ns {
                len += ns.len() + 1;
            }
            len
        }
    }
}

/// Measure the flat width of a sequence with open/close delimiters.
fn flat_len_seq(items: &[Node], pp: &PrettyPrinter, _open: char, _close: char) -> usize {
    if items.is_empty() {
        return 2; // "()" or "[]" etc.
    }
    let mut len: usize = 2; // open + close
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            len = len.saturating_add(1); // space
        }
        let item_len = pp.flat_len(item);
        if item_len == usize::MAX {
            return usize::MAX;
        }
        len = len.saturating_add(item_len);
    }
    len
}

/// Emit `n` spaces for indentation.
fn push_indent(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push(' ');
    }
}

/// Check if a symbol name is a "body indent" special form.
fn is_body_indent_form(name: &str) -> bool {
    matches!(
        name,
        "defn"
            | "fn"
            | "def"
            | "do"
            | "when"
            | "unless"
            | "module"
            | "deftype"
            | "defeffect"
            | "defprotocol"
            | "defmacro"
            | "handle"
            | "import"
            | "try"
            | "each"
            | "times"
            | "for"
    )
}

/// Return how many leading items go on the first line for a body-indent form.
fn body_form_leading_count(head: &str, items: &[Node]) -> usize {
    match head {
        // (defn name [params] body...)
        // (defn name "doc" [params] body...)
        "defn" => {
            if items.len() < 2 {
                return items.len();
            }
            // head + name
            let mut count = 2;
            // optional docstring
            if let Some(Node {
                kind: NodeKind::Atom(Atom::Str(_)),
                ..
            }) = items.get(2)
            {
                count = 3;
            }
            // parameter vector
            if count < items.len()
                && let NodeKind::Vector(_) = &items[count].kind
            {
                count += 1;
            }
            count.min(items.len())
        }
        // (fn [params] body...)
        "fn" => {
            let mut count = 1; // head
            if let Some(Node {
                kind: NodeKind::Vector(_),
                ..
            }) = items.get(1)
            {
                count = 2;
            }
            count.min(items.len())
        }
        // (def name value) — usually fits on one line, but if not: head + name
        "def" => 2.min(items.len()),
        // (module name :key val ...) — head + name
        "module" => 2.min(items.len()),
        // (handle body :effect handler) — just head on first line
        "handle" | "try" => 1.min(items.len()),
        // (import ...) — head
        "import" => 1.min(items.len()),
        // (deftype name fields...) — head + name
        "deftype" | "defeffect" | "defprotocol" => 2.min(items.len()),
        // (defmacro name [params] body) — head + name + params
        "defmacro" => {
            let mut count = 2; // head + name
            if let Some(Node {
                kind: NodeKind::Vector(_),
                ..
            }) = items.get(2)
            {
                count = 3;
            }
            count.min(items.len())
        }
        // (each x coll body...) — head + binding + coll
        "each" | "times" => 3.min(items.len()),
        // (for clauses body) — just head
        "for" => 1.min(items.len()),
        // Default: head + all args that aren't body
        _ => 1.min(items.len()),
    }
}

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
        let pp4 = PrettyPrinter::new(PrintConfig {
            indent_width: 4,
            ..PrintConfig::default()
        });
        let node = atom_node(Atom::Bool(true));
        assert_eq!(pp4.print(&node), "true");
    }

    // Test 39 (roundtrip_simple_list) lives in nexl-reader to avoid a
    // circular crate dependency.

    // =====================================================================
    // Multi-line formatter tests (print_file / write_node_indented)
    // =====================================================================

    fn sym(name: &str) -> Node {
        atom_node(Atom::Symbol {
            ns: None,
            name: name.to_string(),
        })
    }

    fn int(value: i128) -> Node {
        atom_node(Atom::Int {
            value,
            suffix: None,
        })
    }

    fn kw(name: &str) -> Node {
        atom_node(Atom::Keyword {
            ns: None,
            name: name.to_string(),
        })
    }

    fn str_node(s: &str) -> Node {
        atom_node(Atom::Str(s.to_string()))
    }

    fn list(items: Vec<Node>) -> Node {
        Node::new(NodeKind::List(items), sp())
    }

    fn vec_node(items: Vec<Node>) -> Node {
        Node::new(NodeKind::Vector(items), sp())
    }

    fn map_node(pairs: Vec<(Node, Node)>) -> Node {
        Node::new(NodeKind::Map(pairs), sp())
    }

    fn pp_narrow() -> PrettyPrinter {
        PrettyPrinter::new(PrintConfig {
            max_line_width: 30,
            ..PrintConfig::default()
        })
    }

    // ── 40. print_file_single_form ──────────────────────────────────────
    #[test]
    fn print_file_single_form() {
        let node = list(vec![sym("def"), sym("x"), int(42)]);
        assert_eq!(pp().print_file(&[node]), "(def x 42)\n");
    }

    // ── 41. print_file_multiple_forms_separated ─────────────────────────
    #[test]
    fn print_file_multiple_forms_separated() {
        let a = list(vec![sym("def"), sym("x"), int(1)]);
        let b = list(vec![sym("def"), sym("y"), int(2)]);
        assert_eq!(pp().print_file(&[a, b]), "(def x 1)\n\n(def y 2)\n");
    }

    // ── 42. print_file_trailing_newline ─────────────────────────────────
    #[test]
    fn print_file_trailing_newline() {
        let node = int(42);
        let out = pp().print_file(&[node]);
        assert!(out.ends_with('\n'));
    }

    // ── 43. short_list_stays_flat ───────────────────────────────────────
    #[test]
    fn short_list_stays_flat() {
        let node = list(vec![sym("+"), int(1), int(2)]);
        assert_eq!(pp().print_file(&[node]), "(+ 1 2)\n");
    }

    // ── 44. long_call_breaks_multi_line ─────────────────────────────────
    #[test]
    fn long_call_breaks_multi_line() {
        // With narrow width, a call form should break
        let node = list(vec![
            sym("some-long-function"),
            str_node("argument-one"),
            str_node("argument-two"),
        ]);
        let out = pp_narrow().print_file(&[node]);
        assert!(out.contains('\n'));
        // Head + first arg on line 1, rest aligned
        assert!(out.starts_with("(some-long-function"));
    }

    // ── 45. defn_body_indent ────────────────────────────────────────────
    #[test]
    fn defn_body_indent() {
        // (defn greet [name] (io/println name))
        // Even if it fits, test the structure when forced to break
        let node = list(vec![
            sym("defn"),
            sym("greet"),
            vec_node(vec![sym("name")]),
            list(vec![sym("io/println"), sym("name")]),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 25,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        // Should have body indented
        assert!(out.contains("(defn greet [name]"));
        assert!(out.contains("  (io/println name)"));
    }

    // ── 46. let_form_indent ─────────────────────────────────────────────
    #[test]
    fn let_form_indent() {
        let node = list(vec![
            sym("let"),
            vec_node(vec![sym("x"), int(10), sym("name"), str_node("Alice")]),
            list(vec![sym("+"), sym("x"), int(1)]),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 25,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        // Body should be indented +2
        assert!(out.contains("(let"));
        assert!(out.contains("  (+"));
    }

    // ── 47. if_form_indent ──────────────────────────────────────────────
    #[test]
    fn if_form_indent() {
        let node = list(vec![
            sym("if"),
            list(vec![sym(">"), sym("x"), int(0)]),
            str_node("positive"),
            str_node("non-positive"),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 25,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        // "if" + condition on line 1, then/else indented
        assert!(out.contains("(if (> x 0)"));
        assert!(out.contains("  \"positive\""));
        assert!(out.contains("  \"non-positive\""));
    }

    // ── 48. cond_form_indent ────────────────────────────────────────────
    #[test]
    fn cond_form_indent() {
        let node = list(vec![
            sym("cond"),
            list(vec![sym("<"), sym("x"), int(0)]),
            kw("negative"),
            kw("else"),
            kw("other"),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 30,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        assert!(out.contains("(cond"));
    }

    // ── 49. match_form_indent ───────────────────────────────────────────
    #[test]
    fn match_form_indent() {
        let node = list(vec![
            sym("match"),
            sym("direction"),
            kw("north"),
            kw("south"),
            kw("east"),
            kw("west"),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 30,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        assert!(out.contains("(match direction"));
    }

    // ── 50. do_form_indent ──────────────────────────────────────────────
    #[test]
    fn do_form_indent() {
        let node = list(vec![
            sym("do"),
            list(vec![sym("step1")]),
            list(vec![sym("step2")]),
            list(vec![sym("step3")]),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 20,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        assert!(out.contains("(do"));
        assert!(out.contains("  (step1)"));
        assert!(out.contains("  (step2)"));
        assert!(out.contains("  (step3)"));
    }

    // ── 51. nested_multiline ────────────────────────────────────────────
    #[test]
    fn nested_multiline() {
        // (defn f [x] (if (> x 0) x (- x)))
        let node = list(vec![
            sym("defn"),
            sym("f"),
            vec_node(vec![sym("x")]),
            list(vec![
                sym("if"),
                list(vec![sym(">"), sym("x"), int(0)]),
                sym("x"),
                list(vec![sym("-"), sym("x")]),
            ]),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 25,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        // Should nest the if inside the defn body
        assert!(out.contains("(defn f [x]"));
        assert!(out.contains("  (if"));
    }

    // ── 52. map_alignment ───────────────────────────────────────────────
    #[test]
    fn map_alignment() {
        let node = map_node(vec![
            (kw("name"), str_node("Alice")),
            (kw("age"), int(30)),
            (kw("email"), str_node("alice@example.com")),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 20,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        // Should be multi-line with aligned keys/values
        assert!(out.contains(":name"));
        assert!(out.contains(":age"));
        assert!(out.contains(":email"));
    }

    // ── 53. let_binding_alignment ───────────────────────────────────────
    #[test]
    fn let_binding_alignment() {
        let node = list(vec![
            sym("let"),
            vec_node(vec![sym("x"), int(10), sym("name"), str_node("Alice")]),
            sym("x"),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 25,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        // Bindings should be aligned
        assert!(out.contains("["));
    }

    // ── 54. match_arm_alignment ─────────────────────────────────────────
    #[test]
    fn match_arm_alignment() {
        let node = list(vec![
            sym("match"),
            sym("dir"),
            kw("north"),
            kw("south"),
            kw("east"),
            kw("west"),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 25,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        // Arms should appear as pairs
        assert!(out.contains(":north"));
        assert!(out.contains(":east"));
    }

    // ── 55. cond_clause_alignment ───────────────────────────────────────
    #[test]
    fn cond_clause_alignment() {
        let node = list(vec![
            sym("cond"),
            list(vec![sym("<"), sym("x"), int(0)]),
            kw("negative"),
            list(vec![sym(">"), sym("x"), int(100)]),
            kw("large"),
            kw("else"),
            kw("ok"),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 40,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        assert!(out.contains("(cond"));
        assert!(out.contains(":negative"));
        assert!(out.contains(":large"));
    }

    // ── 56. alignment_disabled ──────────────────────────────────────────
    #[test]
    fn alignment_disabled() {
        let node = map_node(vec![(kw("name"), str_node("Alice")), (kw("age"), int(30))]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 15,
            align_columns: false,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        // Should still work, just without alignment padding
        assert!(out.contains(":name"));
        assert!(out.contains(":age"));
    }

    // ── 57. collection_flat_if_fits ─────────────────────────────────────
    #[test]
    fn collection_flat_if_fits() {
        let node = vec_node(vec![int(1), int(2), int(3)]);
        assert_eq!(pp().print_file(&[node]), "[1 2 3]\n");
    }

    // ── 58. collection_breaks_if_long ───────────────────────────────────
    #[test]
    fn collection_breaks_if_long() {
        let node = vec_node(vec![
            str_node("a-long-string"),
            str_node("another-long-string"),
            str_node("yet-another"),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 25,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        // Should break into multiple lines
        assert!(out.contains('\n'));
    }

    // ── 59. comment_in_print_file ───────────────────────────────────────
    #[test]
    fn comment_in_print_file() {
        use crate::node::Comment;
        let mut node = list(vec![sym("def"), sym("x"), int(42)]);
        node.leading_comments.push(Comment(" define x".to_string()));
        let out = pp().print_file(&[node]);
        assert!(out.contains("; define x\n"));
        assert!(out.contains("(def x 42)"));
    }

    // ── 60. trailing_comment_in_formatted ───────────────────────────────
    #[test]
    fn trailing_comment_in_formatted() {
        use crate::node::Comment;
        let mut node = list(vec![sym("def"), sym("x"), int(42)]);
        node.trailing_comment = Some(Comment(" the answer".to_string()));
        let out = pp().print_file(&[node]);
        assert!(out.contains("(def x 42) ; the answer"));
    }

    // ── 61. custom_width ────────────────────────────────────────────────
    #[test]
    fn custom_width() {
        let node = list(vec![sym("some-function"), int(1), int(2), int(3)]);
        // Very narrow width forces multi-line
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 15,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        assert!(out.contains('\n'));
    }

    // ── 62. custom_indent ───────────────────────────────────────────────
    #[test]
    fn custom_indent() {
        let node = list(vec![
            sym("do"),
            list(vec![sym("step1")]),
            list(vec![sym("step2")]),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            indent_width: 4,
            max_line_width: 15,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        // With indent_width=4, body should be indented 4 spaces
        assert!(out.contains("    (step1)"));
    }

    // ── 63. flat_len_basic ──────────────────────────────────────────────
    #[test]
    fn flat_len_basic() {
        let p = pp();
        assert_eq!(p.flat_len(&int(42)), 2);
        assert_eq!(p.flat_len(&sym("hello")), 5);
        assert_eq!(p.flat_len(&str_node("hi")), 4); // "hi" = 4 chars
        assert_eq!(p.flat_len(&kw("status")), 7); // :status = 7
        assert_eq!(p.flat_len(&list(vec![sym("+"), int(1), int(2)])), 7); // (+ 1 2)
    }

    // ── 64. flat_len_with_comment_is_max ────────────────────────────────
    #[test]
    fn flat_len_with_comment_is_max() {
        use crate::node::Comment;
        let mut node = int(42);
        node.leading_comments.push(Comment(" hi".to_string()));
        assert_eq!(pp().flat_len(&node), usize::MAX);
    }

    // ── 65. defn_with_docstring ─────────────────────────────────────────
    #[test]
    fn defn_with_docstring() {
        let node = list(vec![
            sym("defn"),
            sym("greet"),
            str_node("Greet someone."),
            vec_node(vec![sym("name")]),
            list(vec![sym("io/println"), sym("name")]),
        ]);
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 30,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        assert!(out.contains("\"Greet someone.\""));
        assert!(out.contains("[name]"));
    }

    // ── 66. empty_file ──────────────────────────────────────────────────
    #[test]
    fn empty_file() {
        assert_eq!(pp().print_file(&[]), "");
    }

    // ── 67. set_multiline ───────────────────────────────────────────────
    #[test]
    fn set_multiline() {
        let node = Node::new(
            NodeKind::Set(vec![
                str_node("a-long-string"),
                str_node("another-long-string"),
            ]),
            sp(),
        );
        let pp = PrettyPrinter::new(PrintConfig {
            max_line_width: 20,
            ..PrintConfig::default()
        });
        let out = pp.print_file(&[node]);
        assert!(out.starts_with("#{"));
        assert!(out.contains('\n'));
    }
}
