# nexl.test — Testing Library Specification

> *"Tests are the compiler's conversation with the programmer about intent."*

---

## 1. Design Philosophy

### 1.1 Principles

1. **Effects are mocks.** Every side effect in Nexl flows through the algebraic
   effect system. To test code that performs `Net`, `Db`, or `Console` effects,
   supply a test handler via `handle`. No mock objects, no patching, no separate
   mocking library. This is the defining insight of nexl.test.

2. **No API is the best API.** The most common assertion requires zero learning:
   write a boolean expression inside `(is ...)`. The macro introspects the AST
   to produce rich diagnostics automatically. No `assertEqual`, `assertGreater`,
   `assertContains` zoo — just `is`.

3. **Progressive complexity.** Start with a bare `(is ...)`. Scale to named
   tests, grouped suites, fixtures, property tests — each step a local, additive
   change. Beginners aren't overwhelmed; experts aren't constrained.

4. **Contracts are tests.** `:examples` clauses on functions are automatically
   collected and executed by `nexl test`. Writing a contract is writing a test.

5. **Tests are data.** Test cases are vectors. Matchers are functions. Generators
   are composable values. No class hierarchies, no inheritance.

6. **Deterministic always.** Concurrent code under test runs on a sequential
   executor by default. Random seeds are explicit. Collections have deterministic
   ordering.

7. **Capability-aware.** Tests that perform unhandled effects are compile errors.
   The effect system IS the permission system — no separate sandboxing needed.

### 1.2 What Makes This "Nexl-Like"

| Nexl Feature          | Testing Leverage |
|-----------------------|------------------|
| Algebraic effects     | Mocking = providing a test handler. Fixtures = effect handler scope. Sandboxing = effect row checking. |
| `defhandler`          | Named, reusable effect handlers work identically in tests and production. No separate mock library. |
| Pattern matching      | `is-match` asserts on structure with destructuring, guards, or-patterns, pin patterns. |
| Hygienic macros       | `is` decomposes expressions at expansion time for power-assert diagnostics. |
| ADTs / deftype        | `(derive Arbitrary)` auto-generates property-test generators. Exhaustive matching in assertions. |
| Contracts (:examples) | Embedded examples become regression tests. `:requires`/`:ensures` are checked on generated inputs. |
| Module system         | `(submodule test ...)` enables in-file testing with private access, excluded from release builds. |
| S-expressions         | Natural format for inline snapshots — no serialization layer needed. |
| Effect rows           | Type system proves test isolation: unhandled effects = compile error. Smart re-runs via content hashing. |

---

## 2. Quick Start

```clojure
(module my-app.math-test
  :imports [[nexl.test :refer [deftest is describe]]])

;; Simplest possible test
(deftest "addition"
  (is (= (+ 1 2) 3)))

;; Grouped tests
(describe "math operations"
  (deftest "addition"
    (is (= (+ 1 2) 3))
    (is (= (+ 0 0) 0)))

  (deftest "division by zero throws"
    (throws? ArithmeticError (/ 1 0))))
```

Run:
```
nexl test test/                  # run all tests
nexl test test/math_test.nx      # run one file
nexl test --filter "addition"    # filter by name
```

---

## 3. Core Assertions: `is`

### 3.1 The `is` Macro

`is` is the single assertion form. It takes a boolean expression and, on
failure, produces a rich diagnostic by analyzing the expression's AST at macro
expansion time.

```clojure
(is expr)
(is expr "optional failure message")
```

### 3.2 Power-Assert Diagnostics

When `is` receives a compound expression, the macro decomposes it and captures
the values of all subexpressions. On failure, it renders a visual diagnostic.

```clojure
(is (= (+ a b) expected))
```

On failure (when `a` = 2, `b` = 3, `expected` = 6):

```
FAIL at test/math_test.nx:7:3

  (is (= (+ a b) expected))

  left:  5
  right: 6
```

The macro recognizes common forms and provides specialized formatting:

| Expression Form       | Failure Shows |
|-----------------------|---------------|
| `(= a b)`            | left/right values; diff for strings and collections |
| `(not= a b)`         | both values (which are unexpectedly equal) |
| `(< a b)`, `(> a b)` | both values with the comparison |
| `(pred? x)`           | the value of `x` and which predicate failed |
| `(and ...)`, `(or ...)` | which clause failed and intermediate values |
| `(contains? coll x)` | the collection contents and the missing element |
| Any other form        | the expression and its boolean result |

### 3.3 Diff Output for Structured Values

Equality failures on strings show unified diffs:

```
FAIL at test/template_test.nx:8:3

  (is (= actual expected))

  --- expected
  +++ actual
  @@ -1,3 +1,3 @@
   Hello, Alice!
  -Welcome to the app.
  +Welcome to the application.
   Please log in.
```

Equality failures on collections show element diffs:

```
FAIL at test/list_test.nx:5:3

  (is (= actual expected))

  expected: [1 2 3 4 5]
  actual:   [1 2 4 5]
  missing:  [3]
```

