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
    /// String literal. Content is the raw characters between the opening and closing
    /// `"` delimiters, with no escape processing applied (escape sequences are
    /// resolved in a later lexer pass). Interpolation spans `{...}` are preserved
    /// as-is for resolution by a later compiler pass.
    Str(String),
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
    /// The opening `"` must not yet have been consumed. Content is collected
    /// verbatim — escape sequences are left unprocessed; a `\` followed by any
    /// character is consumed as a two-character unit so that `\"` does not
    /// prematurely end the string. Interpolation spans `{...}` are preserved
    /// as-is for a later compiler pass.
    fn lex_string(&mut self) -> Result<Token, Box<Diagnostic>> {
        let start = self.pos;
        self.advance(); // consume opening `"`

        let mut content = String::new();
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
                    // Consume the backslash and whatever follows without
                    // interpreting the escape — that is task 2.
                    content.push(self.advance().unwrap());
                    if let Some(escaped) = self.advance() {
                        content.push(escaped);
                    }
                }
                Some(ch) => {
                    content.push(ch);
                    self.advance();
                }
            }
        }

        let span = self.span_from(start);
        Ok(Token { kind: TokenKind::Str(content), span })
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
        assert_eq!(lex_one("3.14"), TokenKind::Float(3.14, None));
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
        assert_eq!(lex_one("3.14f32"), TokenKind::Float(3.14, Some(FloatSuffix::F32)));
    }

    // --- float test 4 ---
    #[test]
    fn lex_float_suffix_f64() {
        use nexl_ast::FloatSuffix;
        assert_eq!(lex_one("3.14f64"), TokenKind::Float(3.14, Some(FloatSuffix::F64)));
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
        let tokens = lex("42 3.14").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Int(42, None));
        assert_eq!(tokens[1].kind, TokenKind::Float(3.14, None));
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

    // --- string test 1 ---
    #[test]
    fn lex_plain_string() {
        assert_eq!(lex_one("\"hello\""), TokenKind::Str("hello".to_string()));
    }

    // --- string test 2 ---
    #[test]
    fn lex_empty_string() {
        assert_eq!(lex_one("\"\""), TokenKind::Str("".to_string()));
    }

    // --- string test 3 ---
    #[test]
    fn lex_string_with_interpolation() {
        // {name} is preserved as-is for a later compiler pass (spec §2.4)
        assert_eq!(
            lex_one("\"hello {name}!\""),
            TokenKind::Str("hello {name}!".to_string()),
        );
    }

    // --- string test 4 ---
    #[test]
    fn lex_string_multiple_interpolations() {
        assert_eq!(
            lex_one("\"{a} and {b}\""),
            TokenKind::Str("{a} and {b}".to_string()),
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
        assert_eq!(tokens[0].kind, TokenKind::Str("hi".to_string()));
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
    fn lex_string_backslash_skipped_for_boundary() {
        // `"a\"b"` — the `\"` must NOT end the string; the string ends at the
        // final `"`. Raw content stored as-is; escape resolution is task 2.
        assert_eq!(lex_one("\"a\\\"b\""), TokenKind::Str("a\\\"b".to_string()));
    }
}
