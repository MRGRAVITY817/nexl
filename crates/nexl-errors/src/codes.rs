//! Pre-defined error codes for each compiler phase.
//!
//! Lexer codes: `NXL-L0001` – `NXL-L0999`
//! Reader codes: `NXL-R0001` – `NXL-R0999`

use crate::{ErrorCode, ErrorPhase};

// --- Lexer errors (NXL-L…) ---

/// String literal was opened but never closed before end-of-file.
pub const UNCLOSED_STRING: ErrorCode = ErrorCode {
    phase: ErrorPhase::Lexer,
    number: 1,
};

/// Unrecognised or malformed escape sequence inside a string or character literal.
pub const INVALID_ESCAPE: ErrorCode = ErrorCode {
    phase: ErrorPhase::Lexer,
    number: 2,
};

/// Character literal is malformed (empty, multi-char, invalid Unicode code point, etc.).
pub const INVALID_CHAR_LITERAL: ErrorCode = ErrorCode {
    phase: ErrorPhase::Lexer,
    number: 3,
};

/// Integer or float literal suffix is unrecognised (e.g. `42x`).
pub const INVALID_NUMERIC_SUFFIX: ErrorCode = ErrorCode {
    phase: ErrorPhase::Lexer,
    number: 4,
};

/// Suffixed integer literal is out of range for its type (e.g. `256u8`).
pub const LITERAL_OUT_OF_RANGE: ErrorCode = ErrorCode {
    phase: ErrorPhase::Lexer,
    number: 5,
};

/// Keyword is malformed (bare `:`, unknown namespace form, etc.).
pub const INVALID_KEYWORD: ErrorCode = ErrorCode {
    phase: ErrorPhase::Lexer,
    number: 6,
};

// --- Reader errors (NXL-R…) ---

/// A closing delimiter (`)`, `]`, `}`) has no matching opener.
pub const UNMATCHED_DELIMITER: ErrorCode = ErrorCode {
    phase: ErrorPhase::Reader,
    number: 1,
};

/// End-of-file was reached while a delimiter was still open.
pub const UNCLOSED_DELIMITER: ErrorCode = ErrorCode {
    phase: ErrorPhase::Reader,
    number: 2,
};

/// A map literal has an odd number of forms (keys without values).
pub const ODD_MAP_FORMS: ErrorCode = ErrorCode {
    phase: ErrorPhase::Reader,
    number: 3,
};
