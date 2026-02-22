Pick the next task from the current milestone's todo list and implement it.

## Instructions

1. Read `docs/current-milestone.md` to identify the active milestone number (N).
2. Read `docs/todo-m{N}.md` and find the first unchecked (`- [ ]`) item that is not in the "Blocked" section.
3. Announce which task you're picking up and why it's the logical next step (consider dependency order: types before functions that use them, etc.).
4. Read the relevant section(s) of `nexl-spec.md` for the feature being implemented. Use the section index in `CLAUDE.md` to find line ranges — do NOT read the entire spec.
5. If the task involves a design decision already captured in `decisions/`, read the relevant ADR.
6. Implement the feature:
   - Write the Rust code in the appropriate crate under `crates/`.
   - Follow the code style rules in `CLAUDE.md`.
   - Add unit tests in a `#[cfg(test)] mod tests` block in the same file.
7. Run `cargo test -p nexl-{crate}` to verify. Fix any failures.
8. Run `cargo clippy -p nexl-{crate}` and fix warnings.
9. Update `docs/todo-m{N}.md`: check off the completed item(s).
10. If you discovered new tasks or blockers during implementation, add them to the todo under the appropriate section.
