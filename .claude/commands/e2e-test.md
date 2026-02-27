---
model: haiku
---

Run or add end-to-end tests for the Nexl CLI.

## Arguments
$ARGUMENTS — One of: "run" (run all E2E tests), "run <pattern>" (run matching tests), or "add <name> <description>" (create a new test case).

## Instructions

### Running tests
1. If the argument is "run" (with optional pattern), execute `cargo test -p nexl-cli --test e2e` (or with `-- <pattern>` filter).
2. Report pass/fail counts and show any failing test diffs.
3. Do NOT fix anything automatically.

### Adding a new test
1. Parse the test name and description from arguments.
2. Create `crates/nexl-cli/tests/fixtures/<name>.nx` with a minimal Nexl program that exercises the described behavior.
   - Use single-file mode (no `(module ...)` declaration) for simplicity.
   - Use `io/println` for output.
3. Run the program with `cargo run -- run crates/nexl-cli/tests/fixtures/<name>.nx` to capture actual output.
4. Save the output to `crates/nexl-cli/tests/fixtures/<name>.expected`.
5. Verify the new test passes: `cargo test -p nexl-cli --test e2e`.
6. Report what was created.
