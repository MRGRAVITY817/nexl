# Current Milestone: M0 — Project Foundation

**Goal:** Rust workspace builds, lexer tokenizes all of §2, reader produces AST.

**Crates:** `nexl-ast`, `nexl-reader`, `nexl-errors`

**Spec sections to reference:**
- §2 Lexical Grammar (lines 51–164 of `nexl-spec.md`)
- Appendix D Formal Grammar (lines 3809–end)

**Acceptance criteria:**
- `cargo test` passes across all three crates
- Lexer handles every token type from §2
- Reader produces a span-annotated AST from S-expressions
- All `.nxl` files in `examples/` parse without errors
- AST pretty-printer can round-trip simple programs

**When done:** Update this file to point to M1.

See `docs/todo-m0.md` for the task checklist.
See `milestones.md` for the full plan.