### 3.4 Type Mismatch Guidance

When comparing values of different types, the diagnostic includes a hint:

```
FAIL at test/math_test.nx:7:3

  (is (= result 5))

  left:  "5" : Str
  right: 5   : Int

  hint: comparing Str with Int — did you mean (= (parse-int result) 5)?
```

### 3.5 `is` with Matchers

When the second argument is a **matcher** (a value implementing the `Matcher`
protocol), `is` applies the matcher instead of evaluating a boolean:

```clojure
(is result (ok? 42))              ;; matches (Ok 42)
(is name (starts-with "Dr."))     ;; string prefix
(is items (has-length 3))         ;; collection length
(is score (approx 3.14 :within 0.01))  ;; floating point
```

See §15 for the Matcher protocol and built-in matchers.

---

## 4. Pattern Assertions: `is-match`

Leverages Nexl's full pattern matching (§4.9) for structural assertions.
Supports destructuring, guards, or-patterns, pin patterns, and view patterns.

```clojure
(is-match value pattern)
(is-match value pattern guard)
```

```clojure
;; ADT matching
(is-match (fetch-user id)
  (Ok user) (> (:age user) 0))

;; Nested destructuring
(is-match response
  (Ok {:status 200 :body body}) (not (string/empty? body)))

;; Or-pattern
(is-match status (:pending | :processing))

;; Pin — assert against a bound value
(let [expected-id 42]
  (is-match user {:id ^expected-id}))

;; View pattern
(is-match (get-items) (view count 3))

;; Any Some value
(is-match (find-item items) (Some _))
```

On failure:

```
FAIL at test/user_test.nx:12:3

  (is-match result (Ok user) (> (:age user) 0))

  expected pattern: (Ok user) where (> (:age user) 0)
  actual value:     (Err "not found")
```

When the pattern matches but the guard fails:

```
FAIL at test/user_test.nx:12:3

  pattern matched:  (Ok user)
  guard failed:     (> (:age user) 0)
  bindings:         user = {:name "Alice" :age -1}
```

### 4.1 Common Mistake Detection

```
FAIL at test/option_test.nx:3:3

  (is-match result None)

  expected pattern: None
  actual value:     None

  hint: bare `None` is a variable pattern (matches anything).
        Use `(None)` for the nullary constructor.
```

---

## 5. Exception Assertions: `throws?`

```clojure
(throws? body...)
(throws? ErrorType body...)
(throws? ErrorType message-pattern body...)
```

```clojure
;; Assert any error
(throws? (/ 1 0))

;; Assert specific error type
(throws? ArithmeticError (/ 1 0))

;; Assert error type AND message
(throws? ArithmeticError "division by zero"
  (/ 1 0))
```

---

## 6. Test Definition: `deftest`

### 6.1 Basic Form

```clojure
(deftest "descriptive test name"
  body...)
```

`deftest` is a macro that wraps `body` in a zero-argument function and registers
it with the test runner. Tests are **not** executed at definition time — they are
collected and run by `nexl test`.

### 6.2 Metadata Annotations

```clojure
;; Tags for filtering
(deftest "slow integration test" :tags [:slow :integration]
  ...)

;; Skip with optional reason
(deftest "broken feature" :skip "waiting on #123"
  ...)

;; Focus (CI rejects any committed :focus tests — exit code 3)
(deftest "debugging this" :focus
  ...)

;; Timeout in milliseconds
(deftest "network call" :timeout 5000
  ...)

;; Retry flaky tests before reporting failure
(deftest "eventually consistent" :flaky 3
  ...)
```

When any test has `:focus`, only focused tests run.

### 6.3 Tests with Effects

When code under test performs effects, supply handlers in the test body.
The compiler verifies that no unhandled effects escape.

```clojure
(deftest "create user persists"
  (let [captured (atom [])]
    (handle [Db
              (exec! [resume query params]
                (swap! captured conj {:query query :params params})
                (resume 1))
              (query [resume q p] (resume []))]
      (let [result (create-user! "alice" "alice@example.com")]
        (is (= (Ok 1) result))
        (is (= 1 (count (deref captured))))))))
```

---

## 7. Test Organization: `describe`

### 7.1 Nesting

```clojure
(describe "Calculator"
  (describe "addition"
    (deftest "positive numbers"
      (is (= (+ 1 2) 3)))
    (deftest "negative numbers"
      (is (= (+ -1 -2) -3))))

  (describe "division"
    (deftest "basic"
      (is (= (/ 10 2) 5)))
    (deftest "by zero"
      (throws? ArithmeticError (/ 1 0)))))
```

The test runner displays the full path: `Calculator > addition > positive numbers`.

### 7.2 Shared Setup via `:let`

```clojure
(describe "user operations" :let [user (make-test-user)]
  (deftest "has a name"
    (is (= (:name user) "Alice")))
  (deftest "has an email"
    (is (string/contains? (:email user) "@"))))
```

`:let` bindings are evaluated **once per test** (fresh each time), preventing
state leakage between tests.

---

## 8. Test Submodules

