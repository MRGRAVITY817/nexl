use nexl_ast::{Atom, Comment, FileId, Node, NodeKind, Span};
use nexl_errors::{codes, Diagnostic, Label, Severity};

use crate::lexer::{Lexer, StringPart, Token, TokenKind};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse all top-level forms in `src` and return them as a [`Vec<Node>`].
///
/// Runs the lexer internally; any lex error propagates immediately.
/// Every node carries a byte-accurate [`Span`], and line comments are attached
/// as `leading_comments` / `trailing_comment` for round-trip formatting.
pub fn read(src: &str, file_id: FileId) -> Result<Vec<Node>, Box<Diagnostic>> {
    let tokens = Lexer::new(src, file_id).tokenize()?;
    let mut reader = Reader { tokens, pos: 0, src };
    reader.read_all()
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

struct Reader<'src> {
    tokens: Vec<Token>,
    pos: usize,
    /// Original source text, used to detect same-line trailing comments.
    src: &'src str,
}

impl<'src> Reader<'src> {
    fn read_all(&mut self) -> Result<Vec<Node>, Box<Diagnostic>> {
        let mut nodes = Vec::new();
        loop {
            match self.peek_no_comment() {
                None => break,
                Some(t) if is_close(&t) => return Err(self.unmatched_delimiter(&t)),
                _ => self.read_forms_into(&mut nodes)?,
            }
        }
        Ok(nodes)
    }

    /// Read a single form for single-form contexts (Quote/Deref/`#_` target).
    ///
    /// Comments before the form are discarded; this path does not attach
    /// leading or trailing comments (the caller owns that responsibility).
    fn read_form(&mut self) -> Result<Node, Box<Diagnostic>> {
        self.skip_comments();
        let tok = self.advance().expect("called after checking peek");
        self.dispatch(tok)
    }

