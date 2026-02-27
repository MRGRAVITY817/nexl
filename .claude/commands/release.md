---
model: sonnet
---

Prepare a release for the Nexl compiler.

## Arguments
$ARGUMENTS — The version to release (e.g. "0.2.0") or "patch"/"minor"/"major" for auto-bump.

## Instructions

1. **Pre-flight checks** — run these and stop if any fail:
   a. `cargo fmt --check` — formatting clean
   b. `cargo clippy --all-targets` — no warnings
   c. `cargo test` — all tests pass
   d. `git status` — working tree clean (no uncommitted changes)

2. **Determine version**:
   - If "patch"/"minor"/"major": read current version from root `Cargo.toml` workspace.package.version, bump accordingly.
   - If explicit version: use it directly.
   - Show the user the version bump (e.g. "0.1.0 → 0.2.0") and wait for confirmation.

3. **Update version**:
   - Update `version` in `[workspace.package]` in the root `Cargo.toml`.
   - Run `cargo check` to verify workspace is consistent.

4. **Create release commit and tag**:
   - Stage the changed `Cargo.toml`.
   - Commit: `release: v{version}`
   - Tag: `v{version}`

5. **Report** — show the commit hash, tag, and remind the user to push:
   `git push origin main --tags`

6. Do NOT push automatically. The user decides when to push.
