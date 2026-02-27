---
model: opus
---

Implement a feature by reading the relevant spec section and translating it to Rust.

## Arguments
$ARGUMENTS — What to implement (e.g. "integer literal lexing from §2", "character escape sequences", "span type for AST nodes").

## Instructions

1. Parse the argument to identify which spec section(s) are relevant.
2. Read the relevant section(s) of `nexl-spec.md` using line ranges from the index in `CLAUDE.md`. Also check Appendix D (formal grammar) if the feature involves syntax.
3. Check `decisions/` for any ADRs related to this feature.
4. Read the existing code in the target crate to understand current types, patterns, and conventions.
5. Present a brief implementation plan: which files to create/modify and what types and
   functions are needed. Wait for approval before proceeding.

6. **Write the test plan.**
   Before writing any test code, print a numbered list of every test case you intend
   to write: name, what it exercises, and which spec example or ADR consequence drives
   it (see `next-task.md` for the format). **Stop and wait for user approval.**

7. **Beck loop — one test at a time.**
   For each test in the approved plan, in order:
   a. **Red**: Write exactly that one test. Add minimum stubs to compile. Run
      `cargo test -p nexl-{crate}` and confirm it fails.
   b. **Green**: Write the minimum implementation to pass this test. Confirm it passes.
   c. **Refactor**: Tidy up. Tests stay green. Move to the next test.

8. After all tests pass: run `cargo clippy -p nexl-{crate}` and fix all warnings.
9. Update `docs/todo-m{N}.md` if this completes a checklist item.