    /// Dispatch on `tok` to build the appropriate AST node.
    fn dispatch(&mut self, tok: Token) -> Result<Node, Box<Diagnostic>> {
        match tok.kind.clone() {
            TokenKind::Int(value, suffix) => {
                Ok(Node::atom(Atom::Int { value, suffix }, tok.span))
            }
            TokenKind::Float(value, suffix) => {
                Ok(Node::atom(Atom::Float { value, suffix }, tok.span))
            }
            TokenKind::Ratio(n, d) => {
                let (numer, denom) = reduce_ratio(n, d);
                Ok(Node::atom(Atom::Ratio { numer, denom }, tok.span))
            }
            TokenKind::Bool(b) => Ok(Node::atom(Atom::Bool(b), tok.span)),
            TokenKind::Unit => Ok(Node::atom(Atom::Unit, tok.span)),
            TokenKind::Char(c) => Ok(Node::atom(Atom::Char(c), tok.span)),
            TokenKind::Str(parts) => {
                Ok(Node::atom(Atom::Str(reassemble_str(&parts)), tok.span))
            }
            TokenKind::Keyword { ns, name, .. } => {
                Ok(Node::atom(Atom::Keyword { ns, name }, tok.span))
            }
            TokenKind::Symbol { ns, name } => {
                Ok(Node::atom(Atom::Symbol { ns, name }, tok.span))
            }
            TokenKind::LParen => self.read_list(tok.span),
            TokenKind::LBracket => self.read_vector(tok.span),
            TokenKind::LBrace => self.read_map(tok.span),
            TokenKind::SetOpen => self.read_set(tok.span),
            TokenKind::Quote => {
                let inner = self.require_form("'", tok.span)?;
                let span = tok.span.merge(inner.span);
                Ok(Node::new(NodeKind::Quote(Box::new(inner)), span))
            }
            TokenKind::Deref => {
                let inner = self.require_form("@", tok.span)?;
                let span = tok.span.merge(inner.span);
                Ok(Node::new(NodeKind::Deref(Box::new(inner)), span))
            }
            TokenKind::Discard => {
                let inner = self.require_form("#_", tok.span)?;
                let span = tok.span.merge(inner.span);
                Ok(Node::new(NodeKind::Discard(Box::new(inner)), span))
            }
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                Err(self.unmatched_delimiter(&tok))
            }
            TokenKind::Comment(_) => {
                unreachable!("comments must be drained before dispatch")
            }
        }
    }

    fn read_list(&mut self, open_span: Span) -> Result<Node, Box<Diagnostic>> {
        let mut items = Vec::new();
        loop {
            match self.peek_no_comment() {
                None => return Err(self.unclosed_delimiter(open_span, "(")),
                Some(t) if matches!(t.kind, TokenKind::RParen) => {
                    self.skip_comments();
                    let close = self.advance().unwrap();
                    return Ok(Node::new(NodeKind::List(items), open_span.merge(close.span)));
                }
                Some(t) if is_close(&t) => return Err(self.unmatched_delimiter(&t)),
                _ => self.read_forms_into(&mut items)?,
            }
        }
    }

    fn read_vector(&mut self, open_span: Span) -> Result<Node, Box<Diagnostic>> {
        let mut items = Vec::new();
        loop {
            match self.peek_no_comment() {
                None => return Err(self.unclosed_delimiter(open_span, "[")),
                Some(t) if matches!(t.kind, TokenKind::RBracket) => {
                    self.skip_comments();
                    let close = self.advance().unwrap();
                    return Ok(Node::new(NodeKind::Vector(items), open_span.merge(close.span)));
                }
                Some(t) if is_close(&t) => return Err(self.unmatched_delimiter(&t)),
                _ => self.read_forms_into(&mut items)?,
            }
        }
    }

    fn read_map(&mut self, open_span: Span) -> Result<Node, Box<Diagnostic>> {
        // Collect all forms flatly (Discard nodes included) then pair them up.
        let mut forms: Vec<Node> = Vec::new();
        let close_span;
        loop {
            match self.peek_no_comment() {
                None => return Err(self.unclosed_delimiter(open_span, "{")),
                Some(t) if matches!(t.kind, TokenKind::RBrace) => {
                    self.skip_comments();
                    close_span = self.advance().unwrap().span;
                    break;
                }
                Some(t) if is_close(&t) => return Err(self.unmatched_delimiter(&t)),
                _ => self.read_forms_into(&mut forms)?,
            }
        }
        if !forms.len().is_multiple_of(2) {
            let key_span = forms.last().unwrap().span;
            return Err(self.odd_map(open_span, key_span));
        }
        let mut pairs = Vec::with_capacity(forms.len() / 2);
        let mut iter = forms.into_iter();
        while let Some(key) = iter.next() {
            let val = iter.next().unwrap();
            pairs.push((key, val));
        }
        Ok(Node::new(NodeKind::Map(pairs), open_span.merge(close_span)))
    }

    fn read_set(&mut self, open_span: Span) -> Result<Node, Box<Diagnostic>> {
        let mut items = Vec::new();
        loop {
            match self.peek_no_comment() {
                None => return Err(self.unclosed_delimiter(open_span, "#{")),
                Some(t) if matches!(t.kind, TokenKind::RBrace) => {
                    self.skip_comments();
                    let close = self.advance().unwrap();
                    return Ok(Node::new(NodeKind::Set(items), open_span.merge(close.span)));
                }
                Some(t) if is_close(&t) => return Err(self.unmatched_delimiter(&t)),
                _ => self.read_forms_into(&mut items)?,
            }
        }
    }

    /// Push one logical form-unit into `items`, attaching comments.
    ///
    /// **Leading comments** (lines immediately before the form) are placed in
    /// `node.leading_comments`.  **Trailing comment** (same line as the form's
    /// last token) is placed in `node.trailing_comment`.
    ///
    /// For a run of N consecutive `#_` tokens (skipping inter-token comments):
    /// advances past all N, reads N forms, and wraps each in `Discard`. The
    /// pre-chain leading comments go on the first Discard node.
    fn read_forms_into(&mut self, items: &mut Vec<Node>) -> Result<(), Box<Diagnostic>> {
        // Step 1: collect leading comments and any Discard chain.
        let leading = self.drain_comments();

        let mut discard_spans: Vec<Span> = Vec::new();
        let mut scan = self.pos;
        loop {
            // Skip inter-Discard comments (they are absorbed, not attached).
            while let Some(TokenKind::Comment(_)) = self.tokens.get(scan).map(|t| &t.kind) {
                scan += 1;
            }
            if let Some(TokenKind::Discard) = self.tokens.get(scan).map(|t| &t.kind) {
                discard_spans.push(self.tokens[scan].span);
                scan += 1;
            } else {
                break;
            }
        }
        self.pos = scan; // consumed all Discards and inter-Discard comments

        // Step 2: build nodes.
        if discard_spans.is_empty() {
            let tok = self.advance().expect("called after verifying peek_no_comment is Some");
            let mut form = self.dispatch(tok)?;
            form.leading_comments = leading;
            form.trailing_comment = self.try_trailing(form.span.end());
            items.push(form);
        } else {
            let mut first_leading = Some(leading);
            for discard_span in discard_spans {
                // Comments between #_ and its target are absorbed (not yet attached).
                self.skip_comments();
                match self.peek() {
                    None => {
                        let mut d = Diagnostic::new(
                            Severity::Error,
                            "expected a form after `#_`, found end of file",
                        );
                        d.code = Some(codes::UNCLOSED_DELIMITER);
                        d.push_label(Label::new(
                            discard_span,
                            "this `#_` expects a following form",
                        ));
                        return Err(Box::new(d));
                    }
                    Some(t) if is_close(t) => {
                        let t = t.clone();
                        return Err(self.unmatched_delimiter(&t));
                    }
                    _ => {
                        let tok = self.advance().unwrap();
                        let inner = self.dispatch(tok)?;
                        let span = discard_span.merge(inner.span);
                        let mut node = Node::new(NodeKind::Discard(Box::new(inner)), span);
                        node.leading_comments = first_leading.take().unwrap_or_default();
                        node.trailing_comment = self.try_trailing(node.span.end());
                        items.push(node);
                    }
                }
            }
        }

        Ok(())
    }

    /// Consume the next form, erroring if none is available.
    ///
    /// Used for single-form contexts (`'`, `@`, `#_`). Comments before the
    /// target form are discarded — the caller owns comment-attachment.
    fn require_form(&mut self, prefix: &str, prefix_span: Span) -> Result<Node, Box<Diagnostic>> {
        self.skip_comments();
        match self.peek() {
            None => {
                let mut d = Diagnostic::new(
                    Severity::Error,
                    format!("expected a form after `{prefix}`, found end of file"),
                );
                d.code = Some(codes::UNCLOSED_DELIMITER);
                d.push_label(Label::new(prefix_span, "this prefix expects a following form"));
                Err(Box::new(d))
            }
            Some(t) if is_close(t) => {
                let t = t.clone();
                Err(self.unmatched_delimiter(&t))
            }
            _ => self.read_form(),
        }
    }

    // --- Comment helpers ---

    /// Advance past comment tokens, returning them as [`Comment`] values.
    fn drain_comments(&mut self) -> Vec<Comment> {
        let mut out = Vec::new();
        while let Some(TokenKind::Comment(text)) = self.tokens.get(self.pos).map(|t| &t.kind) {
            out.push(Comment(text.clone()));
            self.pos += 1;
        }
        out
    }

    /// Advance past comment tokens, discarding them.
    fn skip_comments(&mut self) {
        while matches!(self.peek().map(|t| &t.kind), Some(TokenKind::Comment(_))) {
            self.advance();
        }
    }

    /// Peek at the next non-comment token without advancing.
    ///
    /// Returns a cloned [`Token`] so callers can inspect it without holding
    /// a borrow over a mutable call.
    fn peek_no_comment(&self) -> Option<Token> {
        let mut i = self.pos;
        while let Some(t) = self.tokens.get(i) {
            if !matches!(t.kind, TokenKind::Comment(_)) {
                return Some(t.clone());
            }
            i += 1;
        }
        None
    }

    /// If the next token is a comment on the **same source line** as byte
    /// `after_byte`, consume it and return it as a trailing [`Comment`].
    fn try_trailing(&mut self, after_byte: u32) -> Option<Comment> {
        let tok = self.tokens.get(self.pos)?;
        let TokenKind::Comment(text) = &tok.kind else { return None };
        let start = tok.span.start as usize;
        let end = after_byte as usize;
        // Guard: comment must start after `after_byte` (it always should).
        if start < end {
            return None;
        }
        let between = &self.src[end..start];
        if between.contains('\n') {
            return None; // comment is on a different line
        }
        let comment = Comment(text.clone());
        self.pos += 1;
        Some(comment)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    // --- Diagnostic helpers ---

    fn unmatched_delimiter(&self, tok: &Token) -> Box<Diagnostic> {
        let delim = match &tok.kind {
            TokenKind::RParen => ")",
            TokenKind::RBracket => "]",
            TokenKind::RBrace => "}",
            _ => unreachable!("unmatched_delimiter called with non-close token"),
        };
        let mut d = Diagnostic::new(
            Severity::Error,
            format!("unexpected `{delim}` — no matching opener"),
        );
        d.code = Some(codes::UNMATCHED_DELIMITER);
        d.push_label(Label::new(tok.span, "unmatched delimiter"));
        Box::new(d)
    }

    fn unclosed_delimiter(&self, open_span: Span, opener: &str) -> Box<Diagnostic> {
        let mut d = Diagnostic::new(
            Severity::Error,
            format!("unclosed `{opener}` — expected matching closer before end of file"),
        );
        d.code = Some(codes::UNCLOSED_DELIMITER);
        d.push_label(Label::new(open_span, "unclosed delimiter opened here"));
        Box::new(d)
    }

    fn odd_map(&self, open_span: Span, key_span: Span) -> Box<Diagnostic> {
        let mut d = Diagnostic::new(
            Severity::Error,
            "map literal has an odd number of forms — every key must have a value",
        );
        d.code = Some(codes::ODD_MAP_FORMS);
        d.push_label(Label::new(open_span, "this map"));
        d.push_label(Label::new(key_span, "this key has no matching value"));
        d.set_help("add a value for the last key, or remove the unpaired key");
        Box::new(d)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `true` if the token is a closing delimiter (`)`/`]`/`}`).
fn is_close(tok: &Token) -> bool {
    matches!(tok.kind, TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace)
}

/// Reduce `numer/denom` to lowest terms.
///
/// The lexer guarantees `denom != 0`.
fn reduce_ratio(numer: i64, denom: i64) -> (i64, i64) {
    let g = gcd(numer.abs(), denom.abs());
    if g == 0 { (numer, denom) } else { (numer / g, denom / g) }
}

fn gcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Reassemble lexer `StringPart`s into a single string.
///
/// `Lit` segments are used verbatim (escapes already resolved by the lexer).
/// `Interp` segments are wrapped in `{}` to preserve interpolation syntax for
/// later compiler passes (see `Atom::Str`).
fn reassemble_str(parts: &[StringPart]) -> String {
    let mut out = String::new();
    for part in parts {
        match part {
            StringPart::Lit(s) => out.push_str(s),
            StringPart::Interp(s) => {
                out.push('{');
                out.push_str(s);
                out.push('}');
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nexl_ast::{Atom, FileId, IntSuffix, NodeKind};

    fn fid() -> FileId {
        FileId(0)
    }

    // ── 1. parse_integer_atom ─────────────────────────────────────────────
    #[test]
    fn parse_integer_atom() {
        let nodes = read("42", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Atom(Atom::Int { value: 42, suffix: None }));
    }

    // ── 2. parse_symbol_atom ──────────────────────────────────────────────
    #[test]
    fn parse_symbol_atom() {
        let nodes = read("foo", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0].kind,
            NodeKind::Atom(Atom::Symbol { ns: None, name: "foo".into() })
        );
    }

    // ── 3. parse_bool_atom ────────────────────────────────────────────────
    #[test]
    fn parse_bool_atom() {
        let t = read("true", fid()).expect("parse true");
        let f = read("false", fid()).expect("parse false");
        assert_eq!(t[0].kind, NodeKind::Atom(Atom::Bool(true)));
        assert_eq!(f[0].kind, NodeKind::Atom(Atom::Bool(false)));
    }

    // ── 4. parse_unit_atom ────────────────────────────────────────────────
    #[test]
    fn parse_unit_atom() {
        let nodes = read("unit", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Atom(Atom::Unit));
    }

    // ── 5. parse_keyword_atom ─────────────────────────────────────────────
    #[test]
    fn parse_keyword_atom() {
        let nodes = read(":http/ok", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0].kind,
            NodeKind::Atom(Atom::Keyword { ns: Some("http".into()), name: "ok".into() })
        );
    }

    // ── 6. parse_string_atom ──────────────────────────────────────────────
    #[test]
    fn parse_string_atom() {
        let nodes = read("\"hello\"", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Atom(Atom::Str("hello".into())));
    }

    // ── 7. parse_empty_list ───────────────────────────────────────────────
    #[test]
    fn parse_empty_list() {
        let nodes = read("()", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::List(vec![]));
    }

    // ── 8. parse_non_empty_list ───────────────────────────────────────────
    #[test]
    fn parse_non_empty_list() {
        let nodes = read("(+ 1 2)", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::List(items) = &nodes[0].kind else { panic!("expected List") };
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].kind, NodeKind::Atom(Atom::Symbol { ns: None, name: "+".into() }));
        assert_eq!(items[1].kind, NodeKind::Atom(Atom::Int { value: 1, suffix: None }));
        assert_eq!(items[2].kind, NodeKind::Atom(Atom::Int { value: 2, suffix: None }));
    }

    // ── 9. parse_nested_list ──────────────────────────────────────────────
    #[test]
    fn parse_nested_list() {
        let nodes = read("((a b) c)", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::List(outer) = &nodes[0].kind else { panic!("expected List") };
        assert_eq!(outer.len(), 2);
        assert!(matches!(outer[0].kind, NodeKind::List(_)));
        assert!(matches!(outer[1].kind, NodeKind::Atom(Atom::Symbol { .. })));
    }

    // ── 10. parse_empty_vector ────────────────────────────────────────────
    #[test]
    fn parse_empty_vector() {
        let nodes = read("[]", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Vector(vec![]));
    }

    // ── 11. parse_non_empty_vector ────────────────────────────────────────
    #[test]
    fn parse_non_empty_vector() {
        let nodes = read("[1 2 3]", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Vector(items) = &nodes[0].kind else { panic!("expected Vector") };
        assert_eq!(items.len(), 3);
    }

    // ── 12. parse_empty_map ───────────────────────────────────────────────
    #[test]
    fn parse_empty_map() {
        let nodes = read("{}", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Map(vec![]));
    }

    // ── 13. parse_non_empty_map ───────────────────────────────────────────
    #[test]
    fn parse_non_empty_map() {
        let nodes = read("{:a 1 :b 2}", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Map(pairs) = &nodes[0].kind else { panic!("expected Map") };
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0.kind, NodeKind::Atom(Atom::Keyword { ns: None, name: "a".into() }));
        assert_eq!(pairs[0].1.kind, NodeKind::Atom(Atom::Int { value: 1, suffix: None }));
    }

    // ── 14. parse_set ─────────────────────────────────────────────────────
    #[test]
    fn parse_set() {
        let nodes = read("#{1 2 3}", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Set(items) = &nodes[0].kind else { panic!("expected Set") };
        assert_eq!(items.len(), 3);
    }

    // ── 15. parse_quote_macro ─────────────────────────────────────────────
    #[test]
    fn parse_quote_macro() {
        let nodes = read("'x", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Quote(inner) = &nodes[0].kind else { panic!("expected Quote") };
        assert_eq!(inner.kind, NodeKind::Atom(Atom::Symbol { ns: None, name: "x".into() }));
    }

    // ── 16. parse_deref_macro ─────────────────────────────────────────────
    #[test]
    fn parse_deref_macro() {
        let nodes = read("@x", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Deref(inner) = &nodes[0].kind else { panic!("expected Deref") };
        assert_eq!(inner.kind, NodeKind::Atom(Atom::Symbol { ns: None, name: "x".into() }));
    }

    // ── 17. parse_discard_macro ───────────────────────────────────────────
    // #_ is retained in the AST for tooling; semantic phases skip it (spec §2.1).
    #[test]
    fn parse_discard_macro() {
        let nodes = read("#_ 42 \"hi\"", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 2);
        let NodeKind::Discard(inner) = &nodes[0].kind else { panic!("expected Discard") };
        assert_eq!(inner.kind, NodeKind::Atom(Atom::Int { value: 42, suffix: None }));
        assert_eq!(nodes[1].kind, NodeKind::Atom(Atom::Str("hi".into())));
    }

    // ── 18. parse_multiple_top_level ─────────────────────────────────────
    #[test]
    fn parse_multiple_top_level() {
        let nodes = read("42 :key \"str\"", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 3);
        assert!(matches!(nodes[0].kind, NodeKind::Atom(Atom::Int { .. })));
        assert!(matches!(nodes[1].kind, NodeKind::Atom(Atom::Keyword { .. })));
        assert!(matches!(nodes[2].kind, NodeKind::Atom(Atom::Str(_))));
    }

    // ── 19. span_on_atom ─────────────────────────────────────────────────
    // Span of an integer atom must cover exactly the source bytes.
    #[test]
    fn span_on_atom() {
        let src = "42";
        let nodes = read(src, fid()).expect("parse failed");
        let span = nodes[0].span;
        assert_eq!(span.start, 0);
        assert_eq!(span.len, 2);
    }

    // ── 20. span_on_list ─────────────────────────────────────────────────
    // Span of a list covers from `(` through `)` inclusive.
    #[test]
    fn span_on_list() {
        let src = "(1 2)";
        let nodes = read(src, fid()).expect("parse failed");
        let span = nodes[0].span;
        assert_eq!(span.start, 0);
        assert_eq!(span.len as usize, src.len());
    }

    // ── 21. span_on_nested ───────────────────────────────────────────────
    // Outer list span covers the entire nested expression.
    #[test]
    fn span_on_nested() {
        let src = "(a (b c))";
        let nodes = read(src, fid()).expect("parse failed");
        let span = nodes[0].span;
        assert_eq!(span.start, 0);
        assert_eq!(span.len as usize, src.len());
    }

    // ── 22. error_unclosed_list ───────────────────────────────────────────
    #[test]
    fn error_unclosed_list() {
        let err = read("(1 2", fid()).expect_err("expected error");
        assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
        // label points at `(`
        assert_eq!(err.labels[0].span.start, 0);
    }

    // ── 23. error_unclosed_vector ─────────────────────────────────────────
    #[test]
    fn error_unclosed_vector() {
        let err = read("[1 2", fid()).expect_err("expected error");
        assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
        assert_eq!(err.labels[0].span.start, 0);
    }

    // ── 24. error_odd_map ─────────────────────────────────────────────────
    #[test]
    fn error_odd_map() {
        let err = read("{:a}", fid()).expect_err("expected error");
        assert_eq!(err.code, Some(codes::ODD_MAP_FORMS));
    }

    // ── 25. error_unexpected_close ────────────────────────────────────────
    #[test]
    fn error_unexpected_close() {
        let err = read(")", fid()).expect_err("expected error");
        assert_eq!(err.code, Some(codes::UNMATCHED_DELIMITER));
    }

    // ── Extra: suffixed integer preserved ─────────────────────────────────
    #[test]
    fn parse_integer_with_suffix() {
        let nodes = read("42i32", fid()).expect("parse failed");
        assert_eq!(
            nodes[0].kind,
            NodeKind::Atom(Atom::Int { value: 42, suffix: Some(IntSuffix::I32) })
        );
    }

    // ── Extra: ratio is reduced ───────────────────────────────────────────
    #[test]
    fn parse_ratio_reduced() {
        let nodes = read("6/4", fid()).expect("parse failed");
        assert_eq!(nodes[0].kind, NodeKind::Atom(Atom::Ratio { numer: 3, denom: 2 }));
    }

    // ── Extra: string interpolation preserved ─────────────────────────────
    #[test]
    fn parse_string_with_interp() {
        let nodes = read("\"hello {name}!\"", fid()).expect("parse failed");
        assert_eq!(nodes[0].kind, NodeKind::Atom(Atom::Str("hello {name}!".into())));
    }

    // ── Discard nesting ───────────────────────────────────────────────────

    // ── D1. discard_chain_two_forms ───────────────────────────────────────
    // spec §2.1: "To discard N consecutive forms, use N `#_` markers."
    #[test]
    fn discard_chain_two_forms() {
        let nodes = read("#_ #_ a b", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 2);
        assert!(matches!(nodes[0].kind, NodeKind::Discard(_)));
        assert!(matches!(nodes[1].kind, NodeKind::Discard(_)));
        let NodeKind::Discard(inner0) = &nodes[0].kind else { panic!() };
        let NodeKind::Discard(inner1) = &nodes[1].kind else { panic!() };
        assert_eq!(inner0.kind, NodeKind::Atom(Atom::Symbol { ns: None, name: "a".into() }));
        assert_eq!(inner1.kind, NodeKind::Atom(Atom::Symbol { ns: None, name: "b".into() }));
    }

    // ── D2. discard_chain_three_forms ─────────────────────────────────────
    #[test]
    fn discard_chain_three_forms() {
        let nodes = read("#_ #_ #_ a b c", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 3);
        for n in &nodes {
            assert!(matches!(n.kind, NodeKind::Discard(_)), "expected all Discard, got {:?}", n.kind);
        }
    }

    // ── D3. discard_chain_inside_list ─────────────────────────────────────
    #[test]
    fn discard_chain_inside_list() {
        let nodes = read("(1 #_ #_ a b 2)", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::List(items) = &nodes[0].kind else { panic!("expected List") };
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].kind, NodeKind::Atom(Atom::Int { value: 1, suffix: None }));
        assert!(matches!(items[1].kind, NodeKind::Discard(_)));
        assert!(matches!(items[2].kind, NodeKind::Discard(_)));
        assert_eq!(items[3].kind, NodeKind::Atom(Atom::Int { value: 2, suffix: None }));
    }

    // ── D4. discard_chain_inside_vector ───────────────────────────────────
    #[test]
    fn discard_chain_inside_vector() {
        let nodes = read("[#_ #_ x y z]", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Vector(items) = &nodes[0].kind else { panic!("expected Vector") };
        assert_eq!(items.len(), 3);
        assert!(matches!(items[0].kind, NodeKind::Discard(_)));
        assert!(matches!(items[1].kind, NodeKind::Discard(_)));
        assert_eq!(items[2].kind, NodeKind::Atom(Atom::Symbol { ns: None, name: "z".into() }));
    }

    // ── D5. single_discard_unchanged ──────────────────────────────────────
    // Regression: existing single-#_ behaviour must be preserved.
    #[test]
    fn single_discard_unchanged() {
        let nodes = read("#_ a b", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 2);
        let NodeKind::Discard(inner) = &nodes[0].kind else { panic!("expected Discard") };
        assert_eq!(inner.kind, NodeKind::Atom(Atom::Symbol { ns: None, name: "a".into() }));
        assert_eq!(nodes[1].kind, NodeKind::Atom(Atom::Symbol { ns: None, name: "b".into() }));
    }

    // ── D6. discard_chain_with_comment_between ────────────────────────────
    // A line comment between two #_ tokens is skipped; both forms are discarded.
    #[test]
    fn discard_chain_with_comment_between() {
        let nodes = read("#_ ; note\n#_ a b", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 2);
        assert!(matches!(nodes[0].kind, NodeKind::Discard(_)));
        assert!(matches!(nodes[1].kind, NodeKind::Discard(_)));
    }

    // ── Round-trip comment attachment ─────────────────────────────────────

    use nexl_ast::Comment;

    // ── RT1. leading_comment_on_atom ──────────────────────────────────────
    #[test]
    fn leading_comment_on_atom() {
        let nodes = read("; note\n42", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].leading_comments, vec![Comment(" note".into())]);
    }

    // ── RT2. multiple_leading_comments ────────────────────────────────────
    #[test]
    fn multiple_leading_comments() {
        let nodes = read("; a\n; b\n42", fid()).expect("parse failed");
        assert_eq!(nodes[0].leading_comments, vec![
            Comment(" a".into()),
            Comment(" b".into()),
        ]);
    }

    // ── RT3. trailing_comment_on_atom ─────────────────────────────────────
    #[test]
    fn trailing_comment_on_atom() {
        let nodes = read("42 ; answer", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].trailing_comment, Some(Comment(" answer".into())));
    }

    // ── RT4. comment_on_next_line_not_trailing ────────────────────────────
    // A comment on the line after a form is leading for the next form, not trailing.
    #[test]
    fn comment_on_next_line_not_trailing() {
        let nodes = read("42\n; note\n:x", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].trailing_comment, None);
        assert_eq!(nodes[1].leading_comments, vec![Comment(" note".into())]);
    }

    // ── RT5. leading_comment_on_list ──────────────────────────────────────
    #[test]
    fn leading_comment_on_list() {
        let nodes = read("; header\n(+ 1 2)", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert!(matches!(nodes[0].kind, NodeKind::List(_)));
        assert_eq!(nodes[0].leading_comments, vec![Comment(" header".into())]);
    }

    // ── RT6. trailing_comment_after_list ──────────────────────────────────
    #[test]
    fn trailing_comment_after_list() {
        let nodes = read("(+ 1 2) ; sum", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].trailing_comment, Some(Comment(" sum".into())));
    }

    // ── RT7. inner_trailing_comment ───────────────────────────────────────
    // A trailing comment inside a list attaches to the element, not the list.
    #[test]
    fn inner_trailing_comment() {
        let nodes = read("(1 ; first\n2)", fid()).expect("parse failed");
        let NodeKind::List(items) = &nodes[0].kind else { panic!("expected List") };
        assert_eq!(items[0].trailing_comment, Some(Comment(" first".into())));
        assert_eq!(items[1].trailing_comment, None);
    }

    // ── RT8. no_comments_empty_vecs ───────────────────────────────────────
    // Regression: forms with no adjacent comments have empty/None comment fields.
    #[test]
    fn no_comments_empty_vecs() {
        let nodes = read("42 :key", fid()).expect("parse failed");
        assert_eq!(nodes[0].leading_comments, vec![]);
        assert_eq!(nodes[0].trailing_comment, None);
        assert_eq!(nodes[1].leading_comments, vec![]);
        assert_eq!(nodes[1].trailing_comment, None);
    }

    // ── RT9. leading_comment_inside_list ─────────────────────────────────
    // A comment at the start of a list body becomes the inner element's leading comment.
    #[test]
    fn leading_comment_inside_list() {
        let nodes = read("(; inner\n42)", fid()).expect("parse failed");
        let NodeKind::List(items) = &nodes[0].kind else { panic!("expected List") };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].leading_comments, vec![Comment(" inner".into())]);
    }

    // ── RT10. comment_before_discard ─────────────────────────────────────
    // A leading comment before a `#_` attaches to the Discard node.
    #[test]
    fn comment_before_discard() {
        let nodes = read("; skip\n#_ 99", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert!(matches!(nodes[0].kind, NodeKind::Discard(_)));
        assert_eq!(nodes[0].leading_comments, vec![Comment(" skip".into())]);
    }
}
