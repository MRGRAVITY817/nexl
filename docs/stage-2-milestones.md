# Stage 2 — Real-World Readiness (M23–M27)

**Premise:** Stage 0 built the compiler. Stage 1 proved the evaluator works for
real programs. Stage 2 bridges from "complete language" to "language people build
things with." The focus shifts from compiler internals to ecosystem, interop, and
proving the value proposition.

**Research basis:** Gleam, Roc, Koka, Unison, Zig, and Rust's adoption histories
all confirm the same pattern — language features do not drive adoption; ecosystem
does. A language without JSON, HTTP, and database access is a language without
production users.

```
Stage 2: Real-World Readiness   M23–M28
  M23  WASI Integration & Interop
  M24  Hello Production Stack
  M25  Developer Experience & Toolchain Polish
  M26  nexl.test: Effect-Powered Testing Library
  M27  nexl.test in Nexl (Macro Self-Hosting)
  M28  Flagship Project & 1.0 Preparation
```

---

## M23 — WASI Integration & Interop

**Goal:** Nexl programs can import existing libraries via WASM Component Model
and run on standard WASM runtimes with full WASI support.

M16 built the compiler plumbing (`import-component`, `export-component`, WIT
generation). M23 makes it work end-to-end with real WASI interfaces and
third-party components.

**Why this is first:** Every successful new language stands on an existing
ecosystem. Gleam leverages Hex/npm. Zig leverages C libraries. Nexl's path is
the WASM Component Model — it lets Nexl components interop with Rust, Go, Python,
and anything else that speaks WIT.

**Deliverables:**

1. **WASI 0.2 full compliance**
   - `wasi:cli` — args, env, stdin/stdout/stderr, exit
   - `wasi:filesystem` — read, write, stat, readdir, open/close
   - `wasi:http` — outgoing requests and incoming handler
   - `wasi:sockets` — TCP client and server
   - `wasi:clocks` — monotonic and wall clocks
   - `wasi:random` — cryptographic and insecure random
   - All via Component Model interfaces (not Preview 1 adapters)

2. **`wit-import` — generate Nexl bindings from WIT files**
   - `(wit-import "wasi:http/outgoing-handler@0.2.0")` → typed Nexl functions
   - Resource types map to opaque Nexl types with `:drop` hooks
   - WIT lists/records/variants map to Nexl Vec/records/ADTs
   - Generated bindings feel like native Nexl code (no manual marshaling)

3. **`wit-export` — expose Nexl modules as WIT interfaces**
   - `(export-component :wit "my-service.wit")` on a module
   - Nexl types → WIT types (records, variants, enums, resources)
   - Effect declarations → WIT imported interfaces (natural mapping)
   - Canonical ABI serialization at component boundaries (automatic)

4. **Component composition — practical test**
   - Import a real Rust component (e.g., a regex engine or crypto library)
   - Export a Nexl component consumable from another language
   - Compose two Nexl components via `wasm-tools compose`
   - Document the full workflow

5. **WASI 0.3 async readiness** (if WASI 0.3 is finalized)
   - Map Nexl's `Concurrent` effect to WASI async I/O
   - Non-blocking HTTP, filesystem, and socket operations
   - If WASI 0.3 is not yet final, design the mapping and gate behind a flag

6. **Effect ↔ WASI capability mapping**
   - Nexl's `:performs [Net]` ↔ WASI `wasi:http` import
   - Nexl's `:performs [FileSystem]` ↔ WASI `wasi:filesystem` import
   - A component that doesn't declare `:performs [Net]` cannot import
     `wasi:http` — the effect system enforces sandboxing at the WASM level
   - This is Nexl's unique advantage: effect tracking as a security boundary

**After M23 you can:** Import a Rust HTTP client via WIT and call it from Nexl:
```clojure
(module my-app.main
  :imports [[wasi.http :as http]]
  :performs [Net])

(defn fetch-data [url]
  (let [resp (http/outgoing-request :get url {})]
    (http/response-body resp)))
```

---

## M24 — Hello Production Stack

**Goal:** The standard library has everything needed to build a real web service:
JSON, HTTP, database access, and structured testing.

