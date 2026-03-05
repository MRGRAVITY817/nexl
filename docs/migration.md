# Migration Guide: Pre-1.0 to 1.0

## Breaking changes

The following changes were made during Stage 0 → Stage 1 → Stage 2 development
that may require updates to code written against earlier pre-release versions.

### Naming conventions (ADR-011)

Functions were renamed to follow consistent conventions:

| Old name | New name | Module |
|----------|----------|--------|
| `append!` | `append` | builtin |
| `push` | `append` | vec |
| `assoc` | `put` | map |
| `dissoc` | `remove` | map |
| `defmacro` | `defn-macro` | — |
| `defmacro-syntax` | `defmacro-syntax` | — (unchanged) |

### `Unit` vs `nil` (ADR-001)

Nexl does not have a `nil` value. The unit value is spelled `()` and its
type is `Unit`. Functions that previously returned `nil` now return `()`.

### Boolean-only conditionals (ADR-004)

`if` and `when` require a `Bool` condition. Truthy/falsy values are not
supported. Non-boolean expressions in condition position are a compile error.

### Cross-type arithmetic (ADR-006)

Adding `Int` to `Float` is a compile error. Use `(conv/to-float n)` to
convert explicitly.

### Effect declarations

Modules that perform side effects must declare them with `:performs`:

```nexl
;; Old (pre-1.0):
(module my.app)
(io/println "hello")  ;; worked without declaration

;; New (1.0):
(module my.app :performs [Console])
(io/println "hello")
```

### `defhandler` syntax

The `defhandler` form was added in M26. Code using `handle` with inline
operation maps should migrate to named handlers:

```nexl
;; Old:
(handle [(Net request [req resume] (resume (make-response req)))]
  body)

;; New (using defhandler):
(defhandler MockNet Net
  (request [req resume] (resume (make-response req))))

(handle [MockNet] body)
```

### `match` patterns

Record patterns require keyword syntax:

```nexl
;; Old:
(match resp
  {:status 200 body} (handle-ok body))

;; New:
(match resp
  {:status 200 :body body} (handle-ok body))
```

---

## Automated migration

The `nexl migrate` tool (available from 1.0) handles most mechanical fixups:

```
nexl migrate src/
```

It will:
1. Rename `push` → `append` in vec contexts
2. Add explicit `conv/to-float` conversions where needed
3. Add `:performs` declarations based on static analysis
4. Report patterns it cannot fix automatically

---

## Changelog

### Stage 2 (M23–M32)

- **M23**: WASI 0.2 integration, `wit-import`, `wit-export`
- **M24**: Production stdlib: `json`, `http`, `db`, `env`, `log`
- **M25**: Single-binary distribution, `nexl new`, LSP completions
- **M26**: `nexl test` — effect-powered testing with power-assert
- **M27**: nexl.test macros in Nexl (proves macro system production-grade)
- **M28**: Stdlib core enrichment — `option`, `result`, `iter`, threading macros
- **M29**: Collections — `vec`, `map`, `set`, `char`, `regex`, `iter` lazy sequences
- **M30**: Production stack — `path`, `uri`, `csv`, `toml`, `base64`, `uuid`, `bit`, crypto/time/random enhancements
- **M31**: Concurrency — `channel`, `async`, `process`, `sys`, `log` enhancements
- **M32**: `nexl-functions` flagship, 1.0 stability contract

### Stage 1 (M19–M22)

- **M19**: Self-hosting preparation — better collections, algorithm library
- **M20**: Eval completeness — full `match`, tail calls, `loop/recur`
- **M21**: Macro system — `defmacro-syntax`, `defn-macro`, `syntax-rules`
- **M22**: Stage 1 complete

### Stage 0 (M0–M18)

- **M0–M5**: Lexer, reader, AST, evaluator bootstrap
- **M6–M10**: Type system, effects, modules
- **M11–M15**: WASM backend, native backend, IR
- **M16–M18**: LSP, package manager, doc generator