Test submodules provide in-file testing with access to private definitions.
This is Nexl's equivalent of Rust's `#[cfg(test)] mod tests` and Zig's
in-file `test` blocks.

```clojure
(module my-app.parser
  :exports [parse])

(defn- tokenize [s] ...)  ;; private

(defn parse [s : Str] -> (Option Ast)
  (-> s tokenize analyze))

;; Inline test submodule — compiled only in test mode
(submodule test my-app.parser-tests
  :imports [[nexl.test :refer [deftest is check]]]

  (deftest "tokenize basic"
    (is (= ["hello" "world"] (tokenize "hello world"))))

  (deftest "parse roundtrip"
    (check [s (gen/str)]
      (when-let [(Some ast) (parse s)]
        (is (= s (ast->str ast))))))

  (deftest "parse empty"
    (is (= (None) (parse "")))))
```

Test submodules:
- Access **all** private definitions in the enclosing module.
- Are **excluded** from release builds (do not affect module hash).
- Appear as `my-app.parser-tests` in test output.

---

## 9. Data-Driven Tests: `each`

Table-driven testing with destructuring, inspired by Go and Midje tabular.

```clojure
(each [a b expected]
  [[1 2 3]
   [0 0 0]
   [-1 1 0]
   [100 200 300]]
  (is (= (+ a b) expected)))
```

For each row, `each` destructures into the binding vector and evaluates the
body. On failure, it reports which row failed with the bound values.

### 9.1 Named Rows

For better diagnostics, rows can be maps with a `:name` key:

```clojure
(each {:name name :in [a b] :out expected}
  [{:name "basic"    :in [1 2]  :out 3}
   {:name "zeros"    :in [0 0]  :out 0}
   {:name "negative" :in [-1 1] :out 0}]
  (is (= (+ a b) expected)))
```

Failure reports include the row name:

```
FAIL in each "negative" at test/math_test.nx:8:3

  bindings: a = -1, b = 1, expected = 0
  (is (= (+ a b) expected))

  left:  0
  right: 0

  (all rows passed in this example — but you get the idea)
```

### 9.2 `each` Inside `deftest`

`each` is typically used inside a `deftest`:

```clojure
(deftest "HTTP status codes"
  (each [code category]
    [[200 :ok] [301 :redirect] [404 :client-error] [500 :server-error]]
    (is (= (categorize-status code) category))))
```

---

## 10. Lifecycle Hooks

Lifecycle hooks follow ExUnit's model: scoped to the enclosing `describe` or
module, with context maps that thread setup state to tests.

```clojure
(describe "database integration"
  ;; Runs once before all tests in this describe block
  (setup-all []
    {:db (create-test-db!)})

  ;; Runs after all tests
  (teardown-all [ctx]
    (destroy-test-db! (:db ctx)))

  ;; Runs before each test — receives context from setup-all
  (setup [ctx]
    (clear-tables! (:db ctx))
    ctx)

  ;; Runs after each test
  (teardown [ctx]
    (rollback! (:db ctx)))

  (deftest "insert and query"
    ;; ctx is available via the Test/context effect
    (let [db (:db (Test/context))]
      (Db/exec! db "INSERT INTO users VALUES (1, 'alice')" [])
      (is (= 1 (count (Db/query db "SELECT * FROM users" [])))))))
```

Setup functions return a map that is merged into the test context. Each test
receives a fresh copy. `Test/context` is an effect operation handled
automatically by the test runner.

---

## 11. `defhandler` — Named Effect Handlers

`defhandler` is a **language-level** form, not specific to testing. It defines
a named, reusable implementation of one or more effects. The same form works
identically in production and test code — because mocking IS just providing a
different handler.

This creates a clean symmetry in the language:

```
defeffect   — declares WHAT operations exist
defhandler  — declares HOW to implement them
handle      — installs a handler for a scope
```

### 11.1 Syntax

`defhandler` follows the same structure as `impl`: bare uppercase symbols name
the effect, operations follow underneath. No wrapping brackets needed.

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

### 11.2 All Forms

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

### 11.3 Usage with `handle`

`handle` keeps its existing square-bracket syntax (needed to separate handler
declarations from the body):

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

;; Multiple named handlers
(handle [ConsoleLog]
  (handle [SqliteDb]
    (run-app!)))

;; Inline handler still works for one-off cases
(handle [Log
          (info [msg] (println msg))]
  (do-stuff!))
```

### 11.4 Parsing Rule

The parser distinguishes parameter vectors from effect sections by the same
rule `impl` uses: **an uppercase bare symbol starts a new effect section**;
a lowercase vector after the handler name is the parameter list.

```clojure
(defhandler JsonLog [config]   ;; [config] — lowercase = params
  Log                          ;; Log      — uppercase = effect section starts
  (info [msg] ...))            ;; (info…)  — operation implementation
```

---

## 12. Effect-Based Mocking

With `defhandler` as a language primitive, mocking in tests requires **zero
test-specific abstractions**. You define test handlers the same way you define
production handlers.

### 12.1 Test Handlers

```clojure
;; Test handler — returns canned data
(defhandler TestDb
  Db
  (query [sql params] [{:id 1 :name "alice"}])
  (exec! [sql params] 1))