These libraries are built on top of M23's interop — backed by WASI interfaces or
imported WASM components where appropriate, with pure-Nexl logic on top.

**Why this is second:** Research across Gleam, Unison, and Koka shows that
missing JSON/HTTP/database support is the #1 blocker for production adoption.
Gleam's survey flagged JSON decoding as a top pain point. Unison had to build
HTTP, TLS, and JSON as runtime builtins before cloud deployment was possible.

**Deliverables:**

1. **`json` module — production-grade**
   - `(json/encode val)` → Str and `(json/decode Type str)` → `(Result T JsonError)`
   - `:derive [JsonCodec]` on `deftype` for automatic encode/decode
   - Streaming parser for large payloads
   - Pretty-printing with configurable indent
   - Handles all JSON edge cases (Unicode escapes, large numbers, nested nulls)

2. **`http` module — client and server**
   - **Client:** `(http/get url)`, `(http/post url body headers)`, etc.
     Returns `(Result Response HttpError)`. Backed by WASI HTTP.
   - **Server:** `(http/serve handler port)` where handler is
     `(Fn [Request] -> Response ! [Net])`. Request routing via pattern matching.
   - Middleware as function composition:
     `(http/serve (-> handler (with-logging) (with-cors)) 8080)`
   - Request/response bodies as streams for large payloads

3. **`db` module — SQLite**
   - `(db/open path)` → `(Result Db DbError) ! [FileSystem]`
   - `(db/query db "SELECT ..." params)` → `(Result (Vec Row) DbError)`
   - `(db/execute db "INSERT ..." params)` → `(Result Int DbError)`
   - Parameterized queries only (no string interpolation — prevent SQL injection)
   - Row → record mapping via `:derive [FromRow]`
   - Transaction support: `(db/transaction db (fn [tx] ...))`
   - Backed by SQLite compiled to WASM (via imported component) or native SQLite

4. **`db/pg` module — PostgreSQL client** (stretch goal)
   - Wire protocol implementation or imported component
   - Same API shape as `db` module
   - Connection pooling

5. **`test` module — effect-powered testing**
   - `(deftest name body)` with `(is (= expected actual))` assertions
   - **Mock via effects:** Replace real `Net`/`FileSystem` handlers with test
     doubles — no mocking framework needed, effects make this natural:
     ```clojure
     (deftest "fetch handles errors"
       (handle [Net (request [resume req]
                      (resume (Err (HttpError "timeout"))))]
         (is (= (Err ...) (fetch-data "http://example.com")))))
     ```
   - Property-based testing: `(check "name" gen (fn [x] (is ...)))`
   - Test discovery: `nexl test` finds and runs all `deftest` forms
   - Parallel test execution with isolated effect handlers

6. **`env` module — configuration**
   - `(env/get "VAR")` → `(Option Str)`
   - `(env/require "VAR")` → `Str` (panics if missing)
   - `.env` file loading for development
   - Typed config: `(env/load Config)` where Config has `:derive [FromEnv]`

7. **`log` module — structured logging (improved)**
   - JSON-formatted structured logs by default
   - Log levels: `debug`, `info`, `warn`, `error`
   - Context fields: `(log/with {:request-id id} body)` adds fields to all
     logs within body
   - Backed by `Log` effect — testable, replaceable

**After M24 you can:** Build a real web service:
```clojure
(module my-app.api
  :imports [[my-app.db :as db]
            [json]
            [http]]
  :exports [start!]
  :performs [Net FileSystem Log])

(defn handle-request [req]
  (match (:path req)
    "/users" (let [users (db/query db "SELECT * FROM users" [])?]
               (http/response 200 (json/encode users)))
    _        (http/response 404 "not found")))

(defn start! []
  (http/serve handle-request 8080))
```

---

## M25 — Developer Experience & Toolchain Polish

**Goal:** The experience of writing Nexl feels polished and professional. A new
developer can go from zero to running code in under 5 minutes. Error messages
are best-in-class. Packages are discoverable.

**Why this is third:** Gleam's survey shows that even with good language design,
developers hit walls on: IDE support gaps (rename missing), installation
complexity (external dependencies), and documentation. These are solvable
problems that compound into adoption friction.

