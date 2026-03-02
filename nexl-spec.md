# Nexl Language Specification

**Version:** 0.1 (Draft)
**Date:** 2026-02-18

---

## Table of Contents

1. [Introduction](#1-introduction)
2. [Lexical Grammar](#2-lexical-grammar)
3. [Data Model](#3-data-model)
4. [Core Forms](#4-core-forms)
5. [Type System](#5-type-system)
6. [Effect System](#6-effect-system)
7. [Macro System](#7-macro-system)
8. [Module System](#8-module-system)
9. [Error Handling](#9-error-handling)
10. [Concurrency](#10-concurrency)
11. [Standard Library](#11-standard-library)
12. [Compilation Model](#12-compilation-model)
13. [Runtime Model](#13-runtime-model)
14. [Toolchain](#14-toolchain)
15. [Interoperability](#15-interoperability)
- [Appendix A: Syntax Quick Reference](#appendix-a-syntax-quick-reference)
- [Appendix B: Effect Row Notation](#appendix-b-effect-row-notation)
- [Appendix C: Keyword Index](#appendix-c-keyword-index)
- [Appendix D: Formal Grammar (EBNF)](#appendix-d-formal-grammar-ebnf)

---

## 1. Introduction

Nexl is a statically-typed, effect-tracked Lisp that compiles to WebAssembly and native code. It preserves Lisp's core strengths — homoiconicity, macros, and REPL-driven development — while providing a modern type system, algebraic effects, capability-based security, and content-addressed code.

### 1.1 Design Principles

1. **Composability is the master virtue.** Every feature must compose with every other. Effects compose with types. Types compose with macros. Macros compose with modules.
2. **Explicitness over magic.** Side effects are declared. Capabilities are granted. Types are tracked. Nothing happens behind the programmer's back.
3. **Practicality over purity.** Nexl has mutable locals, imperative loops, and a dynamic escape hatch. Dogmatic purity serves the language designer's ego, not the programmer's productivity.
4. **One way to do it.** Opinionated defaults, a mandatory formatter, one concurrency model, one error-handling approach, one module system.
5. **Fast feedback over fast execution.** Development speed is prioritized over benchmark performance — though the latter is not sacrificed.
6. **The compiler is a conversational partner.** Error messages are helpful explanations, not cryptic diagnostics.

### 1.2 File Extension

Nexl source files use the `.nx` extension.

### 1.3 Encoding

All source files are UTF-8. Identifiers may contain Unicode letters and digits. Operator characters are restricted to ASCII.

---

## 2. Lexical Grammar

> A formal EBNF grammar for all syntax forms is provided in Appendix D.

### 2.1 Comments

```
; single-line comment until end of line
#_ expr   ; discard reader macro: skip the following expression
```

`#_` discards exactly one following form. To discard multiple forms, chain them: `#_ #_ x y` discards `y` first (as `#_ y` applied by the outer `#_`), so `x` is still read. To discard both `x` and `y`, write `#_ (x y)` or use two separate `#_` markers. The outer `#_` sees `#_ y` as a single form (a discard-reader application to `y`), so the first form consumed by the outer `#_` is the entire `#_ y` expression, and `x` is still read. **To discard N consecutive forms, use N `#_` markers.**

### 2.2 Whitespace

Spaces, tabs, newlines, and commas are whitespace. Commas are treated as whitespace to allow `{:a 1, :b 2}` formatting.

### 2.3 Numeric Literals

```
42            ; Int
-7            ; Int (negative)
1_000_000     ; Int (underscore separators, for readability only)
3.14          ; Float
-0.5          ; Float
3/4           ; Ratio (exact rational, stored as numerator/denominator pair)
0xFF          ; Int (hexadecimal)
0b1010        ; Int (binary)
0o17          ; Int (octal)
1.5e10        ; Float (scientific notation)
42i8          ; Int8 (suffixed — fixed-width signed)
42i32         ; Int32 (suffixed)
255u8         ; U8 (suffixed — fixed-width unsigned)
1000u16       ; U16 (suffixed)
3.14f32       ; F32 (suffixed — single-precision float)
3.14f64       ; F64 (suffixed — alias for Float)
```

Unsuffixed integer literals default to `Int` (64-bit signed). Unsuffixed floating-point literals default to `Float` (64-bit double). Suffixes follow Rust convention: `i` + bit width for signed integers, `u` + bit width for unsigned integers, `f` + bit width for floats. Valid suffixes: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`. Literal values are range-checked at compile time: `256u8` is a compile error.

### 2.4 String Literals

```
"hello"                      ; Str — basic string
"hello, {name}!"             ; Str — interpolation (compile-time type-checked)
"line1\nline2"               ; Str — standard escape sequences: \n \t \r \\ \"
"{{literal braces}}"         ; Str — {{ produces a literal {, }} produces a literal }
"""
multiline
string literal
"""                          ; Str — triple-quoted, auto-dedents common leading indent
r"no escapes: \n is literal backslash-n"  ; Str — raw string
r#"can contain " double quotes inside"#   ; Str — raw with one hash
r##"can contain "# sequences inside"##   ; Str — raw with two hashes
```

Interpolation: `{expr}` inside a string evaluates `expr` and calls `str` on the result. To include a literal `{` or `}` in a string, double it: `{{` → `{`, `}}` → `}`. This applies in both regular strings and triple-quoted strings. The expression must be in scope at the point of interpolation. The type of `expr` may be any type that implements the `Str` coercion (all primitive types do by default).

**Triple-quoted strings** (`"""..."""`) support the same escape sequences and interpolation as regular strings. The reader applies `inspect.cleandoc`-style auto-dedenting: if the first line is blank it is dropped, if the last line is blank it is dropped, then the minimum common leading indentation across all remaining non-empty lines is stripped. This makes multi-line docstrings inside indented `defn` bodies appear flush-left when rendered.

**Raw strings** (`r"..."`, `r#"..."#`, `r##"..."##`, …) contain verbatim content with no escape processing and no interpolation. A `"` inside a raw string does not terminate it as long as it is not followed by the same number of `#` characters that appeared after the opening `r`. Use raw strings for regular expressions, Windows paths, embedded code snippets, or any content where backslashes should be literal. The hash count (0–N) determines the closing delimiter: `r#"..."#` requires `"#` to close, `r##"..."##` requires `"##`, and so on.

### 2.5 Character Literals

```
\a       ; Char — lowercase a
\space   ; Char — space character
\newline ; Char — newline character
\tab     ; Char — tab character
\u0041   ; Char — Unicode code point (4 hex digits, BMP only: U+0000–U+FFFF)
\u{1F600} ; Char — Unicode code point (1–6 hex digits, full range: U+0000–U+10FFFF)
```

Both `\uXXXX` (exactly 4 hex digits) and `\u{X...}` (1–6 hex digits) are accepted. Code points in the surrogate range (U+D800–U+DFFF) are not valid Unicode scalar values and are a compile error.

### 2.6 Keywords

Keywords are interned, immutable tokens beginning with `:`. They evaluate to themselves and are used as map keys, tags, and named arguments.

```
:status
:http/ok           ; namespaced keyword
::local-alias      ; auto-namespace: resolves to current module's namespace
```

### 2.7 Symbols

Symbols are identifiers that name values. They may contain letters, digits, `-`, `_`, `?`, `!`, `*`, `+`, `/` (with restrictions), `<`, `>`, `=`. A symbol may not begin with a digit.

```
add
http-client
valid?
fetch!
my-module/my-fn    ; qualified symbol (module/name)
```

Convention: `?` suffix for predicates returning `Bool`, `!` suffix for functions that **mutate state** (atoms, transients, mutable locals) or perform tracked side effects. Atom operations (`swap!`, `reset!`) and `set!` carry `!` as a visual reminder of mutation, even though atom operations are deliberately outside the effect system (see §3.4).

### 2.8 Special Reader Syntax

| Syntax | Expands To |
|--------|-----------|
| `'x` | `(quote x)` |
| `` `x `` | `(quasiquote x)` |
| `~x` | `(unquote x)` |
| `~@x` | `(unquote-splice x)` |
| `#'x` | `(var x)` — reference to a var |
| `@x` | `(deref x)` — dereference an atom/ref |
| `#_x` | Discard `x`; reader produces nothing |

### 2.9 Collection Literals

```
[1 2 3]          ; Vector
{:a 1 :b 2}      ; Map (must have even number of forms)
#{1 2 3}         ; Set
'(1 2 3)         ; List (quoted; used primarily in macros)
```

### 2.10 Reader Extensions

Modules may register prefix-dispatched reader extensions using `defreader` and `defreader-text` (see §7.10). Built-in extensions:

```clojure
#r"^\d+$"        ; Regex literal — compiles to a compiled regex at compile time
#inst"2026-01-01T00:00:00Z"   ; Instant literal
#uuid"550e8400-e29b-..."      ; UUID literal
```

---

## 3. Data Model

### 3.1 Primitive Types

| Type | Description | Example |
|------|-------------|---------|
| `Int` | 64-bit signed integer | `42` |
| `Float` | 64-bit IEEE 754 double | `3.14` |
| `Ratio` | Exact rational number | `3/4` |
| `Bool` | Boolean | `true`, `false` |
| `Char` | Unicode scalar value | `\a` |
| `Str` | Immutable UTF-8 string | `"hello"` |
| `Keyword` | Interned string token | `:status` |
| `Symbol` | Named reference (used in macros) | `'add` |
| `Int8` | 8-bit signed integer | `42i8` |
| `Int16` | 16-bit signed integer | `42i16` |
| `Int32` | 32-bit signed integer | `42i32` |
| `Int64` | 64-bit signed integer (alias for `Int`) | `42i64` |
| `U8` | 8-bit unsigned integer | `255u8` |
| `U16` | 16-bit unsigned integer | `1000u16` |
| `U32` | 32-bit unsigned integer | `42u32` |
| `U64` | 64-bit unsigned integer | `42u64` |
| `F32` | 32-bit IEEE 754 single-precision float | `3.14f32` |
| `F64` | 64-bit IEEE 754 double-precision float (alias for `Float`) | `3.14f64` |
| `Unit` | The type with exactly one value | `unit` |
| `Never` | The bottom type — no values; return type of diverging expressions | `(panic "msg")` |

`Unit` has exactly one value, `unit`. It is used as the return type of effectful operations that produce no meaningful result (e.g., `Console/print`, `FileSystem/write-file`). For optional values, use `(Option a)` with `Some` and `None`. There is no `nil` in Nexl.

#### Numeric Semantics

- **Int overflow:** Int arithmetic wraps on overflow (two's complement). The standard library provides `checked-add`, `checked-mul`, etc., which return `(Option Int)` on overflow. For arbitrary-precision needs, use the `BigInt` extended module.
- **Float:** IEEE 754 double-precision. `(/ 1.0 0.0)` produces `Inf`. `(/ 0.0 0.0)` produces `NaN`. `NaN` is not equal to itself: `(= NaN NaN)` is `false`.
- **Ratio:** Exact rational arithmetic with arbitrary-precision numerator and denominator. `(/ 1 3)` produces the ratio `1/3`, not a float. Ratios auto-simplify: `(/ 2 4)` produces `1/2`. `(/ n 0)` is a panic (division by zero).
- **Mixed arithmetic (numeric tower only):** `Int` and `Ratio` operations produce `Ratio`. `Int` or `Ratio` operations with `Float` produce `Float` (lossy coercion — the compiler emits a warning unless explicitly converted via `(->float r)`). These promotion rules apply exclusively to the three numeric-tower types (`Int`, `Float`, `Ratio`). Fixed-width types (`Int8`, `Int32`, `U8`, `F32`, etc.) never participate in implicit promotion — cross-type arithmetic between any combination involving a fixed-width type is a compile error (see "Cross-type arithmetic" below).

#### Fixed-Width Numeric Semantics

Fixed-width types exist for WASM compilation, C FFI, binary protocols, and GPU interop. They do not participate in mixed arithmetic with `Int`, `Float`, or `Ratio`.

**Signed integers (`Int8`, `Int16`, `Int32`, `Int64`):**
- Two's complement representation. Arithmetic wraps on overflow.
- `Int64` is an alias for `Int` — they are the same type.
- Ranges: `Int8` [-128, 127], `Int16` [-32768, 32767], `Int32` [-2³¹, 2³¹-1], `Int64` [-2⁶³, 2⁶³-1].

**Unsigned integers (`U8`, `U16`, `U32`, `U64`):**
- Unsigned representation. Arithmetic wraps on overflow.
- Ranges: `U8` [0, 255], `U16` [0, 65535], `U32` [0, 2³²-1], `U64` [0, 2⁶⁴-1].

**Floats (`F32`, `F64`):**
- `F32`: IEEE 754 single-precision (32-bit).
- `F64` is an alias for `Float` — they are the same type (IEEE 754 double-precision, 64-bit).

**Cross-type arithmetic (fixed-width types):**
- Arithmetic between any two *different* fixed-width numeric types is a **compile error**. `(+ 1i32 2u8)` does not compile. Explicit conversion is required: `(+ 1i32 (->int32 2u8))`.
- This applies to all combinations involving fixed-width types: `Int` + `Int32`, `U8` + `U16`, `F32` + `Float`, `Int` + `U8`, etc.
- The numeric-tower promotion rules (Int/Float/Ratio above) are the only exception to this rule and apply only when both operands are tower types.
- The `conv` module (§11.1) provides all conversion functions.

**Literal range checking:**
- The compiler checks that suffixed literals fit their target type at compile time. `256u8` is a compile error because 256 exceeds the `U8` range. `(-129i8)` is a compile error.

**Protocol membership:**
- Fixed-width types implement `Ord`, `Eq`, `Hash`, `Show`, and `Numeric` (each type independently).
- For generic numeric code across all numeric types (including fixed-width), use `:where [(Numeric a)]`. For integer-specific operations, use `:where [(IntLike a)]`. For fractional-specific operations, use `:where [(FracLike a)]`.

#### String Semantics

Strings are immutable UTF-8 byte sequences. Indexing and length operate on **Unicode scalar values (codepoints)**, not bytes or grapheme clusters:

- `(count "café")` returns `4` (four codepoints).
- `(get "café" 3)` returns `(Some \é)`.
- `(slice "café" 1 3)` returns `"af"`.

Codepoint indexing is O(n) for arbitrary positions (UTF-8 does not support O(1) random access). For performance-critical indexed access, convert to a `(Vec Char)` first. For grapheme-cluster-aware operations (emoji, combining characters), use the `str/graphemes` function from the standard library, which returns a `(Vec Str)` of grapheme clusters.

### 3.2 Collection Types

All collections are **persistent** (immutable with structural sharing) by default.

| Type | Description | Access | Construction |
|------|-------------|--------|--------------|
| `(Vec a)` | Indexed vector, O(log₃₂ n) | `(get v i)` | `[1 2 3]` |
| `(Map k v)` | Hash map, O(log₃₂ n) | `(get m k)` | `{:a 1}` |
| `(Set a)` | Hash set, O(log₃₂ n) | `(contains? s x)` | `#{1 2}` |
| `(List a)` | Singly-linked list, O(1) head | `(first l)` | `'(1 2 3)` |
| `(Tuple a b)`, `(Tuple a b c)`, ... | Heterogeneous product (2–8 elements) | `(fst t)`, `(snd t)`, `(nth t 2)` | `[a b]` (2-tuple), `[a b c]` (3-tuple) |

**Tuples** are fixed-length, heterogeneous products. A 2-element vector literal `[a b]` is inferred as `(Tuple a b)` when the context demands a tuple (e.g., a function return type annotation or a pattern expecting `(Tuple ...)`) — otherwise it remains `(Vec a)`. When inference is ambiguous, prefer annotating: `(: (Tuple Int Str) [42 "hello"])`. Tuples are structurally typed: `(Tuple Int Str)` and `(Tuple Int Str)` from different sources are the same type. Accessor shorthands: `(fst t)` = first element, `(snd t)` = second element, `(nth t n)` = nth element (0-indexed, compile-time constant index).

Structural sharing: operations that "modify" a collection return a new collection that shares unchanged sub-structure with the original. The original is unchanged.

### 3.3 Transients

For batch mutation, transients allow O(1) amortized updates followed by conversion back to persistent form:

```clojure
(freeze
  (reduce (fn [acc i] (put! acc i (* i i)))
          (transient {})
          (range 1000)))
```

Transients must not escape their creation scope. The compiler enforces this via **escape analysis**: a transient value may not be stored in a data structure, returned from a function, or captured by a closure. It may only be passed to `put!`, `append!`, `remove!`, and `freeze` within the same lexical scope. Violating these rules is a compile error. This is a targeted restriction on transient types specifically, not a general linear type system.

### 3.4 Atoms

An `Atom` is a mutable, thread-safe reference to an immutable value:

```clojure
(def counter (atom 0))
(swap! counter inc)       ; atomically apply fn to current value
(reset! counter 0)        ; unconditionally set
@counter                  ; deref: read current value
```

Atoms use compare-and-swap internally. `swap!` retries if the atom's value changes during the function call.

**Atoms and the effect system.** Atom operations (`swap!`, `reset!`, `deref`) are **not** tracked by the effect system. This is a deliberate exception. Atoms hold immutable values and use atomic compare-and-swap — individual operations are linearizable and cannot produce partial/corrupted state. In single-threaded code, atoms behave like local mutable state. In concurrent code, the `Concurrent` effect is already required (for `fork`/`join`), which serves as the capability gate. Tracking atoms as a separate effect would add annotation burden to nearly every stateful program without meaningfully improving safety. The `!` suffix on `swap!` and `reset!` serves as a visual reminder that mutation occurs.

### 3.5 Value Equality

All values support structural equality via `=`. Two collections are equal if they have the same type, length, and equal elements. Equality is recursive and handles cycles (returning false on cycle detection).

### 3.6 Collection Ordering Guarantees

All collection operations in Nexl have **fully specified, deterministic ordering**. There are no undefined or implementation-dependent behaviors. This is critical for AI agents, which cannot reason about or test code whose outputs vary unpredictably.

| Collection | Enumeration order |
|------------|-------------------|
| `(Vec a)` | Index order (0, 1, 2, ...) |
| `(Map k v)` | Insertion order — keys appear in the order they were first `put`-ed |
| `(Set a)` | Canonical hash order — deterministic and consistent across runs and implementations, but not insertion order |
| `(List a)` | Head-to-tail order |

**Map ordering rules:**
- `put` on a new key appends to the end.
- `put` on an existing key updates the value without changing position.
- `remove` removes the key; remaining keys retain their relative order.
- `merge` appends keys from the right map in their insertion order; existing keys retain their left-map position with their value updated.
- `(keys m)`, `(vals m)`, `(entries m)` all respect insertion order.

**Sort stability:**
`sort` and `sort-by` are stable — equal elements retain their relative order from the input sequence. This holds for all collection types.

**Consequence:** `(map f coll)`, `(filter pred coll)`, `(reduce f init coll)`, `(for [...] ...)`, and `(each [...] ...)` all process elements in the collection's canonical enumeration order. The result is always reproducible given the same input.

---

## 4. Core Forms

### 4.1 `def` — Top-Level Definition

```clojure
(def name expr)
(def name : Type expr)
```

Creates an immutable top-level binding. The value is evaluated once at module load time in top-down order (forward references within the same module are not allowed unless via `declare`).

### 4.2 `defn` — Function Definition

```clojure
;; No annotation — fully inferred
(defn name [params] body)

;; Partial — annotate individual params, return type inferred
(defn name [x : TypeA y : TypeB] body)

;; Full — params and return type
(defn name [x : TypeA y : TypeB] -> ReturnType body)

;; With effects — effects follow the return type
(defn name [x : TypeA] -> ReturnType ! [EffectA EffectB]
  body)

;; With docstring — docstring comes immediately after the name
(defn name "Documentation string." [params] body)
(defn name "Documentation string." [x : TypeA] -> ReturnType body)

;; Using a type alias as a return type annotation.
;; Here my-handler takes a request and returns a *new* handler function.
;; (Handler is itself a function type, so -> Handler means "returns a function".)
(deftype-alias Handler (Fn [Request] -> Response ! [IO]))
(defn make-handler [config : Config] -> Handler
  (fn [req] (handle-with-config config req)))

;; To annotate that a function *is* a Handler (has the same signature),
;; annotate its parameters and effects individually — they must match Handler's shape:
(defn my-handler [req : Request] -> Response ! [IO]
  body)
```

Parameters are annotated with `: Type` inline. The return type is `-> ReturnType` after the closing `]`. Effects are `! [E1 E2]` after the return type. All three are optional; the compiler infers anything not annotated.

Protocol constraints on type parameters use `:where` after the effects annotation (see §5.11):

```clojure
(defn sort [xs : (Vec a)] -> (Vec a)
  :where [(Ord a)]
  (coll/sort xs))
```

Contract clauses (`:requires`, `:ensures`, `:examples`) come after `:where` (or after effects if no `:where`), before the body. All are optional and may appear in any order (see §4.2.1).

```clojure
;; With contracts
(defn sort-users [users : (Vec User)] -> (Vec User)
  :requires [(not-empty? users)]
  :ensures  [(= (count result) (count users))
             (monotone-by? :name result)]
  :examples [{:in [[[{:name "Bob"} {:name "Alice"}]]] :out [{:name "Alice"} {:name "Bob"}]}]
  (sort-by :name users))
```

Multi-arity:

```clojure
(defn greet
  ([] -> Str "Hello!")
  ([name : Str] -> Str (str "Hello, " name "!")))
```

### 4.2.1 Function Contracts

Contracts embed behavioural specifications directly in function definitions. They serve three purposes simultaneously: documenting intent, guiding AI code generation (agents have concrete input/output context), and providing automatic test cases.

#### `:requires` — Preconditions

A vector of boolean expressions over the function's parameters. Evaluated before the body. A failing precondition signals a caller bug — the error is a `:precondition-failed` panic, not a `Result` error.

```clojure
(defn divide [n : Int d : Int] -> Ratio
  :requires [(not= d 0)]
  (/ n d))

(defn head [xs : (Vec a)] -> a
  :requires [(not-empty? xs)
             (vector? xs)]
  (get xs 0))
```

#### `:ensures` — Postconditions

A vector of boolean expressions. The special binding `result` refers to the function's return value. Evaluated after the body in development/test mode. A failing postcondition signals an implementor bug.

```clojure
(defn sort [xs : (Vec Int)] -> (Vec Int)
  :ensures [(= (count result) (count xs))
            (monotone? result)
            (same-elements? result xs)]
  (coll/sort xs))

(defn register! [email : Str] -> (Result User Error) ! [Db]
  :ensures [(match result
              (Ok user) (= (:email user) email)
              _         true)]
  ...)
```

#### `:examples` — Concrete Input/Output Pairs

A vector of `{:in [...] :out value}` maps providing concrete test cases. Each `:in` entry is a vector of argument values (one per parameter). Examples are:

- Run automatically by `nexl test` and the `:test` REPL command
- Included in generated documentation
- Embedded in the structured REPL protocol response so AI agents can validate generated code against them before submission

```clojure
(defn fibonacci [n : Int] -> Int
  :requires [(>= n 0)]
  :ensures  [(>= result 0)]
  :examples [{:in [0]  :out 0}
             {:in [1]  :out 1}
             {:in [10] :out 55}
             {:in [20] :out 6765}]
  (match n
    0 0
    1 1
    _ (+ (fibonacci (- n 1)) (fibonacci (- n 2)))))
```

#### Runtime Behaviour

| Mode | `:requires` | `:ensures` |
|------|-------------|------------|
| Development (default) | Checked, panic on failure | Checked, panic on failure |
| Test (`nexl test`) | Checked | Checked |
| Release (`--release`) | Elided (no runtime cost) | Elided (no runtime cost) |
| Release + keep (`--keep-contracts`) | Checked | Checked |

The compiler uses `:requires` predicates to narrow types within the function body, reducing redundant checks. A `:requires [(string? x)]` on an `Any`-typed param narrows `x` to `Str` inside the body.

#### Querying Contracts via REPL

```
nxl> :contract fibonacci
;; :requires [(>= n 0)]
;; :ensures  [(>= result 0)]
;; :examples 4 cases

nxl> :test fibonacci
;; Running examples...
;; ✓ fibonacci(0) = 0
;; ✓ fibonacci(1) = 1
;; ✓ fibonacci(10) = 55
;; ✓ fibonacci(20) = 6765
;; 4/4 examples passed
```

### 4.3 `fn` — Anonymous Function

```clojure
(fn [x y] (+ x y))
(fn [x] -> Int (* x x))
(fn [& rest] (count rest))    ; variadic: rest is a Vec
```

`fn` always produces a value. The capitalised form `Fn` is the corresponding **type** — see §5.3.

### 4.4 `let` — Local Bindings

```clojure
(let [x 10
      y (+ x 5)]
  (* x y))
```

Bindings are evaluated sequentially; each binding is in scope for subsequent bindings and the body. Destructuring is supported in binding positions (see §4.11).

Mutable local:

```clojure
(let [mut total 0]
  (each [x items]
    (set! total (+ total x)))
  total)
```

`mut` locals are lexically scoped to their enclosing `let` block. `set!` is valid anywhere within that scope — including nested `let`, `do`, `match` arms, and loop bodies — but `mut` bindings **cannot be captured by closures** (the compiler enforces this). `set!` assigns a new value; the binding must already be declared `mut`.

### 4.5 `if` — Conditional

```clojure
(if condition then-expr else-expr)
```

Both branches must have compatible types. Only one branch is evaluated. The condition must be `Bool`; non-boolean conditions are a compile error.

### 4.6 `cond` — Multi-branch Conditional

```clojure
(cond
  (< x 0)  :negative
  (= x 0)  :zero
  :else     :positive)
```

Each test expression must be `Bool`. Tests are evaluated top-to-bottom; the value of the first matching branch is returned. `:else` is a special marker that always matches. If no branch matches at runtime (no `:else` and no test evaluated to `true`), a panic is raised. The compiler emits a **warning** when `:else` is absent, recommending that the programmer either add an `:else` branch or confirm that one of the tests is exhaustive. For exhaustiveness enforcement based on type, use `match` instead of `cond`.

### 4.7 `when` / `unless`

```clojure
(when condition body...)     ; (if condition (do body...) unit)
(unless condition body...)   ; (if (not condition) (do body...) unit)
```

Both are macros. The condition must be `Bool`. `when` returns `unit` when condition is false; `unless` returns `unit` when condition is true.

### 4.8 `do` — Sequential Evaluation

```clojure
(do expr1 expr2 expr3)
```

Evaluates expressions in order; returns the value of the last. Prior expressions are evaluated only for side effects.

### 4.9 `match` — Pattern Matching

```clojure
(match value
  pattern1 result1
  pattern2 :when guard result2
  _         default)
```

Patterns are matched top-to-bottom. The compiler enforces exhaustiveness: all cases must be covered for the given type. An exhaustiveness warning is issued when matching on `Any`.

#### Basic Pattern Forms

```clojure
42                       ; literal Int match
3.14                     ; literal Float match
:keyword                 ; keyword match
"string"                 ; exact string match
true / false / unit      ; literal bool/unit
x                        ; bind to x (any value)
_                        ; wildcard — discard value, match anything
[a b c]                  ; vector — exact length 3
[a b & rest]             ; vector — head elements + tail
{:key a :other b}        ; map — any map with at least these keys
(SomeType x)             ; ADT constructor match
(SomeType x) :when pred  ; constructor match with guard predicate
```

#### Or-Patterns `(| p1 p2 ...)`

Match if any of the sub-patterns matches. All sub-patterns must bind the same names.

```clojure
(match status
  (| :pending :processing) :in-flight
  :done                    :complete
  :failed                  :error)

;; Works with constructors too — bind the same names in each branch
(match event
  (| (Click x y) (Tap x y)) (handle-pointer x y)
  (Key k)                    (handle-key k))

;; In let destructuring:
(let [(| :left :right) side] ...)   ; matches either keyword
```

#### Pin Operator `^`

Matches against a variable's *current value* instead of binding a new name. Prevents accidental shadowing.

```clojure
(let [expected :ok]
  (match result
    ^expected  :matched          ; matches only if result = :ok
    _          :no-match))

;; Useful in sequence patterns to match a known sentinel
(let [sep :|]
  (match tokens
    [a ^sep b] (parse-binary a b)
    [a]        (parse-unary a)))

;; In map destructuring
(let [known-version 2]
  (match request
    {:version ^known-version :keys [body]} (handle-v2 body)
    {:version v}                           (unsupported v)))
```

#### Named Captures `:as`

Binds the matched value to a name *in addition to* destructuring it. Available in all pattern positions, not just maps.

```clojure
;; ADT: name the whole value AND bind inner fields
(match user
  (Admin {:keys [name email]} :as admin) (log-admin admin name))

;; Vector: name the whole sequence AND bind elements
(match coll
  [head & tail :as all] (when (> (count all) 1) (process head tail)))

;; Nested: name an intermediate node
(match data
  {:address {:city city :zip zip :as addr} :as person}
  (ship addr person))
```

For maps, `:as` continues to work at the top level (Clojure compatibility):

```clojure
(match person
  {:keys [name age] :as p} (process p name age))
```

#### Type-Asserting Patterns `(: Type pat)`

Asserts the matched value has the given type, then applies `pat` to it. Checked at compile time where possible; a runtime assertion is inserted otherwise.

**Note:** In patterns, the type comes *before* the name — `(: Int n)` — because patterns are prefix S-expressions. In function signatures, the type comes *after* the name — `[n : Int]` — because `:` acts as an infix annotation separator. The two contexts use different syntax for the same reason Lisp uses prefix notation: patterns are nested, composable forms where `(: (Vec Int) xs)` reads naturally, while signatures are flat declaration lists where `[x : Int, y : Str]` reads naturally.

```clojure
(match x
  (: Int n)    :when (> n 0)  :positive-int
  (: Str s)                   :string
  (: Bool b)                  :bool
  _                           :other)

;; Useful when matching on Any or a union type
(defn describe [x : Any] -> Str
  (match x
    (: Int n)         (str "integer: " n)
    (: Str s)         (str "string: " s)
    (: (Vec Int) xs)  (str "int vector of length " (count xs))
    _                 "unknown"))
```

#### View Patterns `(view fn pat)`

Applies a function to the matched value, then matches the result against `pat`. The original value is not bound; only `pat`'s bindings are in scope.

```clojure
;; Apply a field accessor before matching
(match user
  (view :role :admin)  :admin
  (view :role :user)   :regular
  _                    :unknown)

;; Apply count before matching
(match items
  (view count 0)  :empty
  (view count 1)  :singleton
  _               :many)

;; Apply a parsing function
(match raw-str
  (view parse-int (Some n)) :when (> n 0)  (process n)
  (view parse-int None)                    (error "not a number")
  _                                         :unexpected)

;; Compose with other patterns
(match response
  (view :status (| 200 201))  :success
  (view :status (: Int code)) :when (>= code 400)  (error code))
```

#### And-Patterns `(& p1 p2 ...)`

Matches only if *all* sub-patterns match the same value. Every sub-form must be a **structural pattern** — `(: Type p)`, `(Constructor ...)`, `[...]`, `{...}`, `(view f p)`, `(| ...)`, `^var`, literals, or bare binding names. Boolean conditions are not allowed inside `(&)`; use `:when` at the arm level for guards.

`:as` capture is still valid as the final sub-form.

```clojure
;; Type assertion + structural match
(match x
  (& (: Int n) (view abs m))  :when (> n 0)  :positive-int
  _                                           :other)

;; ADT match + named capture
(match result
  (& (Ok {:keys [id]}) :as validated)  :when (uuid? id)
  (process validated id))

;; Two structural patterns on the same value
(match items
  (& (view count n) [first & _])  :when (> n 0)
  (str "non-empty, first is " first))

;; Type assertion + named capture with guard
(match event
  (& (: MouseEvent e) :as raw)  :when (and (= (:button e) :left) (pos? (:x e)))
  (handle-click (:x e) (:y e)))
```

Guards always use `:when` at the arm level:

```clojure
;; Simple guard
(match n
  x :when (even? x)  :even
  _                   :odd)

;; Structural and-pattern with guard
(match n
  (& (: Int x) (view abs a))  :when (even? x)  :even-int
  _                                              :other)
```

#### String Interpolation Patterns

Matches a string against a template and extracts the interpolated portions. Templates use the same `{name}` syntax as string literals. Compiled to efficient string splitting / regex at compile time.

```clojure
;; Extract from structured strings
(match url
  "https://{host}/{path}"  {:host host :path path}
  "http://{host}/{path}"   {:host host :path path}
  _                        None)

(match log-line
  "[{level}] {timestamp}: {message}"
  {:level level :timestamp timestamp :message message}
  _ None)

;; Works in let too (irrefutable if the template has no literal prefix/suffix):
;; Use if-let for safety with refutable string patterns (§4.12)
(if-let ["Error: {msg}" line]
  (handle-error msg)
  (handle-ok line))
```

**Matching algorithm:** The template is split at literal segments. Each capture `{name}` matches the substring between its surrounding literals. The algorithm is:

1. The template is implicitly anchored at both the start and end of the input string. A leading literal must appear at position 0; a trailing literal must end at the last character.
2. Literal segments are scanned left-to-right. Each literal is matched at the **earliest** position in the remaining input (greedy-left).
3. Each capture binds the text between its surrounding literals. A capture between two literals that are adjacent in the input binds the empty string `""`.
4. The last capture (with no literal after it) extends to the end of the string.
5. Adjacent captures with no literal separator (`"{a}{b}"`) are a **compile error** — the boundary is ambiguous.

This means `"https://{host}/{path}"` splits on `"https://"` and `"/"`: `host` gets everything between `"https://"` and the **first** `"/"`, and `path` gets the rest. If the input is `"https://example.com/foo/bar"`, then `host` = `"example.com"` and `path` = `"foo/bar"`.

A leading capture with no preceding literal is anchored to position 0: `"{a}/{b}"` on `"x/y/z"` gives `a` = `"x"`, `b` = `"y/z"` (first `/` matched). If the separator is not found, the pattern fails.

The empty-binding form `"{x}"` matches the entire string (equivalent to just `x`).

#### `defpattern` — Named Reusable Patterns

Defines a named pattern that can be used in `match` arms and `let` bindings. Patterns are compile-time macro expansions; they have no runtime overhead.

```clojure
(defpattern pattern-name [bindings...]
  pattern-form)
```

```clojure
;; A pattern that matches positive integers
(defpattern pos-int [n]
  (: Int n) :when (pos? n))

;; A pattern that matches an authenticated admin user
(defpattern admin [{:keys [name email permissions]}]
  {:role :admin :active true :keys [name email permissions]})

;; A pattern for non-empty collections
(defpattern non-empty [first rest]
  [first & rest])

;; Use them:
(match x
  (pos-int n)     (process n)
  _               (error "expected positive int"))

(match user
  (admin {:keys [name permissions]})  (admin-view name permissions)
  _                                   (redirect :login))

(match coll
  (non-empty head tail)  (reduce f head tail)
  []                     default-value)
```

Patterns may be parameterised:

```clojure
;; A pattern parameterised by a predicate
(defpattern satisfies [pred x]
  x :when (pred x))

(match n
  (satisfies even? k)  (process-even k)
  (satisfies odd? k)   (process-odd k))
```

**Expansion semantics:** A `defpattern` expands as an **arm-level fragment** — the pattern form and any trailing `:when` guard are spliced into the match arm at the call site. For example, `(pos-int n)` expands to `(: Int n) :when (pos? n)`, so the full arm `(pos-int n) (process n)` becomes `(: Int n) :when (pos? n) (process n)`. If the call site also has a `:when`, the guards are combined with `and`: `(pos-int n) :when (even? n) ...` expands to `(: Int n) :when (and (pos? n) (even? n)) ...`.

#### Natural Language Patterns `:nl`

`:nl` matches a value against a plain English description, evaluated by the `LLM` effect at runtime. It is the bridge between formal control flow and the natural language reasoning that agents already use internally.

```clojure
(match user-message : Str ! [LLM]
  :nl "user wants to cancel their subscription"  (cancel-flow user)
  :nl "user has a billing or payment question"    (billing-flow user)
  :nl "user is reporting a bug or technical issue" (bug-report-flow user)
  _                                               (general-support user))
```

`:nl "description"` desugars to `(view (fn [v] (LLM/classify "description" v)) true)` — it applies the `LLM` effect operation `classify` to the matched value and succeeds when the result is `true`. Because it uses the `LLM` effect, any function containing `:nl` patterns must declare `! [LLM]`.

**Rules:**
- Arms are tested top-to-bottom; the first matching classification wins.
- `:nl` patterns **never contribute to exhaustiveness** — an LLM classification can always return `false` for any input. The compiler requires a structural catch-all arm (typically `_`) whenever any `:nl` pattern appears in a `match`. Omitting the catch-all is a compile error.
- `:nl` patterns can be mixed freely with structural patterns in the same `match`.

```clojure
;; Mix structural and NL patterns
(match event
  (Click x y)            (handle-click x y)     ; structural first
  :nl "user is angry"    (de-escalate event)     ; NL fallback
  _                      (log-and-ignore event))
```

**Testing `:nl` patterns:** because `LLM` is an effect, replace it with a deterministic mock handler in tests:

```clojure
(defn mock-llm-handler [descriptions]
  {LLM {:classify (fn [desc val]
                    ;; match by keyword presence for unit tests
                    (str/includes? (str/lower (str val))
                                   (first (str/split (str/lower desc) " "))))}})

(handle (mock-llm-handler [...])
  (handle-message "cancel my account"))
```

### 4.10 `loop` / `recur` — Tail-Recursive Loop

```clojure
(loop [i 0, acc []]
  (if (>= i 10)
    acc
    (recur (+ i 1) (append acc (* i i)))))
```

`recur` jumps back to the nearest `loop` (or function boundary for tail-recursive functions) with new values. The compiler enforces that `recur` is always in tail position.

### 4.11 Destructuring

Destructuring is available in `let`, `fn`, `defn`, `for`, `each`, and `match` binding positions. All pattern forms from §4.9 are valid wherever a destructuring pattern is accepted.

#### Vector Destructuring

```clojure
;; Positional binding
(let [[a b c] [1 2 3]] ...)

;; Head and tail
(let [[head & tail] coll] ...)

;; Named capture: bind the whole vector AND destructure it
(let [[head & tail :as coll] items] ...)

;; Nested vector destructuring
(let [[[x1 y1] [x2 y2]] points] ...)

;; Skip elements with _ (wildcard)
(let [[_ second _ fourth] coll] ...)
```

#### Map Destructuring

```clojure
;; Key shorthand: bind values of keywords to same-named locals
(let [{:keys [name age email]} person] ...)

;; Explicit rename: bind value of :name as n
(let [{n :name, a :age} person] ...)

;; Default values for missing keys
(let [{:keys [name] :or {name "World", role :user}} opts] ...)

;; Named capture: bind the whole map AND destructure it
(let [{:keys [name age] :as person} data] ...)

;; Capture remaining keys after destructuring (`:rest`)
;; rest-map contains all keys not listed in :keys
(let [{:keys [name age] :rest attrs} person]
  attrs)   ; => {:email "..." :role "..." ...}

;; Namespace key shorthand: :ns/keys extracts :ns/key and binds as key
(let [{:user/keys [name email role]} record] ...)
;; equivalent to: (let [{name :user/name, email :user/email, role :user/role} record] ...)

;; Multiple namespace shorthands in one pattern
(let [{:http/keys  [status body]
       :meta/keys  [timestamp request-id]} response] ...)

;; Deep path extraction: use a vector of keywords as the key to call get-in.
;; Only vectors of keywords are valid path-keys; this is unambiguous because
;; a vector of keywords is never a valid map key in normal Nexl code.
(let [{[:address :city]  city
       [:address :zip]   zip
       [:address :country :code] country-code} user] ...)

;; Computed key: evaluate an expression to produce the key
(let [{(config/primary-key) value} record] ...)

;; Nested map destructuring
(let [{:keys [name]
       {:keys [street city]} :address} person] ...)
```

#### ADT Destructuring in `let`

All pattern forms from §4.9, including ADT constructor patterns, are valid in `let`. The compiler classifies patterns as **irrefutable** (always match for the given type) or **refutable** (may not match).

- Irrefutable patterns: map destructuring, wildcard, variable binding, `[head & tail]` (head-and-tail vector patterns) — these are always safe in plain `let`.
- Conditionally irrefutable: fixed-length vector patterns like `[a b c]` are irrefutable only if the type guarantees the length (e.g., when the right-hand side has type `(Vec3 a)` or the compiler can infer a fixed-length collection). When matching against a general `(Vec a)`, fixed-length vector patterns are **refutable** because the vector may have a different length.
- Refutable patterns: ADT constructors, literal values, pin patterns, string templates, fixed-length vector patterns on dynamic-length types — these require `if-let`, `when-let`, or `let-else` (see §4.12).

The compiler emits an error if a refutable pattern appears in a plain `let` without the appropriate form.

```clojure
;; Irrefutable — safe in plain let
(let [[head & tail] items]    ...)  ; always matches (head is first, tail may be empty)
(let [[a b] pair]             ...)  ; irrefutable only if pair's type guarantees length 2
(let [{:keys [x]} m]      ...)  ; always matches (x is None if :x absent, with type (Option V))
```

**Absent key semantics:** When a map key used in `:keys` destructuring is absent, the bound variable has type `(Option V)` and value `None`. Use `:or` to provide a default value, in which case the type is `V` directly. For record types (§5.7), all fields are always present; absent-key semantics apply only to open maps.

```clojure
;; Refutable — compile error in plain let, use if-let or let-else
(let [(Some x) (find-user 42)] ...)  ; ERROR: (Some _) does not cover None
(let [(Ok x) (risky-op)]       ...)  ; ERROR: (Ok _) does not cover (Err _)
```

#### Type-Asserting Bind `(: Type pattern)`

Asserts the bound value has the given type. Checked at compile time where possible; a runtime assertion is inserted otherwise. Works in all binding positions.

```clojure
(let [(: Str name) (get-value key)] ...)
(let [(: Int n) (parse-thing s)?] ...)

;; In function params
(defn process [(: (Vec Int) xs)] (reduce + 0 xs))

;; Combines with other patterns
(let [(: Int n :as raw) (get-val key)
      :when (pos? n)]
  (process n raw))
```

#### View Patterns in `let`

`(view fn pat)` can be used in `let` bindings. The expression is applied to the right-hand side value before matching.

```clojure
;; Bind the JSON-decoded value
(let [(view json/parse {:keys [name age]}) raw-json-str] ...)

;; Parse and bind simultaneously
(let [(view parse-int (Some n)) str-value | (Err :bad-int)]
  (process n))
```

#### Or-Patterns and Pin Patterns in `let`

Both are available in `let` with the same semantics as in `match`. Or-patterns in `let` are irrefutable if all alternatives are irrefutable.

```clojure
;; Pin: match but don't rebind
(let [key :name
      ^key value]          ; matches only if key = :name (trivially true here, but useful in loops)
  ...)

;; In function params with pin
(defn check [expected actual]
  (let [^expected actual] :ok))  ; panics if actual != expected
```

### 4.12 Conditional and Refutable Destructuring

These forms handle patterns that may not match. They are required whenever a refutable pattern appears outside of `match`.

#### `if-let` — Branch on Destructuring Success

```clojure
(if-let [pattern expr]
  then-branch
  else-branch)
```

Evaluates `expr`. If it matches `pattern`, binds the pattern's names and evaluates `then-branch`. Otherwise evaluates `else-branch` (with no new bindings).

```clojure
;; Unwrap an Option
(if-let [(Some user) (find-user id)]
  (render-profile user)
  (render-not-found))

;; Unwrap a Result
(if-let [(Ok result) (try-parse input)]
  (process result)
  (error "bad input"))

;; Multiple bindings (all must match for then-branch)
(if-let [(Some a) (op1)
         (Ok b)   (op2)]
  (combine a b)
  (fallback))

;; Works with any pattern from §4.9
(if-let ["User:{id}" line]
  (handle-user id)
  (skip line))
```

#### `when-let` — Match or Return `unit`

Like `if-let` but with no else branch. Returns `unit` if the pattern does not match. Equivalent to `(if-let [...] body unit)`.

```clojure
(when-let [(Some user) (current-user)]
  (log-access user))
;; => user info logged, or unit

;; Chain: each binding must succeed
(when-let [(Ok conn)    (db/connect config)       ; module function, performs Db
           (Some table) (db/find-table conn "users")] ; module function, performs Db
  (Db/query "SELECT * FROM users" []))
```

#### `let-else` — Refutable `let` with Fallback Expression

```clojure
;; Single binding
(let [pattern expr | fallback-expr]
  body)

;; Multiple bindings — each may have its own fallback
(let [pattern1 expr1 | fallback1
      pattern2 expr2 | fallback2
      ...]
  body)
```

Each binding is written as `pattern expr | fallback-expr`. The `|` operator divides every line into two reading lanes:

- **Left of `|`** — the happy path: what you expect to bind and use.
- **Right of `|`** — the failure case: what to produce if the pattern does not match.

This layout lets readers scan a binding vector top-to-bottom twice: once down the left side to follow the nominal logic, once down the right side to audit all failure modes — without interleaving the two concerns.

**Evaluation semantics.** Bindings are evaluated sequentially; each may reference names introduced by earlier bindings in the same vector. If a pattern fails, its `fallback-expr` is evaluated and its value becomes the value of the entire `let` expression — the remaining bindings and `body` are skipped. The multi-binding form desugars to nested single-binding `let-else` forms.

**Type rule.** Each `fallback-expr` must have a type compatible with the expected type of the enclosing expression: either the same type as `body`, or `Never` (a diverging expression like `(panic ...)`). This is ordinary type-checking, not a special syntactic restriction.

Plain (irrefutable) bindings may appear alongside fallible ones — they have no `|` clause and always succeed.

```clojure
;; Single fallible binding
(let [(Ok n) (parse-int input)  | (Err (ParseError input))]
  (use n))

;; Panic: type is Never, satisfies any expected type
(let [(Some cfg) (load-config)  | (panic "config must be present at startup")]
  (start-server cfg))

;; Works with any refutable pattern
(let ["Bearer {token}" auth-header  | (Err :missing-token)]
  (validate-token token))

;; Multiple fallible bindings in one let — flat and readable
(defn process! [input] -> (Result Output Error) ! [IO]
  (let [(Ok n)      (parse-int input)  | (Err (ParseError input))
        (Some data) (fetch-data! n)    | (Err :not-found)
        (Ok result) (transform data)   | (Err :transform-failed)]
    (Ok result)))

;; Mix of plain and fallible bindings
;; auth/verify and db/find are module functions; json/parse and json/encode are pure.
;; Effects propagate from auth/verify (Net) and db/find (Db).
(defn handle-request! [req] -> (Result Response Error) ! [Db Net Log]
  (let [req-id          (gen-id)
        (Ok user)       (auth/verify (:token req))  | (Err :unauthorized)
        (Ok body)       (json/parse (:body req))     | (Err :bad-json)
        (Some record)   (Db/query user (:id body))   | (Err :not-found)]
    (Ok {:status 200 :body (json/encode record)})))
```

The `|` is part of the binding form syntax, not a pattern operator.

### 4.13 Threading Macros

```clojure
;; -> threads value as first argument
(-> data
    (filter (fn [x] (> (:score x) 80)))
    (map :name)
    sort
    (take 10))

;; ->> threads value as last argument
(->> items
     (group-by :category)
     (map-vals count)
     (sort-by val >))

;; as-> threads with named position
(as-> x $
  (str $ " world")
  (str/upper $))
```

### 4.14 `for` — Sequence Comprehension

```clojure
(for [x (range 10)
      y (range x)
      :when (even? (+ x y))]
  [x y])
```

Returns an `(Iter a)` — a lazy sequence whose elements are computed on demand (see §5.12). Use `vec`, `set`, or `into` to force evaluation into a concrete collection. `:when` filters elements. `:let` adds bindings. `:while` terminates when false. If the body performs effects, the compiler rejects `for` (use `for!` or `each` instead).

### 4.15 `each` / `times`

```clojure
(each [x coll] (println x))     ; iterate for side effects, returns unit
(times [i 5] (println i))       ; i from 0 to 4, returns unit
```

### 4.16 `declare`

```clojure
(declare my-fn)
```

Forward-declares a name to allow mutual recursion within the same module. Must be defined before the end of the module.

---

## 5. Type System

### 5.1 Overview

Nexl uses **bidirectional type inference** with the following features:

- **Inference-first**: most code requires no annotations; types are inferred from usage
- **Row polymorphism**: functions can operate on any record with at least the fields they need
- **Algebraic data types**: sum types with exhaustive pattern matching
- **Effect rows**: effect types tracked in function signatures (see §6)
- **`Any` escape hatch**: explicit opt-out of static typing at a hard boundary

### 5.2 Primitive Types

`Int`, `Float`, `Ratio`, `Bool`, `Char`, `Str`, `Keyword`, `Symbol`, `Unit`, `Any`.

Fixed-width numeric types: `Int8`, `Int16`, `Int32`, `Int64`, `U8`, `U16`, `U32`, `U64`, `F32`, `F64`.

`Int64` is an alias for `Int` — they are the same type. `F64` is an alias for `Float` — they are the same type.

For generic numeric code, use protocol constraints: `:where [(Numeric a)]` for all numbers, `:where [(IntLike a)]` for integers, `:where [(FracLike a)]` for fractional types (see §5.11).

### 5.3 Composite Types

```
(Vec a)               ; Vector of a
(Map k v)             ; Map from k to v
(Set a)               ; Set of a
(List a)              ; Linked list of a
(Tuple a b)           ; 2-element heterogeneous product
(Tuple a b c)         ; 3-element heterogeneous product  (up to 8 elements)
(Fn [A B] -> C)       ; Function type from A, B to C  (cf. fn — value form, §4.3)
(Fn [A] -> C ! [E])   ; Function type from A to C with effects E
(Atom a)              ; Mutable atom holding a
(Task a)              ; Concurrent task producing a
Never                 ; Bottom type — no values; type of diverging expressions
```

**`Fn` vs `fn`:** The capitalised `Fn` is a **type expression** used in type annotations and `defprotocol` operation signatures: `(Fn [Int] -> Str)`. The lowercase `fn` is a **value expression** that creates an anonymous function: `(fn [x] (+ x 1))`. In the EBNF grammar (Appendix D), the production `fn-type` describes the type form `(Fn [...] -> ...)`. The distinction is always from context: types use `Fn`, values use `fn`.

### 5.4 Type Inference

Inference is bidirectional:

- **Check mode**: expression is checked against a known expected type
- **Synthesize mode**: expression's type is inferred from its structure

Inference propagates across module boundaries. Recursive functions require either a type annotation or that the recursion be in tail position with inferrable base cases.

### 5.5 Type Annotations

Annotations are optional except where inference would be ambiguous or circular.

```clojure
;; Top-level binding
(def x : Int 42)

;; Function: params and return type inline
(defn add [x : Int y : Int] -> Int
  (+ x y))

;; Function: with effects
(defn read-config! [path : Str] -> Bytes ! [FileSystem]
  (FileSystem/read-file path))

;; Local binding
(let [x : Int 42] ...)

;; Expression annotation — type first, same order as type assertion patterns (§4.9)
(: Type expr)
(: Int (+ 1 2))            ; assert this expression has type Int

;; (Fn [...] -> ...) is a type expression — describes function types.
;; Used in type aliases, let annotations, and parameter types.
(deftype-alias Transformer (Fn [Int] -> Str))
(deftype-alias Handler (Fn [Request] -> Response ! [IO]))
(let [f : (Fn [Int] -> Str) int->str] ...)
(defn apply-twice [f : (Fn [a] -> a) x : a] -> a (f (f x)))
```

### 5.6 Row Polymorphism

Records (maps with known structure) use row polymorphism. The syntax `{field : Type | _}` means "a record with at least this field."

```clojure
(defn greet [person : {name : Str | _}] -> Str
  (str "Hello, " (:name person)))

(greet {:name "Alice" :age 30})   ; OK — has :name
(greet {:name "Bob"})             ; OK — has :name
(greet {:age 30})                 ; Compile error: missing :name
```

Row variables may be made explicit for polymorphic record functions:

```clojure
(defn add-field [r : {| r}] -> {extra : Int | r}
  (put r :extra 42))
```

**Row types in return position.** When a function's return type is annotated with a row type (`{... | _}`), the concrete nominal type is **existentially hidden** — the caller sees only the fields specified in the row type, not the full concrete type:

```clojure
;; Caller sees Point:
(defn make-point [] -> Point (Point {:x 1.0 :y 2.0}))

;; Caller sees {x : Float | _} — Point identity hidden:
(defn make-thing [] -> {x : Float | _} (Point {:x 1.0 :y 2.0}))
```

When no row annotation is used, type inference preserves the nominal type through the call chain. This gives the programmer explicit control over the API surface: annotate with a row type to expose only the relevant fields, or use the nominal type to preserve full identity.

### 5.7 Algebraic Data Types

```clojure
(deftype Option [a]
  | None
  | (Some a))

(deftype Result [a e]
  | (Ok a)
  | (Err e))

(deftype Tree [a]
  | Leaf
  | (Branch a (Tree a) (Tree a)))
```

Constructors with arguments are functions: `Some` has type `(Fn [a] -> (Option a))`. Nullary constructors like `None` and `Leaf` are polymorphic constants — `None` has type `(Option a)` for any `a`, and can be used anywhere an `(Option a)` is expected without calling it as a function.

Pattern matching on ADTs is exhaustiveness-checked.

#### Record Types

`deftype` also supports **record syntax** for named product types with fields:

```clojure
(deftype Point {:x Float :y Float})
(deftype User {:name Str :age Int :email Str})

;; Construction: map literal syntax
(def p (Point {:x 1.0 :y 2.0}))

;; Field access: keyword on the record
(:x p)         ; => 1.0

;; Functional update: returns new value with field replaced
(put p :x 3.0)   ; => (Point {:x 3.0 :y 2.0})

;; Destructuring: same as map destructuring
(let [{:keys [x y]} p] (+ x y))
```

Record types are **nominal**: two record types with identical fields are distinct types. `(Point {:x 1.0 :y 2.0})` is not equal to `(Vec2 {:x 1.0 :y 2.0})` even if `Vec2` has the same field layout.

Record types participate in row polymorphism — a function accepting `{x : Float | _}` will accept a `Point` because it has field `:x` of type `Float`.

Record types may have type parameters:

```clojure
(deftype Pair [a b] {:fst a :snd b})

(def p (Pair {:fst 1 :snd "hello"}))
(:fst p)     ; => 1 : Int
```

**`deftype` syntax summary.** The `deftype` form is used for three related purposes, distinguished by body shape:

1. **Sum type (ADT):** body contains `|`-prefixed constructors — `(deftype Color | Red | Green | Blue)`
2. **Record type:** body is a map literal — `(deftype Point {:x Float :y Float})`
3. **Combined:** constructors carry record payloads — `(deftype Shape (Circle {:radius Float}) ...)`

A single-variant ADT wrapping a map is distinct from a record type: `(deftype Wrapper | (Wrapper {:x Int}))` is an ADT with constructor `Wrapper`, while `(deftype Thing {:x Int})` is a record type where `Thing` itself is the constructor. The `|` prefix unambiguously marks variants. At runtime, both have the same representation, but in patterns the ADT requires matching the constructor: `(Wrapper {:keys [x]})` vs `{:keys [x]}`.

The `:derive` clause always comes after the name and type parameters (if any), before the body:

```clojure
(deftype Color :derive [Show Eq Hash] | Red | Green | Blue)
(deftype Point :derive [Show Eq] {:x Float :y Float})
(deftype Pair [a b] :derive [Show Eq] {:fst a :snd b})
```

#### ADT + Record Combined

Constructors may carry record payloads:

```clojure
(deftype Shape
  | (Circle {:radius Float})
  | (Rect {:width Float :height Float})
  | (Point {:x Float :y Float}))

(match shape
  (Circle {:keys [radius]})       (* 3.14159 radius radius)
  (Rect {:keys [width height]})   (* width height)
  (Point _)                        0.0)
```

### 5.8 Type Aliases

```clojure
(deftype-alias UserId Str)
(deftype-alias Callback (Fn [Event] -> Unit ! [IO]))
(deftype-alias Bytes (Vec U8))
```

Aliases are transparent to the type checker (structurally equivalent to the aliased type).

`Bytes` is defined as `(Vec U8)`, providing a precise definition for raw byte sequences. All functions that accept `Bytes` also accept `(Vec U8)` and vice versa.

### 5.9 Opaque Types

`deftype-opaque` creates a **nominal wrapper** around an existing type. The wrapper is a distinct type at compile time but has zero runtime overhead — it shares the same representation as the underlying type.

```clojure
(deftype-opaque UserId Str)
(deftype-opaque Meters Float)
(deftype-opaque Feet Float)
```

**Construction and access:** Inside the defining module, `wrap` converts the underlying type to the opaque type and `unwrap` does the reverse:

```clojure
;; In the module that defines UserId:
(defn user-id [s : Str] -> UserId (wrap s))
(defn user-id->str [id : UserId] -> Str (unwrap id))
```

Outside the defining module, `wrap` and `unwrap` are not available. The opaque type cannot be constructed or destructured — only the module's exported functions provide access. This enforces invariants:

```clojure
;; Outside the module:
(user-id "alice")           ; OK — uses the exported constructor
(unwrap some-user-id)       ; Compile error: unwrap not available for UserId
(+ (Meters 1.0) (Feet 3.0)) ; Compile error: Meters ≠ Feet
```

**Protocol derivation:** Opaque types can derive protocols from their underlying type:

```clojure
(deftype-opaque Email Str
  :derive [Show Eq Hash])
```

Auto-derivation delegates to the underlying type's implementation. Custom implementations via `impl` are also supported.

**Drop hook.** An opaque type may declare a `:drop` function that is called automatically when the value's reference count reaches zero (Perceus RC, §13.3). This is primarily useful for opaque types wrapping C resources (`Ptr`):

```clojure
(defextern free-handle : (Fn [Ptr] -> Unit) "free_handle")
(deftype-opaque CHandle Ptr :drop free-handle)

;; CHandle is automatically freed when it goes out of scope —
;; no explicit deallocation needed.
(defextern open-handle  : (Fn [Str] -> CHandle) "open_handle"  :performs [FileSystem])
(defextern use-handle   : (Fn [CHandle Int] -> Int) "use_handle")
```

The `:drop` function must have type `(Fn [UnderlyingType] -> Unit)` (it receives the underlying type, not the opaque wrapper, to avoid re-wrapping). It must be pure or perform only `Unsafe` effects. The `:drop` annotation is valid only on `deftype-opaque`, not on `deftype-alias` or `deftype`.

**Contrast with type aliases:** `(deftype-alias UserId Str)` makes `UserId` transparent — it is interchangeable with `Str` everywhere. `(deftype-opaque UserId Str)` makes `UserId` nominal — it is a distinct type that happens to have the same runtime representation.

### 5.10 The `Any` Escape Hatch

Code annotated with `Any` is dynamically typed. Any function that accepts, returns, or manipulates `Any` values carries the `! [Dynamic]` effect in its signature. This makes the boundary between the typed and dynamic worlds **explicit and trackable** through the effect system.

```clojure
(defn parse-unknown [x : Any] -> Str ! [Dynamic]
  (assert-type x Str))
```

`assert-type` performs the `Dynamic/type-error` effect operation if the runtime check fails. The `Dynamic` effect is defined as:

```clojure
(defeffect Dynamic
  (type-error : (Fn [Str Str] -> Unit)))   ; expected-type, actual-type
```

Handlers for `Dynamic` can recover from type errors, log them, or provide defaults:

```clojure
(handle [Dynamic
          (type-error [resume expected actual]
            (Log/warn (str "Expected " expected ", got " actual))
            (resume unit))]
  (parse-unknown some-value))
```

Functions that never touch `Any` have no `Dynamic` in their effect row — the effect system ensures the dynamic world cannot silently infect typed code. This is a **hard boundary**: there is no implicit coercion between the typed and dynamic worlds.

### 5.11 Protocols

Protocols define named sets of operations that types can implement. They are Nexl's mechanism for ad-hoc polymorphism — the way user-defined types participate in generic operations like string conversion, equality, iteration, and collection processing.

#### Declaring Protocols

```clojure
(defprotocol ProtocolName
  "Optional documentation string."
  (operation-name : (Fn [Self ArgTypes...] -> ReturnType))
  ...)
```

`Self` refers to the implementing type. Each operation must have `Self` as its first parameter (the dispatch target).

```clojure
(defprotocol Show
  "Convert a value to a human-readable string."
  (show : (Fn [Self] -> Str)))

(defprotocol Eq
  "Structural equality."
  (eq? : (Fn [Self Self] -> Bool)))

(defprotocol Ord
  "Total ordering. Assumes Eq."
  :extends [Eq]
  (compare : (Fn [Self Self] -> (| :lt :eq :gt))))

(defprotocol Hash
  "Produce a deterministic hash. Assumes Eq: equal values must hash equal."
  :extends [Eq]
  (hash-code : (Fn [Self] -> Int)))
```

#### Implementing Protocols

```clojure
(impl TypeName
  ProtocolName
  (operation-name [self args...] body)
  ...)
```

```clojure
(deftype Point {:x Float :y Float})

(impl Point
  Show
  (show [p] (str "(" (:x p) ", " (:y p) ")"))

  Eq
  (eq? [a b] (and (= (:x a) (:x b)) (= (:y a) (:y b))))

  Ord
  (compare [a b]
    (let [cx (compare (:x a) (:x b))]
      (if (= cx :eq) (compare (:y a) (:y b)) cx))))
```

#### Coherence Rules

`impl` is subject to the **orphan rule**: an implementation is allowed only if the current module defines the **type** or the **protocol** (or both). A third-party module may not create implementations for types and protocols it does not own.

```clojure
;; In module that defines Point:
(impl Point Show ...)          ; OK — owns Point

;; In module that defines Serializable:
(impl Point Serializable ...)  ; OK — owns Serializable

;; In a module that owns neither:
(impl Point Show ...)          ; Compile error: orphan implementation
```

This prevents incoherent instances where two modules provide conflicting implementations for the same type-protocol pair. If you need to adapt a foreign type to a foreign protocol, use an opaque wrapper (§5.9):

```clojure
(deftype-opaque MyPoint point/Point :derive [Show])
;; Or: (impl MyPoint ForeignProtocol ...)
```

For multi-parameter protocols like `Into`, the rule extends naturally: you can implement `(Into Target)` for `Source` if you own `Source` OR you own `Target`.

#### Implementing Parameterized Protocols

When a protocol has a type parameter (e.g., `Foldable [a]`), the `impl` form specifies the element type explicitly using a type-application syntax:

```clojure
;; Implement Foldable Int for Vec Int:
(impl (Vec Int)
  (Foldable Int)
  (fold [f init self]
    ;; implementation for Vec Int
    ...))

;; Generic implementation using a type variable:
(impl (Vec a)
  (Foldable a)
  (fold [f init self]
    ;; implementation for any Vec a
    ...))
```

The `impl` form's first argument is the **type being implemented for** (the `Self` type). When the protocol has a parameter, it appears as `(ProtocolName ParamType)` — a type application. The compiler requires the `:where` clause if the param type is a variable:

```clojure
;; Generic Vec fold — all element types at once:
(impl (Vec a) (Foldable a)
  (fold [f init self]
    (loop [i 0, acc init]
      (if (= i (count self))
        acc
        (recur (+ i 1) (f acc (get self i)))))))
```

This generic `impl` is the standard way to implement parameterized protocols for built-in collections. User-defined types use the same pattern.

#### Default Implementations

Protocols may provide default implementations for operations in terms of other operations in the same protocol:

```clojure
(defprotocol Foldable [a]
  "A type whose elements can be reduced to a single value."
  (fold : (Fn [(Fn [b a] -> b) b Self] -> b))
  (count   : (Fn [Self] -> Int)
    :default (fn [self] (fold (fn [n _] (+ n 1)) 0 self)))
  (empty?  : (Fn [Self] -> Bool)
    :default (fn [self] (= 0 (count self)))))
```

#### Protocol Constraints on Functions

Functions can require that their type parameters implement specific protocols:

```clojure
(defn sort [xs : (Vec a)] -> (Vec a)
  :where [(Ord a)]
  (coll/sort xs))

(defn deduplicate [xs : (Vec a)] -> (Vec a)
  :where [(Eq a) (Hash a)]
  (vec (into #{} xs)))
```

The `:where` clause constrains the type variable `a` to types that implement the listed protocols. This is checked at call sites.

#### Auto-Derived Protocols

For ADTs and record types, the compiler can auto-derive common protocol implementations:

```clojure
(deftype Color
  :derive [Show Eq Hash]
  | Red | Green | Blue)

(deftype User
  :derive [Show Eq]
  {:name Str :age Int :email Str})
```

Auto-derivation produces canonical implementations: `Show` uses the constructor/field names, `Eq` compares structurally, `Hash` combines field hashes, `Ord` compares fields left-to-right.

#### Built-in Protocols

| Protocol | Operations | Implemented by |
|----------|-----------|----------------|
| `Show` | `show` | All primitive types, all collections |
| `Eq` | `eq?` | All primitive types, all collections |
| `Ord` | `compare` | `Int`, `Float`, `Ratio`, `Str`, `Char`, `Keyword`, `Int8`, `Int16`, `Int32`, `U8`, `U16`, `U32`, `U64`, `F32` |
| `Hash` | `hash-code` | All primitive types, all collections |
| `Foldable` | `fold` | `Vec`, `Map`, `Set`, `List`, `Option`, `Result` |
| `Buildable` | `collect` | `Vec`, `Map`, `Set`, `List` |
| `Numeric` | `add`, `sub`, `mul`, `negate` | `Int`, `Float`, `Ratio`, `Int8`, `Int16`, `Int32`, `U8`, `U16`, `U32`, `U64`, `F32` |
| `IntLike` | `div`, `mod`, `bit-and`, `bit-or` | `Int`, `Int8`, `Int16`, `Int32`, `U8`, `U16`, `U32`, `U64` |
| `FracLike` | `recip` | `Float`, `Ratio`, `F32` |

The `str` function calls `(show x)`. The `=` function calls `(eq? a b)`. The `<`, `>`, `<=`, `>=` functions call `(compare a b)`. These are syntactic conveniences, not separate mechanisms.

#### Compiler-Dispatched Overloads

The functions `map`, `filter`, and `reduce` are **compiler-dispatched overloads** — each collection and container type provides its own implementation, and the compiler resolves the correct one based on the collection argument's type. These are not protocols (which would require higher-kinded types to express generically); they are built-in overloaded functions.

```clojure
(map inc [1 2 3])           ; => [2 3 4] : (Vec Int)
(map inc (Some 1))          ; => (Some 2) : (Option Int)
(map inc '(1 2 3))          ; => '(2 3 4) : (List Int)
(filter even? [1 2 3 4])    ; => [2 4] : (Vec Int)
(reduce + 0 [1 2 3])        ; => 6 : Int
```

This is a pragmatic choice: abstracting over "any mappable container" requires higher-kinded polymorphism, which adds significant type system complexity without proportional practical benefit. Every modern effect-typed language (Koka, Gleam, Roc, Elm) makes the same trade-off.

**Note on `Numeric` and fixed-width types:** `Numeric` dispatches on `Self`, meaning `(add a b)` requires both `a` and `b` to have the same type. `(add 1i32 2u8)` is a compile error because `Int32` and `U8` are different types. Use explicit conversion: `(add 1i32 (->int32 2u8))`.

### 5.12 Iteration

Nexl's iteration model separates three concerns: **consumption** (Foldable), **lazy traversal** (Iter), and **construction** (Buildable).

#### The `Iter` Type

`(Iter a)` is a concrete algebraic data type representing a lazy sequence of elements:

```clojure
(deftype Iter [a]
  | Done
  | (Yield a (Fn [] -> (Iter a))))
```

`Iter` is **not a protocol** — it is a concrete type. This avoids the need for existential types or higher-kinded polymorphism. Any `Foldable` type can produce an `(Iter a)` via `iter`, and any `Buildable` type can be constructed from one via `collect`.

```clojure
;; All collections can produce an Iter
(iter [1 2 3])       ; => (Iter Int) — lazy
(iter {:a 1 :b 2})   ; => (Iter (Tuple Keyword Int))

;; Iter operations: map, filter, take, drop operate on Iter
(take 3 (map inc (iter (range 1000))))
;; => (Iter Int) — only 3 elements computed

;; Materialize back into a collection via Buildable
(vec (filter even? (iter [1 2 3 4])))   ; => [2 4]
(set (map str (iter [1 2 1 3])))        ; => #{"1" "2" "3"}
```

For ergonomic use, `map`, `filter`, `reduce` on concrete collections are compiler-dispatched overloads (see §5.11) that avoid the `iter`/materialize round-trip.

#### Foldable

`Foldable` is the universal consumption protocol. Everything derives from `fold`:

```clojure
;; Derived from Foldable:
(defn iter [coll : a] -> (Iter b) :where [(Foldable a)] ...)
(defn any?   [pred coll] :where [(Foldable coll)] ...)
(defn all?   [pred coll] :where [(Foldable coll)] ...)
(defn find   [pred coll] :where [(Foldable coll)] ...)
```

`Vec`, `Map`, `Set`, `List`, `Option`, and `Result` all implement `Foldable`.

#### Buildable

`Buildable` is the construction protocol — types that can be assembled from a sequence of elements:

```clojure
(defprotocol Buildable [a]
  "Types that can be constructed from an iterator."
  (collect : (Fn [(Iter a)] -> Self)))
```

`Vec`, `Map`, `Set`, and `List` implement `Buildable`. This enables generic collection conversion:

```clojure
(set (iter [1 2 1 3]))    ; => #{1 2 3}
(vec (iter #{1 2 3}))     ; => [1 2 3]
```

#### `first` and `rest`

`first` and `rest` are provided as convenience functions on all `Foldable` types:

```clojure
(first [1 2 3])       ; => (Some 1)
(rest [1 2 3])        ; => [2 3]
(first {:a 1 :b 2})   ; => (Some [:a 1])  — key-value pair
```

`rest` on an empty collection returns the empty collection of the same type:

```clojure
(rest [])     ; => []
(rest '())    ; => '()
```

Note: `cons` is a `List`-specific operation, not part of the iteration protocol. For other collections, use `append` (which adds at the collection's natural insertion point).

#### Lazy Sequences

Nexl is strict (§13.1), but `(Iter a)` provides lazy evaluation. Elements are computed on demand and memoized — each element is computed at most once.

The `for` comprehension (§4.14) returns an `(Iter a)` by default. To force evaluation into a concrete collection, use `vec`, `set`, or `into`:

```clojure
(for [x (range 10) :when (even? x)] (* x x))
;; => (Iter Int) — elements computed on demand

(vec (for [x (range 10) :when (even? x)] (* x x)))
;; => [0 4 16 36 64] — fully evaluated Vec
```

Built-in lazy producers: `range`, `iterate`, `repeat`, `cycle`.

**Interaction with effects:** Lazy sequences must not capture effectful computations. The compiler enforces this: if the body of `for` performs effects, the result type is `(Vec a)` (eager), not `(Iter a)`. To build an eager sequence from effectful operations, use `for!` explicitly:

```clojure
;; Compile error: effectful body cannot produce Iter
(for [x urls] (fetch! x))

;; Explicit eager evaluation — returns Vec, not Iter
(for! [x urls] (fetch! x))
;; => (Vec Response) ! [Net]
```

### 5.13 Refinement Types

> **Status:** Planned (Phase 7). Not in v0.1.

```clojure
(deftype Port (refine [n : Int] (and (>= n 0) (<= n 65535))))
(deftype NonEmpty (refine [s : Str] (> (count s) 0)))
```

Refinement predicates are checked at compile time where statically evaluable, and enforced by runtime assertion otherwise. Refined types are subtypes of their base types.

---

## 6. Effect System

### 6.1 Overview

Nexl uses **algebraic effects** as the universal mechanism for side effects, I/O, dependency injection, concurrency, and error handling. A function declares which effects it needs; handlers supply the implementation.

Effects are tracked in function type signatures using the `!` notation:

```clojure
(Fn [Str] -> Unit ! [Console])   ; performs the Console effect
(Fn [] -> Int)                    ; pure: no effects
```

### 6.2 Declaring Effects

```clojure
(defeffect EffectName
  (operation-name : (Fn [ArgTypes] -> ReturnType))
  ...)
```

Example:

```clojure
(defeffect Console
  (print     : (Fn [Str] -> Unit))
  (println   : (Fn [Str] -> Unit))      ; print with trailing newline
  (eprintln  : (Fn [Str] -> Unit))      ; print to stderr with trailing newline
  (read-line : (Fn [] -> Str)))

(defeffect FileSystem
  (read-file   : (Fn [Str] -> Bytes))
  (write-file  : (Fn [Str Bytes] -> Unit))
  (append-file : (Fn [Str Bytes] -> Unit))  ; append to existing file or create
  (delete-file : (Fn [Str] -> Unit))
  (list-dir    : (Fn [Str] -> (Vec Str)))
  (make-dir    : (Fn [Str] -> Unit))         ; create directory (and parents)
  (stat        : (Fn [Str] -> FileInfo)))

(defeffect Db
  (query  : (Fn [Str (Vec Any)] -> (Vec (Map Keyword Any))))
  (exec!  : (Fn [Str (Vec Any)] -> Int)))
```

Operation signatures describe only the **argument and return types**. The effect membership is implicit from the enclosing `defeffect` — writing `! [Db]` in `exec!`'s signature would be redundant and circular. When a caller invokes `Db/exec!`, the compiler adds `Db` to the caller's effect row automatically.

### 6.3 Performing Effects

There are two kinds of qualified calls in Nexl that look similar but work differently:

1. **Effect operations** (`EffectName/op-name`) — these are the primitives declared in `defeffect`. They are resolved by the effect system, not the module system.
2. **Module functions** (`module-alias/fn-name`) — these are regular functions imported from a module. A module function may *internally perform* effects, which propagate to its callers.

```clojure
;; Effect operation — declared in (defeffect Console)
(Console/print "hello")

;; Module function — imported from the net/http module, internally uses Net effect
(import net.http :as http)
(http/get "https://example.com")   ; http/get is a module function that performs Net
```

The distinction matters: `Console/print` dispatches through the effect handler system (the nearest `handle` for `Console`), while `http/get` is a normal function call where the `Net` effect propagates upward through inference.

#### Calling Effect Operations

Effect operations are in scope when there is an enclosing `handle` for that effect (or they are declared at the program's top-level handler).

Every effect operation can be called in two ways:

| Form | Resolves to |
|------|-------------|
| `(op-name args...)` | Lexical scope — local binding if present, else the effect operation |
| `(EffectName/op-name args...)` | Always the effect operation; unambiguous regardless of local bindings |

Prefer the qualified form `EffectName/op-name` when the call site benefits from clarity about which effect is being performed, or when a local name would otherwise shadow it.

```clojure
(defn greet! [] -> Str ! [Console]
  (Console/print "What is your name? ")   ; qualified — explicit
  (let [name (Console/read-line)]
    (Console/print (str "Hello, " name "!"))
    name))

;; Unqualified form is also valid when unambiguous
(defn greet! [] -> Str ! [Console]
  (print "What is your name? ")
  (let [name (read-line)]
    (print (str "Hello, " name "!"))
    name))
```

**Shadowing.** When a parameter or local `let` binding shares a name with an operation of any effect declared in the function's `!` row, the compiler emits a warning:

```
warning: local binding 'parse-int' shadows Parser/parse-int
```

The local binding takes precedence under standard lexical scoping. To call the effect operation when shadowed, use the qualified form:

```clojure
(defeffect Parser
  (parse-int : (Fn [Str] -> (Result Int ParseError))))

(defn process [parse-int : (Fn [Str] -> Int)] -> (Result Str Error) ! [Parser]
  (let [n (parse-int "42")]            ; calls the parameter — warning emitted
    (Parser/parse-int (str n))))       ; calls the effect operation — unambiguous
```

### 6.4 Handling Effects — Simple Form

```clojure
(handle [EffectName
          (operation-1 [args...] body)
          (operation-2 [args...] body)]
  body...)
```

Each operation is defined inline with the same argument list as its declaration in `defeffect`. The handler's return value is implicitly passed back to the caller of the effect operation.

```clojure
;; Handler implementations use low-level runtime primitives (from the rt module),
;; not effect operations — the handler IS the effect implementation.
(import nexl.rt :as rt)

(handle [Console
          (print [s] (rt/stdout-write s))
          (read-line [] (rt/stdin-read-line))]
  (greet!))
```

Multiple effects may be handled in a single `handle` form:

```clojure
(handle [Console
          (print [s] (rt/stdout-write s))
         Log
          (info [msg] (rt/stdout-write (str "[INFO] " msg)))]
  (body))
```

**Named handlers.** Instead of defining operations inline, `handle` can reference a named handler defined with `defhandler` (§6.10):

```clojure
(handle [ConsoleLog]           ; named handler
  (Log/info "Server started"))

(handle [(JsonLog {:env :prod})]  ; parameterized named handler
  (Log/info "Server started"))
```

### 6.5 Handling Effects — Continuation Form

For effects that need to intercept the computation before resuming it, handlers may expose the **continuation** — a function representing "the rest of the computation after the effect operation returns."

When a handler operation takes `resume` as its first parameter, it receives this continuation. Calling `(resume value)` continues the computation as if the effect operation returned `value`. The handler may perform work before or after calling `resume`, enabling patterns like logging, retrying, and state threading.

**Simple form vs continuation form.** In the simple form (§6.4), the handler operation has the same parameters as the declared operation — the runtime implicitly resumes with the handler's return value. In the continuation form, `resume` is the first parameter and the handler explicitly controls when (and whether) to resume. Both forms may coexist in the same `handle` block.

**`resume` is a reserved identifier in handler parameter lists.** When the compiler parses a handler operation's parameter list, if the first parameter is the identifier `resume`, it unconditionally enters continuation form. `resume` may not be used as a regular parameter name in handler operations. Outside handler parameter lists, `resume` is an ordinary identifier with no special meaning.

```clojure
(handle [EffectName
          (operation [resume arg1 arg2]
            ;; resume continues the computation with the given value
            ;; as the return of the effect operation
            (resume return-value))]
  body...)
```

**Continuations are one-shot.** Each `resume` may be called **at most once**. Calling `resume` a second time is a runtime panic. This constraint enables an efficient stack-based implementation on WASM and native targets — no heap-allocated copyable stacks are needed. For non-deterministic exploration (backtracking, logic programming), model the search explicitly with data structures rather than multi-shot continuations.

Example — logging wrapper:

```clojure
;; Intercept Console/print to add timestamps
(handle [Console
          (print [resume msg]
            (rt/stdout-write (str "[" (Time/now) "] " msg))
            (resume unit))]
  (Console/print "hello"))
```

Example — stateful effect using an atom:

```clojure
(defeffect State [a]
  (get-state : (Fn [] -> a))
  (put-state : (Fn [a] -> Unit)))

;; State is a parameterized effect: State Int and State Str are
;; distinct effects in the effect row. A function using both would
;; declare ! [State Int, State Str] and each needs its own handler.

;; State handler using an atom to hold current state
(defn run-state [initial body-fn]
  (let [cell (atom initial)]
    (handle [State
              (get-state [resume]     (resume (deref cell)))
              (put-state [resume new] (reset! cell new) (resume unit))]
      (body-fn))))
```

### 6.6 Effect Inference

The compiler infers effect rows automatically. If a function calls `print` (a `Console` operation), the compiler infers `[Console]` in its effect row. Effects compose via row union:

```clojure
;; Inferred: (Fn [Str] -> Str ! [Console Net Log])
(defn fetch-and-log! [url]
  (Log/info (str "Fetching " url))    ; Log effect operation (qualified)
  (let [resp (http/get url)]          ; module function; http/get performs Net internally
    (Console/print (str "Response: " resp))   ; Console effect operation (qualified)
    resp))
```

### 6.7 Effect Rows in Signatures

The `!` in a type signature introduces an **effect row**: an unordered set of required effects.

```clojure
(Fn [A] -> B ! [E1 E2])    ; requires E1 and E2
(Fn [A] -> B ! [])         ; pure (no effects, same as omitting !)
(Fn [A] -> B ! [e])        ; polymorphic over effect row e (for higher-order fns)
```

#### Effect Polymorphism in Higher-Order Functions

Standard library higher-order functions like `map`, `filter`, and `reduce` are effect-polymorphic: the effect row of the result includes the effects of the callback function.

```clojure
;; map's type signature with effect polymorphism:
;; (Fn [(Fn [a] -> b ! [e]) (Vec a)] -> (Vec b) ! [e])

;; Pure callback — result is pure
(map (fn [x] (* x x)) [1 2 3])
;; inferred: (Fn [] -> (Vec Int))

;; Effectful callback — effects propagate to the caller
(map (fn [url] (fetch! url)) urls)
;; inferred: (Fn [] -> (Vec Response) ! [Net])
```

The lowercase `e` in `! [e]` is a **row variable** — a type variable ranging over effect rows. The compiler unifies it with the actual effects of the callback at each call site. If the callback is pure, `e` unifies with `[]` and the `!` is elided.

This extends to all HOFs: `filter`, `reduce`, `sort-by`, `group-by`, `for!`, etc. Effect polymorphism is inferred; no annotation is needed on user-defined HOFs unless the compiler cannot determine the relationship.

#### Effect Row Compatibility

A function with **fewer** effects is assignable to a context expecting **more** effects. This is standard row-polymorphism subtyping: a pure function `(Fn [Int] -> Int)` is compatible with `(Fn [Int] -> Int ! [Console])`, because a function that needs no effects can safely run in any context. Similarly, `(Fn [] -> Int ! [Console])` is compatible with `(Fn [] -> Int ! [Console Net])` — providing more handlers than needed is always safe.

This means a pure callback can be passed to any effect-polymorphic HOF, and a `! [Net]` function can be called from a `! [Net Console Log]` context without explicit widening.

### 6.8 Handler Scoping

`handle` forms nest. The innermost handler for a given effect takes precedence. Effects that are not handled by any enclosing `handle` must be declared in the module's `!` annotation. Unhandled effects at the program's entry point are a compile error.

**Dispatch from inside a handler.** When handler code itself performs an effect operation for the *same* effect it handles, the operation dispatches to the **next outer handler** — not to the current handler. This prevents infinite recursion and enables handler composition patterns like middleware:

```clojure
;; Outer handler: writes to file
(handle [Log
          (info [msg] (FileSystem/write-file "app.log" msg))]

  ;; Inner handler: adds timestamp, then delegates to outer
  (handle [Log
            (info [resume msg]
              (Log/info (str "[" (Time/now) "] " msg))  ; dispatches to outer
              (resume unit))]
    (Log/info "server started")))
;; File receives: "[2026-02-20T...] server started"
```

If no outer handler exists for the re-performed effect, it propagates to the module's effect row as usual.

### 6.9 Built-in Effects

The following effects are pre-declared in `core`:

| Effect | Operations |
|--------|-----------|
| `FileSystem` | `read-file`, `write-file`, `append-file`, `delete-file`, `list-dir`, `make-dir`, `stat` |
| `Net` | `tcp-connect`, `tcp-listen`, `dns-resolve` |
| `Console` | `print`, `println`, `eprintln`, `read-line` |
| `Time` | `now`, `sleep` |
| `Log` | `debug`, `info`, `warn`, `error` |
| `Random` | `random-int`, `random-float`, `random-bytes`, `random-u8`, `random-f32`, `random-int-range` |
| `Concurrent` | `fork`, `join`, `race` (see §10) |
| `Dynamic` | `type-error` (see §5.10 — the `Any` escape hatch) |
| `Unsafe` | `ptr-read`, `ptr-write`, `ptr-offset` — raw pointer operations for C FFI (see §15.3) |

**Full declarations for built-in effects.** `Console` and `FileSystem` are declared in §6.2. The remaining built-in effects are declared here for reference:

```clojure
(defeffect Time
  (now   : (Fn [] -> Int))      ; Unix timestamp in milliseconds (monotonic)
  (sleep : (Fn [Int] -> Unit))) ; sleep for n milliseconds

(defeffect Log
  (debug : (Fn [Str] -> Unit))
  (info  : (Fn [Str] -> Unit))
  (warn  : (Fn [Str] -> Unit))
  (error : (Fn [Str] -> Unit)))

(defeffect Random
  (random-int       : (Fn [] -> Int))                    ; full Int range
  (random-int-range : (Fn [Int Int] -> Int))             ; [min, max) exclusive
  (random-float     : (Fn [] -> Float))                  ; [0.0, 1.0)
  (random-bytes     : (Fn [Int] -> Bytes))               ; n random bytes
  (random-u8        : (Fn [] -> U8))                     ; [0, 255]
  (random-f32       : (Fn [] -> F32)))                   ; [0.0f32, 1.0f32)

(defeffect Net
  (tcp-connect : (Fn [Str Int] -> TcpStream))            ; host, port
  (tcp-listen  : (Fn [Int] -> TcpListener))              ; port
  (dns-resolve : (Fn [Str] -> (Vec Str))))               ; hostname -> IP list

(defeffect Unsafe
  (ptr-read   : (Fn [Ptr Int] -> U8))                    ; read byte at ptr+offset
  (ptr-write  : (Fn [Ptr Int U8] -> Unit))               ; write byte at ptr+offset
  (ptr-offset : (Fn [Ptr Int] -> Ptr)))                  ; advance pointer by n bytes
```

**The `Unsafe` effect.** Functions that perform raw memory operations (via `Ptr`, used in C FFI) require the `Unsafe` effect. Like all effects, `Unsafe` must be declared in `! [...]` and handled by an enclosing `handle` or granted via `nexl sandbox --allow-unsafe`. Because `defextern` declarations with `:unsafe` assert that a C function performs raw memory access, the Nexl compiler adds `Unsafe` to the effect row of any function that calls them. `Unsafe` cannot be granted via `--allow-net` or similar capability flags — it requires explicit `--allow-unsafe` or an enclosing `handle [Unsafe ...]` block.

**Effect groups.** `IO` is an **effect group alias**, not a separate effect. It expands to `[FileSystem Console Net Time]` wherever it appears:

```clojure
;; These two declarations are equivalent:
(defn serve! [] -> Unit ! [IO])
(defn serve! [] -> Unit ! [FileSystem Console Net Time])
```

Effect groups are declared with `defeffect-group`:

```clojure
(defeffect-group IO [FileSystem Console Net Time])
```

This avoids circular subsumption — `IO` is purely a shorthand, not a distinct effect that "contains" other effects. A handler for `IO` must provide handlers for all four constituent effects. Granting `IO` grants all four; granting `FileSystem` alone grants only file operations.

### 6.10 Named Effect Handlers (`defhandler`)

While `handle` (§6.4, §6.5) defines anonymous, inline handlers, many handlers are reusable — the same logging strategy, the same database connection, the same test double. `defhandler` gives a name to a handler implementation so it can be referenced by name in `handle` blocks.

This creates a clean three-part symmetry:

```
defeffect    — declares WHAT operations exist
defhandler   — declares HOW to implement them (named, reusable)
handle       — installs a handler for a scope
```

`defhandler` is a **language-level** form — not test-specific. The same form works identically in production and test code. This is what makes algebraic effects a universal abstraction: "mocking" in tests is just providing a different named handler.

#### Syntax

`defhandler` follows the same structure as `impl` (§5.11): bare uppercase symbols name the effect being implemented, and operation implementations follow underneath. No wrapping brackets are needed around effect sections.

```clojure
;; Compare the two — same structure:

(impl Point
  Show
  (show [p] (str "(" (:x p) ", " (:y p) ")"))
  Eq
  (eq? [a b] (and (= (:x a) (:x b)) (= (:y a) (:y b)))))

(defhandler ConsoleLog
  Log
  (info [msg] (rt/stdout-write (str "[INFO] " msg "\n")))
  (warn [msg] (rt/stdout-write (str "[WARN] " msg "\n")))
  (error [msg] (rt/stderr-write (str "[ERROR] " msg "\n"))))
```

#### All Forms

```clojure
;; Simple — single effect
(defhandler ConsoleLog
  Log
  (info [msg] (println msg))
  (warn [msg] (println msg))
  (error [msg] (eprintln msg)))

;; Continuation form — resume is explicit (first param = resume)
(defhandler TimestampLog
  Log
  (info [resume msg]
    (println (str "[" (Time/now) "] [INFO] " msg))
    (resume unit))
  (warn [resume msg]
    (println (str "[" (Time/now) "] [WARN] " msg))
    (resume unit)))

;; Parameterized — takes configuration
(defhandler JsonLog [config]
  Log
  (info [msg]
    (println (json/encode {:level :info :msg msg :env (:env config)})))
  (warn [msg]
    (println (json/encode {:level :warn :msg msg :env (:env config)}))))

;; Multi-effect — bare uppercase symbols delimit sections
(defhandler ProductionStack
  Db
  (query [sql params] (sqlite/query conn sql params))
  (exec! [sql params] (sqlite/exec conn sql params))
  Log
  (info [msg] (println msg))
  (warn [msg] (println msg)))

;; Parameterized + multi-effect
(defhandler ConfiguredStack [db-path log-level]
  Db
  (query [sql params] (sqlite/query (sqlite/open db-path) sql params))
  (exec! [sql params] (sqlite/exec (sqlite/open db-path) sql params))
  Log
  (info [msg] (when (>= :info log-level) (println msg)))
  (warn [msg] (when (>= :warn log-level) (println msg))))
```

#### Usage with `handle`

Named handlers are installed via `handle` by placing the handler name (or a parameterized handler call) in the handler vector:

```clojure
;; Named handler — just the name in the vector
(handle [ConsoleLog]
  (Log/info "Server started"))

;; Parameterized — call to produce handler
(handle [(JsonLog {:env :production})]
  (Log/info "Server started"))

;; Parameterized + multi-effect
(handle [(ConfiguredStack "app.db" :info)]
  (Log/info "Starting up")
  (Db/exec! "CREATE TABLE IF NOT EXISTS users ..." []))

;; Multiple named handlers compose via nesting
(handle [ConsoleLog]
  (handle [SqliteDb]
    (run-app!)))

;; Inline handler still works for one-off cases
(handle [Log
          (info [msg] (println msg))]
  (do-stuff!))
```

#### Parsing Rule

The parser distinguishes parameter vectors from effect sections by case: **an uppercase bare symbol starts a new effect section**; a lowercase vector after the handler name is the parameter list. This is the same rule `impl` uses.

```clojure
(defhandler JsonLog [config]   ;; [config] — lowercase = params
  Log                          ;; Log      — uppercase = effect section starts
  (info [msg] ...))            ;; (info…)  — operation implementation
```

#### Completeness

A `defhandler` must implement **all** operations declared in each effect it handles. Missing an operation is a compile error:

```
error: handler `ConsoleLog` is missing operation `read-line` from effect `Console`
  --> src/main.nx:12:1
   |
12 | (defhandler ConsoleLog
   | ^ Console declares 4 operations, handler provides 3
   |
   = help: add (read-line [] ...) to complete the handler
```

---

## 7. Macro System

### 7.1 Phase Model

Nexl has two compilation phases:

- **Phase 0 (Runtime):** Normal code execution.
- **Phase 1 (Compile time):** Macro expansion and `defn-macro` helpers. Macro expansion precedes type inference in the pipeline (§12.1). Elaboration macros (`defmacro-elab`, §7.8) are the exception: they run interleaved with type checking.

Macros run at Phase 1. They may call:
- `defn-macro` functions defined in the same module (the compiler topologically orders them), or
- `defn-macro` functions imported from another module using `:for-syntax` (see §7.11), or
- Any pure Phase 0 function (no effects).

A macro that calls an effectful Phase 0 function is a compile error.

#### `defn-macro` — Compile-Time Function

`defn-macro` defines a function available at Phase 1 (compile time). These functions are evaluated during compilation, not at runtime.

```clojure
(defn-macro generate-field-accessors [fields : (Vec SyntaxObj)] -> (Vec SyntaxObj)
  (map (fn [f] `(defn ~(symbol (str "get-" (name (syntax-datum f)))) [r]
                  (~f r)))
       fields))

(defmacro defrecord-accessors [& fields]
  (when (empty? fields)
    (syntax-fail &form "defrecord-accessors requires at least one field"))
  `(do ~@(generate-field-accessors fields)))
```

`defn-macro` functions must be pure — they cannot perform effects. They may call other `defn-macro` functions and any pure Phase 0 function.

### 7.2 Syntax Objects

All values flowing through the macro system are **syntax objects** (`SyntaxObj`), not raw s-expressions. A syntax object wraps a datum and carries source location and scope information:

```clojure
;; Available from (import nexl.syntax :for-syntax)
;; Defined in the nexl.syntax standard library:

;; Extract the underlying Nexl value from a syntax object
(defn-macro syntax-datum [s : SyntaxObj] -> Any)

;; Get the source location (file, line, column, span)
(defn-macro syntax-loc [s : SyntaxObj] -> SrcLoc)

;; Unwrap a list-shaped syntax object to its elements
(defn-macro syntax->list [s : SyntaxObj] -> (Vec SyntaxObj))

;; Wrap a plain value in a syntax object, borrowing scopes from ctx
(defn-macro datum->syntax [ctx : SyntaxObj, d : Any] -> SyntaxObj)

;; Predicates
(defn-macro syntax-symbol? [s : SyntaxObj] -> Bool)
(defn-macro syntax-list?   [s : SyntaxObj] -> Bool)
(defn-macro syntax-keyword?[s : SyntaxObj] -> Bool)

;; Generate a fresh, unique symbol (hygiene-safe)
(defn-macro gensym [] -> SyntaxObj)
(defn-macro gensym [hint : Str] -> SyntaxObj)
```

Quasiquotation (`` ` ``, `~`, `~@`) builds `SyntaxObj` trees and propagates source locations automatically. Source locations from the macro call site are preserved in the expansion, so compiler error messages point to the user's code, not the expanded form.

### 7.3 `defmacro` — Defining Macros

```clojure
(defmacro name [params] body)
(defmacro name "Documentation." [params] body)
```

Macro parameters are bound to `SyntaxObj` values. The body must return a `SyntaxObj`. The special binding `&form` is available inside every macro body and holds the entire macro call as a `SyntaxObj` (including its source location).

```clojure
(defmacro unless [condition & body]
  `(if (not ~condition)
     (do ~@body)))

;; Enum macro using proper ADT syntax
(defmacro def-enum [name & variants]
  (when (empty? variants)
    (syntax-fail &form "def-enum requires at least one variant"))
  `(deftype ~name ~@(map (fn [v] `| ~v) variants)))

(def-enum Color Red Green Blue)
;; expands to:
;; (deftype Color | Red | Green | Blue)
```

The `#` suffix inside quasiquote templates auto-generates a fresh hygienic name per expansion. `v#` in a template expands to a unique symbol that cannot clash with user-defined names:

```clojure
(defmacro swap! [a b]
  `(let [tmp# ~a]
     (set! ~a ~b)
     (set! ~b tmp#)))
```

### 7.4 `defmacro-syntax` — Pattern-Based Macros

`defmacro-syntax` defines macros via structural pattern matching on syntax, analogous to Scheme's `syntax-rules`. Each clause matches a syntactic shape; patterns are matched against `SyntaxObj` structure, not values.

```clojure
(defmacro-syntax name
  [(name pattern ...) expansion-template]
  [(name pattern ...) expansion-template]
  ...)
```

`...` in a pattern matches zero or more syntax forms and binds them as a sequence. The corresponding `...` in a template splices the sequence.

```clojure
(defmacro-syntax my-and
  [(my-and)           true]
  [(my-and e)          e]
  [(my-and e1 e2 ...) `(let [v# ~e1] (if v# (my-and ~e2 ...) false))])

(defmacro-syntax my-or
  [(my-or)           false]
  [(my-or e)          e]
  [(my-or e1 e2 ...) `(let [v# ~e1] (if v# v# (my-or ~e2 ...)))])
```

`defmacro-syntax` patterns are checked for exhaustiveness. The expander verifies that all introduced bindings in templates are either pattern variables or `#`-suffixed auto-gensyms, enforcing hygiene structurally.

### 7.5 Quasiquotation

```
`expr           ; quasiquote: builds a SyntaxObj tree preserving source locations
~expr           ; unquote: evaluate expr within quasiquote
~@expr          ; unquote-splice: splice a sequence into the enclosing list
```

Inside quasiquote, identifiers are wrapped with the macro's definition-site scopes (hygienic by default). Unquoted expressions (`~expr`) retain the scopes of the surrounding macro call site.

### 7.6 Hygiene

Nexl macros are **hygienic by default**, implemented via **scope sets** (Flatt, 2016).

**How scope sets work:**

Every syntactic region — a module, a `let` binding, a macro expansion — introduces a unique *scope token*. Each identifier in a `SyntaxObj` carries a *set* of these scope tokens. A binding at scope set `S` captures any reference whose scope set is a superset of `S`.

When a macro transformer runs:
1. The expander adds a fresh *macro-introduction scope* to the entire input syntax.
2. The transformer produces its result.
3. The expander *flips* the macro-introduction scope in the result:
   - Identifiers the macro **introduced** (from its own quasiquote templates) retain the scope → they bind in the macro's definition namespace.
   - Identifiers the macro **received** (from the user's code, passed through `~`) lose the scope → they bind in the user's namespace.

This correctly handles nested macros, `let`-bound macros, and module phase boundaries without special cases.

**Intentional hygiene breaking** — to introduce a name that the user's code can see (anaphoric macros), use `datum->syntax` with a call-site syntax object as the scope context:

```clojure
(defmacro anaphoric-if [test then else]
  (let [it (datum->syntax &form 'it)]   ; borrow call-site scopes for 'it
    `(let [~it ~test]
       (if ~it ~then ~else))))

(anaphoric-if (find-user 42)
  (str "Found: " (:name it))
  "Not found")
```

`gensym` is available for generating unique names when explicit freshness is needed beyond the `#`-suffix shorthand.

### 7.7 Macro Error Reporting

A macro can abort compilation with a positioned error message using `syntax-fail`:

```clojure
(defn-macro syntax-fail [stx : SyntaxObj, msg : Str] -> Never)
(defn-macro syntax-warn [stx : SyntaxObj, msg : Str] -> Unit)
```

`syntax-fail` terminates macro expansion immediately and emits a compile error pointing to `stx`'s source location with `msg`. The compiler formats this identically to its own type errors — it is a first-class diagnostic, not a downstream type-mismatch from malformed output.

`syntax-warn` emits a non-fatal compile warning and continues expansion.

```clojure
(defmacro defrecord-with-accessors [name & fields]
  (when (empty? fields)
    (syntax-fail &form "defrecord-with-accessors requires at least one field"))
  (when (not (syntax-symbol? name))
    (syntax-fail name "expected a type name symbol"))
  `(do
     (deftype ~name {:datum SyntaxObj} ~@(map (fn [f] [f]) fields))
     ~@(map (fn [f]
              `(defn ~(datum->syntax f (symbol (str "get-" (name (syntax-datum f)))))
                 [r] (~(syntax-datum f) r)))
            fields)))
```

### 7.8 `defmacro-elab` — Elaboration Macros

> **Status:** Planned (v1.0). Requires a mature type checker. Not in v0.1.

Standard `defmacro` and `defmacro-syntax` run *before* type inference (see §12.1 pipeline). Elaboration macros (`defmacro-elab`) run *during* type checking, interleaved with bidirectional inference, and receive **typed** syntax. This is the correct way to write type-dispatch macros.

```clojure
(defmacro-elab name [param : TypedSyntax] -> SyntaxObj
  body)
```

`TypedSyntax` wraps a `SyntaxObj` together with its inferred `TypeDescriptor`:

```clojure
(deftype TypedSyntax
  {:syntax SyntaxObj
   :type   TypeDescriptor})

;; Accessors
(defn-macro typed-syntax/syntax [ts : TypedSyntax] -> SyntaxObj)
(defn-macro typed-syntax/type   [ts : TypedSyntax] -> TypeDescriptor)
```

`TypeDescriptor` is the fully-specified type descriptor ADT:

```clojure
(deftype TypeDescriptor
  | TdPrim   Primitive                                  ; Int, Float, Bool, Str, Unit, …
  | TdFn     (Vec TypeDescriptor) TypeDescriptor (Set Effect)  ; function type
  | TdApp    TypeDescriptor (Vec TypeDescriptor)        ; (Vec Int), (Map Str a), …
  | TdRecord (Map Keyword TypeDescriptor)               ; record type
  | TdADT    Symbol (Vec ADTVariant)                    ; algebraic data type
  | TdVar    Symbol                                     ; type variable (polymorphic)
  | TdAny)                                              ; Any escape hatch
```

The elaboration macro receives the argument expression after partial type inference has determined its type, so `TypeDescriptor` is always concrete:

```clojure
(defmacro-elab auto-to-str [expr : TypedSyntax] -> SyntaxObj
  (let [stx (typed-syntax/syntax expr)]
    (match (typed-syntax/type expr)
      (TdPrim Int)   `(int->str ~stx)
      (TdPrim Float) `(float->str ~stx)
      (TdPrim Bool)  `(bool->str ~stx)
      (TdPrim Str)   stx
      _              `(str ~stx))))
```

An elaboration macro may only replace the form it is attached to — it cannot alter the surrounding type environment or emit new top-level definitions.

### 7.9 `macroexpand`

```clojure
(macroexpand-1 '(unless true (panic "bad")))
;; => (if (not true) (do (panic "bad")))

(macroexpand '(unless true (panic "bad")))
;; => fully expanded form
```

`macroexpand` and `macroexpand-1` are **REPL and development tools**. They are available in the REPL and via `:expand` in the toolchain (§14). Using them inside macro bodies is possible but discouraged — the expansion environment at expansion time may differ from the environment at REPL time, producing confusing results.

### 7.10 Controlled Reader Extensions

Two forms of reader extension are available:

**`defreader`** — receives a single already-parsed `SyntaxObj` form following the `#tag`:

```clojure
(defreader #tag [form : SyntaxObj]
  expansion-expr)
```

**`defreader-text`** — receives the raw text content between matching delimiters (`[…]`, `(…)`, or `"…"`), enabling embedded DSLs whose syntax is not valid Nexl:

```clojure
(defreader-text #tag [text : Str, loc : SrcLoc]
  expansion-expr)
```

Both forms are registered per-module and are only active when the defining module is imported. They cannot override built-in reader syntax.

```clojure
;; Compile-time regex — receives a string SyntaxObj
(defreader #r [pattern : SyntaxObj]
  (when (not (str? (syntax-datum pattern)))
    (syntax-fail pattern "#r requires a string literal"))
  `(compile-regex ~(syntax-datum pattern)))

#r"^\d{3}-\d{4}$"
;; => (compile-regex "^\\d{3}-\\d{4}$") at compile time

;; Type-safe SQL — receives raw text; user-id is interpolated by the DSL parser
(defreader-text #sql [text : Str, loc : SrcLoc]
  (let [q (parse-and-validate-sql text loc)]
    (match q
      (Ok {:sql s, :params p}) `(prepared-statement ~s ~p)
      (Err msg)                (syntax-fail (datum->syntax (synthetic-syntax loc) 'sql)
                                            msg))))

#sql[SELECT name FROM users WHERE id = {user-id}]
;; => (prepared-statement "SELECT name FROM users WHERE id = $1" [user-id])
```

### 7.11 Cross-Module Macro Dependencies

When a `defmacro` or `defn-macro` defined in module A is used at Phase 1 while compiling module B, those Phase 1 definitions must be available to the compiler at expansion time.

**Same-module `defn-macro`:** `defn-macro` functions defined in the same module as a `defmacro` are available to that macro. The compiler topologically sorts `defn-macro` definitions (they are pure, so no ordering ambiguity exists) and compiles them before expanding macros in the same module.

**Cross-module Phase 1 helpers:** To use a `defn-macro` from a different module inside a macro body, import it with `:for-syntax`:

```clojure
;; module my-app.macros
(import nexl.syntax :for-syntax)               ; syntax utilities at Phase 1
(import my-app.codegen :for-syntax [make-ffi]) ; specific Phase 1 helper

(defmacro def-ffi-wrapper [name sig]
  `(defextern ~name : ~sig ~(str (syntax-datum name))))
```

`:for-syntax` imports are phase-separated: `make-ffi` is compiled into the compiler's expansion environment and is not part of the runtime binary of any module that imports `my-app.macros`.

**Content hashing:** The content hash of a module (§8.4) includes the compiled transformer bodies of all its macros. Changes to a `defn-macro` helper invalidate all modules that use macros depending on that helper.

---

## 8. Module System

### 8.1 Module Declaration

Every source file is a module. Modules are declared at the top of the file with a `module` form:

```clojure
(module my-app.server
  :performs [Net IO Log]   ; effects that escape this module's exported functions
  :exports  [start! stop!  ; explicit public API
             ServerConfig]
  :imports  [[my-app.model :as model]          ; alias import
             [my-app.db    :refer [query!]]])   ; selective import
```

All fields are optional:

- `:performs` declares which algebraic effects the module's exported functions may perform. If present, the compiler verifies that every exported function's inferred effect row is a subset of the declared effects — a mismatch is a compile error. If omitted, the compiler infers it automatically.
- `:exports` lists the names that are part of the module's public API. If omitted, all top-level `def`/`defn` forms are exported.
- `:imports` is a vector of import specifiers (see §8.2). This is equivalent to writing standalone `(import ...)` forms at the top of the file — both syntaxes are supported and can be mixed.

A minimal module declaration with no options is valid:

```clojure
(module app.main)
```

### 8.2 Importing Modules

Imports can be written either inline in the module header (`:imports` field, §8.1) or as standalone top-level forms after the module declaration. Both forms are equivalent; they can be mixed in the same file.

```clojure
;; Alias import — access via alias/name
(import my-lib.http :as http)
(http/get "https://example.com")

;; Selective import — bring specific names into scope unqualified
(import my-lib.coll :refer [map filter reduce])

;; Import all exports unqualified (no qualifier option)
(import my-lib.util)

;; Exclude specific names when importing all
(import lib.str :exclude [format])

;; Rename on import — bring in under a different local name
(import lib.coll :rename {map collect})
```

The five import kinds are:

| Syntax | Effect |
|--------|--------|
| `(import mod :as alias)` | All exports accessible via `alias/name` |
| `(import mod :refer [a b])` | Named exports brought in unqualified |
| `(import mod)` | All exports brought in unqualified |
| `(import mod :exclude [a b])` | All exports except the listed names |
| `(import mod :rename {old new})` | Brings exports in under new local names |

**Effect propagation.** A module's exported functions carry their effect rows in their type signatures. When you call `(http/get url)`, the `Net` effect propagates to the calling function's inferred effect row automatically — no import-time declaration is needed.

**Sandboxing untrusted code.** To restrict what an imported module can actually do at runtime, wrap calls to it in a `handle` form that intercepts and audits the relevant effects:

```clojure
(import untrusted-plugin :as plugin)

(defn call-plugin-safely [input] -> Result ! [Net]
  (handle [Net
            (get-url [resume url]
              (when (not (allowed-url? url))
                (panic (str "plugin attempted disallowed URL: " url)))
              (resume (http/get url)))]
    (plugin/process input)))
```

This is the correct mechanism for capability restriction — the algebraic effect system is the capability system. Wrapping calls in `handle` intercepts the plugin's effects, audits or transforms them, and controls what actually executes.

### 8.3 Namespace

Imported modules are accessed via their alias. Module-qualified calls (`alias/fn`) are distinct from effect-qualified calls (`EffectName/op`) — see §6.3 for the distinction.

```clojure
;; Module function calls — http is a module alias from (import ... :as http)
(http/get "https://example.com")
(http/post "https://example.com/users" body)

;; Effect operation calls — Console is an effect name from (defeffect Console ...)
(Console/print "hello")
```

### 8.4 Content-Addressed Definitions

Every top-level definition is internally identified by a **content hash**: a SHA-256 of its **type-annotated, normalized AST** (after full type and effect inference) plus the content hashes of its direct dependencies.

The hash is computed after type inference — not after macro expansion — so the hash captures the fully-resolved types. Two definitions with identical source text but different inferred types (possible under different import contexts) produce different hashes. `canonical-serialize` is a deterministic, alpha-renaming-normalized serialization that strips source location and comment data.

Names are human-readable aliases pointing to hashes. Renaming a function changes only the name→hash mapping; no recompilation occurs. The content hash is the true identity.

Benefits:
- Diamond dependency conflicts are structurally impossible for functions and values.
- Incremental compilation is perfectly correct: only changed hashes recompile.
- Two modules depending on different versions of a third module coexist without conflict for code.

The compiler maintains a local **definition store** (SQLite-backed) mapping hashes to compiled artifacts, type signatures, and effect signatures.

**Type identity across versions.** Types are identified by content hash, not by name. If module A depends on `user-lib@1.0` defining `(deftype User {:name Str})` and module B depends on `user-lib@2.0` defining `(deftype User {:name Str :email Str})`, these are **different types** with different hashes. A `user-lib@1.0/User` cannot be passed where `user-lib@2.0/User` is expected — the compiler rejects this at the call site. If both versions define a type with identical structure, they produce identical hashes and are the same type — diamond dependency resolved structurally. When types differ, the programmer must provide an explicit conversion function at the boundary.

### 8.5 Semantic Versioning Enforcement

The compiler can compare two versions of a module and classify each change:

| Change | Classification |
|--------|---------------|
| New export (function, type, or re-export) | **Minor** |
| Removed export | **Major** (breaking) |
| Changed function parameter or return type | **Major** (breaking) |
| Effect added to exported function's row | **Major** (breaking — pure callers break) |
| Effect removed from exported function's row | **Minor** |
| ADT variant added to exported type | **Major** (breaking — exhaustive `match` in callers breaks) |
| ADT variant removed from exported type | **Major** (breaking) |
| Record field added to exported type | **Major** (breaking — record construction `{:a 1}` in callers breaks) |
| Record field removed from exported type | **Major** (breaking) |
| Protocol implementation removed | **Major** (breaking — generic callers using that protocol break) |
| New protocol implementation | **Minor** |
| Internal changes only (private functions, comments) | **Patch** |

```
$ nexl pkg diff http-client@1.2.0 http-client@1.3.0
  get    : effect row changed (added Net) — MAJOR (breaking)
  patch  : new export (Fn [Str Body] -> Response ! [Net]) — minor
  Status : new export (| Pending | Done | Failed) — MAJOR (added variant)
```

### 8.6 Circular Dependencies

Circular imports are disallowed. The compiler detects them and reports an error showing the import cycle. Mutual recursion within a single module is supported via `declare`.

### 8.7 Re-exports

> **Planned — not yet implemented.** The `(export ...)` and `(re-export ...)` forms described below are designed but not yet available in the current compiler. Re-export from the current stage is achieved by explicitly re-defining the name in the module body.

A module may re-export names from its dependencies as part of its own public API. Re-exported names appear in documentation, LSP completion, and semver diff output.

```clojure
;; Re-export a specific name under a new or identical name
(export get!    http/get!)
(export post!   http/post!)
(export Response http/Response)

;; Re-export a selection from another module
(re-export net.http :select [get! post! put! delete! Response])

;; Re-export everything from another module
(re-export net.http)
```

Re-exported names must be listed in the module's `:exports` to be public. The `:exports` list may mix locally-defined names and re-exported names:

```clojure
(module my-app.http
  :performs [Net]
  :exports  [get! post! Response       ; re-exported from net.http
             retry-get!])              ; defined in this module

(import net.http :as http-impl)
(re-export net.http :select [get! post! Response])

(defn retry-get! [url : Str, retries : Int] -> Response ! [Net]
  ...)
```

### 8.8 Visibility

Nexl has two visibility levels (a third — package-private — is planned):

**Public** — listed in `:exports`. Accessible by any importer.

```clojure
(defn public-fn [x] ...)   ; in :exports list
(deftype PublicType ...)    ; in :exports list
```

**Module-private** — all top-level definitions not listed in `:exports`. Only accessible within the defining file.

```clojure
(defn also-private [x] ...)         ; private if not listed in :exports
```

> **Planned — not yet implemented.** A third level, **package-private** (`^:package`), is designed for the future. It will be accessible within the same package (modules sharing the same `project.nx` `:prefix` field, §8.11) but not by external importers. The compiler will enforce this: importing a `^:package` symbol from outside its package will be a compile error.
>
> ```clojure
> (defn ^:package internal-helper [x] ...)
> ```
>
> Package-private visibility will enable white-box testing between sibling modules and shared internal helpers without leaking implementation details to external consumers.

### 8.9 Module Initialization Order

Top-level `def` forms are evaluated at module load time. The order is:

1. **Within a module:** top-down, left-to-right. Forward references within a module require `declare`.
2. **Across modules:** reverse topological order of the import graph. If module A imports B and C, B and C are fully initialized before A's top-level forms run.
3. **Tie-breaking:** when two modules are at the same topological depth (neither depends on the other), they are initialized in lexicographic order by fully-qualified module name.

This order is deterministic and documentable. Run `nexl build --explain-init-order` to print the full initialization sequence for debugging.

### 8.10 Test Submodules

> **Planned — not yet implemented.** The `(submodule :test ...)` form is designed but not yet available. The current way to write tests is to use `test/register!` and `nexl test` (see the `test` stdlib module).

A module may contain an inline test submodule with access to private definitions. Test submodules are compiled only when running `nexl test` and are excluded from release builds.

```clojure
(module my-app.parser
  :exports [parse])

(defn- tokenize [s] ...)   ; private

(defn parse [s : Str] -> (Option Ast)
  (-> s tokenize analyze))

;; Inline test submodule — compiled only in test mode
(submodule :test my-app.parser/tests
  (import nexl.test :refer [deftest is check])

  (deftest tokenize-basic
    (is (= (tokenize "hello world") ["hello" "world"])))

  (deftest parse-roundtrip
    (check [s : Str]
      (when-let [(Some ast) (parse s)]
        (= s (ast->str ast))))))
```

Test submodule rules:
- Named `parent-module/tests` by convention, but any suffix is allowed.
- Have full access to all private definitions in the containing file.
- May import any module (including `nexl.test`).
- Are not included in `:exports` and do not affect the parent module's content hash.
- Test output identifies them as `parent-module/tests`.

### 8.11 Package ↔ Module Relationship

A **package** is the unit of distribution and versioning (`project.nx`). A **module** is the unit of compilation (one `.nx` file). Every module belongs to exactly one package.

The package declares a `:prefix` in `project.nx`. Every module in the package must have a name starting with that prefix, and the file path must match the module name (dots replaced by slashes):

```clojure
;; project.nx
{:package {:name "my-app"
           :version "1.0.0"
           :prefix "my-app"}}
```

`my-app/server.nx` must declare `(module my-app.server ...)`. The compiler verifies this and reports a mismatch as an error.

**Module path resolution.** The module name uses dots as separators (e.g. `my-app.server`), which map to filesystem paths by replacing dots with slashes and appending `.nx` (e.g. `my-app/server.nx`).

**Import resolution.** When you write `(import http-client.core :as http)`, the compiler finds the package whose `:prefix` is `http-client` in the dependency graph (from `project.nx`), then resolves `http-client.core` as a module within that package. Module names are globally unique via package prefixes — two packages with the same `:prefix` field cannot coexist in the registry.

**Package identity.** Packages are identified by content hash in the lockfile. The `name` and `version` fields are human-readable aliases. Two packages with identical source content produce identical hashes regardless of their declared names or versions — the lockfile is the authoritative identity for reproducible builds.

---

## 9. Error Handling

### 9.1 Philosophy

Nexl distinguishes three categories of errors:

1. **Expected failures**: parsing, network errors, missing files — represented as `(Result ok err)` values.
2. **Recoverable interactions**: user confirmation, policy decisions, retryable operations — represented as effects (see §9.5).
3. **Programmer errors**: calling `head` on an empty list, violating a precondition — represented as `panic`, which terminates the program with a stack trace.

**When to use `Result` vs effects.** Use `(Result ok err)` for errors that are part of a function's **return contract** — callers receive the error as a value and decide how to handle it. This is the default for all public API boundaries, library functions, and cross-module interfaces. Use effect-based errors for **context-dependent recovery** — when the same code should behave differently depending on who is running it (interactive vs batch, production vs test, strict vs lenient). Effect-based errors are dependency injection for failure policies, not a replacement for `Result`.

### 9.2 `Result` and `Option`

```clojure
(deftype Option [a]
  | None
  | (Some a))

(deftype Result [a e]
  | (Ok a)
  | (Err e))
```

These are the standard types for fallible operations. Functions that may fail return `(Result a e)`. Optional values use `(Option a)`.

### 9.3 The `?` Operator

Within a function returning `(Result a e)`, the `?` operator propagates errors upward:

```clojure
(defn process! [input : Str] -> (Result Output Error) ! [IO]
  (let [n   (parse-int input)?        ; returns Err early if parsing fails
        data (fetch-data! n)?]         ; returns Err early if fetch fails
    (Ok (transform data))))
```

`expr?` is a **compiler primitive** (not a macro). The compiler transforms `expr?` in the body of a function returning `(Result a e)` as follows:

1. Evaluate `expr` to produce a `(Result x err)` value.
2. If the result is `(Ok v)`, the `expr?` sub-expression produces `v` — execution continues normally.
3. If the result is `(Err e)`, execution of the current function **terminates immediately** and the function returns `(Err e)` (after optional error coercion, see below). No remaining code in the function body executes.

The early-exit is implemented as a **non-local jump** to the function's return point — equivalent to a `return` statement in other languages. It is not an effect and does not appear in the function's effect row. The compiler inserts the jump during the lowering pass (§12.1) after type and effect inference.

The error type `e` must be compatible with the enclosing function's return error type. If the error types differ, there are two mechanisms:

**Explicit conversion** — a transform function inline with `?`:

```clojure
(let [n (parse-int input (fn [e] (AppError "parse" e)))?] ...)
```

**Automatic coercion via `Into` protocol** — if the source error type implements `Into` for the target error type, `?` applies the conversion automatically:

```clojure
(defprotocol Into [Target]
  (into : (Fn [Self] -> Target)))

;; Define coercion from ParseError to AppError
(impl ParseError
  (Into AppError)
  (into [e] (AppError "parse" (:message e))))

;; Now ? auto-converts — no explicit (fn [e] ...) needed
(defn process [input : Str] -> (Result Int AppError)
  (let [n (parse-int input)?]   ; ParseError auto-coerced to AppError
    (Ok n)))
```

When both an `Into` implementation and an explicit `(fn [e] ...)` conversion are present, the explicit conversion takes precedence.

**Multi-parameter protocol resolution.** `Into` is a **multi-parameter protocol** — it has both `Self` (the source type) and `Target`. The compiler resolves which `Into` implementation to use based on type context: the target type must be inferrable from a return type annotation, a function parameter type, or an explicit type annotation. If the target is ambiguous, the compiler reports an error and suggests adding an annotation. The orphan rule applies: implement `(Into Target)` for `Source` only if the current module defines `Source` or `Target`.

#### `?` on `Option`

Within a function returning `(Option a)`, the `?` operator propagates absent values upward:

```clojure
(defn find-display-name [user-id : Int] -> (Option Str)
  (let [user  (db/find-user user-id)?      ; returns None early if user not found
        name  (get user :display-name)?]   ; returns None early if field absent
    (Some name)))
```

`expr?` on an `(Option a)` value behaves as follows:

1. Evaluate `expr` to produce an `(Option x)` value.
2. If the result is `(Some v)`, the `expr?` sub-expression produces `v` — execution continues normally.
3. If the result is `None`, execution of the current function **terminates immediately** and the function returns `None`. No remaining code in the function body executes.

The same non-local jump mechanism used for `Result?` is used for `Option?`.

**Return type determines `?` mode.** The compiler inspects the enclosing function's declared return type to select the propagation behavior:

| Return type | `?` applied to | Early-exit value |
|---|---|---|
| `(Result a e)` | `(Result x e')` | `(Err e)` (with optional coercion) |
| `(Option a)` | `(Option x)` | `None` |

Using `?` on a `(Result a e)` inside a function returning `(Option b)`, or vice versa, is a **compile error**. Use explicit conversion:

```clojure
;; Convert Result → Option (discard the error)
(let [v (match (some-result-fn) (Ok x) (Some x) _ None)?] ...)

;; Convert Option → Result (supply an error value)
(let [v (match (some-option-fn) (Some x) (Ok x) None (Err :not-found))?] ...)
```

### 9.4 `panic`

```clojure
(panic "message")
(panic "message with {data}")
```

Terminates the program with an error message and stack trace. Use only for programmer errors (broken invariants, unreachable code). Never use `panic` for user-facing errors.

```clojure
(defn head [xs]
  (match xs
    [x & _] x
    []       (panic "head called on empty list")))
```

### 9.5 Resumable Errors via Effects

For errors that may need recovery, the effect system provides resumable error handling:

```clojure
(defeffect AskUser
  (confirm : (Fn [Str] -> Bool)))

(defn delete-files! [files : (Vec Str)] -> Unit ! [AskUser FileSystem]
  (each [f files]
    (when (confirm (str "Delete " f "?"))
      (FileSystem/delete-file f))))

;; In production: ask interactively
(handle [AskUser
          (confirm [msg] (read-yes-no msg))]
  (delete-files! files))

;; In CI: auto-confirm
(handle [AskUser
          (confirm [_] true)]
  (delete-files! files))
```

---

## 10. Concurrency

### 10.1 Model

Nexl uses **structured concurrency** expressed as an algebraic effect. All concurrency is explicit. There are no implicit background threads or goroutines.

### 10.2 The `Concurrent` Effect

```clojure
(defeffect Concurrent
  (fork : (Fn [(Fn [] -> a ! [e])] -> (Task a)))
  (join : (Fn [(Task a)] -> a))
  (race : (Fn [(Vec (Task a))] -> a)))
```

**Structured lifetime guarantee**: all tasks forked within a `handle [Concurrent ...]` scope are guaranteed to complete or be cancelled when the scope exits. Tasks cannot outlive the scope that created them.

**Default handler bootstrap.** The runtime installs a default `Concurrent` handler at program startup. `structured-executor` (from the `async` module, §11.1) returns a pre-built handler value that provides this structured execution semantics. For top-level programs, `main!` functions that declare `! [Concurrent]` are automatically wrapped in `(handle (async/structured-executor) ...)` by the runtime. For tests and the REPL, a sequential executor is used instead (§10.5).

```clojure
(handle (async/structured-executor)
  (let [t1 (fork (fn [] (fetch! url1)))
        t2 (fork (fn [] (fetch! url2)))]
    [(join t1) (join t2)]))
;; If t1 throws, t2 is cancelled. Always.
```

### 10.3 Channels

Channels are first-class values for communicating between concurrent tasks. Unlike other effects, `Chan` is a single non-parameterized effect — channel operations work on `(Channel a)` values of any element type without specializing the effect itself.

```clojure
(defeffect Chan
  (make-channel : (Fn [Int] -> (Channel a)))
  (send!        : (Fn [(Channel a) a] -> Unit))
  (recv!        : (Fn [(Channel a)] -> a))
  (close!       : (Fn [(Channel a)] -> Unit)))
```

The type parameter `a` in the operation signatures is universally quantified per-operation — a function using `(Channel Int)` and `(Channel Str)` both require `! [Chan]`, not distinct effects per element type.

```clojure
(let [ch (Chan/make-channel 10)]   ; buffered channel with capacity 10
  (fork (fn [] (Chan/send! ch 42)))
  (Chan/recv! ch))                 ; => 42
```

### 10.4 Atoms for Shared State

For shared mutable state across concurrent tasks, use atoms:

```clojure
(def counter (atom 0))
(fork (fn [] (swap! counter inc)))
(fork (fn [] (swap! counter inc)))
```

`swap!` is atomic (compare-and-swap). For coordinated multi-atom updates where all-or-nothing semantics are needed, restructure the state so a single atom holds a composite value: `(swap! state (fn [s] (put (put s :a new-a) :b new-b)))`. Because `swap!` is a single CAS, the composite update is atomic.

### 10.5 Testing Concurrent Code

Because concurrency is an effect, it can be replaced with a deterministic test handler:

```clojure
(handle (sequential-executor)
  (run-my-concurrent-code))
```

The sequential executor runs all forked tasks to completion in the order they were forked, without actual parallelism, enabling deterministic unit tests.

### 10.6 `par-let` — Explicit Parallel Bindings

`par-let` evaluates all right-hand side expressions in parallel, then binds all names once every expression has completed. It is the ergonomic form for the common agent pattern of making several independent calls concurrently.

```clojure
(par-let [binding1 expr1
          binding2 expr2
          binding3 expr3]
  body)
```

**Rules:**
- Bindings may not reference each other. `(par-let [a x  b (f a)] ...)` is a compile error — `b` depends on `a`.
- All expressions are started simultaneously under a structured-concurrency scope. If any expression throws, the remaining are cancelled and the error propagates.
- `:when` guards and `:as` captures are not valid in `par-let` binding positions. Use plain names only.

```clojure
;; Three independent calls — run concurrently without manual fork/join.
;; db/, config/, metrics/ are module aliases (see §8.2), not effect names.
(par-let [user    (db/find-user id)
          config  (config/load)
          metrics (metrics/fetch-today)]
  (render user config metrics))

;; Compare to the equivalent explicit form:
(let [t1     (fork (fn [] (db/find-user id)))
      t2     (fork (fn [] (config/load)))
      t3     (fork (fn [] (metrics/fetch-today)))
      user   (join t1)
      config (join t2)
      metrics (join t3)]
  (render user config metrics))
```

`par-let` requires the `Concurrent` effect to be in scope, like any `fork` call. It is syntactic sugar over `fork`/`join` with an enforced independence check at compile time.

**Automatic parallelization of pure bindings:** When the compiler can prove that `let` bindings are mutually independent and perform no effects, it may parallelize them automatically. This is safe because pure functions have no observable ordering difference. Effectful bindings are never auto-parallelized — use `par-let` explicitly.

---

## 11. Standard Library

### 11.1 Core Modules (Always Available)

| Module | Contents |
|--------|---------|
| `core` | Arithmetic, comparison, boolean logic, string coercion, `identity`, `comp`, `partial`, `constantly`, `juxt`, `apply`; built-in protocols: `Show`, `Eq`, `Ord`, `Hash`, `Foldable`, `Buildable`, `Numeric`, `IntLike`, `FracLike`, `Into` |
| `coll` | `map`, `filter`, `reduce`, `fold`, `group-by`, `sort`, `sort-by`, `zip`, `take`, `drop`, `partition`, `frequencies`, `distinct`, `flatten`, `interleave`, `interpose`, `nth`, `last`, `drop-last`, `reverse`, `append`, `put`, `remove`, `iter`, `collect`, `slice`, `keys` (map keys as `(Vec k)`), `vals` (map values as `(Vec v)`), `entries` (map key-value pairs as `(Vec (Tuple k v))`), `count`, `empty?`, `first`, `rest`, `find`, `any?`, `all?` — compiler-dispatched for `Vec`, `Map`, `Set`, `List`, `Iter`; generic fold operations work on any `Foldable` |
| `str` | `split`, `join`, `trim`, `trim-start`, `trim-end`, `upper`, `lower`, `starts-with?`, `ends-with?`, `contains?`, `replace`, `index-of`, `format`, `blank?`, `chars` (returns `(Vec Char)`), `graphemes` (returns `(Vec Str)`, §3.1) |
| `io` | Convenience module wrapping `FileSystem` and `Console` effect operations: `read-file`, `write-file`, `append-file`, `delete-file`, `list-dir`, `make-dir`, `path-join`, `path-exists?`, `stdin`, `stdout`, `stderr`. Functions in this module perform the corresponding effects. |
| `net` | HTTP client/server, TCP client/server, WebSocket, DNS |
| `json` | `parse`, `stringify`, typed codecs via macros |
| `time` | `now`, `instant`, `duration`, `add-duration`, `format-instant`, `parse-instant`, timezone support |
| `math` | `abs`, `floor`, `ceil`, `round`, `pow`, `sqrt`, `log`, `exp`, trig functions, `min`, `max`, `clamp` |
| `crypto` | `hash-sha256`, `hash-sha3`, `hmac`, `random-bytes`, `random-int`, `constant-time=` |
| `log` | Structured logging via the `Log` effect; `debug`, `info`, `warn`, `error` with key-value metadata |
| `test` | `deftest`, `is`, `check` (property-based), `gen` namespace for generators, test runner; also runs `:examples` from function contracts |
| `async` | `fork`, `join`, `race`, `timeout`, `sleep`, channel operations (`make-channel`, `send!`, `recv!`, `close!`), `par-let`, `structured-executor` (default `Concurrent` handler). `go` is shorthand for `(fork (fn [] ...))` within a structured executor scope: `(go body...)` expands to `(Concurrent/fork (fn [] body...))`. |
| `llm` | Provides the `LLM` effect and its standard handler. Full effect declaration: `(defeffect LLM (classify : (Fn [Str Any] -> Bool)) (complete : (Fn [Str] -> Str)) (embed : (Fn [Str] -> (Vec Float))))`. `classify` takes a description string and a value, returns `true` if the value matches the description. `complete` takes a prompt string and returns a completion. `embed` returns a semantic embedding vector. Also exports `mock-llm-handler` for deterministic testing. |
| `conv` | Numeric conversion functions: `->int`, `->int8`, `->int16`, `->int32`, `->u8`, `->u16`, `->u32`, `->u64`, `->float`, `->f32`, `->f64`, `->ratio`. Widening conversions are total (e.g., `(->int32 42i8)` always succeeds). Narrowing conversions return `(Option T)` (e.g., `(->u8 256)` returns `None`). Also provides `Into` protocol implementations for safe widening chains (e.g., `Int8 → Int16 → Int32 → Int`). |

### 11.2 Extended Modules (Installable)

| Module | Contents |
|--------|---------|
| `nexl.sync` | CRDT data structures (LWW-register, g-counter, or-set, RGA text, MV-register), `defconvergent`, `merge-convergent`, WebSocket sync protocol |
| `nexl.ui` | Reactive UI targeting browser DOM (via WASM) and native; hiccup-style templates |
| `nexl.db` | SQL (SQLite, PostgreSQL), key-value, connection pooling via effect |
| `nexl.cli` | Argument parsing, interactive prompts, TUI components, progress bars |
| `nexl.ml` | ONNX runtime via WASM component; tensor operations |
| `nexl.agent` | Agent loop scaffolding, tool registry, session persistence (growing toolkit across REPL sessions), self-healing REPL hooks |

---

## 12. Compilation Model

### 12.1 Pipeline Overview

```
Source (.nx)
    │
    ▼
[Reader]                ; text → s-expression AST
    │
    ▼
[Macro Expansion]       ; Phase 1: expand all macros recursively (scope-set hygiene)
    │
    ▼
[Name Resolution]       ; resolve scope sets, bind qualified names, enforce import visibility
    │
    ▼
[Type & Effect          ; bidirectional inference, row unification,
  Inference]            ; effect row computation, exhaustiveness check
    │                   ; NOTE: elaboration macros (defmacro-elab, §7.8) run interleaved
    │                   ;       with this stage — not during Macro Expansion above
    ▼
[Evidence Insertion]    ; transform effect operations into evidence vector lookups;
    │                   ; insert evidence parameters into function signatures
    ▼
[Continuation           ; CPS/yield transform for handlers that capture continuations;
  Transform]            ; tail-resumptive handlers are left as direct calls
    │
    ▼
[Lowering to IR]        ; desugar match, ?, let-bindings, closures, for/for!
    │
    ▼
[Optimization]          ; inlining, escape analysis, Perceus reuse analysis, DCE
    │
    ├──▶ [WASM Backend]      ; .wasm core module or component (primary target)
    ├──▶ [Native Backend]    ; ELF/Mach-O binary via Cranelift
    └──▶ [Bytecode Backend]  ; .nxc file for REPL / dev mode
```

**Pipeline notes:**

- **Macro expansion** runs all standard macros (`defmacro`, §7) to completion before name resolution. This is a full recursive expansion pass using scope-set hygiene (Decision 009).
- **Elaboration macros** (`defmacro-elab`, §7.8) are the exception: they run interleaved with type inference, because they depend on type information. The pipeline shows this as a note on the Type & Effect Inference stage, not a separate pass before it.
- **Name resolution** resolves scope sets (Flatt 2016), binds qualified names to their definitions, and enforces module import visibility (§8). In a language with scope-set hygiene, this is a distinct pass — it is where lexical scoping, macro-introduced bindings, and import resolution all converge.
- **Evidence insertion** transforms effect operation calls (e.g., `(Log/info "hello")`) into evidence vector lookups (e.g., `(ev-lookup evidence 3 :info "hello")`). It also inserts evidence vector parameters into function signatures for functions that perform effects.
- **Continuation transform** applies to handlers that capture continuations (§6.5). For these handlers, the compiler performs a yield/bubble transformation — the effect operation yields control up to the handler, which receives the captured continuation. Tail-resumptive handlers (§6.4) are left as direct function calls; no continuation transform is needed.

### 12.2 Compilation Targets

| Target | Use Case | Output | Notes |
|--------|----------|--------|-------|
| **WASM Core Module** | Single-module compilation | `.wasm` | Direct function calls, single linear memory. One Nexl module → one WASM core module. |
| **WASM Component** | Package/sandbox boundary | `.wasm` component | Shared-nothing; Canonical ABI serialization at boundaries. Inter-package imports and `nexl sandbox` boundaries use components. |
| **Native (Cranelift)** | CLI tools, long-running services | ELF / Mach-O | x86-64 and aarch64; Linux, macOS. See tradeoffs below. |
| **Bytecode** | REPL, development, hot-reload | `.nxc` | Stack-based VM; interpreted, fast startup. |

**WASM core modules vs components.** A single Nexl module (in the §8 sense) compiles to a WASM core module — a single linear-memory program with direct function calls. When modules cross a package boundary or sandbox boundary, the compiler emits a WASM component — a self-describing, shared-nothing unit that communicates via the Canonical ABI. This distinction matters because data crossing a component boundary is serialized and copied (see §15.1). Effects cannot implicitly cross component boundaries; they must be mapped to WIT interfaces (§15.1).

**Cranelift tradeoffs.** Cranelift produces code approximately 10–15% slower than LLVM but compiles approximately 10x faster. For development builds (`nexl build`), Cranelift is the default — this aligns with Principle 5 (fast feedback over fast execution). An LLVM backend may be added later for optimized release builds (`nexl build --release --backend llvm`). Cranelift does not currently support auto-vectorization, LTO, or PGO. Cranelift supports both WASM and native code generation.

**WASM GC.** When targeting WASM GC-capable hosts (browsers, Wasmtime), the compiler can emit WASM GC types (`struct`, `array`, `i31ref`) instead of managing memory in linear memory. This eliminates shipping a custom GC runtime in the WASM binary (significant size reduction). See §13.3 for how this interacts with memory management strategy.

### 12.3 Incremental Compilation

Compilation is **content-addressed at definition granularity**. Each top-level definition (`defn`, `deftype`, `defeffect`, `def`, etc.) is hashed after type inference and elaboration. The definition store (an on-disk cache, keyed by content hash) maps each hash to:

- The compiled artifact (WASM function, native code, bytecode)
- The inferred type signature
- The inferred effect row
- The list of dependency hashes

A definition is recompiled only when its content hash or any dependency's hash changes.

**Macro invalidation.** When a macro's body changes, all definitions that were expanded using that macro are re-expanded. The compiler tracks which macros were used during expansion of each definition. Elaboration macros (`defmacro-elab`, §7.8) additionally invalidate the type inference stage for their dependents, since they interleave with type checking.

**Reproducibility.** A cold build with an empty cache produces identical output to a warm build. Content addressing ensures this by construction — the same input always produces the same hash, which maps to the same compiled artifact.

### 12.4 Self-Hosting

> **Status:** Aspirational. The initial compiler will be implemented in Rust. Self-hosting is a milestone goal.

The Nexl compiler is designed to be self-hosting via a staged bootstrap process:

**Stage 0 — Rust bootstrap compiler.** A minimal Rust compiler (~8,000 lines) that handles a kernel subset of Nexl: no macros, no type inference (all types annotated), no effects (effectful code uses a monadic encoding). Stage 0 emits bytecode or C. The kernel subset is deliberately impoverished — writing the Stage 1 compiler in it is tedious but possible.

**Stage 1 — Basic Nexl compiler.** Written in the kernel subset. Adds basic macro expansion, bidirectional type inference, and effect encoding. Stage 1 compiles itself to WASM using Stage 0.

**Stage 2 — Full self-hosted compiler.** Written in full Nexl (macros, effects, all features). Compiled by Stage 1. Once Stage 2 can compile itself and produce identical output to Stage 1's compilation of it, bootstrap is complete.

After bootstrap, the Rust compiler is frozen. All future development occurs in Nexl. The Rust compiler remains in the repository for cold-bootstrapping from scratch.

The self-hosting design means the compiler can leverage Nexl's own macro system during compilation. Compiler passes can be extended with macros. This is the core architectural insight: a Lisp whose compiler is written in C cannot extend its own compilation via its own macros.

---

## 13. Runtime Model

### 13.1 Evaluation Order

Nexl uses **strict (eager) evaluation** by default. Arguments are evaluated before function application. Evaluation order within a form is left-to-right.

### 13.2 Value Representation

**Unboxed values.** Numeric types (`Int`, `Float`, `Int32`, `F32`, etc.) and `Bool` are represented as unboxed machine values on the stack or in registers. They are never heap-allocated unless captured by a closure or stored in a collection.

**Tagged pointers (native target).** On the native target, heap-allocated values use tagged pointers with the low 3 bits encoding the type tag:

| Tag | Type |
|-----|------|
| `000` | Pointer to heap object (closure, record, ADT, collection) |
| `001` | Small integer (63-bit, sign-extended) |
| `010` | `Bool` (`false` = `0x2`, `true` = `0xA`) |
| `011` | `Unit` (single value: `0x3`) |

This avoids heap allocation for `Bool`, `Unit`, and small integers.

**Closures.** A closure is represented as a pointer to a heap-allocated structure containing:
1. A code pointer (the function body)
2. An arity field
3. The captured environment (one slot per captured variable, laid out as a flat struct)

When escape analysis determines a closure does not escape its defining scope, the environment may be stack-allocated.

**WASM GC target.** When targeting WASM GC-capable hosts, values map to WASM GC types:
- Records and ADT variants → `struct`
- Vectors and other variable-size collections → `array`
- Small tagged values (`Bool`, `Unit`, small `Int`) → `i31ref`
- Closures → `struct` containing a `funcref` and an environment `struct`

### 13.3 Memory Management

The default memory management strategy is **Perceus reference counting** (Reinking et al., 2021), as proven by Koka. Perceus provides:

- **Deterministic deallocation** — no GC pauses, predictable latency.
- **Reuse analysis** — when a value is uniquely owned (reference count = 1), destructive updates are performed in-place rather than copying. This is critical for persistent data structure performance.
- **No stop-the-world phase** — suitable for interactive and real-time applications.
- **Natural fit with WASM** — no stack scanning needed; reference counts are explicit.

The memory management strategy varies by target and use case:

| Strategy | Flag / Condition | Use Case |
|----------|-----------------|----------|
| **Perceus RC** | Default (native and WASM linear-memory targets) | General-purpose; the primary strategy |
| **WASM GC** | Automatic when targeting WASM GC-capable hosts | Browser, Wasmtime; delegates GC to host VM; dramatically smaller binaries |
| **Arena** | `--gc none` | Short-lived WASM plugins, embedded; all memory freed at module exit |
| **Tracing GC** | Aspirational / future | Concurrent tracing GC for the native target; planned for workloads where RC cycle detection overhead is measurable |

**WASM GC mode.** When targeting hosts that support WASM GC (browsers, Wasmtime), the compiler emits WASM GC types (`struct`, `array`, `i31ref`) and delegates garbage collection entirely to the host VM. This eliminates the need to ship a custom GC runtime in the WASM binary. WASM GC and the Component Model are orthogonal — GC types do not cross component boundaries (the Canonical ABI handles serialization).

### 13.4 Persistent Data Structures

- **Vectors**: Wide-branching trie with structural sharing. O(log₃₂ n) indexed access, O(log₃₂ n) `append`, O(1) `count`. Implementation details (branching factor, node layout) are left to the compiler.
- **Maps**: Hash Array Mapped Trie (HAMT) with insertion-order iteration. O(log₃₂ n) `get`/`put`/`remove`.
- **Sets**: HAMT-backed. O(log₃₂ n) membership test, insertion, removal.
- **Strings**: Immutable UTF-8 byte sequences. Pointer + length. O(1) `count` (byte length), O(n) grapheme iteration.

**Transient optimization.** When escape analysis determines that a persistent collection is uniquely owned (§3.3), the compiler may use in-place mutation instead of structural sharing. This is transparent to the programmer — the semantics remain purely functional.

### 13.5 Effect Runtime

Effects are compiled using **evidence passing** (Leijen, 2017; the approach used by Koka). This section describes the runtime mechanism in detail.

#### 13.5.1 Evidence Vector

Every function that performs effects receives an implicit **evidence vector** parameter. The evidence vector is an array of handler records, one per effect in the function's effect row. Each handler record contains function pointers for each operation of that effect.

```
;; Conceptual: a function (defn greet! [] ! [Console Log] ...)
;; receives an implicit evidence parameter:
;;   greet!(ev: EvidenceVector) where ev[0] = Console handler, ev[1] = Log handler
```

The evidence vector is represented as:
- **Empty** (pure function): no parameter passed; optimized away entirely.
- **Single effect**: a single pointer to the handler record (no array indirection).
- **Multiple effects**: a small array of handler record pointers.

#### 13.5.2 Tail-Resumptive Fast Path

Most handlers are **tail-resumptive**: the handler calls `resume` exactly once as the last action. For these handlers (the simple form, §6.4), evidence passing gives O(1) dispatch — the effect operation is compiled as a direct function call through the evidence vector, with no stack manipulation and no continuation capture.

```clojure
;; This handler is tail-resumptive: resume is called once, in tail position.
(handle (greet!)
  (Console/print [msg] (resume (host-print msg))))
;; Compiled as: replace the Console slot in the evidence vector with a record
;; containing a direct function pointer to host-print. No CPS transform needed.
```

#### 13.5.3 Continuation-Capturing Path

Handlers that use `resume` in non-tail position (§6.5) — or that do not call `resume` at all — require **yield/bubble**: the effect operation yields control up the call stack to the nearest enclosing handler, which receives a one-shot continuation. The cost model:

- **Yield**: O(n) where n is the call-stack depth between the operation and the handler. The runtime must capture stack frames between the operation site and the handler.
- **Resume**: O(1) — restoring a one-shot continuation replays the captured frames.
- **Handler installation**: O(1) — the handler is pushed onto the evidence vector (copy-on-write for the vector itself).

This is the fundamental tradeoff of evidence passing: tail-resumptive handlers are fast (O(1)), and continuation-capturing handlers pay the yield cost. Since most handlers in practice are tail-resumptive, this is a good tradeoff.

#### 13.5.4 Closures and Evidence

When a closure captures a reference to the current scope, the evidence vector at the closure's **creation site** is captured along with it. When the closure is later invoked, it uses the captured evidence vector, not the evidence vector at the call site. This ensures that a closure always uses the handlers that were in scope when it was created.

#### 13.5.5 Future: WASM Stack Switching

The WASM stack switching proposal (Phase 3) would enable direct continuation capture in WASM without the yield/bubble mechanism. When this proposal is standardized, the WASM backend can use it for continuation-capturing handlers, eliminating the O(n) yield cost.

### 13.6 Tail Call Optimization

All tail calls are optimized (TCO).

- **`loop`/`recur`**: Compiles to a simple loop (WASM `loop`/`br` instruction, or native jump). No tail-call instruction needed — `recur` is always in tail position (enforced by compiler).
- **General tail calls (WASM target)**: Use the `return_call` and `return_call_indirect` instructions, standardized in WASM 3.0. These are true tail calls at the instruction level.
- **General tail calls (native target)**: Cranelift supports tail calls. Mutual recursion between top-level functions is optimized when both functions are in tail position.

**TCO and effects.** Tail calls that cross effect handler boundaries (`handle`) are **not** TCO-eligible when the handler captures continuations (§6.5). Simple handlers (§6.4) that do not capture continuations preserve tail-call optimization. The compiler emits a warning when a `recur` inside a `handle` using continuation-style handlers would break TCO, suggesting refactoring the recursion outside the handler scope.

---

## 14. Toolchain

### 14.1 CLI

All functionality is accessible via a single `nexl` binary:

```
nexl new <name>        ; scaffold a new project
nexl build             ; compile to WASM (default target)
nexl build -t native   ; compile to native binary
nexl build -t bytecode ; compile to bytecode
nexl run <file>        ; compile and execute
nexl test              ; run all tests
nexl check             ; type-check and effect-check without building
nexl fmt               ; format all source files (no config, like gofmt)
nexl fmt --check       ; exit non-zero if any files need formatting
nexl doc               ; generate HTML documentation
nexl repl              ; start interactive REPL
nexl lsp               ; start Language Server Protocol server
nexl dev               ; watch mode: rebuild and re-check on file change
nexl bench             ; run benchmarks (functions annotated with :bench)
nexl fix               ; auto-fix common migration issues for language evolution
nexl audit             ; dependency security audit + FFI trust boundary report
nexl clean             ; clear build cache and compiled artifacts
nexl pkg add <dep>     ; add a dependency
nexl pkg remove <dep>  ; remove a dependency
nexl pkg publish       ; publish to the registry
nexl pkg lock          ; generate lockfile
nexl pkg diff <v1> <v2> ; show API differences between two versions (see §8.5)
nexl sandbox <file>    ; run with no capabilities (pure only)
nexl sandbox --allow-net --allow-fs=/tmp <file>
```

**`nexl dev`** watches the project directory and incrementally re-checks all files on change, reporting errors immediately. This is the primary development workflow, aligned with Principle 5 (fast feedback). It uses the same incremental compilation engine as `nexl build`.

**`nexl sandbox`** uses per-effect permission flags. Each flag maps directly to a capability in the effect system:

| Flag | Capability granted |
|------|--------------------|
| `--allow-net` | `Net` (all network access) |
| `--allow-net=api.example.com` | `Net` (restricted to specified host) |
| `--allow-fs` | `FileSystem` (all file access) |
| `--allow-fs=/tmp` | `FileSystem` (restricted to specified path) |
| `--allow-console` | `Console` (stdin/stdout/stderr) |
| `--allow-time` | `Time` (clock access) |
| `--allow-random` | `Random` (random number generation) |
| `--allow-concurrent` | `Concurrent` (thread/task creation) |
| `--allow-unsafe` | `Unsafe` (raw pointer operations via C FFI) |
| `--allow-all` | All capabilities (equivalent to unrestricted execution) |

With no flags, `nexl sandbox` grants no capabilities — only pure computation is permitted.

**`nexl audit`** performs two functions: (1) scans dependencies for known vulnerabilities against the registry's advisory database, and (2) lists all FFI trust boundaries (`defextern` declarations) with their declared effect annotations, highlighting any that use `:unsafe`.

### 14.2 REPL

The REPL provides a live environment for interactive development.

```
$ nexl repl
nexl 0.1.0 | :help for commands

nxl> (defn fibonacci [n]
       (match n
         0 0
         1 1
         _ (+ (fibonacci (- n 1)) (fibonacci (- n 2)))))
;; => #'user/fibonacci : (Fn [Int] -> Int)

nxl> (fibonacci 10)
;; => 55

nxl> :type fibonacci
;; (Fn [Int] -> Int)

nxl> :effects fetch-user!
;; Direct: #{Net}
;; Transitive: #{Net Log}

nxl> :deps fibonacci
;; Depends on: #{+ - match}
;; Depended on by: nothing yet

nxl> :profile (fibonacci 30)
;; => 832040
;; Wall: 11ms | Allocations: 2.1KB | GC pauses: 0

nxl> :expand (unless false (println "ok"))
;; => (if (not false) (do (println "ok")))

nxl> :test fibonacci
;; Running property tests...
;; ✓ fibonacci(0) = 0
;; ✓ fibonacci(n) = fibonacci(n-1) + fibonacci(n-2) for n in [2..20]
;; 2/2 properties passed

nxl> :source fibonacci
;; (defn fibonacci [n]
;;   (match n ...))
```

**REPL commands:**

| Command | Description |
|---------|-------------|
| `:type <expr>` | Show inferred type |
| `:effects <fn>` | Show direct and transitive effects |
| `:deps <fn>` | Show dependency graph |
| `:expand <expr>` | Show macro expansion |
| `:profile <expr>` | Profile an expression |
| `:test <fn>` | Run property tests and `:examples` contracts for a function |
| `:contract <fn>` | Show `:requires`, `:ensures`, and `:examples` for a function |
| `:source <fn>` | Show source of a definition |
| `:doc <fn>` | Show documentation |
| `:check` | Type-check all definitions in session |
| `:snapshot` | Save current REPL state |
| `:restore <id>` | Restore a previous REPL state |
| `:help` | Show all commands |
| `:quit` | Exit |

**Redefinition semantics.** When a definition is re-entered in the REPL, the old definition is replaced and all dependent definitions in the session are re-typechecked. If the replacement has the same content hash, dependents are not invalidated (content-addressing makes this check trivial).

**Default capabilities.** The REPL grants `Console` and `Time` by default — enough for printing and basic profiling. Additional capabilities can be granted with `:grant <Effect>` or by starting the REPL with `nexl repl --allow-net --allow-fs=/tmp` (same flags as `nexl sandbox`).

**Module context.** The REPL operates in an implicit `user` module. `(import ...)` works as in source files and adds bindings to the session. Definitions are incrementally compiled using the same bytecode backend as `nexl build -t bytecode`.

**Snapshots.** `:snapshot` saves the current session state: all definitions (as AST + compiled bytecode), the type environment, and the current evidence vector (active effect handlers). `:restore <id>` reverts to a previous snapshot. Snapshots are stored as serialized definition stores, not full VM memory images.

### 14.3 Structured REPL Protocol

The REPL accepts machine-readable messages over stdin/stdout (or TCP) for AI agent and IDE integration. The protocol uses Nexl source code as input (not JSON-encoded ASTs) and returns structured JSON responses.

**Request format:**

```json
{"op": "eval", "code": "(+ 1 2)", "session": "s1"}
```

```json
{"op": "define", "code": "(defn double [x : Int] -> Int (* x 2))", "session": "s1"}
```

**Response format:**

```json
{"status": "ok",
 "value": "3",
 "type": "Int",
 "effects": [],
 "diagnostics": [],
 "output": ""}
```

**Error response:**

```json
{"status": "error",
 "diagnostics": [
   {"severity": "error",
    "message": "Type mismatch: expected Int, got Str",
    "line": 1,
    "column": 5,
    "suggestion": "Use (int/parse s) to convert Str to Int"}
 ]}
```

**Protocol operations:** `eval`, `define`, `type-of`, `effects-of`, `deps`, `expand`, `test`, `complete`, `session-create`, `session-destroy`, `capabilities-grant`, `capabilities-revoke`.

**Streaming.** Long-running evaluations produce multiple response messages: intermediate `{"status": "output", "text": "..."}` messages for console output, followed by a final `{"status": "ok", ...}` or `{"status": "error", ...}` message. This follows the nREPL multi-message model.

### 14.4 Formatter

`nexl fmt` applies a canonical, non-configurable style to all `.nx` files. There are no options. All Nexl code looks the same.

Rules (abbreviated):
- 2-space indentation
- Closing delimiters stacked on the final line of the form (standard Lisp convention — no closing delimiters on their own line)
- Function argument list on same line as `defn`/`fn`
- Body on next line, indented
- Blank line between top-level forms
- `:keys` destructuring in alphabetical order
- Special-form-aware indentation: `defn`, `let`, `cond`, `handle`, `match`, `fn`, `do` each have specific indentation rules matching Lisp convention

```clojure
;; Example of canonical formatting:
(defn process-users [users : (Vec User)] -> (Vec Result) ! [Log DB]
  (let [active (filter (fn [u] (= (:status u) :active)) users)]
    (each active (fn [u]
      (Log/info (str "Processing " (:name u)))
      (DB/update-last-seen (:id u) (Time/now))))))
```

The formatter uses the same parser as the compiler. It is idempotent: `nexl fmt` applied twice produces the same output as applied once.

### 14.5 Package Manager

```clojure
;; project.nx
{:package {:name "my-app"
           :version "1.0.0"
           :description "My application"
           :prefix "my-app"}   ; all modules in this package must begin with "my-app."

 :dependencies {"http-server" "^2.1.0"
                "json"        "~1.0.0"}

 :dev-dependencies {"test-utils"  "^1.0.0"
                    "bench-tools" "^0.5.0"}}
```

**Version resolution.** Dependencies use semver range syntax:

| Syntax | Meaning |
|--------|---------|
| `"1.2.3"` | Exact version |
| `"^1.2.3"` | Compatible with 1.2.3 (≥1.2.3, <2.0.0) |
| `"~1.2.3"` | Approximately 1.2.3 (≥1.2.3, <1.3.0) |
| `">=1.2.0"` | At least 1.2.0 |

- Packages are identified by content hash, not name+version. The registry stores content-addressed blobs signed by the author's key.
- The `prefix` field defines the module namespace owned by this package; the compiler verifies that every `.nx` file's module declaration matches the prefix.
- `nexl pkg lock` generates a lockfile recording exact hashes; reproducible builds require the lockfile.
- Semantic versioning is enforced: the compiler prevents publishing a package with incompatible changes without a major version bump (full rules in §8.5).
- Workspace support: `workspace.nexl` groups multiple packages in a monorepo.

**Private registries.** Organizations can host private registries. The `project.nx` file supports a `:registries` section:

```clojure
{:registries {"internal" {:url "https://registry.corp.example.com"
                          :token-env "NEXL_CORP_TOKEN"}}

 :dependencies {"internal-lib" {:version "^1.0.0" :registry "internal"}}}
```

**Supply chain security.** Published packages support Sigstore signatures and SLSA provenance metadata. `nexl audit` verifies signatures and provenance for all dependencies. The registry enforces that published packages include a signed build provenance record, enabling downstream consumers to verify the build environment and source commit.

### 14.6 Language Server (LSP)

`nexl lsp` provides standard LSP features and Nexl-specific capabilities:

**Standard features:**
- Go-to-definition (using content-addressed store)
- Find references (reverse dependency index)
- Hover: type signature, effects, docstring
- Completion: symbols in scope, record fields, effect operations, keyword names
- Diagnostics: type errors, effect errors, unused bindings, missing patterns
- Code actions: add type annotation, add missing match patterns, wrap in handler, extract function
- Rename: pure metadata operation (content hash unchanged)
- Inlay hints: inferred types on `let` bindings

**Nexl-specific features:**
- **Effect inlay hints**: Show the inferred `! [Effects]` on function definitions, not just types.
- **Handler resolution**: "Go to handler" — from an effect operation call, jump to the `handle` form that provides its implementation at the current call site.
- **Capability visualization**: Show which effects are available at the cursor position (based on enclosing `handle` forms and module `:performs` declarations).
- **Sandbox preview**: Show what `nexl sandbox` would allow/deny for the current file.

**Architecture.** The LSP and the compiler share the same analysis engine, built as a library of incremental queries (Salsa-style). The compiler is not a batch process that the LSP shells out to — both are views over the same query database. This is essential for handling incomplete and invalid code, which is what the LSP sees most of the time.

### 14.7 Documentation Generator

`nexl doc` generates HTML documentation from source files.

- **Type signatures and effects** are included automatically — no need to repeat them in doc comments.
- **Contract clauses** (`:requires`, `:ensures`, `:examples` from §4) are rendered as part of the function documentation.
- **Cross-linking** uses content hashes: links to definitions remain stable across versions.
- **Effect documentation**: For each public function, the documentation shows which effects it performs, transitively. This enables consumers to understand the capability requirements of a dependency at a glance.
- **Module-level documentation** is written as a doc comment at the top of the module file.

---

## 15. Interoperability

### 15.1 WASM Component Model

Nexl modules can import and export WASM components using the **Component Model** and WIT (WebAssembly Interface Types) interfaces.

#### Importing Components

```clojure
;; Import a WASM component with WIT type verification at compile time
(import-component "image-processing" :as img
  {:resize (Fn [Bytes Int Int] -> Bytes)
   :blur   (Fn [Bytes Float] -> Bytes)})

(def thumb (img/resize raw-bytes 200 200))
```

#### Exporting as a Component

```clojure
;; Export a Nexl module as a WASM component
(export-component "string-utils"
  {:reverse-words (Fn [Str] -> Str)
   :word-count    (Fn [Str] -> Int)})
```

The compiler generates a WIT interface from the `export-component` declaration. The generated WIT for the above:

```wit
package nexl:string-utils;

interface string-utils {
    reverse-words: func(s: string) -> string;
    word-count: func(s: string) -> s64;
}
```

Nexl types map to WIT types as shown in §15.2. Records map to WIT records, ADT variants map to WIT variants, and enums map to WIT enums.

#### WIT Resource Types

WIT resources represent stateful handles passed across component boundaries without copying. They map naturally to Nexl's effect and capability model.

```clojure
;; Import a WIT resource type
(import-component "database" :as db
  {:Connection (Resource
     {:open    (Fn [Str] -> Connection ! [Net])
      :query   (Fn [Connection Str] -> (Result Rows DbError))
      :close   (Fn [Connection] -> Unit)})})

;; Use: the Connection is an opaque handle, not a copied value
(let [conn (db/Connection.open "postgres://localhost/mydb")]
  (let [rows (db/Connection.query conn "SELECT * FROM users")]
    (db/Connection.close conn)
    rows))
```

Resource types have a defined lifecycle: they are created (constructor), used (methods), and destroyed (destructor). The Nexl compiler verifies that resources are not leaked (all resources must be closed or transferred).

#### Canonical ABI and Copying

The Component Model uses shared-nothing architecture: **all data crossing component boundaries is serialized via the Canonical ABI and copied** into the target component's linear memory. For large data (images, buffers, model weights), this can be a significant cost. Strategies to minimize overhead:

- Pass resource handles instead of raw data — resources are not copied.
- Minimize the number of cross-component calls (batch operations where possible).
- Use `Bytes` for bulk data transfer (serialized once, not per-field).

#### Effect-to-WIT Interface Mapping

Nexl effects can be mapped to WIT interfaces for cross-language interop:

```clojure
;; A Nexl effect:
(defeffect Log
  (info  : (Fn [Str] -> Unit))
  (error : (Fn [Str] -> Unit)))

;; When exported as part of a component, this generates:
;; interface log {
;;     info: func(msg: string);
;;     error: func(msg: string);
;; }
```

This means a WASM component written in Rust or Go can implement a handler for a Nexl effect, and a WIT interface imported into Nexl can be wrapped as an effect. This bidirectional mapping is a key interop mechanism.

#### Closures Across Component Boundaries

Nexl closures cannot be passed directly across component boundaries (the Component Model has no first-class function type). When a Nexl function must be passed to a foreign component, it is exported as a WIT callback interface:

```clojure
;; Passing a callback to a foreign component:
(import-component "event-system" :as events
  {:on-click (Fn [(Fn [Event] -> Unit)] -> Unit)})

;; The (Fn [Event] -> Unit) callback is compiled as a WIT resource
;; with a single `call` method. The foreign component invokes
;; the callback through this resource interface.
```

### 15.2 WASM Core Type Mapping

| Nexl Type | WASM Core Type | WIT Type |
|-----------|---------------|----------|
| `Int` / `Int64` | `i64` | `s64` |
| `Int32` | `i32` | `s32` |
| `Int16` | `i32` (sign-extended) | `s16` |
| `Int8` | `i32` (sign-extended) | `s8` |
| `U64` | `i64` | `u64` |
| `U32` | `i32` | `u32` |
| `U16` | `i32` (zero-extended) | `u16` |
| `U8` | `i32` (zero-extended) | `u8` |
| `Float` / `F64` | `f64` | `float64` |
| `F32` | `f32` | `float32` |
| `Bool` | `i32` (0 or 1) | `bool` |
| `Str` | `i32` ptr + `i32` len | `string` |
| `Bytes` | `i32` ptr + `i32` len | `list<u8>` |
| `Unit` | (no value) | (empty result) |

Sub-word types (`Int8`, `Int16`, `U8`, `U16`) are stored as `i32` in WASM core and narrowed/extended at component boundaries.

### 15.3 C ABI FFI

#### C Type Mapping

| Nexl Type | C Type | Size |
|-----------|--------|------|
| `Int` / `Int64` | `int64_t` | 8 bytes |
| `Int32` | `int32_t` | 4 bytes |
| `Int16` | `int16_t` | 2 bytes |
| `Int8` | `int8_t` | 1 byte |
| `U64` | `uint64_t` | 8 bytes |
| `U32` | `uint32_t` | 4 bytes |
| `U16` | `uint16_t` | 2 bytes |
| `U8` | `uint8_t` | 1 byte |
| `Float` / `F64` | `double` | 8 bytes |
| `F32` | `float` | 4 bytes |
| `Bool` | `_Bool` / `bool` | 1 byte |
| `Str` | `const char*` + `size_t` len | pointer + size |
| `Ptr` | `void*` | pointer-sized |
| `Unit` | `void` | 0 bytes |

```clojure
;; Import a C function — pure (no effects)
(defextern sin  : (Fn [Float] -> Float) "sin")
(defextern sinf  : (Fn [F32] -> F32) "sinf")
(defextern abs   : (Fn [Int32] -> Int32) "abs")

;; Import a C function with specific effect annotation
(defextern puts      : (Fn [Str] -> Int32) "puts"       :performs [Console])
(defextern read-file : (Fn [Str] -> Bytes) "read_file"  :performs [FileSystem])

;; Import an unsafe C function (raw memory access)
(defextern malloc : (Fn [U64] -> Ptr) "malloc"  :unsafe)
(defextern free   : (Fn [Ptr] -> Unit) "free"   :unsafe)
```

**Effect annotations.** `defextern` declarations support three levels of effect tracking:

1. **No annotation** (pure): The programmer asserts the C function has no side effects. The compiler treats it as pure.
2. **`:performs [Effect ...]`**: The programmer declares which specific effects the C function performs. The compiler tracks these effects through the call chain.
3. **`:unsafe`**: The function requires the `Unsafe` capability (raw memory access via `Ptr`). It must be explicitly granted.

**Trust boundary.** `defextern` declarations are **programmer assertions**, not compiler-verified facts. The compiler cannot verify that a C function declared as pure is actually pure, or that a function declared with `:performs [Console]` does not also perform `Net`. Incorrect declarations silently violate the type/effect system. For untrusted C code, use WASM component boundaries (§15.1) where capabilities are enforced by the runtime.

**Memory ownership rules:**

1. **Nexl values passed to C** are pinned (not moved by GC) for the duration of the C function call. The C function receives a pointer to the live Nexl value. The C function **must not** store this pointer beyond the call — if the pointer is needed later, the C function must copy the data.
2. **C-allocated memory** (returned as `Ptr` from C functions) is not managed by Nexl's memory management. The programmer must explicitly free it via a `defextern`'d free function.
3. **Resource wrapping**: For C resources that need deterministic cleanup, use an opaque type with a drop function:

```clojure
(deftype-opaque CHandle Ptr :drop free-handle)

(defextern open-handle  : (Fn [Str] -> CHandle) "open_handle"  :performs [FileSystem])
(defextern free-handle  : (Fn [CHandle] -> Unit) "free_handle")
(defextern use-handle   : (Fn [CHandle Int] -> Int) "use_handle")
```

The `:drop` function is called automatically when the `CHandle` value is no longer referenced.

### 15.4 Exporting for C

```clojure
;; Expose a Nexl function with a C-compatible ABI
(defexport add_ints : (Fn [Int Int] -> Int)
  [a b]
  (+ a b))
```

**Restrictions.** Exported functions must use flat, C-compatible types only:

- Closures cannot be exported (C has no closure representation).
- ADT variants cannot be exported directly (use integer tags or serialization).
- Effects cannot appear in exported function signatures (the C caller cannot provide handlers).

**GC safety.** When a C program calls an exported Nexl function, the Nexl runtime's memory management is active for the duration of the call. The C caller must not call exported Nexl functions concurrently from multiple threads unless the Nexl runtime is initialized for multi-threaded use.

### 15.5 JSON Interop

JSON is the lingua franca of data exchange. Nexl provides type-safe JSON codecs:

```clojure
;; Dynamic: parse to Any
(json/parse "{\"name\": \"Alice\"}")
;; => {:name "Alice"}

;; Typed: decode to a specific type
(json/decode Str "{\"name\": \"Alice\"}")  ; error: top-level is a map

;; Auto-derive codecs for record types (see §5.7)
(deftype User
  :derive [JsonCodec]
  {:name Str
   :age  Int
   :role (| :admin :user)})

(json/decode User "{\"name\":\"Alice\",\"age\":30,\"role\":\"admin\"}")
;; => {:name "Alice" :age 30 :role :admin}

(json/encode {:name "Alice" :age 30 :role :admin})
;; => "{\"name\":\"Alice\",\"age\":30,\"role\":\"admin\"}"
```

**Codec configuration.** Derived codecs can be customized:

```clojure
(deftype ApiUser
  :derive [(JsonCodec {:rename-fields :snake-case
                       :option-handling :omit    ; Option fields omitted when None (vs null)
                       :enum-repr :untagged})]   ; enum variants as bare strings
  {:user-name   Str
   :email       (Option Str)
   :access-level (| :admin :user :guest)})

;; Encodes as: {"user_name": "Alice", "access_level": "admin"}
;; (email omitted because it is None)
```

**Streaming.** For large JSON payloads, use the streaming API:

```clojure
(json/parse-stream reader)
;; => (Iter (Result JsonValue JsonError))
```

**Error type.** JSON decoding errors are represented as `JsonError`:

```clojure
(deftype JsonError
  {:path     (Vec Str)       ; path into the JSON document, e.g., ["users" "0" "age"]
   :expected Str             ; expected type or value, e.g., "Int"
   :actual   Str             ; actual JSON value found, e.g., "\"not-a-number\""
   :message  Str})           ; human-readable error message
```

### 15.6 Capability Security Across Boundaries

Nexl's capability security model extends across all interop boundaries. This section describes how the static effect system and runtime enforcement work together.

**The capability chain:**

1. **Effect declaration** — A function's `! [Effects]` annotation declares which capabilities it requires.
2. **Handler grant** — A `handle` form (or module-level `:performs`) provides the capability by installing a handler.
3. **Interop boundary** — At WASM component boundaries, capabilities are mapped to WIT interfaces. At C FFI boundaries, capabilities are declared by the programmer via `:performs`.
4. **Runtime enforcement** — WASM component boundaries enforce capabilities at runtime: a component cannot call an imported function unless the corresponding WIT interface was linked.

**WASM component enforcement.** When `nexl sandbox --allow-net <file>` runs a program, the sandbox compiles the program as a WASM component and links only the WIT interfaces corresponding to the granted capabilities. If the program attempts to use `Net` but `--allow-net` was not specified, the component instantiation fails — the import is not satisfied. This is defense in depth: even if the static effect system were bypassed (e.g., via `Unsafe` or incorrect `defextern` annotations), the WASM component runtime enforces the capability boundary.

**C FFI trust boundary.** For C FFI (`defextern`), capabilities are declared by the programmer and trusted by the compiler. This is the weakest link in the security chain. `nexl audit` reports all `defextern` declarations and their effect annotations, enabling manual review of trust boundaries.

**Audit tooling.** `nexl audit` provides:

- A list of all `defextern` declarations with their declared effects.
- Warnings for `defextern` declarations with no effect annotation (implicitly pure — is this correct?).
- A list of all `import-component` boundaries with their WIT interfaces.
- A summary of which capabilities each module requires, transitively.

---
## Appendix A: Syntax Quick Reference

```clojure
;; --- Definitions ---
(def pi 3.14159)
(defn square [x : a] -> a :where [(Numeric a)] (* x x))
(defmacro unless [c & body] `(if (not ~c) (do ~@body)))
(deftype Option [a] | None | (Some a))
(defeffect Log (info : (Fn [Str] -> Unit)))

;; --- Bindings ---
(let [x 10, y 20] (+ x y))
(let [mut n 0] (set! n 1) n)
(let [{:keys [name age] :rest extra} person] extra)          ; rest map
(let [{:user/keys [name email]} record] name)                ; ns shorthand
(let [{[:address :city] city} user] city)                    ; deep path
(let [[head & tail :as coll] items] coll)                    ; named capture
(let [(: Int n) (get-val k)] n)                              ; type-asserting
(let [(view json/parse {:keys [x]}) raw] x)                  ; view pattern
(if-let [(Some user) (find-user id)] (process user) :missing)
(when-let [(Ok x) (try-op)] (use x))
(let [(Ok n) (parse-int s) | (Err :bad)] (use n))             ; let-else

;; --- Pattern matching ---
(if cond then else)
(cond (< x 0) :neg :else :non-neg)
(match opt
  (Some x)          x
  None              :missing)
(match status
  (| :pending :processing)  :in-flight     ; or-pattern
  :done                     :complete)
(match n
  (& (: Int x) (pos? x))  :positive         ; and-pattern
  _                              :other)
(match s
  "User:{id}"  (handle id)                  ; string pattern
  _            :skip)
(let [expected :ok]
  (match result ^expected :yes _ :no))      ; pin operator
(match user
  (view :role :admin)  (admin-view user)    ; view pattern
  _                    (deny))
(match u
  (admin {:keys [name]} :as whole)  (process whole name))  ; :as capture
(loop [i 0] (if (= i 5) i (recur (+ i 1))))

;; --- Functions ---
(fn [x y] (+ x y))
(fn [& args] (count args))

;; --- Effects ---
(defeffect Console (print : (Fn [Str] -> Unit)))
(handle [Console (print [s] (println s))] body)        ; inline handler
(defhandler StdoutConsole                               ; named handler
  Console (print [s] (rt/stdout-write s)))
(defhandler JsonLog [config]                            ; parameterized handler
  Log (info [msg] (println (json/encode {:msg msg :env (:env config)}))))
(handle [StdoutConsole] (Console/print "hi"))           ; use named handler
(handle [(JsonLog {:env :prod})] (Log/info "hi"))       ; use parameterized handler
(defn f! [] -> Unit ! [Console] (Console/print "hi"))   ; qualified form
(defn f! [] -> Unit ! [Console] (print "hi"))           ; unqualified, also valid

;; --- Error handling ---
(let [n (parse-int s)?] ...)
(panic "unreachable")
(match (risky-op) (Ok v) v (Err e) (panic (str e)))

;; --- Modules ---
(module my.mod :performs [IO] :exports [run!])
(import other.mod :as other)
(re-export other.mod :select [run!])

;; --- Collections ---
[1 2 3]            ; Vec
{:a 1 :b 2}        ; Map
#{1 2 3}           ; Set
'(1 2 3)           ; List

;; --- Threading ---
(-> x (f a) (g b))       ; (g (f x a) b)
(->> x (f a) (g b))      ; (g b (f a x))

;; --- Protocols ---
(defprotocol Show (show : (Fn [Self] -> Str)))
(impl Point Show (show [p] (str "(" (:x p) ", " (:y p) ")")))
(deftype Color :derive [Show Eq Hash] | Red | Green | Blue)

;; --- Record types ---
(deftype Point {:x Float :y Float})
(def p (Point {:x 1.0 :y 2.0}))
(:x p)                           ; => 1.0

;; --- Fixed-width numerics ---
(def byte-val 255u8)                           ; U8 literal
(def pixel : Int32 (->int32 x))               ; explicit conversion
(deftype-alias Bytes (Vec U8))              ; Bytes alias
(deftype-opaque UserId Str :derive [Show Eq])  ; opaque nominal wrapper
(defextern sinf : (Fn [F32] -> F32) "sinf")         ; C FFI with F32
(let [n 42i32] (+ n 1i32))                     ; same-type arithmetic
;; (+ 42i32 1u8)                               ; compile error: cross-type

;; --- Effect groups ---
(defeffect-group IO [FileSystem Console Net Time])

;; --- Tuples ---
(let [pair : (Tuple Int Str) [42 "hello"]] ...)  ; 2-tuple
(fst pair)                                        ; => 42
(snd pair)                                        ; => "hello"

;; --- Bottom type ---
(defn must-crash [] -> Never (panic "always fails"))
;; Never is assignable to any return type — panic can appear in any branch
(defn div [a : Int b : Int] -> Int
  (if (= b 0)
    (panic "division by zero")   ; type Never — OK here, result is Int
    (/ a b)))
```

---

## Appendix B: Effect Row Notation

| Notation | Meaning |
|----------|---------|
| `(Fn [] -> A)` | Pure function, no effects |
| `(Fn [] -> A ! [E])` | Performs effect E |
| `(Fn [] -> A ! [E1 E2])` | Performs effects E1 and E2 |
| `(Fn [] -> A ! [])` | Explicitly pure (same as no `!`) |
| `(Fn [] -> A ! [e])` | Effect-polymorphic; `e` is a row variable |

---

## Appendix C: Keyword Index

| Form | Section |
|------|---------|
| `->`, `->>`, `as->` | §4.13 |
| `&` (and-pattern) | §4.9 |
| `\|` (or-pattern) | §4.9 |
| `^` (pin operator) | §4.9 |
| `Any` | §5.10 |
| `assert-type` | §5.10 |
| `atom` | §3.4 |
| `cond` | §4.6 |
| `declare` | §4.16 |
| `def` | §4.1 |
| `defextern` | §15.3 |
| `defeffect` | §6.2 |
| `defexport` | §15.4 |
| `defhandler` | §6.10 |
| `export-component` | §15.1 |
| `datum->syntax` | §7.6 |
| `defmacro-elab` | §7.8 |
| `defmacro` | §7.3 |
| `defn` | §4.2 |
| `defpattern` | §4.9 |
| `defreader` | §7.10 |
| `defmacro-syntax` | §7.4 |
| `defreader-text` | §7.10 |
| `:ensures`, `:requires`, `:examples` | §4.2.1 |
| `:for-syntax` (import modifier) | §7.11 |
| `gensym` | §7.6 |
| `par-let` | §10.6 |
| `:nl` (natural language pattern) | §4.9 |
| `SyntaxObj` | §7.2 |
| `syntax-datum`, `syntax-loc`, `syntax->list` | §7.2 |
| `syntax-fail`, `syntax-warn` | §7.7 |
| `TypeDescriptor` | §7.8 |
| `TypedSyntax` | §7.8 |
| `deftype` | §5.7 |
| `deftype-alias` | §5.8 |
| `deftype-opaque` | §5.9 |
| `do` | §4.8 |
| `each`, `times` | §4.15 |
| `fn` | §4.3 |
| `for` | §4.14 |
| `if` | §4.5 |
| `if-let` | §4.12 |
| `export` (re-export form) | §8.7 |
| `import` | §8.2 |
| `import-component` | §15.1 |
| `let` | §4.4 |
| `let-else` | §4.12 |
| `loop`, `recur` | §4.10 |
| `match` | §4.9 |
| `module` | §8.1 |
| `:performs` (module effect declaration) | §8.1 |
| `re-export` | §8.7 |
| `:signature` (module contract) | §8.1 |
| `submodule` (`:test` submodule) | §8.10 |
| `^:package` (visibility) | §8.8 |
| `panic` | §9.4 |
| `set!` | §4.4 |
| `view` (view pattern) | §4.9 |
| `when`, `unless` | §4.7 |
| `when-let` | §4.12 |
| `handle` | §6.4, §6.5 |
| `?` operator | §9.3 |
| `:as` (named capture) | §4.9, §4.11 |
| `:keys`, `:or`, `:rest` | §4.11 |
| `:ns/keys` | §4.11 |
| `defeffect-group` | §6.9 |
| `defn-macro` | §7.1 |
| `defprotocol` | §5.11 |
| `impl` | §5.11 |
| `:where` (protocol constraints) | §5.11 |
| `:derive` (auto-derived protocols) | §5.11 |
| `Foldable` | §5.12 |
| `Iter` | §5.12 |
| `Buildable` | §5.12 |
| `for!` | §5.12 |
| `Dynamic` (effect) | §5.10, §6.9 |
| `Unsafe` (effect) | §6.9, §15.3 |
| `Into` (error coercion) | §9.3 |
| `Unit`, `unit` | §3.1 |
| `Never` (bottom type) | §3.1, §5.3 |
| `Tuple` | §3.2, §5.3 |
| `Int8`, `Int16`, `Int32`, `Int64` | §3.1 |
| `U8`, `U16`, `U32`, `U64` | §3.1 |
| `F32`, `F64` | §3.1 |
| `Bytes` | §5.8 |
| `Numeric` / `IntLike` / `FracLike` | §5.11 |
| `conv` (module) | §11.1 |
| `go` (concurrent block shorthand) | §11.1 |
| `entries`, `keys`, `vals` (map accessors) | §3.6, §11.1 |
| `:drop` (opaque type destructor) | §5.9 |
| `LLM` (effect) | §6.9, §11.1 |
| `structured-executor` | §10.2, §11.1 |
| Suffixed numeric literals (`42i32`, `255u8`, `3.14f32`) | §2.3 |

---

## Appendix D: Formal Grammar (EBNF)

This grammar uses Extended Backus-Naur Form. `(* ... *)` denotes comments. `{ x }` denotes zero or more repetitions of `x`. `[ x ]` denotes an optional `x`. `|` separates alternatives.

### D.1 Lexical Grammar

```ebnf
(* --- Whitespace and Comments --- *)
whitespace     = " " | "\t" | "\n" | "\r" | "," ;
comment        = ";" , { any-char - "\n" } , "\n" ;
discard        = "#_" , form ;

(* --- Atoms --- *)
atom           = int-literal | float-literal | ratio-literal
               | string-literal | char-literal
               | keyword | symbol | bool-literal | "unit" ;

bool-literal   = "true" | "false" ;

(* --- Numeric Literals --- *)
digit          = "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" ;
hex-digit      = digit | "a" | "b" | "c" | "d" | "e" | "f"
               | "A" | "B" | "C" | "D" | "E" | "F" ;
underscore-sep = "_" ;
digits         = digit , { digit | underscore-sep } ;

int-literal    = [ "-" ] , digits , [ int-suffix ]
               | [ "-" ] , "0x" , hex-digit , { hex-digit | underscore-sep }
               | [ "-" ] , "0b" , ( "0" | "1" ) , { "0" | "1" | underscore-sep }
               | [ "-" ] , "0o" , oct-digit , { oct-digit | underscore-sep } ;
int-suffix     = "i8" | "i16" | "i32" | "i64"
               | "u8" | "u16" | "u32" | "u64" ;

float-literal  = [ "-" ] , digits , "." , digits , [ exponent ] , [ float-suffix ]
               | [ "-" ] , digits , exponent , [ float-suffix ] ;
exponent       = ( "e" | "E" ) , [ "+" | "-" ] , digits ;
float-suffix   = "f32" | "f64" ;

ratio-literal  = [ "-" ] , digits , "/" , digits ;

(* --- Strings --- *)
string-literal = '"' , { string-char | interpolation | escape-seq } , '"'
               | '"""' , { any-char | interpolation } , '"""'
               | 'r' , { '#' } , '"' , { raw-char } , '"' , { '#' } ;
raw-char       = any-char - closing-raw-delimiter ;
string-char    = any-char - ( '"' | "\\" | "{" ) ;
interpolation  = "{" , expression , "}" ;
escape-seq     = "\\" , ( "n" | "t" | "r" | "\\" | '"' | "{" ) ;

(* --- Characters --- *)
char-literal   = "\\" , ( letter | "space" | "newline" | "tab"
               | "u" , hex-digit , hex-digit , hex-digit , hex-digit ) ;

(* --- Keywords --- *)
keyword        = ":" , symbol-chars
               | "::" , symbol-chars
               | ":" , symbol-chars , "/" , symbol-chars ;

(* --- Symbols --- *)
symbol         = symbol-start , { symbol-cont }
               | symbol-chars , "/" , symbol-chars ;
symbol-start   = letter | "_" | "?" | "!" | "*" | "+" | "<" | ">" | "=" | "-" ;
symbol-cont    = symbol-start | digit ;
symbol-chars   = symbol-start , { symbol-cont } ;
letter         = "a"-"z" | "A"-"Z" | unicode-letter ;
```

### D.2 Collection Literals

```ebnf
form           = atom | list | vector | map | set
               | quoted | quasiquoted | unquoted | unquote-spliced
               | deref-form | var-ref | discard | reader-ext ;

list           = "(" , { form } , ")" ;
vector         = "[" , { form } , "]" ;
map            = "{" , { form , form } , "}" ;
set            = "#" , "{" , { form } , "}" ;

quoted         = "'" , form ;
quasiquoted    = "`" , form ;
unquoted       = "~" , form ;
unquote-spliced = "~@" , form ;
deref-form     = "@" , form ;
var-ref        = "#'" , symbol ;
reader-ext     = "#" , letter , { letter | digit } , form ;
```

### D.3 Declarations

```ebnf
(* --- Top-Level Declarations --- *)
definition     = def-form | defn-form | defn-macro-form | deftype-form | defeffect-form
               | defhandler-form | defprotocol-form | defmacro-form | defmacro-syntax-form
               | defmacro-elab-form | defreader-form | defreader-text-form | module-form
               | import-form | impl-form | defpattern-form ;

def-form       = "(" , "def" , symbol , [ type-annot ] , expression , ")" ;
defn-form      = "(" , "defn" , symbol , [ docstring ] ,
                 param-list , [ type-annot ] , [ effect-annot ] ,
                 [ where-clause ] , { contract-clause } , expression , ")"
               | "(" , "defn" , symbol , [ docstring ] ,
                 { "(" , param-list , [ type-annot ] , [ effect-annot ] ,
                   expression , ")" } , ")" ;

param-list     = "[" , { param-decl } , [ "&" , symbol ] , "]" ;
param-decl     = pattern | ( symbol , ":" , type-expr ) ;

type-annot     = ":" , type-expr ;
effect-annot   = "!" , "[" , { type-expr } , "]" ;
where-clause   = ":where" , "[" , { constraint } , "]" ;
constraint     = "(" , symbol , symbol , ")" ;
contract-clause = requires-clause | ensures-clause | examples-clause ;
requires-clause = ":requires" , "[" , { expression } , "]" ;
ensures-clause  = ":ensures" , "[" , { expression } , "]" ;
examples-clause = ":examples" , "[" , { example-map } , "]" ;
example-map    = "{" , ":in" , vector , ":out" , form , "}" ;

docstring      = string-literal ;

(* --- Type Definitions --- *)
deftype-form   = "(" , "deftype" , symbol , [ type-params ] ,
                 [ derive-clause ] , type-body , ")" ;
type-params    = "[" , { symbol } , "]" ;
derive-clause  = ":derive" , "[" , { symbol } , "]" ;
type-body      = record-body | variant-body ;
record-body    = "{" , { keyword , type-expr } , "}" ;
variant-body   = { "|" , variant } ;
variant        = symbol | "(" , symbol , { type-expr | record-body } , ")" ;

defalias-form  = "(" , "deftype-alias" , symbol , type-expr , ")" ;
defopaque-form = "(" , "deftype-opaque" , symbol , type-expr ,
                 [ derive-clause ] , ")" ;

(* --- Effect Definitions --- *)
defeffect-form = "(" , "defeffect" , symbol , [ type-params ] ,
                 { effect-op-decl } , ")" ;
effect-op-decl = "(" , symbol , ":" , fn-type , ")" ;

(* --- Named Effect Handlers --- *)
defhandler-form = "(" , "defhandler" , symbol , [ handler-params ] ,
                  { handler-effect-section } , ")" ;
handler-params  = "[" , { symbol } , "]" ;
handler-effect-section = symbol , { handler-op-impl } ;
handler-op-impl = "(" , symbol , param-list , { expression } , ")" ;
(* handler-effect-section: uppercase symbol names the effect, *)
(* followed by operation implementations. Same structure as impl. *)
(* handler-params: lowercase vector after handler name = configuration params. *)

(* --- Protocol Definitions --- *)
defprotocol-form = "(" , "defprotocol" , symbol , [ type-params ] ,
                   [ docstring ] , [ extends-clause ] ,
                   { protocol-op-decl } , ")" ;
extends-clause   = ":extends" , "[" , { symbol } , "]" ;
protocol-op-decl = "(" , symbol , ":" , fn-type , [ default-impl ] , ")" ;
default-impl     = ":default" , expression ;

(* --- Protocol Implementation --- *)
impl-form      = "(" , "impl" , symbol ,
                 { symbol , { "(" , symbol , param-list , expression , ")" } } ,
                 ")" ;

(* --- Named Patterns --- *)
defpattern-form = "(" , "defpattern" , symbol , "[" , { symbol } , "]" ,
                  pattern , ")" ;
(* The body is a single pattern form (may include :when guard at top level). *)
(* Parameterised patterns use the binding names from the param list in the body. *)

(* --- Modules --- *)
module-form    = "(" , "module" , qualified-name ,
                 [ ":performs" , "[" , { symbol } , "]" ] ,
                 [ ":exports"  , "[" , { symbol } , "]" ] ,
                 [ ":imports"  , "[" , { import-spec } , "]" ] , ")" ;
import-spec    = "[" , qualified-name ,
                 ( ":as" , symbol
                 | ":refer"  , "[" , { symbol } , "]"
                 | ":exclude" , "[" , { symbol } , "]"
                 | ":rename"  , "{" , { symbol , symbol } , "}"
                 | (* empty — import all *) ) , "]" ;
import-form    = "(" , "import" , qualified-name ,
                 ( ":as" , symbol
                 | ":refer"  , "[" , { symbol } , "]"
                 | ":exclude" , "[" , { symbol } , "]"
                 | ":rename"  , "{" , { symbol , symbol } , "}"
                 | (* empty — import all *) ) , ")" ;
(* Planned — not yet implemented: *)
export-form    = "(" , "export" , symbol , qualified-ref , ")" ;
re-export-form = "(" , "re-export" , qualified-name ,
                 [ ":select" , "[" , { symbol } , "]" ] , ")" ;
submodule-form = "(" , "submodule" , ":test" , qualified-name ,
                 { definition } , ")" ;
qualified-name = symbol , { "." , symbol } ;
qualified-ref  = symbol , "/" , symbol ;
```

### D.4 Expressions

```ebnf
expression     = atom | list | vector | map | set
               | if-form | cond-form | match-form | let-form
               | do-form | fn-form | loop-form
               | for-form | each-form | times-form
               | handle-form | threading-form
               | type-assert | error-prop ;

if-form        = "(" , "if" , expression , expression , expression , ")" ;
cond-form      = "(" , "cond" , { expression , expression } , ")" ;
do-form        = "(" , "do" , { expression } , ")" ;

fn-form        = "(" , "fn" , param-list , [ type-annot ] ,
                 [ effect-annot ] , expression , ")" ;

let-form       = "(" , "let" , "[" , { let-binding } , "]" , expression , ")" ;
let-binding    = pattern , expression , [ "|" , expression ]
               | "mut" , symbol , expression ;

loop-form      = "(" , "loop" , "[" , { symbol , expression } , "]" ,
                 expression , ")" ;
recur-form     = "(" , "recur" , { expression } , ")" ;

for-form       = "(" , ( "for" | "for!" ) , "[" , { for-clause } , "]" ,
                 expression , ")" ;
for-clause     = pattern , expression
               | ":when" , expression
               | ":while" , expression
               | ":let" , "[" , { let-binding } , "]" ;

each-form      = "(" , "each" , "[" , pattern , expression , "]" ,
                 { expression } , ")" ;
times-form     = "(" , "times" , "[" , symbol , expression , "]" ,
                 { expression } , ")" ;

type-assert    = "(" , ":" , type-expr , expression , ")" ;
error-prop     = expression , "?" ;

(* --- Threading --- *)
threading-form = "(" , ( "->" | "->>" | "as->" ) , expression ,
                 { expression } , ")" ;
```

### D.5 Patterns

```ebnf
pattern        = literal-pat | binding-pat | wildcard-pat
               | vector-pat | map-pat | ctor-pat
               | or-pat | and-pat | pin-pat | type-pat
               | view-pat | string-pat | nl-pat
               | named-pat-use ;

literal-pat    = int-literal | float-literal | string-literal
               | keyword | bool-literal | "unit" ;
binding-pat    = symbol ;
wildcard-pat   = "_" ;

vector-pat     = "[" , { pattern } , [ "&" , symbol ] ,
                 [ ":as" , symbol ] , "]" ;
map-pat        = "{" , { map-pat-entry } ,
                 [ ":as" , symbol ] , [ ":rest" , symbol ] , "}" ;
map-pat-entry  = ":keys" , "[" , { symbol } , "]"
               | ":or" , "{" , { symbol , form } , "}"
               | keyword , pattern
               | ":" , symbol , "/keys" , "[" , { symbol } , "]"
               | vector , symbol ;

ctor-pat       = "(" , constructor-symbol , { pattern } ,
                 [ ":as" , symbol ] , ")" ;
or-pat         = "(" , "|" , pattern , pattern , { pattern } , ")" ;
and-pat        = "(" , "&" , pattern , pattern , { pattern } , ")" ;
pin-pat        = "^" , symbol ;
type-pat       = "(" , ":" , type-expr , pattern , ")" ;
view-pat       = "(" , "view" , expression , pattern , ")" ;
string-pat     = '"' , { string-char | "{" , symbol , "}" } , '"' ;
nl-pat         = ":nl" , string-literal ;
named-pat-use  = "(" , symbol , { pattern } , ")" ;
(* Use of a defpattern — the symbol must resolve to a defpattern definition. *)
(* Syntactically identical to ctor-pat; resolved during name resolution. *)

guard          = ":when" , expression ;
match-arm      = pattern , [ guard ] , expression ;
match-form     = "(" , "match" , expression , { match-arm } , ")" ;
```

### D.6 Type Expressions

```ebnf
type-expr      = type-name
               | type-app
               | fn-type
               | row-type
               | union-type ;

type-name      = symbol ;                           (* Int, Float, Bool, etc. *)
type-app       = "(" , type-name , { type-expr } , ")" ;  (* (Vec Int), (Map k v) *)
fn-type        = "(" , "fn" , "[" , { type-expr } , "]" ,
                 "->" , type-expr , [ effect-annot ] , ")" ;
row-type       = "{" , { symbol , ":" , type-expr } ,
                 [ "|" , ( "_" | symbol ) ] , "}" ;
union-type     = "(" , "|" , { type-expr } , ")" ;
```

### D.7 Effect Handlers

```ebnf
handle-form    = "(" , "handle" , handler-spec , { expression } , ")" ;
handler-spec   = "[" , { handler-entry } , "]" ;
handler-entry  = handler-ref                   (* named handler: ConsoleLog *)
               | handler-call                  (* parameterized: (JsonLog config) *)
               | handler-effect ;              (* inline: Log (info [msg] ...) *)
handler-ref    = symbol ;                      (* must resolve to a defhandler *)
handler-call   = "(" , symbol , { expression } , ")" ;
handler-effect = symbol , { handler-op } ;
handler-op     = "(" , symbol , param-list , { expression } , ")" ;
```

---

*This specification is a living document. It will be updated as the language evolves through implementation. Where the spec is silent, defer to the principle of least surprise and the nearest analogous behavior in Clojure or Rust.*