;; Silent logger — discards all output
(defhandler SilentLog
  Log
  (info [_] unit)
  (warn [_] unit)
  (debug [_] unit)
  (error [_] unit))

;; Use in tests — same syntax as production
(deftest "process uses db"
  (handle [TestDb]
    (handle [SilentLog]
      (is (= (Ok "done") (process! "input"))))))
```

### 12.2 Inline Handlers for One-Off Tests

For handlers used in only one test, inline `handle` works as always:

```clojure
(deftest "fetch retries on failure"
  (let [call-count (atom 0)]
    (handle [Net
              (get [resume url]
                (swap! call-count inc)
                (if (< (deref call-count) 3)
                  (resume (Err :timeout))
                  (resume (Ok {:status 200 :body "data"}))))]
      (let [result (fetch-with-retry! "http://example.com")]
        (is (= (deref call-count) 3))
        (is-match result (Ok _))))))
```

### 12.3 Parameterized Test Handlers

When tests need varying behavior from the same effect:

```clojure
;; Handler that returns configurable data
(defhandler StubDb [rows]
  Db
  (query [sql params] rows)
  (exec! [sql params] (count rows)))

(deftest "handles empty results"
  (handle [(StubDb [])]
    (is-match (find-user! "nobody") (Err _))))

(deftest "handles multiple results"
  (handle [(StubDb [{:id 1 :name "alice"} {:id 2 :name "bob"}])]
    (is (= 2 (count (list-users!))))))
```

### 12.4 `call-log` — Recording Wrapper

`call-log` is the one test-specific utility: it wraps any named handler with
automatic call recording. It is orthogonal — add recording to any `defhandler`.

```clojure
(deftest "user creation calls db"
  (let [log (call-log TestDb)]
    (handle [(:handler log)]
      (handle [SilentLog]
        (create-user! "bob" "bob@example.com")

        (is (= 1 (count (deref (:calls log)))))
        (is-match (first (deref (:calls log)))
          {:op :exec! :args [_ _]})))))
```

`call-log` wraps a handler and returns `{:handler wrapped :calls (atom [])}`.
Each recorded call is `{:op :keyword :args [...] :returned value}`.

### 12.5 Capturing Handler — Stateful Inline Pattern

For tests that need to inspect what happened inside a handler, use the
continuation form with an atom:

```clojure
(deftest "logging captures messages"
  (let [messages (atom [])]
    (handle [Log
              (info [resume msg]
                (swap! messages conj msg)
                (resume unit))
              (warn  [resume _] (resume unit))
              (debug [resume _] (resume unit))
              (error [resume _] (resume unit))]
      (process-order! sample-order)
      (is (= 2 (count (deref messages))))
      (is (string/contains? (first (deref messages)) "Processing")))))
```

### 12.6 Testing Concurrent Code

Because concurrency is an effect (`Concurrent`), it can be replaced with a
deterministic executor:

```clojure
(defhandler SequentialExecutor
  Concurrent
  (fork [resume thunk] (resume (thunk)))
  (join [resume future] (resume future)))

(deftest "concurrent aggregation is deterministic"
  (handle [SequentialExecutor]
    (let [results (par-let
                    a (fetch-metrics! :cpu)
                    b (fetch-metrics! :memory)
                    c (fetch-metrics! :disk))]
      (is (= 3 (count results))))))
```

By default, `deftest` uses a sequential executor for the `Concurrent` effect,
making concurrent code deterministic under test. To test with real concurrency,
annotate with `:tags [:concurrent]`.

---

## 12. Property-Based Testing

### 12.1 `check` — Properties Inside Tests

`check` is used **inside** `deftest`, not as a separate top-level form.
Property tests are just tests that happen to use generators.

```clojure
(deftest "sort is idempotent"
  (check [xs (gen/vec gen/int)]
    (is (= (sort xs) (sort (sort xs))))))

(deftest "addition is commutative"
  (check [x gen/int  y gen/int]
    (is (= (+ x y) (+ y x)))))
```

`check` takes a binding vector of `[name generator]` pairs and a body of
assertions. The runner generates 100 random inputs (configurable), and on
failure, shrinks to a minimal counterexample.

### 12.2 Configuration

```clojure
(deftest "stress test"
  (check [x gen/int] :num-tests 500 :seed 12345
    (is (= x (identity x)))))
```

### 12.3 Generators

```clojure
;; Primitives
gen/int                              ;; Int (scaled by trial size)
(gen/int-range 0 100)                ;; Int in [0, 100]
gen/float                            ;; Float
(gen/float-range 0.0 1.0)            ;; Float in range
gen/bool                             ;; true or false
gen/str                              ;; arbitrary string
(gen/str-of gen/char-alpha 1 50)     ;; custom alphabet, bounded length
gen/char                             ;; single character
gen/keyword                          ;; arbitrary keyword
(gen/bytes 16)                       ;; Bytes (fixed size)

