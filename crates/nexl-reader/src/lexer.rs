use nexl_ast::{FileId, FloatSuffix, IntSuffix, Span};
use nexl_errors::{codes, Diagnostic, ErrorCode, Label, Severity};

// ---------------------------------------------------------------------------
// Token
// ---------------------------------------------------------------------------

/// A single lexical token with its source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// A segment of a string literal after escape processing.
///
/// String content is split into alternating literal runs and interpolation
/// holes at lex time so that later compiler passes see a clean boundary
/// between text and embedded expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    /// Literal text with all escape sequences already resolved.
    Lit(String),
    /// An interpolation hole: the raw expression text between `{` and `}`.
    Interp(String),
}

/// The structural kind and value of a token.
///
/// Variants are added as each lexer task is implemented. Non-exhaustive
/// matching is intentional — new variants will be added across M0 tasks.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// Integer literal with optional width suffix, e.g. `42`, `255u8`.
    Int(i128, Option<IntSuffix>),
    /// Float literal with optional precision suffix, e.g. `3.14`, `3.14f32`.
    Float(f64, Option<FloatSuffix>),
    /// Ratio literal, e.g. `3/4`. Stored as raw numerator/denominator; reduction
    /// to lowest terms is deferred to the reader pass.
    Ratio(i64, i64),
    /// String literal split into literal runs and interpolation holes.
    ///
    /// Escape sequences (`\n`, `\t`, `\\`, `\"`, `\{`) are resolved to their
    /// actual characters inside `Lit` segments. `{{` and `}}` produce literal
    /// `{` / `}` without triggering interpolation. `{expr}` spans become
    /// `Interp` segments containing the raw expression text.
    Str(Vec<StringPart>),
    /// Character literal, e.g. `\a`, `\newline`, `\u{1F600}`.
    Char(char),
    /// Keyword literal, e.g. `:foo`, `:http/ok`, `::local-alias`.
    ///
    /// `ns` is `Some("http")` for `:http/ok`, `None` otherwise.
    /// `auto_ns` is `true` for the `::name` form (resolves to the current module namespace).
    Keyword { ns: Option<String>, name: String, auto_ns: bool },
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

/// Splits a Nexl source string into a flat sequence of [`Token`]s.
pub struct Lexer<'src> {
    src: &'src str,
    pos: usize,
    file_id: FileId,
}

impl<'src> Lexer<'src> {
    /// Create a lexer for `src`, tagging all spans with `file_id`.
    pub fn new(src: &'src str, file_id: FileId) -> Self {
        Self { src, pos: 0, file_id }
    }

