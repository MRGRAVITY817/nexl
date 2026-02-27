# A Tour of Nexl

This is a hands-on tutorial for the Nexl programming language. You'll write real
code, run it, and see results. By the end you'll be comfortable with Nexl's core
features: expressions, functions, collections, higher-order programming, error
handling, and I/O.

**Prerequisites:** Build the `nexl` CLI from this repo with `cargo build`.

---

## 1. Hello, World

Create a file called `hello.nx`:

```clojure
(io/println "Hello, world!")
```

Run it:

```
$ nexl run hello.nx
Hello, world!
```

That's it. `io/println` prints a value to stdout with a newline. Unlike many
REPLs, `nexl run` does **not** print expression results automatically — you
must explicitly print what you want to see.

---

## 2. The REPL

For interactive exploration, start the REPL:

```
$ nexl repl
nexl 0.1.0 | :help for commands

nexl> (+ 1 2)
3

nexl> (* 6 7)
42
```

The REPL prints the result of every expression. Use `:quit` to exit.

Throughout this tutorial, lines starting with `nexl>` are REPL input. Lines
without a prefix are output.

---

## 3. Values and Types

Nexl has the types you'd expect:

```clojure
nexl> 42          ;; Int
42

nexl> 3.14        ;; Float
3.14

nexl> true        ;; Bool
true

nexl> "hello"     ;; Str
"hello"

nexl> ()          ;; Unit (like void / nil — but it's NOT nil, see ADR-001)
()
```

Use `:type` to ask the REPL for the inferred type:

```clojure
nexl> :type 42
Int

nexl> :type "hello"
Str
```

---

## 4. Arithmetic

All arithmetic operators are prefix (this is a Lisp!):

```clojure
nexl> (+ 1 2 3)
6

nexl> (- 10 3)
7

nexl> (* 2 3 4)
24

nexl> (/ 10 3)
3

nexl> (mod 10 3)
1
```

`+` and `*` are variadic — they accept any number of arguments.

**Important:** Nexl does not allow mixing `Int` and `Float` in arithmetic.
This is a deliberate design choice (ADR-006). Convert explicitly:

```clojure
nexl> (+ 1 1.0)
;; Error: cross-type arithmetic

nexl> (+ 1.0 2.0)
3.0

nexl> (+ (? (conv/->float 1)) 2.0)
3
```

---

## 5. Comparisons and Logic

```clojure
nexl> (= 1 1)
true

nexl> (< 3 5)
true

nexl> (>= 10 10)
true

nexl> (not true)
false

nexl> (and true false)
false

nexl> (or false true)
true
```

---

## 6. Bindings with `def` and `let`

`def` creates a top-level binding:

```clojure
nexl> (def x 42)
()

nexl> x
42
```

`let` creates local bindings that are scoped to the body:

```clojure
nexl> (let [a 10
            b 20]
       (+ a b))
30
```

Bindings are immutable by default. Nexl values are persistent data structures —
you create new values rather than mutating old ones.

---

## 7. Conditionals

`if` takes exactly three arguments: condition, then-branch, else-branch:

```clojure
nexl> (if (> 5 3)
       "yes"
       "no")
"yes"
```

The condition **must** be a `Bool`. Nexl does not treat `0`, `""`, or `nil` as
falsy — only `true` and `false` are valid conditions (ADR-004).

For multi-expression branches, wrap in `do`:

```clojure
(if (> x 0)
  (do
    (io/println "positive")
    x)
  (do
    (io/println "non-positive")
    0))
```

---

## 8. Functions

### Defining functions

```clojure
nexl> (defn square [x]
       (* x x))
()

nexl> (square 5)
25
```

Functions can have multiple expressions in the body — the last one is the return
value:

```clojure
(defn greet [name]
  (let [msg (str "Hello, " name "!")]
    msg))
```

### Anonymous functions

```clojure
nexl> (def double (fn [x] (* x 2)))
()

nexl> (double 21)
42
```

### Recursion

Functions defined with `defn` can call themselves:

```clojure
(defn factorial [n]
  (if (<= n 1)
    1
    (* n (factorial (- n 1)))))
```

```clojure
nexl> (factorial 10)
3628800
```

---

## 9. Collections

### Vectors

Vectors are ordered, indexed sequences:

```clojure
nexl> [1 2 3]
[1 2 3]

nexl> (append [1 2] 3)
[1 2 3]

nexl> (count [10 20 30])
3
```

Accessing elements — `get` returns an `Option` (either `(Some value)` or `None`):

```clojure
nexl> (get [10 20 30] 1)
(Some 20)

nexl> (get [10 20 30] 99)
None
```

Other vector operations:

```clojure
nexl> (first [10 20 30])
(Some 10)

nexl> (rest [10 20 30])
[20 30]

nexl> (last [10 20 30])
(Some 30)

nexl> (slice [10 20 30 40 50] 1 4)
[20 30 40]
```

### Maps

Maps are key-value collections:

```clojure
nexl> {:name "Alice" :age 30}
{:name "Alice" :age 30}

nexl> (get {:name "Alice"} :name)
(Some "Alice")

nexl> (put {:x 1} :y 2)
{:x 1 :y 2}

nexl> (keys {:a 1 :b 2})
[:a :b]

nexl> (vals {:a 1 :b 2})
[1 2]
```

### Sets

```clojure
nexl> #{1 2 3}
#{1 2 3}

nexl> (contains? #{1 2 3} 2)
true

nexl> (add #{1 2} 3)
#{1 2 3}

nexl> (union #{1 2} #{2 3})
#{1 2 3}
```

---

## 10. Higher-Order Functions

`map`, `filter`, and `reduce` work on all collections:

```clojure
nexl> (map (fn [x] (* x x)) [1 2 3 4 5])
[1 4 9 16 25]

nexl> (filter (fn [x] (> x 3)) [1 2 3 4 5])
[4 5]

nexl> (reduce + 0 [1 2 3 4 5])
15
```

Composition with `core/comp`:

```clojure
nexl> (def add1-then-double (core/comp (fn [x] (* x 2)) (fn [x] (+ x 1))))
()

nexl> (add1-then-double 3)
8
```

---

## 11. Strings

The `str` function concatenates anything into a string:

```clojure
nexl> (str "Hello" ", " "world!")
"Hello, world!"

nexl> (str "The answer is " 42)
"The answer is 42"
```

String module functions use the `str/` prefix:

```clojure
nexl> (str/split "a,b,c" ",")
["a" "b" "c"]

nexl> (str/join "-" ["2024" "01" "15"])
"2024-01-15"

nexl> (str/upper "hello")
"HELLO"

nexl> (str/trim "  hello  ")
"hello"

nexl> (str/starts-with? "hello world" "hello")
true

nexl> (str/replace "hello world" "world" "nexl")
"hello nexl"

nexl> (count "hello")
5
```

---

## 12. Loops

### `loop` / `recur`

`loop` establishes bindings and a `recur` target. `recur` jumps back to the
loop with new values — this is tail-recursive and won't blow the stack:

```clojure
(defn sum-to [n]
  (loop [i 0 total 0]
    (if (> i n)
      total
      (recur (+ i 1) (+ total i)))))
```

```clojure
nexl> (sum-to 100)
5050
```

### `for` comprehensions

`for` builds a vector from an iteration:

```clojure
nexl> (for [x [1 2 3 4 5]
            :when (> x 2)]
       (* x 10))
[30 40 50]
```

### `each` for side effects

`each` iterates for side effects and returns `Unit`:

```clojure
(each [item ["apple" "banana" "cherry"]]
  (io/println (str "I like " item)))
```

### `times`

`times` repeats a body `n` times with an index:

```clojure
(times [i 5]
  (io/println (str "Step " i)))
;; Prints Step 0 through Step 4
```

---

## 13. Error Handling with Option and Result

Nexl uses `Option` and `Result` types for error handling — no exceptions, no
null pointers.

### Option

`Some` wraps a value; `None` represents absence:

```clojure
nexl> (Some 42)
(Some 42)

nexl> None
None
```

Collection access functions return `Option`:

```clojure
nexl> (get [10 20] 0)
(Some 10)

nexl> (get [10 20] 5)
None
```

### Result

`Ok` wraps a success; `Err` wraps a failure:

```clojure
nexl> (Ok 42)
(Ok 42)

nexl> (Err "something went wrong")
(Err "something went wrong")
```

I/O functions return `Result`:

```clojure
;; Returns (Ok "file contents...") or (Err "error message")
(io/read-file "config.txt")
```

### The `?` operator

`?` unwraps a `Result` or `Option` — if it's `Ok`/`Some`, returns the inner
value. If it's `Err`/`None`, returns early from the enclosing function:

```clojure
(defn read-config [path]
  (let [content (? (io/read-file path))]
    (str "Config: " content)))
```

### `try` / `catch`

`try` evaluates an expression and catches `Err`:

```clojure
(try
  (? (io/read-file "missing.txt"))
  (catch e
    (str "Failed: " e)))
```

---

## 14. Working with `map` on Options

`map`, `filter`, and `reduce` all work on `Option` values:

```clojure
nexl> (map (fn [x] (* x 2)) (Some 21))
(Some 42)

nexl> (map (fn [x] (* x 2)) None)
None

nexl> (filter (fn [x] (> x 10)) (Some 5))
None

nexl> (filter (fn [x] (> x 10)) (Some 42))
(Some 42)
```

