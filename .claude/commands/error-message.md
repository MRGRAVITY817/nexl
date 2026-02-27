---
model: opus
---

Design and implement a compiler error message following Principle 6: "The compiler is a conversational partner."

## Arguments
$ARGUMENTS — The error scenario (e.g. "unterminated string literal", "cross-type arithmetic Int + Float", "unknown escape sequence in character literal").

## Instructions

1. Read the relevant spec section to understand the exact rules that produce this error.
2. Check `decisions/` for any ADRs related to this error (e.g. ADR-006 for cross-type arithmetic).
3. Read existing error types in `crates/nexl-errors/src/` to understand current patterns.
4. Design the error message with these qualities:
   - **Specific**: Say exactly what went wrong, not just "syntax error".
   - **Localized**: Point to the exact span in the source.
   - **Actionable**: Suggest what the user probably meant or how to fix it.
   - **Contextual**: If relevant, explain *why* this is an error (e.g. "Nexl requires explicit conversion between Int and Float").
5. Present the error message design as the user would see it (with miette-style formatting) before implementing.
6. Implement:
   - Add the error variant to the appropriate error enum in `nexl-errors`.
   - Use `#[error(...)]` (thiserror) for the message and `#[diagnostic(...)]` (miette) for the rich rendering.
   - Add `#[label(...)]` annotations for source spans.
   - Add `#[help(...)]` for fix suggestions.
7. Write a test that triggers the error and verifies the message content.
8. Run `cargo test -p nexl-errors` (and the crate that produces this error) to verify.
