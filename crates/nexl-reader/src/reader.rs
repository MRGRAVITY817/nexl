use nexl_ast::{
    Atom,
    Comment,
    FileId,
    ImportDecl,
    ImportKind,
    ModuleDecl,
    Node,
    NodeKind,
    Span,
    parse_import_decl,
    parse_module_decl,
};
use nexl_errors::{Diagnostic, Label, Severity, codes};

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
    let mut reader = Reader {
        tokens,
        pos: 0,
        src,
    };
    reader.read_all()
}

/// Parse the leading `(module ...)` form in `src` into a [`ModuleDecl`].
///
/// Errors if the file is empty or the first form is not a module declaration.
pub fn read_module_decl(src: &str, file_id: FileId) -> Result<ModuleDecl, Box<Diagnostic>> {
    let nodes = read(src, file_id)?;
    let first = nodes.first().ok_or_else(|| {
        let mut d = Diagnostic::new(Severity::Error, "expected a module declaration at top of file");
        d.push_label(Label::new(
            Span::point(file_id, 0),
            "module form expected here",
        ));
        d.set_help("add a `(module <name> ...)` form as the first form in the file");
        Box::new(d)
    })?;

    let NodeKind::List(items) = &first.kind else {
        return Err(expected_module_decl(first.span, "first form is not a `module` declaration"));
    };

    let head = items
        .first()
        .ok_or_else(|| expected_module_decl(first.span, "module form expected here"))?;
    match &head.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "module" => {}
        _ => {
            return Err(expected_module_decl(
                head.span,
                "first form is not a `module` declaration",
            ));
        }
    }

    match parse_module_decl(items) {
        Ok(decl) => Ok(decl),
        Err(err) => {
            let mut d = Diagnostic::new(Severity::Error, err.description);
            d.push_label(Label::new(first.span, "invalid module declaration"));
            d.set_help(
                "check the module declaration syntax, e.g. `(module my-app.core :exports [...] :performs [...])`",
            );
            Err(Box::new(d))
        }
    }
}

fn expected_module_decl(span: Span, label: &str) -> Box<Diagnostic> {
    let mut d = Diagnostic::new(Severity::Error, "expected a module declaration at top of file");
    d.push_label(Label::new(span, label));
    d.set_help("add a `(module <name> ...)` form as the first form in the file");
    Box::new(d)
}

/// Parse a single `(import ...)` form in `src` into an [`ImportDecl`].
pub fn read_import_decl(src: &str, file_id: FileId) -> Result<ImportDecl, Box<Diagnostic>> {
    let nodes = read(src, file_id)?;
    let first = nodes.first().ok_or_else(|| {
        let mut d = Diagnostic::new(Severity::Error, "expected an import declaration");
        d.push_label(Label::new(
            Span::point(file_id, 0),
            "import form expected here",
        ));
        d.set_help("add an `(import <module> :as <alias>)` form");
        Box::new(d)
    })?;

    let NodeKind::List(items) = &first.kind else {
        return Err(expected_import_decl(first.span, "first form is not an `import` declaration"));
    };

    let head = items
        .first()
        .ok_or_else(|| expected_import_decl(first.span, "import form expected here"))?;
    match &head.kind {
        NodeKind::Atom(Atom::Symbol { ns: None, name }) if name == "import" => {}
        _ => {
            return Err(expected_import_decl(
                head.span,
                "first form is not an `import` declaration",
            ));
        }
    }

    let decl = match parse_import_decl(items) {
        Ok(decl) => decl,
        Err(err) => {
            let mut d = Diagnostic::new(Severity::Error, err.description);
            d.push_label(Label::new(first.span, "invalid import declaration"));
            d.set_help("check the import declaration syntax, e.g. `(import my-lib.http :as http)`");
            return Err(Box::new(d));
        }
    };

    match decl.kind {
        ImportKind::Alias(_)
        | ImportKind::Refer(_)
        | ImportKind::Exclude(_)
        | ImportKind::Rename(_) => Ok(decl),
        _ => {
            let mut d = Diagnostic::new(
                Severity::Error,
                "only :as, :refer, :exclude, or :rename imports are supported",
            );
            d.push_label(Label::new(first.span, "unsupported import option"));
            d.set_help(
                "use `(import <module> :as <alias>)`, `(import <module> :refer [...])`, `(import <module> :exclude [...])`, or `(import <module> :rename {...})`",
            );
            Err(Box::new(d))
        }
    }
}