;; Collections
(gen/vec gen/int)                    ;; Vec of arbitrary length
(gen/vec gen/int 3 10)               ;; Vec with length in [3, 10]
(gen/map gen/keyword gen/str)        ;; Map
(gen/set gen/int)                    ;; Set
(gen/tuple gen/int gen/str gen/bool) ;; fixed-length heterogeneous vector

;; ADT generators
(gen/option gen/int)                 ;; None or (Some int)
(gen/result gen/int gen/str)         ;; (Ok int) or (Err str)
(gen/element [:red :green :blue])    ;; pick from collection

;; Combinators
(gen/one-of [gen/int gen/float])     ;; uniform choice among generators
(gen/frequency                       ;; weighted choice
  [[9 gen/int] [1 (gen/constant 0)]])
(gen/such-that pos? gen/int)         ;; filtered (with retry limit)
(gen/fmap str gen/int)               ;; transform generated value
(gen/bind gen/int                    ;; monadic: dependent generation
  (fn [n] (gen/vec gen/bool n)))
(gen/sized (fn [size] (gen/int-range 0 size))) ;; size-dependent
(gen/no-shrink gen/int)              ;; disable shrinking
(gen/sample gen/int 5)               ;; draw 5 samples (for debugging)
```

### 12.4 `Arbitrary` Protocol — Auto-Derived Generators

User types that `derive Arbitrary` get a generator automatically.

```clojure
(deftype User
  :derive [Arbitrary Show Eq]
  {:name Str
   :age  Int
   :role (| :admin :user :guest)})

;; Auto-derived generator works immediately:
(deftest "user roundtrip"
  (check [u (gen/arbitrary User)]
    (is (= u (json/decode User (json/encode u))))))
```

For ADTs with recursive constructors, the derived generator favors base cases
at larger sizes to ensure termination. Shrinking moves toward simpler
constructors and smaller field values.

```clojure
(deftype Expr
  :derive [Arbitrary Show Eq]
  | (Lit Int)
  | (Add Expr Expr)
  | (Mul Expr Expr))

;; Auto-derived: Lit favored at larger sizes, shrinks toward (Lit 0)
(deftest "eval add commutative"
  (check [a (gen/arbitrary Expr)
          b (gen/arbitrary Expr)]
    (is (= (eval-expr (Add a b))
            (eval-expr (Add b a))))))
```

### 12.5 Integrated Shrinking

Shrinking is integrated into generators — every generated value carries a lazy
tree of smaller alternatives. When a property fails, the runner traverses this
tree to find the smallest failing input. The shrink tree is built automatically
by generator combinators; `gen/fmap`, `gen/bind`, `gen/such-that` all preserve
shrinking.

```
FAIL in "sort preserves length" at test/sort_test.nx:5:3

  Property falsified after 23 tests.
  Shrunk 5 times from [3 1 -7 0 12] to [1]:

  (check [xs (gen/vec gen/int)]
    (is (= (count xs) (count (my-broken-sort xs)))))

  left:  1
  right: 0
```

### 12.6 Failure Persistence

When a property test fails, the failing seed is written to `.test-seeds` next
to the test file. On subsequent runs, these seeds are replayed first, ensuring
regressions are caught immediately without waiting for random rediscovery.

---

## 13. Snapshot Testing

### 13.1 `snap!` — Inline Snapshots (Judge-style)

Inspired by Janet's Judge: the test runner writes expected values directly back
into source code. Since Nexl values are s-expressions, no serialization format
is needed.

```clojure
;; First time: no expected value
(deftest "AST pretty-printing"
  (snap! (ast/format (parse "(+ 1 2)"))))

;; After `nexl test --accept`:
(deftest "AST pretty-printing"
  (snap! (ast/format (parse "(+ 1 2)"))
    "(+ 1 2)"))
```

**Workflow:**
1. Write `(snap! expr)` with no expected value
2. Run `nexl test` — the snapshot "fails" and proposes the actual value
3. Run `nexl test --accept` — the runner rewrites source to fill in the value
4. On subsequent runs, `snap!` compares actual to the stored value
5. If the value changes, run `nexl test --accept` to update

The `--review` flag shows diffs interactively before accepting.

### 13.2 File-Based Snapshots

For large outputs that would clutter the source:

```clojure
(deftest "full report"
  (snap-file! (generate-report data)))
```

Stored in `__snapshots__/{test-module}/{test-name}.snap`, serialized as Nexl
literals. Snapshot references are content-addressed, so renaming a test does
not invalidate its snapshot.

### 13.3 No-Op Outside Test Runner

Outside the test runner, `snap!` is a no-op that evaluates and discards its
expression. Snapshots can live inside source files (in `submodule test` blocks)
with zero runtime cost in production.

---

## 14. Contract-Driven Testing

### 14.1 `:examples` Are Tests

`:examples` clauses on functions are automatically collected and executed by
`nexl test`. Well-contracted code has tests without writing any `deftest` forms.

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

Running `nexl test` validates:
1. All `:examples` — each `:in` is applied, result compared to `:out`.
2. `:requires` — checked on each example input (must all pass).
3. `:ensures` — checked on each example output (must all pass).

```
$ nexl test
Running contract examples...
  my-app.math/fibonacci: 4/4 examples passed