**Deliverables:**

1. **Single-binary distribution**
   - `nexl` binary contains: compiler, build tool, package manager, formatter,
     language server, REPL, test runner, doc generator
   - Install via: `curl -fsSL https://nexl-lang.org/install.sh | sh`
   - No external dependencies (no separate LLVM, no JDK, no rebar3)
   - Cross-platform: macOS (arm64, x86_64), Linux (x86_64, arm64)
   - `nexl upgrade` self-updates

2. **`nexl new` — zero-to-running in 60 seconds**
   - `nexl new my-app` → project scaffold with `project.nx`, source dir,
     example module, test module, `.gitignore`
   - `nexl new my-app --template web` → web service scaffold with HTTP handler,
     JSON, logging
   - `cd my-app && nexl run src/main.nx` works immediately

3. **Error message audit — especially effects**
   - Audit every error path in type inference and effect checking
   - Effect row mismatches: explain which effect is missing and where it was
     expected (not raw row variables like Koka's `$h` vs `$h1`)
   - Suggest fixes: "Function `fetch` performs `Net`, but module `my-app.pure`
     does not declare `:performs [Net]`. Add it to the module declaration?"
   - Mismatch cascade prevention: if one error causes downstream noise,
     suppress the noise (already partially done — extend to effects)
   - Test: collect real error scenarios from M23/M24 development, ensure every
     one produces a helpful message

4. **LSP completions — imports and effects**
   - Complete module names in `:imports` (scan project source tree)
   - Complete effect operations: typing `Net/` suggests `request`, `listen`, etc.
   - Complete record fields from inferred types
   - Workspace-wide rename (symbols, modules)
   - "Go to handler" — from an effect operation, jump to its nearest handler
   - Inlay hints: show inferred effect rows on functions

5. **Package registry — content-addressed**
   - `nexl pkg publish` → uploads to registry (content-addressed by definition hash)
   - `nexl pkg add foo` → resolves, downloads, adds to `project.nx`
   - `nexl pkg search "http"` → search by name, description, keywords
   - Web UI: browsable package index with auto-generated docs
   - Semver enforcement: breaking changes detected via type signature diff
   - Private registries for organizations

6. **Documentation site generator**
   - `nexl doc --html` produces navigable HTML documentation
   - Auto-includes: type signatures, effect rows, docstrings, `:examples`
   - Cross-module hyperlinks (powered by content addressing)
   - Module dependency visualization
   - Publish to registry alongside package: `nexl pkg publish` includes docs

7. **Cookbook — 30 recipes**
   - Practical examples indexed by task: "Parse JSON", "Make HTTP request",
     "Query database", "Read file", "Handle errors", "Test with mock effects",
     "Create WASM component", "Import Rust library", etc.
   - Each recipe: problem, solution, explanation, runnable code
   - Serves double duty: documentation for humans + training data for LLMs

**After M25 you can:** Install Nexl and have a web service running in 5 minutes:
```
$ curl -fsSL https://nexl-lang.org/install.sh | sh
$ nexl new my-api --template web
$ cd my-api
$ nexl run src/main.nx
Listening on http://localhost:8080
```

---

## M26 — nexl.test: Effect-Powered Testing Library

**Goal:** Implement Nexl's built-in testing library as specified in
`docs/nexl-test-spec.md`. The library leverages the effect system for mocking
and sandboxing, and provides power-assert `is` macros, property testing,
snapshots, and doctests.

**Why this comes before 1.0:** A language cannot ship 1.0 without a mature
testing story. Nexl's effect system enables a uniquely powerful approach to
mocking — test doubles are just effect handlers. This must be proven and
polished before the stability guarantee.

**Deliverables:**

1. **`defhandler` — named effect handlers** (language primitive, spec §6.10)
   - `(defhandler Name Effect (op [resume args] body))` syntax
   - Simple, continuation, parameterized, and multi-effect handlers
   - `(handle [HandlerName])` and `(handle [(HandlerName args)])` installation
   - Full type inference with completeness checking

2. **Core testing API** (Phase 1)
   - `(deftest "name" body)` with `(is expr)` power-assert macro
   - `(describe "group" body)` for nesting and scoped naming
   - `(throws? ExnType expr)` assertion
   - Updated `nexl test` CLI with `--filter` and output formatting
   - `:skip` and `:focus` annotations

3. **Data, patterns & lifecycle** (Phase 2)
   - `(is-match expr pattern)` pattern matching assertions
   - `(each [row data] body)` table-driven tests
   - String/collection diff output in error messages
   - `setup`/`teardown`/`setup-all`/`teardown-all` lifecycle hooks
   - `:tags` support with CLI `--tags` filtering

4. **Effect-based mocking** (Phase 3)
   - `call-log` recording wrapper for effect operations
   - Capability-aware test sandboxing
   - `SequentialExecutor` for deterministic concurrent testing
   - `submodule test` for compile-time exclusion from release builds

5. **Property testing** (Phase 4)
   - Generator primitives and combinators
   - `(check "name" gen (fn [x] (is ...)))` inside `deftest`
   - Integrated shrinking with shrink trees
   - `Arbitrary` protocol with auto-derive for ADTs/records

6. **Snapshots, doctests & contracts** (Phase 5)
   - `snap!` inline snapshots, `snap-file!` file-based snapshots
   - `--accept` and `--review` CLI commands
   - Doctest `>>>` parsing from docstrings
   - Contract-driven testing (`:examples` auto-execution)

7. **Polish & performance** (Phase 6)
   - `--watch`, `--parallel`, `--format json`
   - `bench` form and `nexl bench` command
   - Matcher protocol and built-in matchers
   - `--coverage`, `:flaky`, `:timeout`

**After M26 you can:** Write expressive tests with power-assert diagnostics,
mock any effect with a handler, run property tests with shrinking, and use
snapshot testing — all built into the language:
```clojure
(deftest "fetch handles timeout"
  (handle [MockNet]
    (is (= (Err "timeout") (fetch-data "http://example.com")))))
```

---

## M27 — nexl.test in Nexl (Macro Self-Hosting)

**Goal:** Move nexl.test from Rust special forms to Nexl macros. Proves the
macro system is production-grade by dogfooding it. Zero nexl.test special forms
in `eval.rs` when done.

**Deliverables:**
1. Expose ~15 new Nexl-callable Rust primitives in test.rs, gen_mod.rs, io.rs
2. Integrate macro expansion into the eval pipeline
3. Fix nested list patterns and `syntax-str` in defmacro-syntax
4. Write all nexl.test forms as macros in test.nx
5. Delete all eval_deftest/eval_describe/eval_is/etc. special forms

---

## M28 — Flagship Project & 1.0

**Goal:** Prove Nexl's value proposition with a real project. Ship 1.0 with a
stability guarantee. Make Nexl ready for early adopters.

**Why this is last:** A 1.0 release without something real built in the language
is just a version number. The flagship project stress-tests every milestone,
surfaces the remaining rough edges, and gives potential users something concrete
to evaluate.

**Deliverables:**

1. **Flagship project: `nexl-functions` — effect-sandboxed WASM plugin host**

   Build a serverless function runtime where Nexl's effect system provides
   capability-based security:
   - Users write functions in Nexl (or any WASM language)
   - Each function declares what it can do: `:performs [Net]`, `:performs [FileSystem]`
   - The runtime enforces capabilities at the WASM boundary — a function that
     doesn't declare `Net` literally cannot make HTTP requests
   - Effect handlers provide: logging, metrics, request tracing, timeout
   - Content-addressed deployment: function identity = hash of code + deps
   - Demonstrates: effects as security, WASM sandboxing, content addressing,
     component composition

   This is the intersection of Nexl's unique features that no other language
   can replicate. It's also a practical tool (think Cloudflare Workers / Shopify
   Functions but with provable capability restrictions).

   Minimum viable scope:
   - HTTP trigger: incoming request → run function → return response
   - 3 capability levels: pure (no effects), read-only (FileSystem read), full (Net + FileSystem)
   - Deploy via `nexl functions deploy`
   - Dashboard showing deployed functions, their capabilities, invocation logs
   - Runs locally via Wasmtime and can target any WASI-compatible runtime

