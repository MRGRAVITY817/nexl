---
model: sonnet
---

Add a new crate to the Cargo workspace.

## Arguments
$ARGUMENTS — The crate name and a short description (e.g. "nexl-eval — Tree-walk evaluator for M1").

## Instructions

1. Parse the crate name from the argument.
2. Run `cargo init --lib crates/{crate-name}` to create the crate.
3. Remove the auto-generated `.git` directory and `.gitignore` from the new crate if present.
4. Update the new crate's `Cargo.toml`:
   - Set `version.workspace = true` and `edition.workspace = true`.
   - Add dependencies on other workspace crates as appropriate (check `docs/crate-map.md` and `milestones.md` for the expected dependency graph).
5. Add the crate to the `members` list in the root `Cargo.toml`.
6. Update `docs/crate-map.md` with the new crate and its dependencies.
7. Run `cargo check` to verify the workspace still builds.
8. Report what was created and which dependencies were added.
