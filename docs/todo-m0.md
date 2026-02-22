# M0: Project Foundation

## Done
- [x] Cargo workspace with `nexl-ast`, `nexl-reader`, `nexl-errors`
- [x] Workspace builds cleanly

## In Progress

## Todo

### nexl-ast
- [ ] Span type (byte offset + length, source file ID)
- [ ] Source location type (line, column, file)
- [ ] AST node types: Atom (Int, Float, Ratio, Bool, Str, Char, Keyword, Symbol, Unit)
- [ ] AST node types: Compound (List, Vector, Map, Set)
- [ ] AST node types: Special (Quote, Deref, Discard)
- [ ] Every node carries a `Span`
- [ ] Comment attachment (for round-trip formatting)

### nexl-errors
- [ ] Diagnostic type with severity, message, span, labels
- [ ] Source snippet rendering (miette integration)
- [ ] Error codes for lexer/reader errors

### nexl-reader — Lexer
- [ ] Integer literals with width suffixes (`42`, `42i32`, `42u8`)
- [ ] Float literals with suffixes (`3.14`, `3.14f32`)
- [ ] Ratio literals (`1/3`)
- [ ] String literals with interpolation spans (`"hello {name}"`)
- [ ] Escape sequences: `\\`, `\n`, `\t`, `\"`, `\{`, `{{`, `}}`
- [ ] Character literals (`\a`, `\newline`, `\u{1F600}`)
- [ ] Keywords (`:foo`, `:bar/baz`)
- [ ] Symbols
- [ ] Booleans (`true`, `false`) and `unit`
- [ ] Reader macros: `'` (quote), `#{}` (set), `#_` (discard), `@` (deref)
- [ ] Line comments (`;`)
- [ ] Form comments with nesting (`#_`, `#_ #_`)

### nexl-reader — Reader (S-expression → AST)
- [ ] Recursive descent S-expression parser
- [ ] Source spans on every node
- [ ] Round-trip formatting preservation (whitespace/comment tokens)
- [ ] `#_` nesting: `#_ #_ a b` discards both `a` and `b`

### AST Pretty-Printer
- [ ] S-expression → formatted string
- [ ] Configurable indentation

### Test Suite
- [ ] Unit tests for each token type
- [ ] Unit tests for reader (nested structures, edge cases)
- [ ] Parse every `examples/*.nxl` file without errors
- [ ] Golden tests for error messages (malformed input)

## Blocked
(none)