2. **1.0 stability contract**
   - Define what's stable (syntax, core forms, type system, effect system,
     standard library, CLI interface, WIT interop)
   - Define what's experimental (specific optimizations, WASI 0.3 async,
     native backend details)
   - Backward compatibility promise: code that compiles on 1.0 compiles on 1.x
   - Edition mechanism for future breaking changes (like Rust editions)
   - Semantic versioning from 1.0 onward

3. **Migration guide & changelog**
   - Document every breaking change from pre-1.0
   - Provide `nexl migrate` tool for automated fixups where possible
   - Changelog covering Stage 0 → Stage 1 → Stage 2 evolution

4. **Public documentation site**
   - Language guide (tutorial progression, not reference dump)
   - Standard library API reference (auto-generated from source)
   - Effect system guide (practical, not theoretical — "how to test with effects",
     "how to add a capability to your module", "how to write a handler")
   - WASM interop guide (import Rust components, export Nexl components)
   - Cookbook (from M25)
   - Blog: design rationale posts explaining key decisions

5. **Community infrastructure**
   - GitHub Discussions or Discord for questions
   - Issue tracker with good-first-issue labels
   - Contributing guide
   - Code of conduct

6. **AI/LLM readiness**
   - All documentation, spec, stdlib source, cookbook, and examples are
     publicly accessible and well-indexed
   - `CLAUDE.md` / context files for AI coding assistants
   - Canonical example corpus: 50+ programs covering all language features
   - Verify: Claude/GPT can write basic Nexl programs from documentation
     context (test with fresh conversations)

