# Contributing to Nexl

Thank you for your interest in contributing to Nexl!

## Before you start

1. Check the [issue tracker](https://github.com/nexl-lang/nexl/issues) for
   existing issues or feature requests.
2. For significant changes, open an issue first to discuss the approach.
3. All contributions are subject to the [Code of Conduct](CODE_OF_CONDUCT.md).

## Development setup

```bash
# Clone the repo
git clone https://github.com/nexl-lang/nexl.git
cd nexl

# Build
cargo build

# Run all tests
cargo test

# Run clippy
cargo clippy --all-targets

# Format
cargo fmt
```

## Project structure

See `docs/crate-map.md` for the full architecture. Key entry points:

- `crates/nexl-eval/` — tree-walk evaluator (Stage 0)
- `crates/nexl-stdlib/` — standard library modules
- `crates/nexl-cli/` — the `nexl` binary
- `crates/nexl-reader/` — lexer + reader
- `crates/nexl-types/` — type system
- `crates/nexl-infer/` — type inference

## Making changes

1. **Find the right crate.** See `docs/crate-map.md`.
2. **Read the spec.** Language behavior is defined in `nexl-spec.md`.
   The section index is in `CLAUDE.md`.
3. **Write a test first.** We follow the Beck Augmented Coding Loop:
   Red → Green → Refactor. The test plan is the primary review artifact.
4. **Run tests.** `cargo test -p nexl-{crate}` for fast iteration.
5. **Check clippy.** `cargo clippy -p nexl-{crate}`.
6. **Open a PR.** Keep PRs focused on one thing.

## Commit messages

```
feat(nexl-stdlib): add path/normalize function [M30]
fix(nexl-reader): handle \u{NNNN} escapes in strings
docs: add example for effect handlers
```

Format: `type(scope): description [optional milestone]`

Types: `feat`, `fix`, `docs`, `test`, `refactor`, `chore`

## Good first issues

Look for issues labeled `good-first-issue`. These are typically:
- Adding a missing stdlib function
- Improving an error message
- Adding an example to the `examples/` directory
- Writing a missing test

## Design decisions

Architecture decisions are documented in `decisions/` (ADRs). Before
proposing a significant design change, read the relevant ADRs. New ADRs
are welcome for decisions that affect the language semantics.

## Questions?

Open a GitHub Discussion or post in the community Discord (link in README).
