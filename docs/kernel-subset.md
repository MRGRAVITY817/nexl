# Nexl Kernel Subset

The kernel subset is the restricted fragment of Nexl used to write the Stage 1
compiler. It is designed to be compilable by the Stage 0 Rust bootstrap compiler
without requiring macro expansion, type inference, or effect row tracking.

## What is excluded

| Feature | Full Nexl | Kernel Subset |
|---------|-----------|---------------|
| Macros (`defmacro`, `defn-macro`, `defmacro-syntax`) | Yes | **No** |
| Type inference | Bidirectional HM | **None** — all types must be annotated |
| Effect system (`!`, `handle`, `resume`) | Algebraic effects | **No** — use explicit Result/monadic encoding |
| Pattern matching (`match`) | Full ADT patterns | **Simplified** — literal + constructor patterns only |
| Operator overloading / protocols | Yes | **No** |
| String interpolation | Yes | **No** — use `str/concat` |

## What is included

The kernel subset retains enough of the language to write a compiler:

### Types (all must be explicitly annotated)

- **Primitives:** `Int`, `Float`, `Bool`, `Str`, `Char`, `Unit`
- **Collections:** `(Vec T)`, `(Map K V)`, `(Set T)`
- **ADTs:** `(type (Option T) None (Some T))`, `(type (Result Ok Err) ...)`
- **Function types:** `(Fn [A B] -> C)`
- **Records:** `(Record {:field Type ...})`
- **Tuples:** `(Tuple A B ...)`

### Forms

- `(def name : Type value)` — value binding with type annotation
- `(defn name [param : Type ...] -> ReturnType body)` — function definition
- `(fn [param : Type ...] -> ReturnType body)` — anonymous function
- `(let [name : Type value ...] body)` — local binding
- `(if cond then else)` — conditional
- `(match expr pattern body ...)` — simplified pattern matching
- `(do form ...)` — sequencing
- `(type Name variants ...)` — ADT definition
- `(import module-path)` — module imports
- `(loop [name : Type init ...] body)` — tail-recursive loop
- `(recur args ...)` — loop continuation

### Standard library available

- Arithmetic: `+`, `-`, `*`, `/`, `mod`, `<`, `>`, `<=`, `>=`, `=`, `not=`
- Logic: `and`, `or`, `not`
- String: `str/concat`, `str/length`, `str/split`, `str/join`, `str/chars`
- Collections: `vec/new`, `vec/push`, `vec/get`, `vec/length`, `vec/map`, `vec/filter`,
  `map/new`, `map/put`, `map/get`, `map/contains?`, `set/new`, `set/add`, `set/contains?`
- I/O: `println`, `print`, `read-line` (via explicit `(Result Str IoError)` return)
- Option/Result: `Some`, `None`, `Ok`, `Err`, `option/map`, `result/map`
- File I/O: `file/read`, `file/write` (via `(Result Str IoError)`)

### Error handling

Instead of algebraic effects, the kernel subset uses `Result` types explicitly:

```nexl
;; Full Nexl (with effects):
(defn read-config! [path : Str] -> Config ! [Fs]
  (let [content (fs/read path)]
    (parse-config content)))

;; Kernel subset (explicit Result):
(defn read-config [path : Str] -> (Result Config IoError)
  (let [content : (Result Str IoError) (file/read path)]
    (match content
      (Ok s) (parse-config s)
      (Err e) (Err e))))
```

### Monadic style for sequencing effectful operations

```nexl
(defn compile-file [path : Str] -> (Result Unit CompileError)
  (let [source : (Result Str IoError) (file/read path)]
    (match source
      (Err e) (Err (CompileError/io e))
      (Ok s)
        (let [ast : (Result Ast ParseError) (parse s)]
          (match ast
            (Err e) (Err (CompileError/parse e))
            (Ok a)
              (let [ir : (Result Ir TypeError) (type-check a)]
                (match ir
                  (Err e) (Err (CompileError/type e))
                  (Ok i) (emit-wasm i))))))))
```

## Sufficiency verification

The kernel subset must be sufficient to implement a basic compiler with these passes:

1. **Lexer** — tokenize source text into tokens (uses `Str`, `Vec`, `match`)
2. **Reader** — parse tokens into s-expression AST (uses ADTs, `Vec`, `match`)
3. **Type checker** — verify type annotations match (uses `Map`, ADTs, `match`)
4. **Code generator** — emit bytecode or WASM from typed AST (uses `Vec`, `match`)

Each of these passes can be implemented using only: ADTs for data, `match` for
dispatch, `Vec`/`Map` for collections, `Result` for errors, and explicit type
annotations throughout. No macros, no inference, no effects needed.

## Relationship to stages

```
Stage 0 (Rust)  ─── compiles ──→  Stage 1 (kernel Nexl)
Stage 1         ─── compiles ──→  Stage 2 (full Nexl)
Stage 2         ─── compiles ──→  itself (bootstrap complete)
```

Stage 0 only needs to handle the kernel subset. Stage 1, written in the kernel
subset, adds macro expansion, type inference, and effect tracking. Stage 2 is
the full self-hosted compiler.