---

## 15. Math

The `math/` module provides standard mathematical functions:

```clojure
nexl> (math/sqrt 2.0)
1.4142135623730951

nexl> (math/pow 2.0 10.0)
1024

nexl> (math/abs -42)
42

nexl> (math/pi)
3.141592653589793

nexl> (math/clamp 15 0 10)
10

nexl> (math/max 3 7)
7
```

---

## 16. JSON

Generate JSON from Nexl values:

```clojure
nexl> (json/stringify {:name "Bob" :scores [95 87 92]})
"{\"name\":\"Bob\",\"scores\":[95,87,92]}"

nexl> (json/stringify [1 true "hello"])
"[1,true,\"hello\"]"

nexl> (json/parse "{}")
(Ok {})
```

`json/stringify` converts any Nexl value to a JSON string. `json/parse` returns
a `Result` — `(Ok value)` on success or `(Err message)` on invalid JSON.

---

## 17. Putting It All Together

Here's a complete program that reads a file, processes its lines, and prints
a summary. Save this as `wordcount.nx`:

```clojure
;; wordcount.nx — count words in a file

(defn count-words [text]
  (let [lines (str/split text "\n")
        non-empty (filter (fn [line] (not (str/blank? line))) lines)
        word-counts (map (fn [line]
                           (count (str/split (str/trim line) " ")))
                         non-empty)]
    (reduce + 0 word-counts)))

(defn summarize [filename]
  (let [content (? (io/read-file filename))
        words (count-words content)
        lines (count (str/split content "\n"))]
    (io/println (str filename ": " lines " lines, " words " words"))))

(summarize "sample.txt")
```

**Before running**, create `sample.txt` in the same directory as the script:

```
Hello world
This is a test file
for the Nexl wordcount example
It has multiple lines
and several words
```

Then run **from that directory**:

```
$ nexl run wordcount.nx
sample.txt: 6 lines, 19 words
```

> **Gotcha:** `summarize` uses `?` to unwrap the `Result` from `io/read-file`. If
> the file doesn't exist, `?` causes an early return from `summarize` and the program
> exits silently with no output. Make sure `sample.txt` exists in the directory where
> you run the command. To handle errors explicitly, use `try`/`catch` (see §13).

---

## 18. What's Next

This tutorial covers the Stage 0 tree-walk evaluator. The full Nexl language
(as described in `nexl-spec.md`) includes much more:

- **Pattern matching** with `match` — destructure ADTs and collections
- **Custom algebraic data types** with `deftype`
- **Algebraic effects** — composable, typed side effects with `handle`/`resume`
- **Protocols** — type classes / interfaces for ad-hoc polymorphism
- **Macros** — extend the language syntax at compile time
- **Modules** — organize code with namespaces and controlled exports
- **Concurrency** — structured concurrency with the `Concurrent` effect
- **WASM compilation** — compile to WebAssembly with `nexl build`

These features are implemented in the type checker and compiler pipeline but
not yet available in the tree-walk evaluator. They'll be fully usable once the
Stage 1 self-hosted compiler is complete.

---

## Quick Reference

| Category | Examples |
|----------|---------|
| Arithmetic | `(+ 1 2)` `(- 5 3)` `(* 2 3)` `(/ 10 3)` `(mod 7 2)` |
| Comparison | `(= a b)` `(< a b)` `(> a b)` `(<= a b)` `(>= a b)` |
| Logic | `(and a b)` `(or a b)` `(not a)` |
| Strings | `(str a b)` `(str/split s sep)` `(str/join sep parts)` `(str/upper s)` |
| Vectors | `[1 2 3]` `(append v x)` `(count v)` `(get v i)` `(first v)` `(rest v)` |
| Maps | `{:k v}` `(get m k)` `(put m k v)` `(keys m)` `(vals m)` |
| Sets | `#{1 2 3}` `(contains? s x)` `(add s x)` `(union a b)` |
| Higher-order | `(map f coll)` `(filter pred coll)` `(reduce f init coll)` |
| Control flow | `(if c t e)` `(do ...)` `(loop [...] ...)` `(recur ...)` |
| Functions | `(defn name [args] body)` `(fn [args] body)` |
| Bindings | `(def name val)` `(let [name val ...] body)` |
| Error handling | `(Some x)` `None` `(Ok x)` `(Err x)` `(? expr)` `(try ... (catch e ...))` |
| I/O | `(io/println x)` `(io/read-file path)` `(io/write-file path content)` |
| Math | `(math/sqrt x)` `(math/pow b e)` `(math/abs x)` `(math/pi)` |
| JSON | `(json/parse s)` `(json/stringify v)` |
