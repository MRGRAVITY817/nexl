use nexl_ast::FileId;
use nexl_errors::codes;
use nexl_reader::Lexer;

#[test]
fn unclosed_string_literal_gives_helpful_message() {
    let src = "\"hello";
    let err = Lexer::new(src, FileId(0)).tokenize().expect_err("should error");

    assert_eq!(err.code, Some(codes::UNCLOSED_STRING));
    assert_eq!(err.message, "unterminated string literal");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 0);
    assert_eq!(err.labels[0].message, "string starts here");
    assert_eq!(
        err.help.as_deref(),
        Some("add a closing '\"' to terminate the string")
    );
}

#[test]
fn invalid_escape_sequence_gives_actionable_help() {
    let src = "\"\\q\"";
    let err = Lexer::new(src, FileId(0)).tokenize().expect_err("should error");

    assert_eq!(err.code, Some(codes::INVALID_ESCAPE));
    assert_eq!(err.message, "unknown escape sequence `\\q`");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 1);
    assert_eq!(err.labels[0].message, "invalid escape here");
    assert_eq!(
        err.help.as_deref(),
        Some("valid escapes: \\\\n, \\\\t, \\\\r, \\\\\\\\, \\\\\\\" , \\\\{")
    );
}

#[test]
fn unknown_numeric_suffix_points_at_suffix() {
    let src = "42x";
    let err = Lexer::new(src, FileId(0)).tokenize().expect_err("should error");

    assert_eq!(err.code, Some(codes::INVALID_NUMERIC_SUFFIX));
    assert_eq!(err.message, "unknown suffix `x`");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 2);
    assert_eq!(err.labels[0].message, "numeric suffix starts here");
    assert_eq!(
        err.help.as_deref(),
        Some("valid integer suffixes: i8, i16, i32, i64, u8, u16, u32, u64; or omit the suffix")
    );
}

#[test]
fn empty_character_literal_is_diagnostic() {
    let src = "\\";
    let err = Lexer::new(src, FileId(0)).tokenize().expect_err("should error");

    assert_eq!(err.code, Some(codes::INVALID_CHAR_LITERAL));
    assert_eq!(err.message, "character literal is empty");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 0);
    assert_eq!(err.labels[0].message, "character starts here");
    assert_eq!(
        err.help.as_deref(),
        Some("add a character after the backslash, e.g. `\\a` or `\\u{1F600}`")
    );
}

#[test]
fn ratio_zero_denominator_is_rejected() {
    let src = "1/0";
    let err = Lexer::new(src, FileId(0)).tokenize().expect_err("should error");

    assert_eq!(err.code, None);
    assert_eq!(err.message, "ratio literal with zero denominator");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 0);
    assert_eq!(err.labels[0].message, "ratio starts here");
    assert_eq!(
        err.help.as_deref(),
        Some("the denominator of a ratio must be non-zero")
    );
}

#[test]
fn unexpected_character_reports_offending_byte() {
    let src = "%";
    let err = Lexer::new(src, FileId(0)).tokenize().expect_err("should error");

    assert_eq!(err.code, None);
    assert_eq!(err.message, "unexpected character `%`");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 0);
    assert_eq!(err.labels[0].message, "unexpected here");
    assert_eq!(
        err.help.as_deref(),
        Some("this character cannot start any token; remove it or use a valid token")
    );
}

#[test]
fn unmatched_closing_delimiter_is_actionable() {
    let src = ")";
    let err = nexl_reader::read(src, FileId(0)).expect_err("should error");

    assert_eq!(err.code, Some(codes::UNMATCHED_DELIMITER));
    assert_eq!(err.message, "unexpected `)` — no matching opener");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 0);
    assert_eq!(err.labels[0].message, "unmatched closing delimiter");
    assert_eq!(
        err.help.as_deref(),
        Some("remove this `)` or add a matching opening `(` earlier")
    );
}

#[test]
fn unclosed_list_points_back_to_opener() {
    let src = "(1 2";
    let err = nexl_reader::read(src, FileId(0)).expect_err("should error");

    assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
    assert_eq!(err.message, "unclosed `(` — expected matching closer before end of file");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 0);
    assert_eq!(err.labels[0].message, "list opened here");
    assert_eq!(
        err.help.as_deref(),
        Some("add a closing `)` before end of file")
    );
}

#[test]
fn unclosed_set_reports_opening_location() {
    let src = "#{1 2";
    let err = nexl_reader::read(src, FileId(0)).expect_err("should error");

    assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
    assert_eq!(err.message, "unclosed `#{` — expected matching closer before end of file");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 0);
    assert_eq!(err.labels[0].message, "set opened here");
    assert_eq!(
        err.help.as_deref(),
        Some("add a closing `}` before end of file")
    );
}

#[test]
fn unclosed_map_reports_opening_location() {
    let src = "{:a 1";
    let err = nexl_reader::read(src, FileId(0)).expect_err("should error");

    assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
    assert_eq!(err.message, "unclosed `{` — expected matching closer before end of file");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 0);
    assert_eq!(err.labels[0].message, "map opened here");
    assert_eq!(
        err.help.as_deref(),
        Some("add a closing `}` before end of file")
    );
}

#[test]
fn odd_map_forms_points_to_dangling_key() {
    let src = "{:a}";
    let err = nexl_reader::read(src, FileId(0)).expect_err("should error");

    assert_eq!(err.code, Some(codes::ODD_MAP_FORMS));
    assert_eq!(
        err.message,
        "map literal has an odd number of forms — every key must have a value"
    );
    assert_eq!(err.labels.len(), 2);
    assert_eq!(err.labels[0].message, "this map");
    assert_eq!(err.labels[1].message, "this key has no matching value");
    assert_eq!(
        err.help.as_deref(),
        Some("add a value for the last key, or remove the unpaired key")
    );
}

#[test]
fn discard_at_eof_suggests_follow_up() {
    let src = "#_";
    let err = nexl_reader::read(src, FileId(0)).expect_err("should error");

    assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
    assert_eq!(err.message, "expected a form after `#_`, found end of file");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 0);
    assert_eq!(err.labels[0].message, "this `#_` expects a following form");
    assert_eq!(
        err.help.as_deref(),
        Some("add the form to discard, or remove the `#_`")
    );
}

#[test]
fn quote_at_eof_points_at_prefix() {
    let src = "'";
    let err = nexl_reader::read(src, FileId(0)).expect_err("should error");

    assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
    assert_eq!(err.message, "expected a form after `'`, found end of file");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 0);
    assert_eq!(err.labels[0].message, "this `'` expects a following form");
    assert_eq!(
        err.help.as_deref(),
        Some("add the quoted form, e.g. `'x`, or remove the `'`")
    );
}

#[test]
fn deref_at_eof_points_at_prefix() {
    let src = "@";
    let err = nexl_reader::read(src, FileId(0)).expect_err("should error");

    assert_eq!(err.code, Some(codes::UNCLOSED_DELIMITER));
    assert_eq!(err.message, "expected a form after `@`, found end of file");
    assert_eq!(err.labels.len(), 1);
    assert_eq!(err.labels[0].span.start, 0);
    assert_eq!(err.labels[0].message, "this `@` expects a following form");
    assert_eq!(
        err.help.as_deref(),
        Some("add the form to dereference, e.g. `@value`, or remove the `@`")
    );
}
