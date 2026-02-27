# M18 — Content Addressing + Self-Hosting Preparation

## Content-Addressed Definitions
- [x] Hash top-level definitions after type inference
- [x] On-disk definition store (hash → artifact + type + effect row + deps)
- [x] Recompile only when hash or dependency hash changes
- [x] Macro invalidation tracking
- [x] Verify cold build = warm build (reproducibility)

## Structured REPL Protocol
- [x] JSON-based machine-readable REPL protocol
- [x] `eval` command with streaming response
- [x] `define` / `type-of` / `effects-of` / `deps` commands
- [x] `expand` / `test` / `complete` commands
- [x] Session management

## Kernel Subset Definition
- [x] Document the kernel subset (no macros, all types annotated, no effect syntax)
- [x] Verify kernel subset is sufficient to write a basic compiler

## Stage 0 → Stage 1 Bootstrap Path
- [x] Write a small kernel-subset Nexl program that parses and type-checks itself
- [x] Verify Stage 0 (Rust) can compile it

## Blocked
- (none)

## Done
- All items complete