Running tests...
  my-app.math-tests: 12/12 tests passed

All 16 checks passed in 42ms.
```

### 14.2 Doctest Syntax in Docstrings

Triple-quoted docstrings can contain REPL-style examples:

```clojure
(defn greet [name : Str] -> Str
  """Greets the given name.

  >>> (greet "Alice")
  "Hello, Alice!"

  >>> (greet "")
  "Hello, stranger!"
  """
  (if (string/empty? name)
    "Hello, stranger!"
    (str "Hello, " name "!")))
```

`>>>` marks an input expression. The next non-blank line is the expected
output. `nexl test --doctests src/` extracts and runs these.

---

## 15. Matchers: The `Matcher` Protocol

Matchers are an **advanced** extensibility mechanism. Most assertions use `is`
with plain expressions. Matchers are for reusable, composable checks.

### 15.1 Protocol

```clojure
(defprotocol Matcher
  "A composable test assertion."
  (match-value : (Fn [Self Any] -> (Result Unit MatchFailure)))
  (describe-expectation : (Fn [Self] -> Str)))

(deftype MatchFailure
  {:expected Str
   :actual   Str
   :message  (Option Str)})
```

### 15.2 Built-in Matchers

```clojure
;; Equality & comparison
(eq val)                    ;; structural equality
(approx val :within delta)  ;; floating-point
(gt n), (gte n), (lt n), (lte n)

;; Strings
(starts-with prefix)
(ends-with suffix)
(contains-str substring)
(matches-regex pattern)

;; Collections
(has-length n)
(contains-element elem)
(is-empty)
(every-element matcher)     ;; all elements satisfy matcher
(some-element matcher)      ;; at least one satisfies

;; Result / Option
(ok?)                       ;; matches any (Ok _)
(ok? val)                   ;; matches (Ok val)
(ok-where matcher)          ;; matches (Ok x) where x satisfies matcher
(err?), (err? val)
(some?), (some? val), (none?)

;; Composition
(all-of m1 m2 ...)          ;; all matchers pass (AND)
(any-of m1 m2 ...)          ;; at least one passes (OR)
(not-m m)                   ;; inverts a matcher
```

### 15.3 Custom Matchers

```clojure
(deftype Between {:lo Float :hi Float})

(impl Between Matcher
  (match-value [self actual]
    (if (and (>= actual (:lo self)) (<= actual (:hi self)))
      (Ok unit)
      (Err (MatchFailure
        {:expected (str "value between " (:lo self) " and " (:hi self))
         :actual   (str actual)
         :message  (None)}))))
  (describe-expectation [self]
    (str "between " (:lo self) " and " (:hi self))))

(defn between [lo hi] (Between {:lo lo :hi hi}))

;; Usage
(is temperature (between 36.0 37.5))
```

---

## 16. Benchmarking

Lightweight benchmarking integrated into the test library, runnable via
`nexl bench`.

```clojure
(bench "fibonacci-perf"
  {:warmup 100 :iterations 1000}
  (fibonacci 30))

(bench "sort-10k"
  (sort (gen/sample (gen/vec gen/int) 10000)))
```

```
$ nexl bench
fibonacci-perf:  832us +/- 12us  (1000 iterations, 100 warmup)
sort-10k:       2.1ms +/- 0.3ms  (100 iterations, 10 warmup)
```

---

## 17. Test Runner & CLI

### 17.1 Commands

```
nexl test [paths...]                # run tests (default: test/)
nexl test --filter <pattern>        # filter by test name
nexl test --tags <tag1,tag2>        # include by tag
nexl test --exclude-tags <tags>     # exclude by tag
nexl test --doctests [paths...]     # run doctests only
nexl test --accept                  # accept snapshot changes
nexl test --review                  # interactively review snapshots
nexl test --seed <n>                # deterministic property test seed
nexl test --num-checks <n>          # property test iterations (default 100)
nexl test --parallel                # force parallel execution
nexl test --fail-fast               # stop on first failure
nexl test --verbose                 # show all test names, not just failures
nexl test --format json             # machine-readable JSON Lines output
nexl test --watch                   # re-run on file changes
nexl test --coverage                # collect coverage data
nexl bench [paths...]               # run benchmarks
```

### 17.2 Test Discovery

The runner discovers tests by:
1. Finding all `.nx` files in the given paths (default: `test/`)
2. Finding all `(submodule test ...)` blocks in source files
3. Collecting `:examples` clauses from all loaded modules
4. Evaluating test files/submodules (registers tests via `deftest`/`describe`)
5. Applying filters (name, tags, focus)

Convention: test files mirror source structure.
```
src/my_app/user.nx       ->  test/my_app/user_test.nx
src/my_app/math.nx       ->  test/my_app/math_test.nx
```

### 17.3 Smart Re-Runs

Every definition has a content hash. The test runner tracks which test depends
on which definitions. On re-run, only tests whose transitive dependency graph
includes a changed hash are re-executed. In watch mode (`nexl test --watch`),
this makes incremental test runs near-instant.

Failed tests from the previous run are executed first, providing faster
feedback during debugging.

### 17.4 Parallel Execution

Tests run in parallel across modules by default. Tests within a single module
run sequentially (to allow shared setup state), but can opt into intra-module
parallelism:

```clojure
(describe "independent pure tests" :tags [:parallel]
  (deftest "a" (is (= 1 1)))
  (deftest "b" (is (= 2 2))))
