Summarize the current milestone's progress for session start.

## Instructions

1. Read `docs/current-milestone.md` to identify the active milestone.
2. Read the corresponding `docs/todo-m{N}.md`.
3. Produce a summary with:
   - **Milestone**: name and goal (one line)
   - **Progress**: count of completed vs total items
   - **Recently completed**: last 3–5 checked items
   - **Next up**: the first 3 unchecked items (in dependency order)
   - **Blocked**: any items in the Blocked section, with explanation
   - **Crate health**: run `cargo test` and `cargo clippy --all-targets` and report pass/fail counts
4. Do NOT make any edits. This is a read-only status check.
