# Nexl 1.0 Stability Contract

## What is stable

The following language and tooling features are **stable** as of 1.0 and will
not change in incompatible ways in any 1.x release:

### Language

- **Syntax and lexical grammar** — all forms defined in §2 of `nexl-spec.md`,
  including keywords, symbols, numeric literals, string literals (regular,
  triple-quoted, raw), character literals, unit `()`, and collection literals.

- **Core evaluation model** — `let`, `if`, `cond`, `fn`, `defn`, `def`,
  `do`, `loop`, `recur`, `match`, `when`, `unless`, all as specified in §4.

- **Data model** — `Int`, `Float`, `Bool`, `Str`, `Char`, `Keyword`, `Unit`,
  `Vec`, `Map`, `Set`, `Ratio`, `Adt` types as specified in §3.

- **Type system** — bidirectional type inference, `deftype`, `defsum`,
  `defprotocol`, `derive`, type annotations, as specified in §5.

- **Effect system** — `:performs` declarations, `handle`, `defhandler`,
  effect rows, `resume`, as specified in §6.

- **Macro system** — `defmacro-syntax`, `syntax-rules`, `defn-macro`,
  as specified in §7.

- **Module system** — `module`, `:imports`, `:exports`, `:performs`,
  `use`, qualified naming, as specified in §8.

- **Standard library** — all public functions in the modules listed in
  `docs/stdlib-spec.md` that are not marked experimental.

- **CLI interface** — `nexl run`, `nexl build`, `nexl test`, `nexl repl`,
  `nexl fmt`, `nexl doc`, `nexl functions`, `nexl new`, `nexl pkg`,
  all flags documented in `nexl --help`.

- **Error message format** — error IDs (e.g., `E0001`) will not be renumbered.
  Message text may improve but will not remove information.

### Tooling

- **`project.nx` manifest format** — all fields documented in
  `docs/project-format.md`.

- **`nexl-lock.json` lockfile format** — content-addressed entries will remain
  forward-compatible.

- **LSP protocol** — all capabilities advertised in `nexl-lsp`'s `initialize`
  response.

---

## What is experimental

The following areas are **not** covered by the 1.0 stability guarantee:

- **Native backend** (`nexl build --target native`) — codegen may change.
- **WASI 0.3 async** (`--experimental-wasi3`) — API may change when WASI 0.3
  stabilizes.
- **Specific optimization passes** — the optimizer may change output without
  notice (observable only via performance, not semantics).
- **Internal compiler APIs** — crates `nexl-ast`, `nexl-eval`, `nexl-reader`,
  etc. are not public APIs; only the CLI and stdlib are.
- **Doc generator HTML** — HTML structure of `nexl doc` output may change.
- Modules marked `(experimental)` in stdlib-spec.md.

---

## Backward compatibility promise

Code that compiles on Nexl 1.0 will compile on any Nexl 1.x release. If a
1.x release requires changes to user code, it is a bug and will be fixed.

### Edition mechanism

Breaking changes (if ever necessary) will be introduced via **editions**,
similar to Rust. User code opts into a new edition via:

```
(edition 2)
```

at the top of `project.nx`. The compiler supports multiple editions
simultaneously. Edition 1 will be maintained indefinitely.

---

## Versioning

From 1.0 onward, Nexl uses strict semantic versioning:

- **Patch** (1.0.x) — bug fixes only, no new features.
- **Minor** (1.x.0) — new features, backward-compatible changes.
- **Major** (x.0.0) — new editions only; old editions still compile.

Pre-releases (`1.0.0-rc.1`) make no stability promises.
