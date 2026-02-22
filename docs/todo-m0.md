# M0: Project Foundation

## Done
- [x] Cargo workspace with `nexl-ast`, `nexl-reader`, `nexl-errors`
- [x] Workspace builds cleanly

## In Progress

## Todo

### nexl-ast
- [x] Span type (byte offset + length, source file ID)
- [x] Source location type (line, column, file)
- [x] AST node types: Atom (Int, Float, Ratio, Bool, Str, Char, Keyword, Symbol, Unit)
- [x] AST node types: Compound (List, Vector, Map, Set)
- [x] AST node types: Special (Quote, Deref, Discard)
- [x] Every node carries a `Span`
- [x] Comment attachment (for round-trip formatting)

### nexl-errors
- [x] Diagnostic type with severity, message, span, labels
- [x] Source snippet rendering (miette integration)
- [x] Error codes for lexer/reader errors

### nexl-reader — Lexer
- [x] Integer literals with width suffixes (`42`, `42i32`, `42u8`)
- [x] Float literals with suffixes (`3.14`, `3.14f32`)
- [x] Ratio literals (`1/3`)
- [x] String literals with interpolation spans (`"hello {name}"`)
- [x] Escape sequences: `\\`, `\n`, `\t`, `\"`, `\{`, `{{`, `}}`
- [x] Character literals (`\a`, `\newline`, `\u{1F600}`)
- [x] Keywords (`:foo`, `:bar/baz`)
- [x] Symbols
- [x] Booleans (`true`, `false`) and `unit`
- [x] Reader macros: `'` (quote), `#{}` (set), `#_` (discard), `@` (deref)
- [x] Line comments (`;`)
- [x] Form comments with nesting (`#_`, `#_ #_`)

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