```

### 17.5 Output Format

**Default (compact):**
```
test/math_test.nx
  . addition (0.2ms)
  . subtraction (0.1ms)
  X division — (is (= (/ 10 3) 3))
      left:  3.333...
      right: 3
      at test/math_test.nx:12:5

3 tests, 2 passed, 1 failed (0.5ms)
```

**Verbose:**
```
test/math_test.nx
  Calculator > addition
    . positive numbers (0.1ms)
    . negative numbers (0.1ms)
  Calculator > division
    . basic (0.1ms)
    X by zero — expected ArithmeticError, got (Ok 0)
      at test/math_test.nx:18:5

4 tests, 3 passed, 1 failed (0.8ms)
```

### 17.6 Exit Codes

| Code | Meaning |
|------|---------|
| 0 | All tests passed |
| 1 | One or more tests failed |
| 2 | Compilation / parse error |
| 3 | `:focus` detected in CI mode |

---

## 18. Complete Example

This example demonstrates the full power of nexl.test — effect-based mocking,
property testing, contracts, lifecycle hooks, and assertions working together.

```clojure
(module my-app.user-service
  :performs [Db Log]
  :exports [create-user! find-user! UserInput])

(deftype UserInput
  :derive [Show Eq Arbitrary]
  {:name  Str
   :email Str})

(deftype User
  :derive [Show Eq]
  {:id    Int
   :name  Str
   :email Str})

(defn create-user! [input : UserInput] -> (Result User AppError) ! [Db Log]
  :requires [(not (string/blank? (:name input)))
             (string/contains? (:email input) "@")]
  :ensures  [(match result
               (Ok u) (= (:name u) (:name input))
               (Err _) true)]
  :examples [{:in [{:name "alice" :email "a@b.com"}]
              :out (Ok {:id 1 :name "alice" :email "a@b.com"})}]
  (Log/info (str "Creating user: " (:name input)))
  (let [rows (Db/exec! "INSERT INTO users (name, email) VALUES (?, ?)"
                       [(:name input) (:email input)])]
    (if (= 1 rows)
      (Ok (User {:id 1 :name (:name input) :email (:email input)}))
      (Err (AppError {:message "Insert failed"})))))

;; ---- Named Handlers (used in both tests and production) ----

(defhandler TestDb
  Db
  (query [sql params] [])
  (exec! [sql params] 1))

(defhandler SilentLog
  Log
  (info [_] unit)
  (warn [_] unit)
  (debug [_] unit)
  (error [_] unit))

;; ---- Test Submodule ----

(submodule test my-app.user-service-tests
  :imports [[nexl.test :refer [deftest describe is is-match check call-log]]]

  ;; ---- Unit Tests ----

  (describe "create-user!"
    (deftest "success path"
      (handle [TestDb]
        (handle [SilentLog]
          (let [result (create-user! (UserInput {:name "alice" :email "a@b.com"}))]
            (is-match result (Ok _))
            (is-match result (Ok {:name "alice"}))))))

    (deftest "records db call"
      (let [log (call-log TestDb)]
        (handle [(:handler log)]
          (handle [SilentLog]
            (create-user! (UserInput {:name "bob" :email "b@c.com"}))
            (is (= 1 (count (deref (:calls log)))))
            (is-match (first (deref (:calls log)))
              {:op :exec! :args [_ ["bob" "b@c.com"]]})))))

    (deftest "logs creation"
      (let [messages (atom [])]
        (handle [Log
                  (info [resume msg]
                    (swap! messages conj msg)
                    (resume unit))
                  (warn  [resume _] (resume unit))
                  (debug [resume _] (resume unit))
                  (error [resume _] (resume unit))]
          (handle [TestDb]
            (create-user! (UserInput {:name "carol" :email "c@d.com"}))
            (is (= 1 (count (deref messages))))
            (is (string/contains? (first (deref messages)) "carol")))))))

  ;; ---- Property Tests ----

  (describe "create-user! properties"
    (def gen-valid-input
      (gen/fmap
        (fn [[n e]] (UserInput {:name n :email e}))
        (gen/tuple
          (gen/such-that (fn [s] (not (string/blank? s)))
            (gen/str-of gen/char-alpha 1 50))
          (gen/fmap (fn [[local domain]] (str local "@" domain ".com"))
            (gen/tuple
              (gen/str-of gen/char-alpha 1 20)
              (gen/str-of gen/char-alpha 1 10))))))

    (deftest "name preserved in result"
      (check [input gen-valid-input]
        (handle [TestDb]
          (handle [SilentLog]
            (is-match (create-user! input)
              (Ok {:name ^(:name input)}))))))))
