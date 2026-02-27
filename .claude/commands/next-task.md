---
model: opus
---

Pick the next task from the current milestone's todo list and implement it,
following the Beck Augmented Coding loop (test plan → one test at a time → human
stays in the loop).

## Instructions

1. Read `docs/current-milestone.md` to identify the active milestone number (N).
2. Read `docs/todo-m{N}.md` and find the first unchecked (`- [ ]`) item that is not
   in the "Blocked" section.
3. Announce which task you're picking up and why it's the logical next step (consider
   dependency order: types before functions that use them, etc.).
4. Read the relevant section(s) of `nexl-spec.md` for the feature being implemented.
   Use the section index in `CLAUDE.md` to find line ranges — do NOT read the entire spec.
5. If the task involves a design decision already captured in `decisions/`, read the
   relevant ADR.

6. **Write the test plan.**
   Before writing any test code, print a numbered list of every test case you intend
   to write: name, what it exercises, and which spec example or ADR consequence drives
   it. This list is the human's primary review point. Example format:

   ```
   Test plan for <task>:
   1. test_name — exercises X (spec §2.3 example "42i32")
   2. test_name — edge case: negative fits i128 (spec §2.3)
   3. test_name — ADR-001: Unit is not nil
   ...
   ```

   **Stop here and wait for the user to approve or adjust the plan before continuing.**

7. **Beck loop — one test at a time.**
   For each test in the approved plan, in order:
   a. **Red**: Write exactly that one test function. Add the minimum stub (empty struct,
      `unimplemented!()`) needed to compile. Run `cargo test -p nexl-{crate}` and
      confirm *this test* fails. If it passes without implementation, the test is wrong —
      fix it before continuing.
   b. **Green**: Write the minimum implementation to make *this test* pass. Run
      `cargo test -p nexl-{crate}` and confirm it passes (and earlier tests still pass).
   c. **Refactor**: Tidy names, remove duplication, improve doc comments. Tests stay green.
   d. Mark the test in the plan as done. Move to the next one.

8. After all tests pass: run `cargo clippy -p nexl-{crate}` and fix all warnings.
9. Update `docs/todo-m{N}.md`: check off the completed item(s).
10. If you discovered new tasks or blockers during implementation, add them to the todo.
