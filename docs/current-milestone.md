# Stage 0 Complete

All milestones (M0–M18) are complete. The Stage 0 bootstrap compiler is finished.

**What was built:**
- Lexer + reader (nexl-reader)
- Tree-walk evaluator (nexl-eval) with standard library (nexl-stdlib)
- Bidirectional type inference + effect row inference (nexl-infer)
- IR lowering with optimization passes (nexl-ir)
- WASM code generation (nexl-wasm) with GC backend option
- Native code generation (nexl-native)
- Macro system (nexl-macros)
- Language server (nexl-lsp)
- Package manager (nexl-pkg) with content-addressed definition store
- Documentation generator (nexl-doc)
- CLI with build, run, repl, check, sandbox, audit, doc, lsp, pkg commands
- Structured REPL protocol for AI agent / IDE integration
- Kernel subset documented for Stage 1 self-hosting

**Next:** Write the Stage 1 compiler in the Nexl kernel subset.
See `docs/kernel-subset.md` for the kernel subset specification.
See `examples/kernel-bootstrap.nxl` for the bootstrap proof-of-concept.
