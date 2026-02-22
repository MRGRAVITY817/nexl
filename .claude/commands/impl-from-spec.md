Implement a feature by reading the relevant spec section and translating it to Rust.

## Arguments
$ARGUMENTS — What to implement (e.g. "integer literal lexing from §2", "character escape sequences", "span type for AST nodes").

## Instructions

1. Parse the argument to identify which spec section(s) are relevant.
2. Read the relevant section(s) of `nexl-spec.md` using line ranges from the index in `CLAUDE.md`. Also check Appendix D (formal grammar) if the feature involves syntax.
3. Check `decisions/` for any ADRs related to this feature.
4. Read the existing code in the target crate to understand current types, patterns, and conventions.
5. Present a brief implementation plan: which files to create/modify, what types and functions are needed, and how tests will be structured. Wait for approval before proceeding.
6. Implement the feature:
   - Follow existing code patterns and the style rules in `CLAUDE.md`.
   - Add doc comments on all public items.
   - Write unit tests covering: normal cases, edge cases, and error cases from the spec.
   - If the spec includes code examples, turn them into test cases.
7. Run `cargo test -p nexl-{crate}` and `cargo clippy -p nexl-{crate}`. Fix any issues.
8. Update `docs/todo-m{N}.md` if this completes a checklist item.
