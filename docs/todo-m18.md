# M18 — Content Addressing + Self-Hosting Preparation

## Content-Addressed Definitions
- [ ] Hash top-level definitions after type inference
- [ ] On-disk definition store (hash → artifact + type + effect row + deps)
- [ ] Recompile only when hash or dependency hash changes
- [ ] Macro invalidation tracking
- [ ] Verify cold build = warm build (reproducibility)

## Structured REPL Protocol
- [ ] JSON-based machine-readable REPL protocol
- [ ] `eval` command with streaming response
- [ ] `define` / `type-of` / `effects-of` / `deps` commands
- [ ] `expand` / `test` / `complete` commands
- [ ] Session management

## Kernel Subset Definition
- [ ] Document the kernel subset (no macros, all types annotated, no effect syntax)
- [ ] Verify kernel subset is sufficient to write a basic compiler

## Stage 0 → Stage 1 Bootstrap Path
- [ ] Write a small kernel-subset Nexl program that parses and type-checks itself
- [ ] Verify Stage 0 (Rust) can compile it

## Blocked
- (none)

## Done
