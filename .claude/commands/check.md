Run a full validation pass on the workspace.

## Arguments
$ARGUMENTS — Optional: a specific crate name (e.g. "nexl-reader"). If omitted, checks the entire workspace.

## Instructions

1. If a crate name is given, scope all commands to `-p {crate}`. Otherwise, run workspace-wide.
2. Run these checks in sequence, reporting results for each:
   a. `cargo fmt --check` (or `cargo fmt -p {crate} --check`) — formatting
   b. `cargo clippy --all-targets` (or `cargo clippy -p {crate} --all-targets`) — lint warnings
   c. `cargo test` (or `cargo test -p {crate}`) — test suite
3. For each step, report: pass/fail, and if failed, show the relevant errors.
4. At the end, give a one-line summary: "All clear" or "N issues found (X fmt, Y clippy, Z test failures)".
5. Do NOT fix anything automatically. Report only. The user decides what to fix.