    /// Consume the lexer and produce a token list, or the first error.
    pub fn tokenize(mut self) -> Result<Vec<Token>, Box<Diagnostic>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            if self.pos >= self.src.len() {
                break;
            }
            tokens.push(self.lex_token()?);
        }
        Ok(tokens)
    }

    // --- source navigation ---

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_ahead(&self, n: usize) -> Option<char> {
        self.src[self.pos..].chars().nth(n)
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn span_from(&self, start: usize) -> Span {
        Span::new(self.file_id, start as u32, (self.pos - start) as u32)
    }

    // --- whitespace ---

    /// Skip spaces, tabs, newlines, and commas (spec §2.2).
    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_ascii_whitespace() || c == ',') {
            self.advance();
        }
    }

    // --- dispatch ---

    fn lex_token(&mut self) -> Result<Token, Box<Diagnostic>> {
        let ch = self.peek().expect("lex_token called at EOF");

        // Integer: digit OR '-' immediately followed by a digit
        let next_is_digit = self.peek_ahead(1).is_some_and(|c| c.is_ascii_digit());
        if ch.is_ascii_digit() || (ch == '-' && next_is_digit) {
            return self.lex_number();
        }

        if ch == '"' {
            return self.lex_string();
        }

        if ch == '\\' {
            return self.lex_char();
        }

        if ch == ':' {
            return self.lex_keyword();
        }

        let start = self.pos;
        self.advance();
        Err(Box::new(self.error_at(start, format!("unexpected character `{ch}`"), None)))
    }

    // --- number lexing ---

    fn lex_number(&mut self) -> Result<Token, Box<Diagnostic>> {
        let start = self.pos;

        // Optional leading minus
        let negative = if self.peek() == Some('-') {
            self.advance();
            true
        } else {
            false
        };

        // Base prefix: 0x / 0b / 0o (non-decimal bases are always integers)
        let is_prefixed_base = self.peek() == Some('0')
            && self
                .peek_ahead(1)
                .is_some_and(|c| matches!(c, 'x' | 'X' | 'b' | 'B' | 'o' | 'O'));

        if is_prefixed_base {
            let (raw, radix): (String, u32) = {
                self.advance(); // '0'
                match self.advance().unwrap() {
                    'x' | 'X' => (
                        self.collect_while(|c| c.is_ascii_hexdigit() || c == '_'),
                        16,
                    ),
                    'b' | 'B' => (
                        self.collect_while(|c| matches!(c, '0' | '1' | '_')),
                        2,
                    ),
                    'o' | 'O' => (
                        self.collect_while(|c| matches!(c, '0'..='7' | '_')),
                        8,
                    ),
                    _ => unreachable!(),
                }
            };
            let clean: String = raw.chars().filter(|&c| c != '_').collect();
            let abs_val = i128::from_str_radix(&clean, radix).map_err(|_| {
                Box::new(self.error_at(start, "integer literal out of range", None))
            })?;
            let value = if negative { -abs_val } else { abs_val };
            let suffix = self.lex_int_suffix()?;
            let span = self.span_from(start);
            return Ok(Token { kind: TokenKind::Int(value, suffix), span });
        }

        // Decimal integer part
        let int_raw = self.collect_while(|c| c.is_ascii_digit() || c == '_');

        // Decide: ratio, float, or int?
        let is_ratio = self.peek() == Some('/')
            && self.peek_ahead(1).is_some_and(|c| c.is_ascii_digit());
        let is_float_dot = self.peek() == Some('.')
            && self.peek_ahead(1).is_some_and(|c| c.is_ascii_digit());
        let is_float_exp =
            self.peek().is_some_and(|c| c == 'e' || c == 'E');

        if is_ratio {
            self.lex_ratio_from(start, negative, &int_raw)
        } else if is_float_dot || is_float_exp {
            self.lex_float_from(start, negative, &int_raw)
        } else {
            let clean: String = int_raw.chars().filter(|&c| c != '_').collect();
            let abs_val = clean.parse::<i128>().map_err(|_| {
                Box::new(self.error_at(start, "integer literal out of range", None))
            })?;
            let value = if negative { -abs_val } else { abs_val };
            let suffix = self.lex_int_suffix()?;
            let span = self.span_from(start);
            Ok(Token { kind: TokenKind::Int(value, suffix), span })
        }
    }

    /// Finish lexing a float literal after the integer part has been collected.
    ///
    /// `int_raw` is the already-consumed digit string (may contain `_`).
    /// The lexer position is at `.` (decimal form) or `e`/`E` (exponent-only form).
    fn lex_float_from(
        &mut self,
        start: usize,
        negative: bool,
        int_raw: &str,
    ) -> Result<Token, Box<Diagnostic>> {
        // Build the full numeric string for f64::from_str
        let mut s = String::new();
        if negative {
            s.push('-');
        }
        s.push_str(&int_raw.replace('_', ""));

        // Optional decimal part: . digits
        if self.peek() == Some('.') {
            self.advance(); // consume '.'
            s.push('.');
            let frac = self.collect_while(|c| c.is_ascii_digit() || c == '_');
            s.push_str(&frac.replace('_', ""));
        }

        // Optional exponent: (e|E) [+|-] digits
        if self.peek().is_some_and(|c| c == 'e' || c == 'E') {
            s.push('e');
            self.advance(); // consume 'e' or 'E'
            if self.peek().is_some_and(|c| c == '+' || c == '-') {
                s.push(self.advance().unwrap());
            }
            let exp = self.collect_while(|c| c.is_ascii_digit());
            s.push_str(&exp);
        }

        let value: f64 = s.parse().map_err(|_| {
            Box::new(self.error_at(start, "float literal out of range", None))
        })?;

        let suffix = self.lex_float_suffix()?;
        let span = self.span_from(start);
        Ok(Token { kind: TokenKind::Float(value, suffix), span })
    }

    /// Finish lexing a ratio literal after the numerator integer part has been
    /// collected. The lexer position is at `/`.
    fn lex_ratio_from(
        &mut self,
        start: usize,
        negative: bool,
        numer_raw: &str,
    ) -> Result<Token, Box<Diagnostic>> {
        self.advance(); // consume '/'

        let denom_raw = self.collect_while(|c| c.is_ascii_digit() || c == '_');
        let denom_clean: String = denom_raw.chars().filter(|&c| c != '_').collect();
        let denom: i64 = denom_clean.parse().map_err(|_| {
            Box::new(self.error_at(start, "ratio denominator out of range", None))
        })?;

        if denom == 0 {
            return Err(Box::new(self.error_at(start, "ratio literal with zero denominator", None)));
        }

        let numer_clean: String = numer_raw.chars().filter(|&c| c != '_').collect();
        let numer_abs: i64 = numer_clean.parse().map_err(|_| {
            Box::new(self.error_at(start, "ratio numerator out of range", None))
        })?;
        let numer = if negative { -numer_abs } else { numer_abs };

        let span = self.span_from(start);
        Ok(Token { kind: TokenKind::Ratio(numer, denom), span })
    }

    fn lex_float_suffix(&mut self) -> Result<Option<FloatSuffix>, Box<Diagnostic>> {
        let suffix_start = self.pos;
        if self.peek() == Some('f') {
            self.advance();
            let w = self.collect_while(|c| c.is_ascii_digit());
            match w.as_str() {
                "32" => return Ok(Some(FloatSuffix::F32)),
                "64" => return Ok(Some(FloatSuffix::F64)),
                _ => {
                    return Err(Box::new(self.error_at(
                        suffix_start,
                        format!("unknown float suffix `f{w}`"),
                        Some(codes::INVALID_NUMERIC_SUFFIX.clone()),
                    )));
                }
            }
        }
        // Any other letter/underscore immediately after is unknown
        if self.peek().is_some_and(|c| c.is_alphabetic() || c == '_') {
            let bad = self.collect_while(|c| c.is_alphanumeric() || c == '_');
            return Err(Box::new(self.error_at(
                suffix_start,
                format!("unknown suffix `{bad}`"),
                Some(codes::INVALID_NUMERIC_SUFFIX.clone()),
            )));
        }
        Ok(None)
    }

    fn lex_int_suffix(&mut self) -> Result<Option<IntSuffix>, Box<Diagnostic>> {
        let suffix_start = self.pos;
        match self.peek() {
            Some('i') => {
                self.advance();
                let w = self.collect_while(|c| c.is_ascii_digit());
                match w.as_str() {
                    "8" => Ok(Some(IntSuffix::I8)),
                    "16" => Ok(Some(IntSuffix::I16)),
                    "32" => Ok(Some(IntSuffix::I32)),
                    "64" => Ok(Some(IntSuffix::I64)),
                    _ => Err(Box::new(self.error_at(
                        suffix_start,
                        format!("unknown integer suffix `i{w}`"),
                        Some(codes::INVALID_NUMERIC_SUFFIX.clone()),
                    ))),
                }
            }
            Some('u') => {
                self.advance();
                let w = self.collect_while(|c| c.is_ascii_digit());
                match w.as_str() {
                    "8" => Ok(Some(IntSuffix::U8)),
                    "16" => Ok(Some(IntSuffix::U16)),
                    "32" => Ok(Some(IntSuffix::U32)),
                    "64" => Ok(Some(IntSuffix::U64)),
                    _ => Err(Box::new(self.error_at(
                        suffix_start,
                        format!("unknown integer suffix `u{w}`"),
                        Some(codes::INVALID_NUMERIC_SUFFIX.clone()),
                    ))),
                }
            }
            // Any other letter/underscore immediately after digits is an unknown suffix
            Some(c) if c.is_alphabetic() || c == '_' => {
                let bad = self.collect_while(|c| c.is_alphanumeric() || c == '_');
                Err(Box::new(self.error_at(
                    suffix_start,
                    format!("unknown suffix `{bad}`"),
                    Some(codes::INVALID_NUMERIC_SUFFIX.clone()),
                )))
            }
            _ => Ok(None),
        }
    }

    // --- string lexing ---

    /// Lex a double-quoted string literal.
    ///
    /// The opening `"` must not yet have been consumed. Returns a
    /// `TokenKind::Str` whose parts are fully resolved: escape sequences
    /// (`\n`, `\t`, `\r`, `\\`, `\"`, `\{`) are expanded, `{{`/`}}` become
    /// literal braces, and `{expr}` spans become `StringPart::Interp`.
    fn lex_string(&mut self) -> Result<Token, Box<Diagnostic>> {
        let start = self.pos;
        self.advance(); // consume opening `"`

        let mut parts: Vec<StringPart> = Vec::new();
        let mut lit = String::new();

        loop {
            match self.peek() {
                None => {
                    return Err(Box::new(self.error_at(
                        start,
                        "unterminated string literal",
                        Some(codes::UNCLOSED_STRING.clone()),
                    )));
                }
                Some('"') => {
                    self.advance(); // consume closing `"`
                    break;
                }
                Some('\\') => {
                    let bs_pos = self.pos;
                    self.advance(); // consume '\'
                    match self.peek() {
                        Some('n') => { self.advance(); lit.push('\n'); }
                        Some('t') => { self.advance(); lit.push('\t'); }
                        Some('r') => { self.advance(); lit.push('\r'); }
                        Some('\\') => { self.advance(); lit.push('\\'); }
                        Some('"') => { self.advance(); lit.push('"'); }
                        Some('{') => { self.advance(); lit.push('{'); }
                        Some(ch) => {
                            let bad_ch = ch;
                            self.advance();
                            return Err(Box::new(self.error_at(
                                bs_pos,
                                format!("unknown escape sequence `\\{bad_ch}`"),
                                Some(codes::INVALID_ESCAPE.clone()),
                            )));
                        }
                        None => {
                            return Err(Box::new(self.error_at(
                                start,
                                "unterminated string literal",
                                Some(codes::UNCLOSED_STRING.clone()),
                            )));
                        }
                    }
                }
                Some('{') => {
                    if self.peek_ahead(1) == Some('{') {
                        // `{{` → literal `{`
                        self.advance();
                        self.advance();
                        lit.push('{');
                    } else {
                        // `{expr}` → interpolation hole
                        flush_lit(&mut lit, &mut parts);
                        self.advance(); // consume '{'
                        let mut expr = String::new();
                        loop {
                            match self.peek() {
                                None => {
                                    return Err(Box::new(self.error_at(
                                        start,
                                        "unterminated string literal",
                                        Some(codes::UNCLOSED_STRING.clone()),
                                    )));
                                }
                                Some('}') => {
                                    self.advance(); // consume '}'
                                    break;
                                }
                                Some(ch) => {
                                    expr.push(ch);
                                    self.advance();
                                }
                            }
                        }
                        parts.push(StringPart::Interp(expr));
                    }
                }
                Some('}') => {
                    if self.peek_ahead(1) == Some('}') {
                        // `}}` → literal `}`
                        self.advance();
                        self.advance();
                        lit.push('}');
                    } else {
                        lit.push('}');
                        self.advance();
                    }
                }
                Some(ch) => {
                    lit.push(ch);
                    self.advance();
                }
            }
        }

        flush_lit(&mut lit, &mut parts);
        let span = self.span_from(start);
        Ok(Token { kind: TokenKind::Str(parts), span })
    }

    // --- keyword lexing ---

    /// Lex a keyword literal.
    ///
    /// The opening `:` must not yet have been consumed. Handles three forms:
    /// - `:name` — plain keyword
    /// - `:ns/name` — namespaced keyword
    /// - `::name` — auto-namespace keyword (resolves to current module)
    fn lex_keyword(&mut self) -> Result<Token, Box<Diagnostic>> {
        let start = self.pos;
        self.advance(); // consume first `:`

        // Check for auto-namespace form `::`
        let auto_ns = if self.peek() == Some(':') {
            self.advance(); // consume second `:`
            true
        } else {
            false
        };

        // Must be followed by at least one symbol-start character
        if !self.peek().is_some_and(is_symbol_start) {
            return Err(Box::new(self.error_at(
                start,
                "keyword must have a name after `:`",
                Some(codes::INVALID_KEYWORD.clone()),
            )));
        }

        let first_name = self.collect_while(is_symbol_cont);

        // Check for namespaced form `:ns/name`
        let (ns, name) = if !auto_ns && self.peek() == Some('/') && self.peek_ahead(1).is_some_and(is_symbol_start) {
            self.advance(); // consume '/'
            let name = self.collect_while(is_symbol_cont);
            (Some(first_name), name)
        } else {
            (None, first_name)
        };

        let span = self.span_from(start);
        Ok(Token { kind: TokenKind::Keyword { ns, name, auto_ns }, span })
    }

    // --- character literal lexing ---

    /// Lex a character literal.
    ///
    /// The opening `\` must not yet have been consumed. Returns a
    /// `TokenKind::Char` for well-formed literals, or an `INVALID_CHAR_LITERAL`
    /// diagnostic otherwise.
    fn lex_char(&mut self) -> Result<Token, Box<Diagnostic>> {
        let start = self.pos;
        self.advance(); // consume `\`

        let first = match self.peek() {
            None => {
                return Err(Box::new(self.error_at(
                    start,
                    "character literal is empty",
                    Some(codes::INVALID_CHAR_LITERAL.clone()),
                )));
            }
            Some(ch) => ch,
        };

        // Unicode escape: `\uXXXX` or `\u{...}`
        if first == 'u' {
            return self.lex_char_unicode(start);
        }

        // Collect a run of letters/digits to distinguish single-char from named forms.
        // Structural delimiters and whitespace terminate the run.
        let word = self.collect_while(|c| !c.is_ascii_whitespace() && !is_structural(c));

        let ch = match word.as_str() {
            "space"   => ' ',
            "newline" => '\n',
            "tab"     => '\t',
            s if s.chars().count() == 1 => s.chars().next().expect("non-empty single char"),
            _ => {
                return Err(Box::new(self.error_at(
                    start,
                    format!("unknown character name `\\{word}`"),
                    Some(codes::INVALID_CHAR_LITERAL.clone()),
                )));
            }
        };

        let span = self.span_from(start);
        Ok(Token { kind: TokenKind::Char(ch), span })
    }

    /// Finish lexing a unicode character escape after `\u` has been peeked.
    ///
    /// Handles both `\uXXXX` (exactly 4 hex digits) and `\u{X...}` (1–6 hex
    /// digits). Rejects code points in the surrogate range (U+D800–U+DFFF).
    fn lex_char_unicode(&mut self, start: usize) -> Result<Token, Box<Diagnostic>> {
        self.advance(); // consume 'u'

        let (code_point, span) = if self.peek() == Some('{') {
            // Braced form: `\u{XXXXXX}`
            self.advance(); // consume '{'
            let hex = self.collect_while(|c| c.is_ascii_hexdigit());
            if hex.is_empty() || hex.len() > 6 {
                return Err(Box::new(self.error_at(
                    start,
                    "unicode escape must have 1–6 hex digits",
                    Some(codes::INVALID_CHAR_LITERAL.clone()),
                )));
            }
            match self.peek() {
                Some('}') => { self.advance(); }
                _ => {
                    return Err(Box::new(self.error_at(
                        start,
                        "missing closing `}` in unicode escape",
                        Some(codes::INVALID_CHAR_LITERAL.clone()),
                    )));
                }
            }
            let cp = u32::from_str_radix(&hex, 16).expect("validated hex digits");
            (cp, self.span_from(start))
        } else {
            // Unbraced form: exactly 4 hex digits `\uXXXX`
            let hex = self.collect_while(|c| c.is_ascii_hexdigit());
            if hex.len() != 4 {
                return Err(Box::new(self.error_at(
                    start,
                    format!(
                        "unicode escape `\\u` must be followed by exactly 4 hex digits, got {}",
                        hex.len()
                    ),
                    Some(codes::INVALID_CHAR_LITERAL.clone()),
                )));
            }
            let cp = u32::from_str_radix(&hex, 16).expect("validated hex digits");
            (cp, self.span_from(start))
        };

        // Reject surrogates (U+D800–U+DFFF)
        if (0xD800..=0xDFFF).contains(&code_point) {
            return Err(Box::new(self.error_at(
                start,
                format!("U+{code_point:04X} is a surrogate and not a valid Unicode scalar value"),
                Some(codes::INVALID_CHAR_LITERAL.clone()),
            )));
        }

        let ch = char::from_u32(code_point).ok_or_else(|| {
            Box::new(self.error_at(
                start,
                format!("U+{code_point:06X} is not a valid Unicode code point"),
                Some(codes::INVALID_CHAR_LITERAL.clone()),
            ))
        })?;

        Ok(Token { kind: TokenKind::Char(ch), span })
    }

    // --- helpers ---

    fn collect_while(&mut self, pred: impl Fn(char) -> bool) -> String {
        let mut s = String::new();
        while let Some(ch) = self.peek() {
            if pred(ch) {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        s
    }

    fn error_at(
        &self,
        start: usize,
        msg: impl Into<String>,
        code: Option<ErrorCode>,
    ) -> Diagnostic {
        let msg = msg.into();
        let span = self.span_from(start);
        let mut d = Diagnostic::new(Severity::Error, msg.clone());
        if let Some(c) = code {
            d.code = Some(c);
        }
        d.push_label(Label::new(span, "here"));
        d
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Move the accumulated literal run into `parts`, leaving `lit` empty.
/// Does nothing if `lit` is already empty (avoids spurious empty `Lit` nodes).
fn flush_lit(lit: &mut String, parts: &mut Vec<StringPart>) {
    if !lit.is_empty() {
        parts.push(StringPart::Lit(std::mem::take(lit)));
    }
}

/// Returns `true` for structural delimiter characters that terminate a
/// character literal name (list/vector/map/set delimiters and `"`).
fn is_structural(c: char) -> bool {
    matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | '"')
}

/// Returns `true` for characters that may start a symbol or keyword name
/// (Appendix D: `symbol-start`).
pub(crate) fn is_symbol_start(c: char) -> bool {
    c.is_alphabetic() || matches!(c, '_' | '?' | '!' | '*' | '+' | '<' | '>' | '=' | '-')
}

/// Returns `true` for characters that may continue a symbol or keyword name
/// (Appendix D: `symbol-cont = symbol-start | digit`).
pub(crate) fn is_symbol_cont(c: char) -> bool {
    is_symbol_start(c) || c.is_ascii_digit()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use nexl_ast::FileId;

    fn lex(src: &str) -> Result<Vec<Token>, Box<Diagnostic>> {
        Lexer::new(src, FileId(0)).tokenize()
    }

    fn lex_one(src: &str) -> TokenKind {
        let tokens = lex(src).expect("expected Ok");
        assert_eq!(tokens.len(), 1, "expected exactly one token, got {}", tokens.len());
        tokens.into_iter().next().unwrap().kind
    }

    // --- float test 1 ---
    #[test]
    fn lex_plain_float() {
        assert_eq!(lex_one("1.25"), TokenKind::Float(1.25, None));
    }

    // --- float test 2 ---
    #[test]
    fn lex_negative_float() {
        assert_eq!(lex_one("-0.5"), TokenKind::Float(-0.5, None));
    }

    // --- float test 3 ---
    #[test]
    fn lex_float_suffix_f32() {
        use nexl_ast::FloatSuffix;
        assert_eq!(lex_one("1.25f32"), TokenKind::Float(1.25, Some(FloatSuffix::F32)));
    }

    // --- float test 4 ---
    #[test]
    fn lex_float_suffix_f64() {
        use nexl_ast::FloatSuffix;
        assert_eq!(lex_one("1.25f64"), TokenKind::Float(1.25, Some(FloatSuffix::F64)));
    }

    // --- float test 5 ---
    #[test]
    fn lex_float_scientific() {
        assert_eq!(lex_one("1.5e10"), TokenKind::Float(1.5e10, None));
    }

    // --- float test 6 ---
    #[test]
    fn lex_float_scientific_uppercase_e() {
        assert_eq!(lex_one("2.0E3"), TokenKind::Float(2000.0, None));
    }

    // --- float test 7 ---
    #[test]
    fn lex_float_scientific_negative_exp() {
        assert_eq!(lex_one("1.0e-2"), TokenKind::Float(0.01, None));
    }

    // --- float test 8 ---
    #[test]
    fn lex_float_exponent_only() {
        // Grammar's second float form: digits exponent (no decimal point)
        assert_eq!(lex_one("1e5"), TokenKind::Float(1e5, None));
    }

    // --- float test 9 ---
    #[test]
    fn lex_float_span_correct() {
        // "  3.14  " — float starts at byte 2, length 4
        let tokens = lex("  3.14  ").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].span.start, 2);
        assert_eq!(tokens[0].span.len, 4);
    }

    // --- float test 10 ---
    #[test]
    fn lex_float_invalid_suffix() {
        // "3.14f16" — f16 is not a valid float suffix
        let err = lex("3.14f16").unwrap_err();
        assert_eq!(err.code, Some(codes::INVALID_NUMERIC_SUFFIX.clone()));
        // label points at the bad suffix, not the whole token
        assert_eq!(err.labels[0].span.start, 4); // 'f' is at byte 4
    }

    // --- float test 11 ---
    #[test]
    fn lex_mixed_int_and_float() {
        let tokens = lex("42 1.25").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Int(42, None));
        assert_eq!(tokens[1].kind, TokenKind::Float(1.25, None));
    }

    // --- test 1 ---
    #[test]
    fn lex_plain_int() {
        assert_eq!(lex_one("42"), TokenKind::Int(42, None));
    }

    // --- test 3 ---
    #[test]
    fn lex_negative_int() {
        assert_eq!(lex_one("-7"), TokenKind::Int(-7, None));
    }

    // --- test 4 ---
    #[test]
    fn lex_underscore_separator() {
        assert_eq!(lex_one("1_000_000"), TokenKind::Int(1_000_000, None));
    }

    // --- test 5 ---
    #[test]
    fn lex_hex_int() {
        assert_eq!(lex_one("0xFF"), TokenKind::Int(255, None));
    }

    // --- test 6 ---
    #[test]
    fn lex_binary_int() {
        assert_eq!(lex_one("0b1010"), TokenKind::Int(10, None));
    }

    // --- test 7 ---
    #[test]
    fn lex_octal_int() {
        assert_eq!(lex_one("0o17"), TokenKind::Int(15, None));
    }

    // --- test 16 ---
    #[test]
    fn lex_multiple_ints() {
        let tokens = lex("42 -7 255u8").unwrap();
        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].kind, TokenKind::Int(42, None));
        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.len, 2);
        assert_eq!(tokens[1].kind, TokenKind::Int(-7, None));
        assert_eq!(tokens[1].span.start, 3);
        assert_eq!(tokens[1].span.len, 2);
        assert_eq!(tokens[2].kind, TokenKind::Int(255, Some(IntSuffix::U8)));
        assert_eq!(tokens[2].span.start, 6);
        assert_eq!(tokens[2].span.len, 5);
    }

    // --- test 17 ---
    #[test]
    fn lex_invalid_suffix_is_error() {
        let err = lex("42x").unwrap_err();
        assert_eq!(err.code, Some(codes::INVALID_NUMERIC_SUFFIX.clone()));
        // label points at the bad suffix, not the whole token
        assert_eq!(err.labels[0].span.start, 2); // 'x' is at byte 2
    }

    // --- tests 8-15: all suffixes ---
    #[test]
    fn lex_suffix_i8() {
        assert_eq!(lex_one("42i8"), TokenKind::Int(42, Some(IntSuffix::I8)));
    }

    #[test]
    fn lex_suffix_i16() {
        assert_eq!(lex_one("100i16"), TokenKind::Int(100, Some(IntSuffix::I16)));
    }

    #[test]
    fn lex_suffix_i32() {
        assert_eq!(lex_one("42i32"), TokenKind::Int(42, Some(IntSuffix::I32)));
    }

    #[test]
    fn lex_suffix_i64() {
        assert_eq!(lex_one("42i64"), TokenKind::Int(42, Some(IntSuffix::I64)));
    }

    #[test]
    fn lex_suffix_u8() {
        assert_eq!(lex_one("255u8"), TokenKind::Int(255, Some(IntSuffix::U8)));
    }

    #[test]
    fn lex_suffix_u16() {
        assert_eq!(lex_one("1000u16"), TokenKind::Int(1000, Some(IntSuffix::U16)));
    }

    #[test]
    fn lex_suffix_u32() {
        assert_eq!(lex_one("42u32"), TokenKind::Int(42, Some(IntSuffix::U32)));
    }

    #[test]
    fn lex_suffix_u64() {
        assert_eq!(lex_one("42u64"), TokenKind::Int(42, Some(IntSuffix::U64)));
    }

    // --- test 2 ---
    #[test]
    fn lex_int_span_correct() {
        // "  42  " — the integer starts at byte 2, length 2
        let tokens = lex("  42  ").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].span.start, 2);
        assert_eq!(tokens[0].span.len, 2);
    }

    // --- ratio test 1 ---
    #[test]
    fn lex_ratio_basic() {
        assert_eq!(lex_one("3/4"), TokenKind::Ratio(3, 4));
    }

    // --- ratio test 2 ---
    #[test]
    fn lex_ratio_one_over_three() {
        assert_eq!(lex_one("1/3"), TokenKind::Ratio(1, 3));
    }

    // --- ratio test 3 ---
    #[test]
    fn lex_ratio_reducible_stored_raw() {
        // 6/4 reduces to 3/2, but the lexer stores raw values; reduction is reader-level
        assert_eq!(lex_one("6/4"), TokenKind::Ratio(6, 4));
    }

    // --- ratio test 4 ---
    #[test]
    fn lex_ratio_negative_numerator() {
        assert_eq!(lex_one("-3/4"), TokenKind::Ratio(-3, 4));
    }

    // --- ratio test 5 ---
    #[test]
    fn lex_ratio_zero_numerator() {
        assert_eq!(lex_one("0/1"), TokenKind::Ratio(0, 1));
    }

    // --- ratio test 6 ---
    #[test]
    fn lex_ratio_zero_denominator_is_error() {
        let err = lex("3/0").unwrap_err();
        assert!(
            err.message.contains("zero denominator"),
            "expected 'zero denominator' in message, got: {}",
            err.message
        );
    }

    // --- ratio test 7 ---
    #[test]
    fn lex_ratio_span_correct() {
        // "  1/3  " — ratio starts at byte 2, length 3
        let tokens = lex("  1/3  ").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].span.start, 2);
        assert_eq!(tokens[0].span.len, 3);
    }

    // --- ratio test 8 ---
    #[test]
    fn lex_ratio_adjacent_tokens() {
        let tokens = lex("3/4 42").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Ratio(3, 4));
        assert_eq!(tokens[1].kind, TokenKind::Int(42, None));
    }

    // --- ratio test 9 ---
    #[test]
    fn lex_ratio_underscore_in_numerator() {
        assert_eq!(lex_one("1_000/3"), TokenKind::Ratio(1000, 3));
    }

    // Small helpers to reduce boilerplate in string tests.
    fn lit(s: &str) -> StringPart { StringPart::Lit(s.to_string()) }
    fn interp(s: &str) -> StringPart { StringPart::Interp(s.to_string()) }

    // --- keyword test 8 ---
    #[test]
    fn lex_keyword_bare_colon_is_error() {
        // `:` at EOF — grammar requires at least one symbol-start char
        let err = lex(":").unwrap_err();
        assert_eq!(err.code, Some(codes::INVALID_KEYWORD.clone()));
    }

    // --- keyword test 7 ---
    #[test]
    fn lex_keyword_adjacent_tokens() {
        // `:foo 42` — keyword followed by an integer
        let tokens = lex(":foo 42").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(
            tokens[0].kind,
            TokenKind::Keyword { ns: None, name: "foo".into(), auto_ns: false },
        );
        assert_eq!(tokens[1].kind, TokenKind::Int(42, None));
    }

    // --- keyword test 6 ---
    #[test]
    fn lex_keyword_span_correct() {
        // "  :foo  " — keyword starts at byte 2, `:foo` is 4 bytes
        let tokens = lex("  :foo  ").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].span.start, 2);
        assert_eq!(tokens[0].span.len, 4); // `:foo` = 4 bytes
    }

    // --- keyword test 4 ---
    #[test]
    fn lex_keyword_namespaced() {
        // `:http/ok` — namespaced keyword (spec §2.6)
        assert_eq!(
            lex_one(":http/ok"),
            TokenKind::Keyword { ns: Some("http".into()), name: "ok".into(), auto_ns: false },
        );
    }

    // --- keyword test 5 ---
    #[test]
    fn lex_keyword_auto_ns() {
        // `::local-alias` — auto-namespace form (spec §2.6)
        assert_eq!(
            lex_one("::local-alias"),
            TokenKind::Keyword { ns: None, name: "local-alias".into(), auto_ns: true },
        );
    }

    // --- keyword test 2 ---
    #[test]
    fn lex_keyword_with_hyphen() {
        // `:my-key` — hyphen is a valid symbol-start char (Appendix D)
        assert_eq!(
            lex_one(":my-key"),
            TokenKind::Keyword { ns: None, name: "my-key".into(), auto_ns: false },
        );
    }

    // --- keyword test 3 ---
    #[test]
    fn lex_keyword_with_special_chars() {
        // `:valid?` — `?` is a valid symbol-start char (Appendix D)
        assert_eq!(
            lex_one(":valid?"),
            TokenKind::Keyword { ns: None, name: "valid?".into(), auto_ns: false },
        );
    }

    // --- keyword test 1 ---
    #[test]
    fn lex_keyword_simple() {
        // `:status` — plain keyword (spec §2.6)
        assert_eq!(
            lex_one(":status"),
            TokenKind::Keyword { ns: None, name: "status".into(), auto_ns: false },
        );
    }

    // --- char test 3 ---
    #[test]
    fn lex_char_named_space() {
        // `\space` — named form (spec §2.5)
        assert_eq!(lex_one("\\space"), TokenKind::Char(' '));
    }

    // --- char test 4 ---
    #[test]
    fn lex_char_named_newline() {
        // `\newline` — named form (spec §2.5)
        assert_eq!(lex_one("\\newline"), TokenKind::Char('\n'));
    }

    // --- char test 12 ---
    #[test]
    fn lex_char_eof_after_backslash() {
        // `\` at EOF — empty character literal
        let err = lex("\\").unwrap_err();
        assert_eq!(err.code, Some(codes::INVALID_CHAR_LITERAL.clone()));
    }

    // --- char test 11 ---
    #[test]
    fn lex_char_surrogate_is_error() {
        // `\uD800` — start of surrogate range, not a valid scalar value (spec §2.5)
        let err = lex("\\uD800").unwrap_err();
        assert_eq!(err.code, Some(codes::INVALID_CHAR_LITERAL.clone()));
        assert!(
            err.message.contains("surrogate"),
            "expected 'surrogate' in message, got: {}",
            err.message
        );
    }

    // --- char test 10 ---
    #[test]
    fn lex_char_adjacent_tokens() {
        // `\a 42` — char literal followed by an integer
        let tokens = lex("\\a 42").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Char('a'));
        assert_eq!(tokens[1].kind, TokenKind::Int(42, None));
    }

    // --- char test 9 ---
    #[test]
    fn lex_char_span_correct() {
        // "  \newline  " — char starts at byte 2, `\newline` is 8 bytes (\ + 7 chars)
        let tokens = lex("  \\newline  ").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].span.start, 2);
        assert_eq!(tokens[0].span.len, 8); // '\' + "newline" = 8 bytes
    }

    // --- char test 7 ---
    #[test]
    fn lex_char_unicode_braced_short() {
        // `\u{41}` — braced form with fewer than 4 digits (spec §2.5 1–6 hex digits)
        assert_eq!(lex_one("\\u{41}"), TokenKind::Char('A'));
    }

    // --- char test 8 ---
    #[test]
    fn lex_char_unicode_braced_full() {
        // `\u{1F600}` — spec §2.5 example: emoji outside BMP
        assert_eq!(lex_one("\\u{1F600}"), TokenKind::Char('😀'));
    }

    // --- char test 6 ---
    #[test]
    fn lex_char_unicode_4hex() {
        // `\u0041` — BMP form: exactly 4 hex digits (spec §2.5)
        assert_eq!(lex_one("\\u0041"), TokenKind::Char('A'));
    }

    // --- char test 5 ---
    #[test]
    fn lex_char_named_tab() {
        // `\tab` — named form (spec §2.5)
        assert_eq!(lex_one("\\tab"), TokenKind::Char('\t'));
    }

    // --- char test 2 ---
    #[test]
    fn lex_char_single_digit() {
        // `\5` — single non-alpha character (still a valid 1-char literal)
        assert_eq!(lex_one("\\5"), TokenKind::Char('5'));
    }

    // --- char test 1 ---
    #[test]
    fn lex_char_single_ascii() {
        // `\a` — single character after `\` (spec §2.5 first example)
        assert_eq!(lex_one("\\a"), TokenKind::Char('a'));
    }

    // --- string test 1 ---
    #[test]
    fn lex_plain_string() {
        assert_eq!(lex_one("\"hello\""), TokenKind::Str(vec![lit("hello")]));
    }

    // --- string test 2 ---
    #[test]
    fn lex_empty_string() {
        assert_eq!(lex_one("\"\""), TokenKind::Str(vec![]));
    }

    // --- string test 3 ---
    #[test]
    fn lex_string_with_interpolation() {
        // {name} is split into an Interp part; surrounding text becomes Lit (spec §2.4)
        assert_eq!(
            lex_one("\"hello {name}!\""),
            TokenKind::Str(vec![lit("hello "), interp("name"), lit("!")]),
        );
    }

    // --- string test 4 ---
    #[test]
    fn lex_string_multiple_interpolations() {
        assert_eq!(
            lex_one("\"{a} and {b}\""),
            TokenKind::Str(vec![interp("a"), lit(" and "), interp("b")]),
        );
    }

    // --- string test 5 ---
    #[test]
    fn lex_string_span_covers_quotes() {
        // "  \"hello\"  " — string starts at byte 2 (the opening `"`), length 7
        // The span covers both `"` delimiters: `"hello"` = 7 bytes
        let tokens = lex("  \"hello\"  ").unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].span.start, 2);
        assert_eq!(tokens[0].span.len, 7); // `"hello"` = 7 bytes including both `"`
    }

    // --- string test 6 ---
    #[test]
    fn lex_string_adjacent_to_int() {
        let tokens = lex("\"hi\" 42").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Str(vec![lit("hi")]));
        assert_eq!(tokens[1].kind, TokenKind::Int(42, None));
    }

    // --- string test 7 ---
    #[test]
    fn lex_unclosed_string_is_error() {
        let err = lex("\"hello").unwrap_err();
        assert_eq!(err.code, Some(codes::UNCLOSED_STRING.clone()));
        // label points at the opening `"` (byte 0)
        assert_eq!(err.labels[0].span.start, 0);
    }

    // --- string test 8 ---
    #[test]
    fn lex_string_escaped_quote_does_not_end_string() {
        // `"a\"b"` — `\"` is resolved to `"` and must NOT terminate the string.
        // After escape processing the content is the 3-char literal `a"b`.
        assert_eq!(lex_one("\"a\\\"b\""), TokenKind::Str(vec![lit("a\"b")]));
    }

    // --- escape test 1 ---
    #[test]
    fn escape_newline() {
        // `"line1\nline2"` — spec §2.4: \n resolves to actual newline character
        assert_eq!(lex_one("\"line1\\nline2\""), TokenKind::Str(vec![lit("line1\nline2")]));
    }

    // --- escape test 2 ---
    #[test]
    fn escape_tab() {
        // `"a\tb"` — spec §2.4: \t resolves to actual tab character
        assert_eq!(lex_one("\"a\\tb\""), TokenKind::Str(vec![lit("a\tb")]));
    }

    // --- escape test 3 ---
    #[test]
    fn escape_carriage_return() {
        // `"a\rb"` — spec §2.4: \r resolves to carriage return
        assert_eq!(lex_one("\"a\\rb\""), TokenKind::Str(vec![lit("a\rb")]));
    }

    // --- escape test 4 ---
    #[test]
    fn escape_backslash() {
        // `"a\\b"` — spec §2.4: \\ resolves to a single backslash
        assert_eq!(lex_one("\"a\\\\b\""), TokenKind::Str(vec![lit("a\\b")]));
    }

    // --- escape test 5 ---
    // (covered by string test 8 above — `\"` resolves without ending the string)

    // --- escape test 6 ---
    #[test]
    fn escape_brace() {
        // `"\{name}"` — \{ is a literal `{`; the span is NOT treated as interpolation
        assert_eq!(
            lex_one("\"\\{name}\""),
            TokenKind::Str(vec![lit("{name}")]),
        );
    }

    // --- escape test 7 ---
    #[test]
    fn double_brace() {
        // `"{{a}}"` — {{ → literal `{`, }} → literal `}` (spec §2.4 example)
        assert_eq!(lex_one("\"{{a}}\""), TokenKind::Str(vec![lit("{a}")]));
    }

    // --- escape test 8 ---
    #[test]
    fn double_brace_mixed_with_interp() {
        // `"{{x}} {name}"` — literal `{x}` then interpolation `name` (spec §2.4)
        assert_eq!(
            lex_one("\"{{x}} {name}\""),
            TokenKind::Str(vec![lit("{x} "), interp("name")]),
        );
    }

    // --- escape test 9 ---
    #[test]
    fn invalid_escape_is_error() {
        // `"\q"` — unrecognized escape must produce NXL-L0002
        let err = lex("\"\\q\"").unwrap_err();
        assert_eq!(err.code, Some(codes::INVALID_ESCAPE.clone()));
    }

    // --- escape test 10 ---
    #[test]
    fn invalid_escape_span_at_backslash() {
        // `"a\qb"` — the error label must point at the `\` (byte 2: `"` at 0, `a` at 1)
        let err = lex("\"a\\qb\"").unwrap_err();
        assert_eq!(err.code, Some(codes::INVALID_ESCAPE.clone()));
        assert_eq!(err.labels[0].span.start, 2); // `\` is at byte 2
    }
}
