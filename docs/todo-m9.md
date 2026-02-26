# M9 — CLI + REPL

## CLI subcommands (nexl-cli crate)
- [x] Add clap dependency — subcommand parsing (build, run, repl, check)
- [ ] `nexl build <file>` — compile to WASM (refactor existing main into subcommand)
- [ ] `nexl run <file>` — parse + evaluate via nexl-eval (tree-walk for now)
- [ ] `nexl repl` — interactive REPL (move existing repl from nexl-eval bin)
- [ ] `nexl check <file>` — parse + type-check via nexl-infer, report errors

## REPL improvements
- [ ] Multi-line input — detect unbalanced delimiters, prompt for continuation
- [ ] REPL commands: `:help`, `:quit`, `:type <expr>`
- [ ] Banner on startup (`nexl 0.1.0 | :help for commands`)

## Error rendering
- [ ] Source-annotated errors with line/column (miette integration for CLI output)

## Blocked
- [ ] Bytecode VM (`nexl-vm`) — deferred; tree-walk eval is sufficient for now
- [ ] `nexl dev` watch mode — requires incremental compilation infrastructure
- [ ] `nexl fmt` — requires a formatter (separate milestone)
- [ ] `nexl test` — requires `deftest` support
- [ ] `:effects`, `:source`, `:expand` REPL commands — require more compiler passes