fn expected_import_decl(span: Span, label: &str) -> Box<Diagnostic> {
    let mut d = Diagnostic::new(Severity::Error, "expected an import declaration");
    d.push_label(Label::new(span, label));
    d.set_help("add an `(import <module> :as <alias>)` form");
    Box::new(d)
}

fn make_src_loc(span: Span) -> Node {
    let file = Node::atom(
        Atom::Int {
            value: span.file_id.0 as i128,
            suffix: None,
        },
        Span::synthetic(),
    );
    let start = Node::atom(
        Atom::Int {
            value: span.start as i128,
            suffix: None,
        },
        Span::synthetic(),
    );
    let len = Node::atom(
        Atom::Int {
            value: span.len as i128,
            suffix: None,
        },
        Span::synthetic(),
    );

    let pairs = vec![
        (
            Node::atom(
                Atom::Keyword {
                    ns: None,
                    name: "file".into(),
                },
                Span::synthetic(),
            ),
            file,
        ),
        (
            Node::atom(
                Atom::Keyword {
                    ns: None,
                    name: "start".into(),
                },
                Span::synthetic(),
            ),
            start,
        ),
        (
            Node::atom(
                Atom::Keyword {
                    ns: None,
                    name: "len".into(),
                },
                Span::synthetic(),
            ),
            len,
        ),
    ];

    Node::new(NodeKind::Map(pairs), Span::synthetic())
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
            TokenKind::Int(value, suffix) => Ok(Node::atom(Atom::Int { value, suffix }, tok.span)),
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
            TokenKind::Str(parts) => Ok(Node::atom(Atom::Str(reassemble_str(&parts)), tok.span)),
            TokenKind::Keyword { ns, name, .. } => {
                Ok(Node::atom(Atom::Keyword { ns, name }, tok.span))
            }
            TokenKind::Symbol { ns, name } => Ok(Node::atom(Atom::Symbol { ns, name }, tok.span)),
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
            TokenKind::DispatchTag { name } => Ok(Node::atom(
                Atom::Symbol {
                    ns: None,
                    name: format!("#{name}"),
                },
                tok.span,
            )),
            TokenKind::DispatchText {
                name,
                text,
                text_span,
            } => {
                let head = Node::atom(
                    Atom::Symbol {
                        ns: None,
                        name,
                    },
                    tok.span,
                );
                let text_node = Node::atom(Atom::Str(text), text_span);
                let loc_node = make_src_loc(text_span);
                Ok(Node::new(
                    NodeKind::List(vec![head, text_node, loc_node]),
                    tok.span,
                ))
            }
            // Operator/separator tokens — treated as symbol atoms so they can
            // appear as elements inside collections (e.g. `[x : Int]`, `| Red`).
            TokenKind::Dot => Ok(Node::atom(
                Atom::Symbol {
                    ns: None,
                    name: ".".into(),
                },
                tok.span,
            )),
            TokenKind::Pipe => Ok(Node::atom(
                Atom::Symbol {
                    ns: None,
                    name: "|".into(),
                },
                tok.span,
            )),
            TokenKind::Amp => Ok(Node::atom(
                Atom::Symbol {
                    ns: None,
                    name: "&".into(),
                },
                tok.span,
            )),
            TokenKind::Colon => Ok(Node::atom(
                Atom::Symbol {
                    ns: None,
                    name: ":".into(),
                },
                tok.span,
            )),
            // Quasiquote / unquote / unquote-splice reader macros.
            TokenKind::Quasiquote => {
                let inner = self.require_form("`", tok.span)?;
                let span = tok.span.merge(inner.span);
                Ok(Node::new(NodeKind::Quasiquote(Box::new(inner)), span))
            }
            TokenKind::Unquote => {
                let inner = self.require_form("~", tok.span)?;
                let span = tok.span.merge(inner.span);
                Ok(Node::new(NodeKind::Unquote(Box::new(inner)), span))
            }
            TokenKind::UnquoteSplice => {
                let inner = self.require_form("~@", tok.span)?;
                let span = tok.span.merge(inner.span);
                Ok(Node::new(NodeKind::UnquoteSplice(Box::new(inner)), span))
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
                    return Ok(Node::new(
                        NodeKind::List(items),
                        open_span.merge(close.span),
                    ));
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
                    return Ok(Node::new(
                        NodeKind::Vector(items),
                        open_span.merge(close.span),
                    ));
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
            let tok = self
                .advance()
                .expect("called after verifying peek_no_comment is Some");
            let mut form = self.dispatch(tok)?;
            form.leading_comments = leading;
            let mut form = self.apply_postfix_question(form);
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
                        d.set_help("add the form to discard, or remove the `#_`");
                        return Err(Box::new(d));
                    }
                    Some(t) if is_close(t) => {
                        let t = t.clone();
                        return Err(self.unmatched_delimiter(&t));
                    }
                    _ => {
                        let tok = self.advance().unwrap();
                        let inner = self.dispatch(tok)?;
                        let inner = self.apply_postfix_question(inner);
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

    fn apply_postfix_question(&mut self, mut node: Node) -> Node {
        loop {
            let Some(tok) = self.peek() else {
                break;
            };
            match &tok.kind {
                TokenKind::Symbol { ns: None, name } if name == "?" => {
                    let tok = self.advance().unwrap();
                    let span = node.span.merge(tok.span);
                    let leading = std::mem::take(&mut node.leading_comments);
                    let inner = node;
                    let mut wrapped = Node::new(
                        NodeKind::List(vec![
                            Node::atom(
                                Atom::Symbol {
                                    ns: None,
                                    name: "?".to_string(),
                                },
                                tok.span,
                            ),
                            inner,
                        ]),
                        span,
                    );
                    wrapped.leading_comments = leading;
                    node = wrapped;
                }
                _ => break,
            }
        }
        node
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
                d.push_label(Label::new(
                    prefix_span,
                    format!("this `{prefix}` expects a following form"),
                ));
                let help = match prefix {
                    "'" => "add the quoted form, e.g. `'x`, or remove the `'`".to_string(),
                    "@" => {
                        "add the form to dereference, e.g. `@value`, or remove the `@`".to_string()
                    }
                    _ => format!("add a form after `{prefix}`, or remove the `{prefix}`"),
                };
                d.set_help(help);
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
        let TokenKind::Comment(text) = &tok.kind else {
            return None;
        };
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
        let opener = match delim {
            ")" => "(",
            "]" => "[",
            "}" => "{",
            _ => "?",
        };
        let mut d = Diagnostic::new(
            Severity::Error,
            format!("unexpected `{delim}` — no matching opener"),
        );
        d.code = Some(codes::UNMATCHED_DELIMITER);
        d.push_label(Label::new(tok.span, "unmatched closing delimiter"));
        d.set_help(format!(
            "remove this `{delim}` or add a matching opening `{opener}` earlier"
        ));
        Box::new(d)
    }

    fn unclosed_delimiter(&self, open_span: Span, opener: &str) -> Box<Diagnostic> {
        let mut d = Diagnostic::new(
            Severity::Error,
            format!("unclosed `{opener}` — expected matching closer before end of file"),
        );
        d.code = Some(codes::UNCLOSED_DELIMITER);
        let label_msg = match opener {
            "(" => "list opened here",
            "[" => "vector opened here",
            "{" => "map opened here",
            "#{" => "set opened here",
            _ => "unclosed delimiter opened here",
        };
        let closer = match opener {
            "(" => ")",
            "[" => "]",
            "{" | "#{" => "}",
            _ => "closing delimiter",
        };
        d.push_label(Label::new(open_span, label_msg));
        d.set_help(format!("add a closing `{closer}` before end of file"));
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
    matches!(
        tok.kind,
        TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace
    )
}

/// Reduce `numer/denom` to lowest terms.
///
/// The lexer guarantees `denom != 0`.
fn reduce_ratio(numer: i64, denom: i64) -> (i64, i64) {
    let g = gcd(numer.abs(), denom.abs());
    if g == 0 {
        (numer, denom)
    } else {
        (numer / g, denom / g)
    }
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
    use nexl_ast::{Atom, FileId, ImportKind, IntSuffix, NodeKind};

    fn fid() -> FileId {
        FileId(0)
    }

    // ── 1. parse_integer_atom ─────────────────────────────────────────────
    #[test]
    fn parse_integer_atom() {
        let nodes = read("42", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0].kind,
            NodeKind::Atom(Atom::Int {
                value: 42,
                suffix: None
            })
        );
    }

    // ── 2. parse_symbol_atom ──────────────────────────────────────────────
    #[test]
    fn parse_symbol_atom() {
        let nodes = read("foo", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0].kind,
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: "foo".into()
            })
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
            NodeKind::Atom(Atom::Keyword {
                ns: Some("http".into()),
                name: "ok".into()
            })
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
        let NodeKind::List(items) = &nodes[0].kind else {
            panic!("expected List")
        };
        assert_eq!(items.len(), 3);
        assert_eq!(
            items[0].kind,
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: "+".into()
            })
        );
        assert_eq!(
            items[1].kind,
            NodeKind::Atom(Atom::Int {
                value: 1,
                suffix: None
            })
        );
        assert_eq!(
            items[2].kind,
            NodeKind::Atom(Atom::Int {
                value: 2,
                suffix: None
            })
        );
    }

    // ── 9. parse_nested_list ──────────────────────────────────────────────
    #[test]
    fn parse_nested_list() {
        let nodes = read("((a b) c)", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::List(outer) = &nodes[0].kind else {
            panic!("expected List")
        };
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
        let NodeKind::Vector(items) = &nodes[0].kind else {
            panic!("expected Vector")
        };
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
        let NodeKind::Map(pairs) = &nodes[0].kind else {
            panic!("expected Map")
        };
        assert_eq!(pairs.len(), 2);
        assert_eq!(
            pairs[0].0.kind,
            NodeKind::Atom(Atom::Keyword {
                ns: None,
                name: "a".into()
            })
        );
        assert_eq!(
            pairs[0].1.kind,
            NodeKind::Atom(Atom::Int {
                value: 1,
                suffix: None
            })
        );
    }

    // ── 14. parse_set ─────────────────────────────────────────────────────
    #[test]
    fn parse_set() {
        let nodes = read("#{1 2 3}", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Set(items) = &nodes[0].kind else {
            panic!("expected Set")
        };
        assert_eq!(items.len(), 3);
    }

    // ── 15. parse_quote_macro ─────────────────────────────────────────────
    #[test]
    fn parse_quote_macro() {
        let nodes = read("'x", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Quote(inner) = &nodes[0].kind else {
            panic!("expected Quote")
        };
        assert_eq!(
            inner.kind,
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: "x".into()
            })
        );
    }

    // ── 16. parse_deref_macro ─────────────────────────────────────────────
    #[test]
    fn parse_deref_macro() {
        let nodes = read("@x", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Deref(inner) = &nodes[0].kind else {
            panic!("expected Deref")
        };
        assert_eq!(
            inner.kind,
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: "x".into()
            })
        );
    }

    // ── 17. parse_discard_macro ───────────────────────────────────────────
    // #_ is retained in the AST for tooling; semantic phases skip it (spec §2.1).
    #[test]
    fn parse_discard_macro() {
        let nodes = read("#_ 42 \"hi\"", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 2);
        let NodeKind::Discard(inner) = &nodes[0].kind else {
            panic!("expected Discard")
        };
        assert_eq!(
            inner.kind,
            NodeKind::Atom(Atom::Int {
                value: 42,
                suffix: None
            })
        );
        assert_eq!(nodes[1].kind, NodeKind::Atom(Atom::Str("hi".into())));
    }

    // ── R1. reader_text_dispatch ────────────────────────────────────────────
    #[test]
    fn reader_text_dispatch() {
        let src = "#sql[SELECT name FROM users WHERE id = {user-id}]";
        let nodes = read(src, fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);

        let NodeKind::List(items) = &nodes[0].kind else {
            panic!("expected List");
        };
        assert_eq!(items.len(), 3);
        assert!(
            matches!(
                items[0].kind,
                NodeKind::Atom(Atom::Symbol { ns: None, ref name }) if name == "sql"
            ),
            "expected sql head"
        );
        let NodeKind::Atom(Atom::Str(text)) = &items[1].kind else {
            panic!("expected Str");
        };
        assert_eq!(
            text,
            "SELECT name FROM users WHERE id = {user-id}"
        );
        let NodeKind::Map(pairs) = &items[2].kind else {
            panic!("expected map loc");
        };

        let mut file = None;
        let mut start = None;
        let mut len = None;
        for (k, v) in pairs {
            let NodeKind::Atom(Atom::Keyword { ns: None, name }) = &k.kind else {
                continue;
            };
            let NodeKind::Atom(Atom::Int { value, .. }) = &v.kind else {
                continue;
            };
            match name.as_str() {
                "file" => file = Some(*value),
                "start" => start = Some(*value),
                "len" => len = Some(*value),
                _ => {}
            }
        }
        assert_eq!(file, Some(0));
        assert_eq!(start, Some(5));
        assert_eq!(len, Some(text.len() as i128));
    }

    // ── 18. parse_multiple_top_level ─────────────────────────────────────
    #[test]
    fn parse_multiple_top_level() {
        let nodes = read("42 :key \"str\"", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 3);
        assert!(matches!(nodes[0].kind, NodeKind::Atom(Atom::Int { .. })));
        assert!(matches!(
            nodes[1].kind,
            NodeKind::Atom(Atom::Keyword { .. })
        ));
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
            NodeKind::Atom(Atom::Int {
                value: 42,
                suffix: Some(IntSuffix::I32)
            })
        );
    }

    // ── Extra: ratio is reduced ───────────────────────────────────────────
    #[test]
    fn parse_ratio_reduced() {
        let nodes = read("6/4", fid()).expect("parse failed");
        assert_eq!(
            nodes[0].kind,
            NodeKind::Atom(Atom::Ratio { numer: 3, denom: 2 })
        );
    }

    // ── Extra: string interpolation preserved ─────────────────────────────
    #[test]
    fn parse_string_with_interp() {
        let nodes = read("\"hello {name}!\"", fid()).expect("parse failed");
        assert_eq!(
            nodes[0].kind,
            NodeKind::Atom(Atom::Str("hello {name}!".into()))
        );
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
        let NodeKind::Discard(inner0) = &nodes[0].kind else {
            panic!()
        };
        let NodeKind::Discard(inner1) = &nodes[1].kind else {
            panic!()
        };
        assert_eq!(
            inner0.kind,
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: "a".into()
            })
        );
        assert_eq!(
            inner1.kind,
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: "b".into()
            })
        );
    }

    // ── D2. discard_chain_three_forms ─────────────────────────────────────
    #[test]
    fn discard_chain_three_forms() {
        let nodes = read("#_ #_ #_ a b c", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 3);
        for n in &nodes {
            assert!(
                matches!(n.kind, NodeKind::Discard(_)),
                "expected all Discard, got {:?}",
                n.kind
            );
        }
    }

    // ── D3. discard_chain_inside_list ─────────────────────────────────────
    #[test]
    fn discard_chain_inside_list() {
        let nodes = read("(1 #_ #_ a b 2)", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::List(items) = &nodes[0].kind else {
            panic!("expected List")
        };
        assert_eq!(items.len(), 4);
        assert_eq!(
            items[0].kind,
            NodeKind::Atom(Atom::Int {
                value: 1,
                suffix: None
            })
        );
        assert!(matches!(items[1].kind, NodeKind::Discard(_)));
        assert!(matches!(items[2].kind, NodeKind::Discard(_)));
        assert_eq!(
            items[3].kind,
            NodeKind::Atom(Atom::Int {
                value: 2,
                suffix: None
            })
        );
    }

    // ── D4. discard_chain_inside_vector ───────────────────────────────────
    #[test]
    fn discard_chain_inside_vector() {
        let nodes = read("[#_ #_ x y z]", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Vector(items) = &nodes[0].kind else {
            panic!("expected Vector")
        };
        assert_eq!(items.len(), 3);
        assert!(matches!(items[0].kind, NodeKind::Discard(_)));
        assert!(matches!(items[1].kind, NodeKind::Discard(_)));
        assert_eq!(
            items[2].kind,
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: "z".into()
            })
        );
    }

    // ── D5. single_discard_unchanged ──────────────────────────────────────
    // Regression: existing single-#_ behaviour must be preserved.
    #[test]
    fn single_discard_unchanged() {
        let nodes = read("#_ a b", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 2);
        let NodeKind::Discard(inner) = &nodes[0].kind else {
            panic!("expected Discard")
        };
        assert_eq!(
            inner.kind,
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: "a".into()
            })
        );
        assert_eq!(
            nodes[1].kind,
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: "b".into()
            })
        );
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
        assert_eq!(
            nodes[0].leading_comments,
            vec![Comment(" a".into()), Comment(" b".into()),]
        );
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
        let NodeKind::List(items) = &nodes[0].kind else {
            panic!("expected List")
        };
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
        let NodeKind::List(items) = &nodes[0].kind else {
            panic!("expected List")
        };
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

    // ── 39. roundtrip_simple_list ─────────────────────────────────────────
    // Parse → pretty-print → re-parse; the two AST kinds must be equal.
    // Spans are not compared because they differ between parses.
    #[test]
    fn roundtrip_simple_list() {
        use nexl_ast::PrettyPrinter;
        let src = "(+ 1 2)";
        let nodes1 = read(src, fid()).expect("first parse");
        let printed = PrettyPrinter::default_config().print(&nodes1[0]);
        let nodes2 = read(&printed, fid()).expect("second parse");
        assert_eq!(nodes1[0].kind, nodes2[0].kind);
    }

    // ── Atom completeness ─────────────────────────────────────────────────────

    // ── 11. parse_float_atom ──────────────────────────────────────────────────
    #[test]
    fn parse_float_atom() {
        use nexl_ast::FloatSuffix;
        let nodes = read("3.75", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0].kind,
            NodeKind::Atom(Atom::Float {
                value: 3.75,
                suffix: None
            })
        );

        let nodes = read("1.5f32", fid()).expect("parse failed");
        assert_eq!(
            nodes[0].kind,
            NodeKind::Atom(Atom::Float {
                value: 1.5,
                suffix: Some(FloatSuffix::F32)
            })
        );
    }

    // ── 13. parse_char_single ─────────────────────────────────────────────────
    #[test]
    fn parse_char_single() {
        let nodes = read("\\a", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Atom(Atom::Char('a')));
    }

    // ── 14. parse_char_named ──────────────────────────────────────────────────
    #[test]
    fn parse_char_named() {
        let nodes = read("\\newline", fid()).expect("parse failed");
        assert_eq!(nodes[0].kind, NodeKind::Atom(Atom::Char('\n')));
    }

    // ── 15. parse_char_unicode ────────────────────────────────────────────────
    #[test]
    fn parse_char_unicode() {
        let nodes = read("\\u{1F600}", fid()).expect("parse failed");
        assert_eq!(nodes[0].kind, NodeKind::Atom(Atom::Char('😀')));
    }

    // ── 16. parse_qualified_symbol ────────────────────────────────────────────
    #[test]
    fn parse_qualified_symbol() {
        let nodes = read("my-mod/my-fn", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0].kind,
            NodeKind::Atom(Atom::Symbol {
                ns: Some("my-mod".into()),
                name: "my-fn".into()
            }),
        );
    }

    // ── 16. module_decl_minimal ─────────────────────────────────────────────
    #[test]
    fn module_decl_minimal() {
        let decl = read_module_decl("(module my-app.server)", fid()).expect("parse failed");
        assert_eq!(decl.name, "my-app.server");
        assert_eq!(decl.exports, None);
        assert_eq!(decl.performs, None);
    }

    // ── 17. module_decl_with_exports_and_performs ───────────────────────────
    #[test]
    fn module_decl_with_exports_and_performs() {
        let src = "(module my-app.server :performs [Net IO] :exports [start! stop!])";
        let decl = read_module_decl(src, fid()).expect("parse failed");
        assert_eq!(decl.name, "my-app.server");
        assert_eq!(
            decl.performs,
            Some(vec!["Net".to_string(), "IO".to_string()])
        );
        assert_eq!(
            decl.exports,
            Some(vec!["start!".to_string(), "stop!".to_string()])
        );
    }

    // ── 18. import_decl_alias ───────────────────────────────────────────────
    #[test]
    fn import_decl_alias() {
        let decl = read_import_decl("(import my-lib.http :as http)", fid())
            .expect("parse failed");
        assert_eq!(decl.module_path, "my-lib.http");
        assert_eq!(decl.kind, ImportKind::Alias("http".to_string()));
    }

    // ── 19. import_decl_refer ───────────────────────────────────────────────
    #[test]
    fn import_decl_refer() {
        let decl = read_import_decl("(import my-lib.coll :refer [map filter])", fid())
            .expect("parse failed");
        assert_eq!(decl.module_path, "my-lib.coll");
        assert_eq!(
            decl.kind,
            ImportKind::Refer(vec!["map".to_string(), "filter".to_string()])
        );
    }

    // ── 20. import_decl_exclude ─────────────────────────────────────────────
    #[test]
    fn import_decl_exclude() {
        let decl = read_import_decl("(import my-lib.coll :exclude [map filter])", fid())
            .expect("parse failed");
        assert_eq!(decl.module_path, "my-lib.coll");
        assert_eq!(
            decl.kind,
            ImportKind::Exclude(vec!["map".to_string(), "filter".to_string()])
        );
    }

    // ── 21. import_decl_rename ──────────────────────────────────────────────
    #[test]
    fn import_decl_rename() {
        let decl = read_import_decl(
            "(import my-lib.coll :rename {map map2 filter filter2})",
            fid(),
        )
        .expect("parse failed");
        assert_eq!(decl.module_path, "my-lib.coll");
        assert_eq!(
            decl.kind,
            ImportKind::Rename(vec![
                ("map".to_string(), "map2".to_string()),
                ("filter".to_string(), "filter2".to_string()),
            ])
        );
    }

    // ── 17. parse_auto_namespace_kw ───────────────────────────────────────────
    // auto_ns is a lexer property only; the reader maps to Keyword { ns: None, name }.
    // Namespace resolution happens in a later phase.
    #[test]
    fn parse_auto_namespace_kw() {
        let nodes = read("::local", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0].kind,
            NodeKind::Atom(Atom::Keyword {
                ns: None,
                name: "local".into()
            }),
        );
    }

    // ── 18. parse_empty_string ────────────────────────────────────────────────
    #[test]
    fn parse_empty_string() {
        let nodes = read("\"\"", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Atom(Atom::Str("".into())));
    }

    // ── Collection edge cases ─────────────────────────────────────────────────

    // ── 19. parse_empty_set ───────────────────────────────────────────────────
    #[test]
    fn parse_empty_set() {
        let nodes = read("#{}", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].kind, NodeKind::Set(vec![]));
    }

    // ── 20. parse_comma_in_map ────────────────────────────────────────────────
    // Commas are whitespace (spec §2.2), so `{:a 1, :b 2}` has two pairs.
    #[test]
    fn parse_comma_in_map() {
        let nodes = read("{:a 1, :b 2}", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Map(pairs) = &nodes[0].kind else {
            panic!("expected Map")
        };
        assert_eq!(pairs.len(), 2);
        assert_eq!(
            pairs[0].0.kind,
            NodeKind::Atom(Atom::Keyword {
                ns: None,
                name: "a".into()
            })
        );
        assert_eq!(
            pairs[1].0.kind,
            NodeKind::Atom(Atom::Keyword {
                ns: None,
                name: "b".into()
            })
        );
    }

    // ── 21. parse_deeply_nested ───────────────────────────────────────────────
    #[test]
    fn parse_deeply_nested() {
        // `((((a))))` — four levels of list nesting; must unpack all four
        let nodes = read("((((a))))", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::List(l1) = &nodes[0].kind else {
            panic!("expected List at level 1")
        };
        let NodeKind::List(l2) = &l1[0].kind else {
            panic!("expected List at level 2")
        };
        let NodeKind::List(l3) = &l2[0].kind else {
            panic!("expected List at level 3")
        };
        let NodeKind::List(l4) = &l3[0].kind else {
            panic!("expected List at level 4")
        };
        assert_eq!(
            l4[0].kind,
            NodeKind::Atom(Atom::Symbol {
                ns: None,
                name: "a".into()
            })
        );
    }

    // ── 22. parse_quote_wrapping_list ─────────────────────────────────────────
    #[test]
    fn parse_quote_wrapping_list() {
        let nodes = read("'(1 2)", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Quote(inner) = &nodes[0].kind else {
            panic!("expected Quote")
        };
        assert!(matches!(inner.kind, NodeKind::List(_)));
    }

    // ── 23. parse_deref_wrapping_compound ─────────────────────────────────────
    #[test]
    fn parse_deref_wrapping_compound() {
        let nodes = read("@[1 2]", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Deref(inner) = &nodes[0].kind else {
            panic!("expected Deref")
        };
        assert!(matches!(inner.kind, NodeKind::Vector(_)));
    }

    // ── 24. parse_mixed_nested ────────────────────────────────────────────────
    #[test]
    fn parse_mixed_nested() {
        // `{:x [1 #{:k}]}` — Map -> Vector -> Set nesting
        let nodes = read("{:x [1 #{:k}]}", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::Map(pairs) = &nodes[0].kind else {
            panic!("expected Map")
        };
        assert_eq!(pairs.len(), 1);
        let NodeKind::Vector(items) = &pairs[0].1.kind else {
            panic!("expected Vector")
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(items[1].kind, NodeKind::Set(_)));
    }

    // ── Error completeness ────────────────────────────────────────────────────

    // ── 25. error_unclosed_set ────────────────────────────────────────────────
    #[test]
    fn error_unclosed_set() {
        let err = read("#{1 2", fid()).expect_err("expected error");
        assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
        assert_eq!(err.labels[0].span.start, 0); // `#{` opens at byte 0
    }

    // ── 26. error_unclosed_map ────────────────────────────────────────────────
    #[test]
    fn error_unclosed_map() {
        let err = read("{:a 1", fid()).expect_err("expected error");
        assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
        assert_eq!(err.labels[0].span.start, 0);
    }

    // ── 27. error_discard_at_eof ─────────────────────────────────────────────
    #[test]
    fn error_discard_at_eof() {
        let err = read("#_", fid()).expect_err("expected error");
        assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
        assert!(
            err.message.contains("#_"),
            "expected '#_' in message, got: {}",
            err.message,
        );
    }

    // ── 28. error_quote_at_eof ────────────────────────────────────────────────
    #[test]
    fn error_quote_at_eof() {
        let err = read("'", fid()).expect_err("expected error");
        assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
        assert!(
            err.message.contains("'"),
            "expected `'` in message, got: {}",
            err.message,
        );
    }

    // ── 29. error_deref_at_eof ────────────────────────────────────────────────
    #[test]
    fn error_deref_at_eof() {
        let err = read("@", fid()).expect_err("expected error");
        assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
        assert!(
            err.message.contains("@"),
            "expected '@' in message, got: {}",
            err.message,
        );
    }

    // ── 30. parse_postfix_question_operator ────────────────────────────────
    #[test]
    fn parse_postfix_question_operator() {
        let nodes = read("(parse-thing s)?", fid()).expect("parse failed");
        assert_eq!(nodes.len(), 1);
        let NodeKind::List(items) = &nodes[0].kind else {
            panic!("expected List");
        };
        assert_eq!(items.len(), 2);
        assert!(
            matches!(
                items[0].kind,
                NodeKind::Atom(Atom::Symbol { ns: None, ref name }) if name == "?"
            ),
            "expected ? operator head"
        );
        let NodeKind::List(call) = &items[1].kind else {
            panic!("expected List for operand");
        };
        assert!(
            matches!(
                call[0].kind,
                NodeKind::Atom(Atom::Symbol { ns: None, ref name }) if name == "parse-thing"
            ),
            "expected parse-thing call as operand"
        );
    }

    // ── Example file smoke tests ──────────────────────────────────────────────

    #[test]
    fn parse_example_01_basics() {
        let src = include_str!("../../../examples/01-basics.nxl");
        read(src, fid()).expect("01-basics.nxl should parse without errors");
    }

    #[test]
    fn parse_example_02_adts() {
        let src = include_str!("../../../examples/02-adts-and-patterns.nxl");
        read(src, fid()).expect("02-adts-and-patterns.nxl should parse without errors");
    }

    #[test]
    fn parse_example_03_effects() {
        let src = include_str!("../../../examples/03-effects.nxl");
        read(src, fid()).expect("03-effects.nxl should parse without errors");
    }

    #[test]
    fn parse_example_04_protocols() {
        let src = include_str!("../../../examples/04-protocols.nxl");
        read(src, fid()).expect("04-protocols.nxl should parse without errors");
    }

    #[test]
    fn parse_example_05_concurrency() {
        let src = include_str!("../../../examples/05-concurrency.nxl");
        read(src, fid()).expect("05-concurrency.nxl should parse without errors");
    }

    #[test]
    fn parse_example_06_macros() {
        let src = include_str!("../../../examples/06-macros.nxl");
        read(src, fid()).expect("06-macros.nxl should parse without errors");
    }

    #[test]
    fn parse_example_07_http_server() {
        let src = include_str!("../../../examples/07-http-server.nxl");
        read(src, fid()).expect("07-http-server.nxl should parse without errors");
    }

    #[test]
    fn parse_example_08_inference() {
        let src = include_str!("../../../examples/08-inference.nxl");
        read(src, fid()).expect("08-inference.nxl should parse without errors");
    }
}
