# M25 — Developer Experience & Toolchain Polish

## Deliverables

- [x] 1. **`nexl new` — project scaffolding**
  - `nexl new <name>` → project scaffold (project.nx, src/main.nx, tests/, .gitignore)
  - `nexl new <name> --template web` → web service scaffold with HTTP, JSON, logging
  - Generated projects run immediately: `nexl run src/main.nx`

- [x] 2. **`nexl upgrade` — self-update command**
  - `nexl upgrade` checks for latest version and updates the binary
  - Stub implementation: prints version + instructions (no real download server yet)

- [x] 3. **Error message audit — type inference**
  - Audit type mismatch errors for clarity
  - Show expected vs actual with source location
  - Cascade suppression: don't flood user with downstream errors

- [x] 4. **Error message audit — effect system**
  - Effect row mismatch: explain which effect is missing and where expected
  - Suggest fix: "add `:performs [Net]` to module declaration"
  - Suppress cascade noise from effect errors

- [x] 5. **LSP completions — module names in `:imports`**
  - Complete module names when typing in `:imports` vectors
  - Scan project source tree + stdlib module list

- [x] 6. **LSP completions — record fields**
  - Complete record fields from inferred types (`:status`, `:body`, etc.)
  - Triggered after typing `:` in a record context

- [x] 7. **LSP completions — stdlib function names**
  - Complete function names when typing `json/`, `http/`, `db/`, etc.
  - Source from stdlib module entries

- [x] 8. **Documentation site generator — HTML output**
  - `nexl doc --html` produces navigable HTML documentation
  - Includes type signatures, effect rows, docstrings
  - Cross-module hyperlinks

- [ ] 9. **Cookbook — 10 core recipes**
  - Parse JSON, make HTTP request, query database, read file
  - Handle errors with Result, test with deftest
  - Each recipe: problem, solution, explanation, runnable code

- [ ] 10. **Cookbook — 10 intermediate recipes**
  - Pattern matching, custom types (deftype), map/filter/reduce
  - Structured logging, environment config, dotenv loading
  - WASM compilation, WIT imports

- [ ] 11. **Cookbook — 10 advanced recipes**
  - Effect handlers, sandboxing, component composition
  - Property-based testing, custom macros, modules & packages
  - Full web service example

## Blocked

(None)