```

---

## 19. API Summary

### Assertions
| Form | Purpose |
|------|---------|
| `(is expr)` | Power-assert any boolean expression |
| `(is expr "msg")` | With custom failure message |
| `(is actual matcher)` | Apply a matcher |
| `(is-match val pattern)` | Pattern-matching assertion |
| `(is-match val pattern guard)` | Pattern + guard assertion |
| `(throws? body...)` | Assert exception |
| `(throws? Type body...)` | Assert typed exception |

### Test Organization
| Form | Purpose |
|------|---------|
| `(deftest "name" body...)` | Define a test |
| `(describe "name" body...)` | Group tests |
| `(submodule test name body...)` | In-file test module |
| `(each bindings data body)` | Table-driven tests |
| `(check bindings body)` | Property-based test |
| `(bench "name" body)` | Benchmark |

### Handlers & Mocking
| Form | Purpose |
|------|---------|
| `(defhandler Name Effect (op ...))` | Named handler (language-level, works in prod and tests) |
| `(defhandler Name [params] Effect (op ...))` | Parameterized named handler |
| `(handle [HandlerName] body)` | Install named handler for a scope |
| `(handle [(HandlerName args)] body)` | Install parameterized handler |
| `(handle [Effect (op [args] impl)] body)` | Inline handler (one-off) |
| `(call-log HandlerName)` | Wrap handler with call recording (test utility) |

### Lifecycle
| Form | Purpose |
|------|---------|
| `(setup [ctx] body)` | Before each test |
| `(setup-all [ctx] body)` | Before all tests in block |
| `(teardown [ctx] body)` | After each test |
| `(teardown-all [ctx] body)` | After all tests in block |

### Snapshots
| Form | Purpose |
|------|---------|
| `(snap! expr)` | Inline snapshot (Judge-style) |
| `(snap-file! expr)` | File-based snapshot |

---

## 20. Migration from Current API

| Current (`test/`)                  | New (`nexl.test`)                  |
|------------------------------------|------------------------------------|
| `(test/register! "name" (fn [] ...))` | `(deftest "name" ...)`          |
| `(test/is cond)`                   | `(is cond)`                        |
| `(test/assert-eq a b)`            | `(is (= a b))`                     |
| `(test/fail msg)`                  | `(is false msg)`                   |
| `(test/skip msg)`                  | `(deftest "name" :skip msg ...)`   |
| `(test/check name vals pred)`      | `(each ...)` or `(check ...)`      |
| `(test/run-registered)`           | Handled by `nexl test` runner       |
| `(test/run-tests [...])`          | Handled by `nexl test` runner       |
| `(assert! cond)`                   | Kept as runtime assertion for production contracts |

`assert!` remains as a **runtime assertion** (like Rust's `assert!`) for
production code. `is` is the **test assertion** with power-assert diagnostics.

---

## 21. Implementation Phases

### Phase 1: Core (MVP)
- `is` macro with power-assert for `=`, `not=`, predicates
- `deftest` macro with registration
- `describe` macro with nesting
- `throws?`
- Updated `nexl test` runner with discovery, filtering, output
- `:skip` and `:focus` support
- Migration: keep old `test/` API working alongside new API

### Phase 2: Data, Patterns & Lifecycle
- `is-match` with full pattern matching (destructuring, guards, pins)
- `each` for table-driven tests
- `:let` clause on `describe`
- `:tags` support with CLI filtering
- String/collection diffs in error messages
- `setup`/`teardown`/`setup-all`/`teardown-all`

### Phase 3: `defhandler` & Mocking
- `defhandler` (language-level named effect handlers, impl-style syntax)
- `handle [HandlerName]` support (install named handler)
- Parameterized handlers (`defhandler Name [params] ...`)
- `call-log` (recording wrapper, test utility)
- Capability-aware test sandboxing (unhandled effects = compile error)
- `SequentialExecutor` for deterministic concurrent testing
- `submodule test` support

### Phase 4: Property Testing
- Generator primitives and combinators
- `check` form inside `deftest`
- Integrated shrinking (generators carry shrink trees)
- `Arbitrary` protocol with auto-derive
- Failure persistence (`.test-seeds`)

### Phase 5: Snapshots, Doctests & Contracts
- `snap!` inline snapshots with source rewriting
- `snap-file!` file-based snapshots
- `--accept` and `--review` CLI commands
- Contract-driven testing (`:examples` auto-execution)
- `>>>` docstring parsing

### Phase 6: Polish & Performance
- `--watch` mode with smart re-runs (content-addressed hashing)
- `--parallel` cross-module execution
- `--format json` output
- `bench` form and `nexl bench` command
- Matcher protocol and built-in matchers
- `--coverage` support
- `:flaky`, `:timeout` annotations