**After M27 you can:** Deploy a production web service written in Nexl, import
Rust libraries via WASM components, test with effect-based mocking, publish
packages to the registry, and tell your team "this is 1.0 — it won't break."

---

## Dependency Graph

```
M22 (Stage 1 complete)
  │
  ▼
M23 (WASI + Interop)
  │
  ▼
M24 (Production Stack) ←── depends on M23 for WASI-backed HTTP/DB
  │
  ▼
M25 (DevEx + Toolchain) ←── depends on M24 for cookbook content / error audit
  │
  ▼
M26 (nexl.test) ←── depends on M25 for effect system maturity + defhandler
  │
  ▼
M27 (nexl.test in Nexl) ←── proves macro system is production-grade
  │
  ▼
M28 (Flagship + 1.0) ←── depends on everything above
```

Milestones are sequential. Each builds on the previous. No parallelism within
Stage 2 — the interop foundation (M23) must exist before libraries (M24), which
must exist before the developer experience can be polished (M25), which must be
solid before the testing library (M26), which must all work before shipping 1.0
(M27).

---

## Risk Register

| Risk | Mitigation |
|------|-----------|
| WASI 0.3 delayed | Design async mapping but ship M23 with WASI 0.2 (sync only). Async becomes M23.1 patch. |
| WIT tooling immature | Start with hand-written bindings for core WASI interfaces. Automate with `wit-import` incrementally. |
| SQLite-in-WASM performance | Benchmark early. Fall back to native SQLite for native target, WASI filesystem for WASM. |
| Effect error messages too complex | Dedicate explicit effort in M25. Collect real errors from M23/M24 dev as test corpus. |
| No early adopters | The flagship project (M27) is both proof-of-concept and marketing. Blog about it during development. |
| LLM training data gap | Publish cookbook and examples early (M25), don't wait for 1.0. |

---

## Success Criteria

Stage 2 is complete when:

1. A Nexl program can import a Rust-compiled WASM component and use it
2. A web service with JSON API + SQLite persistence runs on Wasmtime
3. `nexl new && nexl run` works on a fresh machine in under 5 minutes
4. `nexl test` runs power-assert tests with effect-based mocking out of the box
5. The flagship project (`nexl-functions`) is deployed and handling requests
6. 1.0 is tagged with a stability guarantee
7. A developer unfamiliar with Nexl can follow the tutorial and build something
   useful in an afternoon
