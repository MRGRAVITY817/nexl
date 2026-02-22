# Nexl Compiler

## What
Nexl is a statically-typed, effect-tracked Lisp that compiles to WebAssembly and native code. This repo is the Stage 0 bootstrap compiler, implemented in Rust as a Cargo workspace on macOS.

## Design Principles (ALWAYS respect these)
1. Composability is the master virtue
2. Explicitness over magic
3. Practicality over purity
4. One way to do it
5. Fast feedback over fast execution
6. The compiler is a conversational partner (error messages!)

## Architecture
Cargo workspace under `crates/`, one crate per compiler phase.
See `docs/crate-map.md` for the full dependency graph.

## Key Crates (current)
- `nexl-ast` — AST node types, spans, source locations
- `nexl-reader` — Lexer + reader (text → s-expression AST)
- `nexl-errors` — Diagnostic types (miette + thiserror)

## Key Crates (planned, added as milestones begin)
- `nexl-types` — Type representation, substitution, unification (M2)
- `nexl-infer` — Bidirectional inference + effect row inference (M2)
- `nexl-eval` — Tree-walk evaluator for dev mode (M1, temporary)
- `nexl-runtime` — Value representation, built-in functions (M1)
- `nexl-effects` — Effect declarations, evidence passing (M6)
- `nexl-ir` — Intermediate representation, post-lowering (M8)
- `nexl-wasm` — WASM code generation (M8)
- `nexl-vm` — Bytecode VM for REPL/dev mode (M8)
- `nexl-cli` — The `nexl` binary (M8)

## Commands
- Build: `cargo build`
- Test all: `cargo test`
- Test one crate: `cargo test -p nexl-reader`
- Clippy: `cargo clippy --all-targets`
- Format: `cargo fmt`

## TDD Approach — Beck Augmented Coding Loop (ALWAYS follow)

We follow Kent Beck's Augmented Coding pattern: the human owns the test plan;
the AI executes one test at a time; no one skips the Red step.

### The loop (per task)
1. **Test plan** — Before writing any test code, enumerate every test case as a
   numbered list (name + what it exercises + which spec example or ADR drives it).
   Present this list to the human. **Wait for approval before writing a single test.**
2. **Red** — Write exactly one test from the plan. Add minimum stubs to compile.
   Run `cargo test` and confirm *this test* fails. If it passes without implementation,
   the test is wrong — fix it first.
3. **Green** — Write the minimum implementation to pass this one test. Confirm green.
4. **Refactor** — Tidy up without breaking anything. Then move to the next test.
5. Repeat steps 2–4 for each remaining test in the plan.

### Rules
- The test plan list is the human's primary review artifact. Never skip it.
- Tests are written **before** implementation, one at a time — not in bulk.
- Every spec example becomes a test case. Every ADR consequence is a test candidate.
- A task is not done until all planned tests are green AND `cargo clippy` is clean.
- Never mark a todo item `[x]` if tests were written after the implementation.

## Code Style
- Use `thiserror` for error enums, `miette` for diagnostics
- Every public type needs a doc comment
- Prefer `match` over `if let` chains
- No `unwrap()` in library crates; use `expect()` with message or `?`
- Test files live next to source: `mod tests { ... }` in each file
- Keep functions focused — prefer small, well-named functions over long blocks

## Current Milestone
See `docs/current-milestone.md` for active work.
See `docs/todo-m0.md` for the task checklist.
Read `milestones.md` for the full plan.

## Spec Reference
The full language spec is in `nexl-spec.md` (symlinked from the design repo).
Do NOT read the entire spec. Instead:
- For syntax/tokens → §2 Lexical Grammar (lines 51–164)
- For data model → §3 (lines 166–306)
- For core forms → §4 (lines 308–1148)
- For type system → §5 (lines 1150–1518)
- For effects → §6 (lines 1520–1800)
- For macros → §7 (lines 1802–1941)
- For modules → §8 (lines 1943–2031)
- For error handling → §9 (lines 2033–2145)
- For compilation model → §12 (lines 2833–2926)
- For formal grammar → Appendix D (lines 3809–end)

## Design Decisions
ADRs are in `decisions/` (symlinked from design repo). Key ones:
- ADR-001: `Unit` not Nil
- ADR-003: One-shot continuations
- ADR-004: Bool-only conditionals
- ADR-006: Cross-type arithmetic is a compile error
- ADR-008: No HKT, compiler-dispatched map/filter
- ADR-011: Naming conventions (append/put/remove/each/etc.)
- ADR-013: Def-form renames (defn-macro, defmacro-syntax, etc.)

## Workflow
1. Read `docs/current-milestone.md` and `docs/todo-m0.md`
2. Work on the next unchecked item
3. Run `cargo test -p nexl-{crate}` to verify
4. Update the todo checklist
5. Commit with: `feat(nexl-{crate}): description [M{N}]`
