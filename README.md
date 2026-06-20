# Nexl

**A statically-typed, effect-tracked Lisp that compiles to WebAssembly and native code.**

Nexl preserves Lisp's core strengths — homoiconicity, macros, and REPL-driven development — while adding a modern type system, algebraic effects, capability-based security, and content-addressed code.

---

> ## ⚠️ Very early & experimental
>
> **Nexl is in a very early, experimental phase.** This repository is the **Stage 0 bootstrap compiler** — a work in progress, not a usable language release.
>
> - The language specification (`nexl-spec.md`) is a **v0.1 draft** and will change.
> - Many features described in the spec are **planned or partially implemented**. Some forms are explicitly marked *"Planned — not yet implemented."*
> - Syntax, semantics, the standard library, and the CLI are all **unstable and subject to breaking changes without notice**.
> - There are **no stability or compatibility guarantees**. Do not use Nexl for anything real yet.
>
> This is shared in the open for the curious and for those who want to follow along. Expect rough edges, gaps, and churn.

---

## Why Nexl?

Nexl is designed around a single guiding idea: **composability is the master virtue**. Every feature must compose with every other — effects compose with types, types with macros, macros with modules. A second, increasingly explicit goal is to be a language that **AI agents can reason about reliably**: fully deterministic semantics, behavioural contracts embedded in code, and even natural-language control flow.

### Design Principles

1. **Composability is the master virtue** — every feature composes with every other.
2. **Explicitness over magic** — side effects are declared, capabilities granted, types tracked.
3. **Practicality over purity** — mutable locals, imperative loops, and a dynamic escape hatch exist.
4. **One way to do it** — opinionated defaults, a mandatory formatter, one concurrency model.
5. **Fast feedback over fast execution** — development speed first (without sacrificing performance).
6. **The compiler is a conversational partner** — error messages are explanations, not cryptic diagnostics.

## A Taste

```clojure
;; Hello world
(io/println "Hello, World!")

;; A typed function with a docstring, contract, and inferred effects
(defn fibonacci [n : Int] -> Int
  :requires [(>= n 0)]
  :examples [{:in [0] :out 0}
             {:in [10] :out 55}]
  (match n
    0 0
    1 1
    _ (+ (fibonacci (- n 1)) (fibonacci (- n 2)))))

;; Effects are declared in the signature with `! [...]`
(defn greet! [] -> Str ! [Console]
  (Console/print "What is your name? ")
  (let [name (Console/read-line)]
    (Console/print "Hello, {name}!")
    name))
```

### Success / failure handling in `let`, the pipe way

Refutable bindings in `let` use the `|` operator to split every line into two
reading lanes — the **happy path** on the left, the **failure case** on the right.
If a pattern on the left fails to match, its fallback on the right becomes the value
of the whole `let`, and the rest is skipped. You can scan a binding vector twice:
once down the left to follow the nominal logic, once down the right to audit every
failure mode — without the two ever interleaving.

```clojure
(defn handle-request! [req] -> (Result Response Error) ! [Db Net Log]
  (let [req-id        (gen-id)                              ; plain binding, always succeeds
        (Ok user)     (auth/verify (:token req)) | (Err :unauthorized)
        (Ok body)     (json/parse (:body req))   | (Err :bad-json)
        (Some record) (Db/query user (:id body)) | (Err :not-found)]
    ;; Reached only when every pattern on the left matched:
    (Ok {:status 200 :body (json/encode record)})))
```

No pyramid of nested `if`/`match`, no early-return boilerplate, no error-handling
noise drowning out the success path — just two clean columns. This is Nexl's
answer to railway-oriented error handling.

## Key Features

- **Modern type system on a Lisp** — bidirectional type inference (most code needs no annotations), row polymorphism, algebraic data types with exhaustive `match`, and protocols.
- **Algebraic effects** — side effects, I/O, dependency injection, concurrency, and recoverable errors are all expressed with one mechanism: `defeffect` declares operations, `defhandler`/`handle` provide implementations. Effect rows are tracked in type signatures (`! [Net Db]`) and inferred automatically.
- **Capability-based security** — the effect system *is* the capability system. Untrusted code is sandboxed by wrapping it in a `handle` that audits the effects it tries to perform.
- **Powerful pattern matching** — or-patterns, view patterns, type-asserting patterns, named patterns (`defpattern`), string-interpolation patterns, and `:nl` natural-language patterns evaluated by an `LLM` effect.
- **Hygienic macros** — scope-set hygiene (Flatt 2016), procedural (`defmacro`) and pattern-based (`defmacro-syntax`) macros, plus controlled reader extensions for embedded DSLs.
- **Content-addressed code** — every definition is identified by a hash of its type-annotated AST. Renames are free, incremental compilation is exact, diamond dependencies are structurally impossible, and semantic-versioning changes can be classified automatically.
- **Structured concurrency** — `fork`/`join`/`race` as an effect, with `par-let` for ergonomic parallel bindings and guaranteed task lifetimes.
- **No `nil`** — `Unit`, `(Option a)` with `Some`/`None`, and `(Result a e)` with the `?` propagation operator instead.
- **Determinism by design** — fully specified collection ordering and evaluation semantics, so behaviour is reproducible.
- **Compiles to WASM and native** — WebAssembly (core modules and components), native code via Cranelift, and a bytecode VM for the REPL. Memory is managed with Perceus reference counting by default.

## Repository Layout

This is the Stage 0 bootstrap compiler, implemented in **Rust** as a Cargo workspace.

| Path | Contents |
|------|----------|
| `nexl-spec.md` | The full language specification (v0.1 draft) |
| `crates/` | Compiler crates — one per phase (reader, types, infer, effects, macros, IR, WASM/native codegen, CLI, LSP, package manager) |
| `examples/` | Annotated `.nx` example programs |
| `cookbook/` | Task-oriented recipes |
| `docs/` | Design notes, crate map, milestones, glossary |
| `decisions/` | Architecture Decision Records (ADRs) |
| `editors/` | Editor integrations |

See `docs/crate-map.md` for the full dependency graph.

## Building

Requires a recent Rust toolchain.

```bash
cargo build          # build the workspace
cargo test           # run all tests
cargo clippy --all-targets
cargo fmt
```

The `nexl` binary (`crates/nexl-cli`) is the entry point — `nexl run`, `nexl test`, `nexl repl`, `nexl fmt`, `nexl lsp`, and more. Note that command coverage tracks the in-progress implementation, not the full spec.

## Status & Roadmap

Development proceeds in stages (see `docs/current-milestone.md` and `milestones.md`):

- **Stage 0** — bootstrap compiler: core language features.
- **Stage 1** — self-hosting preparation.
- **Stage 2** — real-world readiness toward a 1.0.

Self-hosting (a Nexl compiler written in Nexl) is an aspirational milestone, not a current reality.

## Learn More

- **[`nexl-spec.md`](./nexl-spec.md)** — the complete language specification.
- **[`examples/`](./examples/)** — runnable example programs.
- **[`CONTRIBUTING.md`](./CONTRIBUTING.md)** — how to contribute.
- **[`CODE_OF_CONDUCT.md`](./CODE_OF_CONDUCT.md)** — community guidelines.

## License

MIT (as declared in `Cargo.toml`). A `LICENSE` file will be added.
