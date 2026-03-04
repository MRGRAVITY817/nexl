# Nexl Standard Library Specification

> Version 0.1 — Draft — 2026-03-04

## 1. Design Principles

### 1.1 Parameter Ordering & Threading Convention

Nexl adopts Clojure's parameter ordering convention, which distinguishes two
fundamental kinds of operation:

#### `->` (Thread-First): Operating on a concrete thing

Functions that **navigate or transform a single data structure** take the
subject as the **first** argument. Use `->` to chain these:

```nexl
;; Map/record operations — the map is arg 1
(-> user
    (assoc :name "Alice")
    (update :age inc)
    (merge {:role :admin})
    (select-keys [:name :role]))

;; String operations — the string is arg 1
(-> raw-input
    (str/trim)
    (str/lower)
    (str/replace "  " " "))

;; Option/Result operations — the wrapper is arg 1
(-> (io/read-file "config.toml")
    (result/map toml/parse)
    (result/unwrap-or default-config))
```

**Functions designed for `->` (subject-first)**:
`assoc`, `dissoc`, `disj`, `conj`, `get`, `get-in`, `assoc-in`, `update`, `update-in`,
`merge`, `merge-with`, `select-keys`, `into`, `count`, `empty?`, `contains?`,
`first`, `last`, `rest`, `slice`, `concat`, all `str/*` functions,
all `option/*` functions, all `result/*` functions, all `path/*` functions.

#### `->>` (Thread-Last): Processing a sequence of things

Functions that **transform a collection as a whole** take a function/config
argument first and the collection **last**. Use `->>` to chain these:

```nexl
;; Sequence pipeline — the collection is the last arg
(->> users
     (filter active?)
     (map :name)
     (sort)
     (take 10))

;; Data processing pipeline
(->> (range 1 100)
     (filter (fn [n] (= 0 (mod n 3))))
     (map (fn [n] (* n n)))
     (reduce + 0))

;; Building derived data
(->> transactions
     (filter (fn [t] (> (:amount t) 100)))
     (group-by :category)
     (map-vals count))
```

**Functions designed for `->>` (collection-last)**:
`map`, `filter`, `remove`, `keep`, `reduce`, `flat-map`, `mapcat`, `some`,
`sort`, `sort-by`, `sort-with`, `group-by`, `partition-by`, `zip`, `zip-with`,
`take`, `drop`, `take-while`, `drop-while`, `find`, `find-index`, `every?`,
`any?`, `not-any?`, `not-every?`, `distinct`, `flatten`, `frequencies`,
`interleave`, `interpose`, `map-indexed`, `reduce-indexed`.

#### The Guiding Principle

| Abstraction | Threading | Rule | Why |
|-------------|-----------|------|-----|
| **Concrete data** (maps, records, strings, Options, Results) | `->` | Subject is arg 1 | "I have a thing and I'm transforming it step by step" |
| **Sequences** (filtering, mapping, reducing collections) | `->>` | Collection is last arg | "I have a stream of data flowing through transformers" |

This convention has a practical benefit: collection-last enables useful partial
application — `(partial map inc)` and `(partial filter odd?)` produce reusable
transformers. Subject-first for `assoc`/`update`/`merge` enables variadic
trailing args — `(assoc m :a 1 :b 2 :c 3)`.

#### When Neither Fits: `as->`

When a pipeline mixes subject-first and collection-last operations, use `as->`
to name the threaded value and place it explicitly:

```nexl
(as-> raw-data $
  (json/decode $)                    ;; $ in arg 1 (subject-first)
  (result/unwrap $)
  (filter :active $)                 ;; $ in last position (collection-last)
  (map :email $)
  (str/join ", " $))                 ;; $ in arg 2 (subject-first)
```

**Rule**: Prefer `->` or `->>` when the pipeline is uniform. Use `as->` only
when crossing the boundary. If `as->` is needed more than occasionally,
consider a `let` block with named intermediates instead.

#### Threading Macro Variants (Full Set)

| Macro | Position | Short-circuit | Description |
|-------|----------|---------------|-------------|
| `->` | First | No | Thread through subject-first operations |
| `->>` | Last | No | Thread through collection-last operations |
| `as->` | Named | No | Explicit placement via binding |
| `some->` | First | On `None` | Thread-first, unwrap `Some`, stop on `None` |
| `some->>` | Last | On `None` | Thread-last, unwrap `Some`, stop on `None` |
| `ok->` | First | On `Err` | Thread-first, unwrap `Ok`, stop on `Err` |
| `ok->>` | Last | On `Err` | Thread-last, unwrap `Ok`, stop on `Err` |
| `cond->` | First | No | Thread-first, each step has a guard condition |
| `cond->>` | Last | No | Thread-last, each step has a guard condition |

**`some->` / `some->>` — Option chaining**

Threads through `Option` values. Each step receives the unwrapped `Some` value.
If any step returns `None`, the entire pipeline short-circuits to `None`.

```nexl
;; Returns (Option Str) — stops at the first None
(some-> config
        (get :database)          ;; (Option Map) → unwrap or stop
        (get :connection-string) ;; (Option Str) → unwrap or stop
        (str/trim))              ;; plain value → auto-wrapped in Some
```

**`ok->` / `ok->>` — Result chaining**

Threads through `Result` values. Each step receives the unwrapped `Ok` value.
If any step returns `Err`, the entire pipeline short-circuits and propagates the `Err`.

```nexl
;; Returns (Result Config Str) — stops at the first Err
(ok-> (io/read-file "config.toml")   ;; (Result Str Str) → unwrap or stop
      (toml/parse)                    ;; (Result Map Str) → unwrap or stop
      (get :database)                 ;; plain value → auto-wrapped in Ok
      (validate-config))              ;; (Result Config Str) → unwrap or stop
```

This is the pipeline equivalent of the `?` operator. Where `?` provides early
return from a **function**, `ok->` provides early return from a **pipeline**:

```nexl
;; These are equivalent:
(ok-> (read-config)
      (validate)
      (apply-defaults))

(let [raw   (read-config)?
      valid (validate raw)?]
  (apply-defaults valid))
```

**Wrapping semantics**: If a step returns a plain value (not `Option`/`Result`),
it is auto-wrapped in `Some`/`Ok` respectively. This allows mixing fallible and
infallible steps in the same pipeline without manual wrapping.

**`cond->` / `cond->>` — Conditional threading**

Applies steps only when their guard condition is true:

```nexl
(cond-> base-query
  include-deleted?  (assoc :show-deleted true)
  (some? tag)       (assoc :tag tag)
  (> limit 0)       (assoc :limit limit))
```

### 1.2 Naming Conventions

| Convention | Meaning | Examples |
|------------|---------|----------|
| `verb` | Transform or action | `map`, `filter`, `split`, `encode` |
| `verb?` | Predicate (returns Bool) | `empty?`, `contains?`, `blank?` |
| `verb!` | Side-effecting / mutates atom | `swap!`, `reset!`, `register!` |
| `->type` | Conversion to type | `->int`, `->float`, `->str` |
| `noun` | Constructor or accessor | `keys`, `vals`, `first`, `last` |

**Consistency rules**:
- One name per concept: `count` (never `length`/`size`/`len`)
- Imperative verbs: `sort` not `sorted`, `reverse` not `reversed`
- Module-qualified: `str/split`, `math/abs`, `vec/chunk` (no ambiguity)
- Unqualified builtins: Only the ~60 most universal operations live in builtins

### 1.3 Return Type Conventions

| Pattern | When | Example |
|---------|------|---------|
| `(Option T)` | Value may not exist | `first`, `last`, `get`, `index-of` |
| `(Result T E)` | Operation can fail with info | `io/read-file`, `json/decode` |
| Same type as input | Collection transforms | `map`, `filter`, `sort` |
| `Bool` | Predicate | `empty?`, `contains?` |
| `Unit` | Side effect only | `println`, `log/info` |

**Rule**: Never return a bare value where `None` is a valid outcome.
Every operation that can fail returns `Option` or `Result` — no panics, no sentinels.

### 1.4 Effect Declarations

Functions that interact with the outside world declare their effects:

```nexl
(defn read-file [path : Str] -> (Result Str Str) ! [FileSystem]
  ...)
```

**Effect taxonomy** (from the effect system):
- `Console` — stdin/stdout/stderr
- `FileSystem` — file read/write/delete
- `Net` — network access
- `Random` — non-deterministic randomness
- `Time` — wall-clock time access
- `Env` — environment variable access
- `Process` — process spawning/signals
- `Database` — database connections

Pure functions (no effects) compose freely; effectful functions require handlers.

### 1.5 Performance Documentation

Every function documents its time complexity:

```nexl
(defn get
  """
  Retrieve value at key.

  - Vec: O(1) by index
  - Map: O(1) amortized by key
  - Set: O(1) membership check
  - Str: O(n) by codepoint index
  """
  [coll key] ...)
```

### 1.6 No Partial Implementations

Every function in the stdlib must be fully implemented and tested.
Stubs, `todo!()`, and "deferred to Stage N" are not acceptable in the stdlib spec.
Functions not yet implementable (e.g., requiring WASI 0.3) belong in an `experimental/` namespace.

### 1.7 Stability Tiers

| Tier | Promise | Modules |
|------|---------|---------|
| **Stable** | Backward-compatible forever | builtins, core, str, math, conv, vec, map, set, option, result, json, io |
| **Standard** | Backward-compatible within major versions | http, db, crypto, log, time, env, regex, uri, path, csv, channel |
| **Experimental** | May change | async, wasi, native |

---

## 2. Module Inventory

### Overview

| Module | Purpose | Status | Functions |
|--------|---------|--------|-----------|
| `builtins` | Unqualified core operations | Enhance | ~70 |
| `core` | Higher-order function utilities | Enhance | ~15 |
| `str` | String manipulation | Enhance | ~35 |
| `char` | Character operations | **New** | ~15 |
| `math` | Mathematical functions | Enhance | ~30 |
| `conv` | Type conversions | Keep | ~5 |
| `vec` | Vector-specific operations | **New** | ~25 |
| `map` | Map-specific operations | **New** | ~20 |
| `set` | Set-specific operations | **New** | ~15 |
| `option` | Option combinators | **New** | ~15 |
| `result` | Result combinators | **New** | ~15 |
| `iter` | Lazy iteration protocol | **New** | ~25 |
| `io` | File I/O and console | Enhance | ~20 |
| `path` | Cross-platform path manipulation | **New** | ~15 |
| `json` | JSON encoding/decoding | Keep | ~5 |
| `csv` | CSV parsing/writing | **New** | ~5 |
| `toml` | TOML parsing/writing | **New** | ~5 |
| `http` | HTTP client/server | Enhance | ~15 |
| `uri` | URI parsing and construction | **New** | ~10 |
| `db` | SQLite database access | Keep | ~5 |
| `env` | Environment variables | Keep | ~5 |
| `time` | Time and duration | Enhance | ~20 |
| `random` | Random number generation | Enhance | ~10 |
| `crypto` | Cryptographic operations | Enhance | ~15 |
| `regex` | Regular expressions | **New** | ~10 |
| `base64` | Base64 encoding/decoding | **New** | ~4 |
| `uuid` | UUID generation | **New** | ~5 |
| `log` | Structured logging | Keep | ~8 |
| `channel` | CSP-style channels | **New** | ~10 |
| `bit` | Bitwise operations (module form) | **New** | ~10 |
| `sys` | System interface | Enhance | ~8 |
| `process` | Child process management | **New** | ~8 |
| `atom` | Advanced atom operations | **New** | ~6 |
| `test` | Testing framework | Keep | ~10 |
| `gen` | Property test generators | Keep | ~8 |
| `async` | Concurrency primitives | Enhance | ~10 |

**Total: ~36 modules, ~540+ functions**

---

## 3. Detailed Module Specifications

### 3.1 `builtins` — Core Operations (Unqualified)

These are available without any import. They form the vocabulary of the language.

**Signature shorthand**: In the tables below, `Num` stands for `a :where [(Numeric a)]`,
`Ord` stands for `a :where [(Ord a)]`, and `Coll` stands for any of Vec/Map/Set/Str
(resolved by compiler-dispatched overloads per spec §5.11). These are documentation
conveniences, not actual types in the Nexl type system.

#### Arithmetic
| Function | Signature | Description | Complexity |
|----------|-----------|-------------|------------|
| `+` | `(Fn [& Num] -> Num)` | Add; 0 args → 0 | O(n) |
| `-` | `(Fn [Num & Num] -> Num)` | Subtract or negate | O(n) |
| `*` | `(Fn [& Num] -> Num)` | Multiply; 0 args → 1 | O(n) |
| `/` | `(Fn [Num Num] -> Num)` | Divide; Int/Int → truncate | O(1) |
| `mod` | `(Fn [Int Int] -> Int)` | Modulo (sign of divisor, Euclidean) | O(1) |
| `rem` | `(Fn [Int Int] -> Int)` | **New.** Remainder (sign of dividend, truncated) | O(1) |
| `quot` | `(Fn [Int Int] -> Int)` | **New.** Truncated division | O(1) |
| `inc` | `(Fn [Num] -> Num)` | **New.** Increment by 1 | O(1) |
| `dec` | `(Fn [Num] -> Num)` | **New.** Decrement by 1 | O(1) |

#### Comparison
| Function | Signature | Description |
|----------|-----------|-------------|
| `=` | `(Fn [Any Any] -> Bool)` | Structural equality |
| `not=` | `(Fn [Any Any] -> Bool)` | **New.** Not equal (complement of `=`) |
| `<` | `(Fn [Ord Ord] -> Bool)` | Less than |
| `>` | `(Fn [Ord Ord] -> Bool)` | Greater than |
| `<=` | `(Fn [Ord Ord] -> Bool)` | Less than or equal |
| `>=` | `(Fn [Ord Ord] -> Bool)` | Greater than or equal |
| `compare` | `(Fn [Ord Ord] -> (| :lt :eq :gt))` | **New.** Three-way comparison (spec §5.11) |
| `min` | `(Fn [Ord Ord] -> Ord)` | Minimum of two values |
| `max` | `(Fn [Ord Ord] -> Ord)` | Maximum of two values |
| `clamp` | `(Fn [Ord Ord Ord] -> Ord)` | Restrict to [lo, hi] |

#### Logic
| Function | Signature | Description |
|----------|-----------|-------------|
| `not` | `(Fn [Bool] -> Bool)` | Boolean negation |
| `and` | Special form | Short-circuit AND |
| `or` | Special form | Short-circuit OR |

#### String
| Function | Signature | Description |
|----------|-----------|-------------|
| `str` | `(Fn [& Any] -> Str)` | Concatenate as strings |
| `pr-str` | `(Fn [Any] -> Str)` | **New.** Readable representation (with quotes, escapes) |
| `println` | `(Fn [& Any] -> Unit ! [Console])` | Print with newline |
| `print` | `(Fn [& Any] -> Unit ! [Console])` | Print without newline |

#### Collections — Accessors (Polymorphic)

These work on Vec, Map, Set, and Str where semantically meaningful.
All follow **subject-first** ordering (for `->` threading).

| Function | Signature | Description | Vec | Map | Set | Str |
|----------|-----------|-------------|-----|-----|-----|-----|
| `count` | `(Fn [Coll] -> Int)` | Element count | O(1) | O(1) | O(1) | O(n)* |
| `empty?` | `(Fn [Coll] -> Bool)` | **New.** True if count is 0 | O(1) | O(1) | O(1) | O(1) |
| `get` | `(Fn [Coll Key] -> (Option Val))` | Fetch by key/index | O(1) | O(1) | O(1) | O(n)* |
| `get-in` | `(Fn [Coll (Vec Key)] -> (Option Val))` | **New.** Nested access | — | O(k) | — | — |
| `contains?` | `(Fn [Coll Key] -> Bool)` | Membership test | O(n) | O(1) | O(1) | O(n) |
| `first` | `(Fn [Coll] -> (Option Val))` | First element | O(1) | O(1) | — | O(1) |
| `last` | `(Fn [Coll] -> (Option Val))` | Last element | O(1) | O(1) | — | O(1) |
| `rest` | `(Fn [Coll] -> Coll)` | All but first | O(n) | O(n) | — | O(n) |
| `slice` | `(Fn [Coll Int Int] -> Coll)` | Sub-range [start, end) | O(k) | — | — | O(k) |
| `nth` | `(Fn [Coll Int] -> (Option Val))` | **New.** Alias for get on indexed colls | O(1) | — | — | O(n)* |

*Str `count` is O(n) for codepoints, O(1) for byte length via `str/byte-count`.

#### Collections — Structural Transforms (Subject-First, for `->`)

These return a new collection. Subject is always the **first** argument
(Clojure convention for associative operations).

| Function | Signature | Description | Complexity |
|----------|-----------|-------------|------------|
| `assoc` | `(Fn [Coll Key Val & Key Val] -> Coll)` | Associate key→val (variadic). Vec by index, Map by key. | O(1)† |
| `assoc-in` | `(Fn [Coll (Vec Key) Val] -> Coll)` | **New.** Nested assoc via key path | O(k) |
| `update` | `(Fn [Coll Key (Fn [Val] -> Val)] -> Coll)` | **New.** Update value at key via fn | O(1)† |
| `update-in` | `(Fn [Coll (Vec Key) (Fn [Val] -> Val)] -> Coll)` | **New.** Nested update via key path | O(k) |
| `dissoc` | `(Fn [Map Key & Key] -> Map)` | Remove key(s) from Map (variadic) | O(n) |
| `disj` | `(Fn [Set Val & Val] -> Set)` | Remove element(s) from Set (variadic) | O(n) |
| `conj` | `(Fn [Coll & Val] -> Coll)` | Add element(s) to collection (polymorphic) | O(1)† |
| `into` | `(Fn [Coll Coll] -> Coll)` | **New.** Pour all elements of src into dest | O(m) |
| `concat` | `(Fn [Coll Coll] -> Coll)` | **New.** Concatenate two collections | O(n+m) |
| `empty` | `(Fn [Coll] -> Coll)` | **New.** Empty collection of same type | O(1) |

†Amortized via persistent data structures with structural sharing.

**`conj` semantics** (polymorphic, matches Clojure):
- **Vec**: appends to end — `(conj [1 2] 3)` → `[1 2 3]`
- **Set**: adds element — `(conj #{1 2} 3)` → `#{1 2 3}`
- **Map**: adds entry (takes `[k v]` pair) — `(conj {:a 1} [:b 2])` → `{:a 1 :b 2}`
- Variadic: `(conj [1] 2 3 4)` → `[1 2 3 4]`

**`dissoc` vs `disj`** (Clojure convention):
- `dissoc` is for Maps (dissociate key) — `(dissoc {:a 1 :b 2} :a)` → `{:b 2}`
- `disj` is for Sets (disjoin element) — `(disj #{1 2 3} 2)` → `#{1 3}`
- Both are variadic: `(dissoc m :a :b :c)`, `(disj s 1 2 3)`

**`into`** pours elements from source into destination:
```nexl
(into [] #{3 1 2})      ;; → [3 1 2] (set into vec)
(into #{} [1 2 2 3])    ;; → #{1 2 3} (vec into set, deduplicates)
(into {:a 1} {:b 2})    ;; → {:a 1 :b 2} (merge maps)
```

#### Higher-Order Sequence Functions (Collection-Last, for `->>`)

These take a function/config first and the collection **last**,
following Clojure's sequence function convention for `->>` threading.

| Function | Signature | Description | Complexity |
|----------|-----------|-------------|------------|
| `map` | `(Fn [(Fn [a] -> b) Coll] -> Coll)` | Transform each element | O(n) |
| `filter` | `(Fn [(Fn [a] -> Bool) Coll] -> Coll)` | Keep elements where pred is true | O(n) |
| `remove` | `(Fn [(Fn [a] -> Bool) Coll] -> Coll)` | Keep elements where pred is **false** (complement of filter) | O(n) |
| `keep` | `(Fn [(Fn [a] -> (Option b)) Coll] -> Coll)` | **New.** Map + filter None in one pass | O(n) |
| `reduce` | `(Fn [(Fn [acc a] -> acc) acc Coll] -> acc)` | Fold left | O(n) |
| `flat-map` | `(Fn [(Fn [a] -> (Vec b)) (Vec a)] -> (Vec b))` | Map then flatten one level | O(n*m) |
| `mapcat` | `(Fn [(Fn [a] -> (Vec b)) (Vec a)] -> (Vec b))` | **New.** Alias for flat-map (Clojure name) | O(n*m) |
| `each` | Special form: `(each [x coll] body)` | Side-effecting iteration (spec §4.15, NOT a HOF) | O(n) |
| `map-indexed` | `(Fn [(Fn [Int a] -> b) Coll] -> Coll)` | **New.** Map with index | O(n) |
| `reduce-indexed` | `(Fn [(Fn [acc Int a] -> acc) acc Coll] -> acc)` | **New.** Fold with index | O(n) |
| `sort` | `(Fn [Coll] -> Coll)` | Stable sort (natural order) | O(n log n) |
| `sort-by` | `(Fn [(Fn [a] -> Ord) Coll] -> Coll)` | Stable sort by key fn | O(n log n) |
| `sort-with` | `(Fn [(Fn [a a] -> Int) Coll] -> Coll)` | **New.** Sort by comparator | O(n log n) |
| `reverse` | `(Fn [Coll] -> Coll)` | Reverse order | O(n) |
| `group-by` | `(Fn [(Fn [a] -> k) (Vec a)] -> (Map k (Vec a)))` | Group by key fn | O(n) |
| `zip` | `(Fn [(Vec a) (Vec b)] -> (Vec (Tuple a b)))` | Pair elements | O(min(n,m)) |
| `zip-with` | `(Fn [(Fn [a b] -> c) (Vec a) (Vec b)] -> (Vec c))` | **New.** Zip with function | O(min(n,m)) |
| `range` | `(Fn [Int] -> (Vec Int))` | Generate [0..n) | O(n) |
| `take` | `(Fn [Int Coll] -> Coll)` | First n elements | O(n) |
| `drop` | `(Fn [Int Coll] -> Coll)` | Skip first n | O(n) |
| `take-while` | `(Fn [(Fn [a] -> Bool) Coll] -> Coll)` | Prefix while pred true | O(k) |
| `drop-while` | `(Fn [(Fn [a] -> Bool) Coll] -> Coll)` | Skip leading while true | O(n) |
| `find` | `(Fn [(Fn [a] -> Bool) Coll] -> (Option a))` | **New.** First match | O(n) |
| `find-index` | `(Fn [(Fn [a] -> Bool) (Vec a)] -> (Option Int))` | **New.** Index of first match | O(n) |
| `some` | `(Fn [(Fn [a] -> (Option b)) Coll] -> (Option b))` | **New.** First non-None result (Clojure) | O(n) |
| `every?` | `(Fn [(Fn [a] -> Bool) Coll] -> Bool)` | **New.** All elements match (Clojure alias) | O(n) |
| `any?` | `(Fn [(Fn [a] -> Bool) Coll] -> Bool)` | **New.** Any element matches | O(n) |
| `not-any?` | `(Fn [(Fn [a] -> Bool) Coll] -> Bool)` | **New.** No element matches (Clojure) | O(n) |
| `not-every?` | `(Fn [(Fn [a] -> Bool) Coll] -> Bool)` | **New.** Not all elements match (Clojure) | O(n) |
| `distinct` | `(Fn [(Vec a)] -> (Vec a))` | **New.** Remove duplicates (stable) | O(n) |
| `flatten` | `(Fn [(Vec (Vec a))] -> (Vec a))` | **New.** Flatten one level | O(n*m) |
| `partition-by` | `(Fn [(Fn [a] -> k) (Vec a)] -> (Vec (Vec a)))` | **New.** Split when key fn changes (Clojure) | O(n) |
| `frequencies` | `(Fn [(Vec a)] -> (Map a Int))` | **New.** Count occurrences | O(n) |
| `interleave` | `(Fn [(Vec a) (Vec b)] -> (Vec a))` | **New.** Alternate elements | O(n) |
| `interpose` | `(Fn [a (Vec a)] -> (Vec a))` | **New.** Insert separator | O(n) |

**`remove` vs `filter`** (Clojure convention):
```nexl
(->> users (filter active?))  ;; keep active users
(->> users (remove active?))  ;; keep INactive users (complement of filter)
```

**`keep`** combines map + filter-None in one pass (Clojure):
```nexl
(->> items (keep (fn [x] (some-> x :email))))  ;; only non-None results
```

**`some`** returns the first non-None result of applying f (Clojure):
```nexl
(->> paths (some (fn [p] (io/read-file p))))  ;; first readable file
```

#### Set Operations (Unqualified)
| Function | Signature | Description |
|----------|-----------|-------------|
| `conj` | `(Fn [Set a & a] -> Set)` | Add element(s) — same polymorphic `conj` |
| `disj` | `(Fn [Set a & a] -> Set)` | Remove element(s) from set |
| `union` | `(Fn [Set Set] -> Set)` | Set union |
| `intersection` | `(Fn [Set Set] -> Set)` | Set intersection |
| `difference` | `(Fn [Set Set] -> Set)` | Set difference (a \ b) |
| `symmetric-difference` | `(Fn [Set Set] -> Set)` | **New.** Elements in either but not both |
| `subset?` | `(Fn [Set Set] -> Bool)` | **New.** a ⊆ b |
| `superset?` | `(Fn [Set Set] -> Bool)` | **New.** a ⊇ b |
| `disjoint?` | `(Fn [Set Set] -> Bool)` | **New.** No common elements |

#### Map Operations (Unqualified)
| Function | Signature | Description |
|----------|-----------|-------------|
| `assoc` | `(Fn [Map Key Val & Key Val] -> Map)` | Associate key(s)→val(s) — same polymorphic `assoc` |
| `dissoc` | `(Fn [Map Key & Key] -> Map)` | Remove key(s) from map |
| `keys` | `(Fn [Map] -> (Vec k))` | Keys in insertion order |
| `vals` | `(Fn [Map] -> (Vec v))` | Values in insertion order |
| `entries` | `(Fn [Map] -> (Vec (Tuple k v)))` | Key-value pairs |
| `merge` | `(Fn [Map & Map] -> Map)` | **New.** Merge maps (rightmost wins, variadic) |
| `merge-with` | `(Fn [(Fn [v v] -> v) Map & Map] -> Map)` | **New.** Merge with conflict resolver |
| `select-keys` | `(Fn [Map (Vec k)] -> Map)` | **New.** Submap of given keys |
| `rename-keys` | `(Fn [Map (Map k k)] -> Map)` | **New.** Rename keys via mapping (Clojure) |
| `zipmap` | `(Fn [(Vec k) (Vec v)] -> Map)` | **New.** Build map from parallel vecs (Clojure) |

#### Bitwise
| Function | Signature | Description |
|----------|-----------|-------------|
| `bit-and` | `(Fn [Int Int] -> Int)` | Bitwise AND |
| `bit-or` | `(Fn [Int Int] -> Int)` | Bitwise OR |
| `bit-xor` | `(Fn [Int Int] -> Int)` | Bitwise XOR |
| `bit-not` | `(Fn [Int] -> Int)` | Bitwise complement |
| `bit-shift-left` | `(Fn [Int Int] -> Int)` | Left shift |
| `bit-shift-right` | `(Fn [Int Int] -> Int)` | Arithmetic right shift |

#### Constructors
| Function | Signature | Description |
|----------|-----------|-------------|
| `Some` | `(Fn [a] -> (Option a))` | Wrap in Some |
| `None` | `(Option a)` | The None value |
| `Ok` | `(Fn [a] -> (Result a e))` | Wrap in Ok |
| `Err` | `(Fn [e] -> (Result a e))` | Wrap in Err |
| `atom` | `(Fn [a] -> (Atom a))` | **New.** Create mutable atom |
| `deref` | `(Fn [(Atom a)] -> a)` | **New.** Read atom value |
| `swap!` | `(Fn [(Atom a) (Fn [a] -> a)] -> a)` | **New.** Update atom via function |
| `reset!` | `(Fn [(Atom a) a] -> a)` | **New.** Set atom value |

---

### 3.2 `core` — Higher-Order Utilities

Functional programming combinators. Pure, composable, essential.

| Function | Signature | Description |
|----------|-----------|-------------|
| `identity` | `(Fn [a] -> a)` | Return argument unchanged |
| `comp` | `(Fn [(Fn [b] -> c) (Fn [a] -> b)] -> (Fn [a] -> c))` | Compose two functions |
| `comp*` | `(Fn [& (Fn [a] -> a)] -> (Fn [a] -> a))` | **New.** Compose N functions (right to left) |
| `pipe` | `(Fn [a & (Fn [a] -> a)] -> a)` | **New.** Apply functions left to right |
| `partial` | `(Fn [(Fn [a b & c] -> d) a] -> (Fn [b & c] -> d))` | Partially apply first args |
| `constantly` | `(Fn [a] -> (Fn [& Any] -> a))` | Ignore args, return constant |
| `complement` | `(Fn [(Fn [a] -> Bool)] -> (Fn [a] -> Bool))` | **New.** Negate a predicate |
| `juxt` | `(Fn [& (Fn [a] -> b)] -> (Fn [a] -> (Vec b)))` | Apply all fns, collect results |
| `apply` | `(Fn [(Fn [& a] -> b) (Vec a)] -> b)` | Spread args from vector |
| `memoize` | `(Fn [(Fn [a] -> b)] -> (Fn [a] -> b))` | **New.** Cache results by argument |
| `trampoline` | `(Fn [(Fn [] -> a)] -> a)` | **New.** Bounce thunks until non-fn result |
| `tap` | `(Fn [a (Fn [a] -> Any)] -> a)` | **New.** Apply side-effect fn, return original value (subject-first for `->`) |

**Design rationale**: `tap` enables debug logging in pipelines without breaking the chain.
`complement` avoids inline lambdas for negated predicates (e.g., `(filter (complement empty?) xs)`).
`memoize` uses an internal atom — the caching itself is not effect-tracked since the wrapped function's effects are already declared.

**Note**: `if-let` and `when-let` are **language special forms** (spec §4.12) for conditional
destructuring, not library functions. They are always available and work with any refutable pattern,
not just Options.

---

### 3.3 `str` — String Manipulation

Unicode-correct string operations. All operations work on UTF-8 encoded strings.
Grapheme-cluster-aware where noted.

| Function | Signature | Description | Complexity |
|----------|-----------|-------------|------------|
| `split` | `(Fn [Str Str] -> (Vec Str))` | Split by literal separator | O(n) |
| `split-first` | `(Fn [Str Str] -> (Option (Tuple Str Str)))` | **New.** Split at first occurrence | O(n) |
| `split-lines` | `(Fn [Str] -> (Vec Str))` | **New.** Split by newline (\n, \r\n) | O(n) |
| `join` | `(Fn [Str (Vec Str)] -> Str)` | Join with separator | O(n) |
| `trim` | `(Fn [Str] -> Str)` | Trim whitespace both ends | O(n) |
| `trim-start` | `(Fn [Str] -> Str)` | Trim leading whitespace | O(n) |
| `trim-end` | `(Fn [Str] -> Str)` | Trim trailing whitespace | O(n) |
| `upper` | `(Fn [Str] -> Str)` | Uppercase (Unicode) | O(n) |
| `lower` | `(Fn [Str] -> Str)` | Lowercase (Unicode) | O(n) |
| `capitalize` | `(Fn [Str] -> Str)` | **New.** Uppercase first, lowercase rest | O(n) |
| `title` | `(Fn [Str] -> Str)` | **New.** Title Case Each Word | O(n) |
| `starts-with?` | `(Fn [Str Str] -> Bool)` | Prefix test | O(k) |
| `ends-with?` | `(Fn [Str Str] -> Bool)` | Suffix test | O(k) |
| `contains?` | `(Fn [Str Str] -> Bool)` | Substring test | O(n) |
| `replace` | `(Fn [Str Str Str] -> Str)` | Replace all occurrences | O(n) |
| `replace-first` | `(Fn [Str Str Str] -> Str)` | **New.** Replace first occurrence | O(n) |
| `index-of` | `(Fn [Str Str] -> (Option Int))` | First occurrence byte offset | O(n) |
| `last-index-of` | `(Fn [Str Str] -> (Option Int))` | **New.** Last occurrence | O(n) |
| `blank?` | `(Fn [Str] -> Bool)` | Empty or only whitespace | O(n) |
| `chars` | `(Fn [Str] -> (Vec Char))` | Unicode scalar values | O(n) |
| `graphemes` | `(Fn [Str] -> (Vec Str))` | Grapheme clusters | O(n) |
| `format` | `(Fn [Str & Any] -> Str)` | `{}` placeholder replacement | O(n) |
| `pad-start` | `(Fn [Str Int Str] -> Str)` | **New.** Left-pad to width | O(n) |
| `pad-end` | `(Fn [Str Int Str] -> Str)` | **New.** Right-pad to width | O(n) |
| `repeat` | `(Fn [Str Int] -> Str)` | **New.** Repeat string n times | O(n*k) |
| `reverse` | `(Fn [Str] -> Str)` | **New.** Reverse by grapheme clusters | O(n) |
| `byte-count` | `(Fn [Str] -> Int)` | **New.** UTF-8 byte length | O(1) |
| `char-count` | `(Fn [Str] -> Int)` | **New.** Unicode codepoint count | O(n) |
| `grapheme-count` | `(Fn [Str] -> Int)` | **New.** Grapheme cluster count | O(n) |
| `from-chars` | `(Fn [(Vec Char)] -> Str)` | **New.** Build string from chars | O(n) |
| `from-code-points` | `(Fn [(Vec Int)] -> (Option Str))` | **New.** Build from codepoint ints | O(n) |
| `to-bytes` | `(Fn [Str] -> (Vec Int))` | **New.** UTF-8 bytes as ints | O(n) |
| `from-bytes` | `(Fn [(Vec Int)] -> (Option Str))` | **New.** Build from UTF-8 bytes | O(n) |
| `kebab-case` | `(Fn [Str] -> Str)` | **New.** Convert to kebab-case | O(n) |
| `snake-case` | `(Fn [Str] -> Str)` | **New.** Convert to snake_case | O(n) |
| `camel-case` | `(Fn [Str] -> Str)` | **New.** Convert to camelCase | O(n) |

**Design rationale**: Three separate count functions (`byte-count`, `char-count`, `grapheme-count`)
make the cost model explicit. The unqualified `count` on strings counts codepoints (most intuitive),
but users needing performance should use `str/byte-count`. Case conversion functions (`kebab-case`,
`snake-case`, `camel-case`) are common in code generation and API interop — including them avoids
every project reimplementing the same logic.

---

### 3.4 `char` — Character Operations (New)

Operations on Unicode scalar values (the `Char` type).

| Function | Signature | Description |
|----------|-----------|-------------|
| `alpha?` | `(Fn [Char] -> Bool)` | Is alphabetic |
| `digit?` | `(Fn [Char] -> Bool)` | Is ASCII digit (0-9) |
| `alphanumeric?` | `(Fn [Char] -> Bool)` | Is letter or digit |
| `whitespace?` | `(Fn [Char] -> Bool)` | Is whitespace |
| `upper?` | `(Fn [Char] -> Bool)` | Is uppercase |
| `lower?` | `(Fn [Char] -> Bool)` | Is lowercase |
| `to-upper` | `(Fn [Char] -> Char)` | Convert to uppercase |
| `to-lower` | `(Fn [Char] -> Char)` | Convert to lowercase |
| `to-int` | `(Fn [Char] -> Int)` | Unicode codepoint as integer |
| `from-int` | `(Fn [Int] -> (Option Char))` | Codepoint integer to Char |
| `to-str` | `(Fn [Char] -> Str)` | Single-char string |
| `ascii?` | `(Fn [Char] -> Bool)` | Is ASCII (0-127) |
| `control?` | `(Fn [Char] -> Bool)` | Is control character |
| `punctuation?` | `(Fn [Char] -> Bool)` | Is Unicode punctuation |

**Design rationale**: Nexl already has the `Char` type. Providing character classification
functions enables lexer/parser construction and input validation without regex.

---

### 3.5 `math` — Mathematical Functions

Pure numeric operations. No effects.

#### Constants
| Name | Type | Value |
|------|------|-------|
| `pi` | `Float` | 3.141592653589793 |
| `e` | `Float` | 2.718281828459045 |
| `tau` | `Float` | **New.** 6.283185307179586 (2π) |
| `inf` | `Float` | **New.** Positive infinity |
| `neg-inf` | `Float` | **New.** Negative infinity |
| `nan` | `Float` | **New.** Not-a-number |

#### Functions
| Function | Signature | Description |
|----------|-----------|-------------|
| `abs` | `(Fn [Num] -> Num)` | Absolute value |
| `sign` | `(Fn [Num] -> Int)` | **New.** -1, 0, or 1 |
| `floor` | `(Fn [Float] -> Float)` | Round toward −∞ |
| `ceil` | `(Fn [Float] -> Float)` | Round toward +∞ |
| `round` | `(Fn [Float] -> Float)` | Round half-away-from-zero |
| `truncate` | `(Fn [Float] -> Float)` | **New.** Round toward zero |
| `pow` | `(Fn [Num Num] -> Float)` | Exponentiation |
| `sqrt` | `(Fn [Num] -> Float)` | Square root |
| `cbrt` | `(Fn [Num] -> Float)` | **New.** Cube root |
| `log` | `(Fn [Num] -> Float)` | Natural log (ln) |
| `log2` | `(Fn [Num] -> Float)` | **New.** Base-2 log |
| `log10` | `(Fn [Num] -> Float)` | **New.** Base-10 log |
| `exp` | `(Fn [Num] -> Float)` | e^x |
| `exp2` | `(Fn [Num] -> Float)` | **New.** 2^x |
| `sin` | `(Fn [Float] -> Float)` | Sine (radians) |
| `cos` | `(Fn [Float] -> Float)` | Cosine |
| `tan` | `(Fn [Float] -> Float)` | Tangent |
| `asin` | `(Fn [Float] -> Float)` | Arc sine |
| `acos` | `(Fn [Float] -> Float)` | Arc cosine |
| `atan` | `(Fn [Float] -> Float)` | Arc tangent |
| `atan2` | `(Fn [Float Float] -> Float)` | Two-argument arc tangent |
| `sinh` | `(Fn [Float] -> Float)` | **New.** Hyperbolic sine |
| `cosh` | `(Fn [Float] -> Float)` | **New.** Hyperbolic cosine |
| `tanh` | `(Fn [Float] -> Float)` | **New.** Hyperbolic tangent |
| `min` | `(Fn [Ord Ord] -> Ord)` | Minimum of two |
| `max` | `(Fn [Ord Ord] -> Ord)` | Maximum of two |
| `clamp` | `(Fn [Num Num Num] -> Num)` | Restrict to [lo, hi] |
| `nan?` | `(Fn [Float] -> Bool)` | **New.** Is NaN |
| `infinite?` | `(Fn [Float] -> Bool)` | **New.** Is ±∞ |
| `finite?` | `(Fn [Float] -> Bool)` | **New.** Is finite (not NaN or ∞) |
| `gcd` | `(Fn [Int Int] -> Int)` | **New.** Greatest common divisor |
| `lcm` | `(Fn [Int Int] -> Int)` | **New.** Least common multiple |
| `divmod` | `(Fn [Int Int] -> (Tuple Int Int))` | **New.** (quotient, remainder) pair |

---

### 3.6 `conv` — Type Conversions

Explicit conversions between primitive types. Returns `Option` for narrowing conversions.

| Function | Signature | Description |
|----------|-----------|-------------|
| `->int` | `(Fn [Any] -> (Option Int))` | To integer (truncates floats, parses strings) |
| `->float` | `(Fn [Any] -> (Option Float))` | To float (widens ints, parses strings) |
| `->str` | `(Fn [Any] -> Str)` | To string representation |
| `->bool` | `(Fn [Any] -> Bool)` | **New.** Explicit Bool conversion (0/0.0/"" → false, all else → true) |
| `->char` | `(Fn [Int] -> (Option Char))` | **New.** Codepoint to Char (None if invalid) |

---

### 3.7 `vec` — Vector Operations (New)

Vector-specific operations beyond what builtins provide.

| Function | Signature | Description | Complexity |
|----------|-----------|-------------|------------|
| `of` | `(Fn [& a] -> (Vec a))` | Construct from args | O(n) |
| `repeat` | `(Fn [Int a] -> (Vec a))` | N copies of value | O(n) |
| `init` | `(Fn [Int (Fn [Int] -> a)] -> (Vec a))` | Build via index function | O(n) |
| `chunk` | `(Fn [(Vec a) Int] -> (Vec (Vec a)))` | Split into fixed-size chunks | O(n) |
| `window` | `(Fn [(Vec a) Int] -> (Vec (Vec a)))` | Sliding window of size n | O(n*k) |
| `intersperse` | `(Fn [(Vec a) a] -> (Vec a))` | Insert between elements | O(n) |
| `split-at` | `(Fn [(Vec a) Int] -> (Tuple (Vec a) (Vec a)))` | Split at index | O(n) |
| `span` | `(Fn [(Vec a) (Fn [a] -> Bool)] -> (Tuple (Vec a) (Vec a)))` | Split at first false | O(n) |
| `dedup` | `(Fn [(Vec a)] -> (Vec a))` | Remove consecutive duplicates | O(n) |
| `dedup-by` | `(Fn [(Vec a) (Fn [a a] -> Bool)] -> (Vec a))` | Dedup with equality fn | O(n) |
| `rotate-left` | `(Fn [(Vec a) Int] -> (Vec a))` | Rotate left by n positions | O(n) |
| `rotate-right` | `(Fn [(Vec a) Int] -> (Vec a))` | Rotate right by n positions | O(n) |
| `swap` | `(Fn [(Vec a) Int Int] -> (Vec a))` | Swap elements at two indices | O(n) |
| `insert` | `(Fn [(Vec a) Int a] -> (Vec a))` | Insert at index | O(n) |
| `remove-at` | `(Fn [(Vec a) Int] -> (Vec a))` | Remove at index | O(n) |
| `scan` | `(Fn [(Vec a) (Fn [acc a] -> acc) acc] -> (Vec acc))` | Running fold (all intermediates) | O(n) |
| `fold-right` | `(Fn [(Vec a) (Fn [a acc] -> acc) acc] -> acc)` | Fold from the right | O(n) |
| `sum` | `(Fn [(Vec Num)] -> Num)` | Sum all elements | O(n) |
| `product` | `(Fn [(Vec Num)] -> Num)` | Multiply all elements | O(n) |
| `min-by` | `(Fn [(Vec a) (Fn [a] -> Ord)] -> (Option a))` | Minimum by key function | O(n) |
| `max-by` | `(Fn [(Vec a) (Fn [a] -> Ord)] -> (Option a))` | Maximum by key function | O(n) |
| `unzip` | `(Fn [(Vec (Tuple a b))] -> (Tuple (Vec a) (Vec b)))` | Inverse of zip | O(n) |
| `permutations` | `(Fn [(Vec a)] -> (Vec (Vec a)))` | All orderings | O(n!) |
| `combinations` | `(Fn [(Vec a) Int] -> (Vec (Vec a)))` | Choose k from n | O(C(n,k)) |
| `binary-search` | `(Fn [(Vec Ord) Ord] -> (Result Int Int))` | Sorted vec lookup | O(log n) |

**Design rationale**: `chunk`/`window` are essential for batch processing and sliding aggregations.
`scan` (running fold) is needed for cumulative sums, running averages, etc.
`binary-search` returns `Ok(index)` if found, `Err(insertion-point)` if not — matching Rust's API.

---

### 3.8 `map` — Map Operations (New)

Map-specific operations beyond what builtins provide.

| Function | Signature | Description | Complexity |
|----------|-----------|-------------|------------|
| `of` | `(Fn [& (Tuple k v)] -> (Map k v))` | Construct from key-value pairs | O(n) |
| `from-entries` | `(Fn [(Vec (Tuple k v))] -> (Map k v))` | Build from pairs | O(n) |
| `get-or` | `(Fn [(Map k v) k v] -> v)` | Get with default value | O(1) |
| `map-keys` | `(Fn [(Map k v) (Fn [k] -> k2)] -> (Map k2 v))` | Transform keys | O(n) |
| `map-vals` | `(Fn [(Map k v) (Fn [v] -> v2)] -> (Map k v2))` | Transform values | O(n) |
| `filter-keys` | `(Fn [(Map k v) (Fn [k] -> Bool)] -> (Map k v))` | Keep entries by key | O(n) |
| `filter-vals` | `(Fn [(Map k v) (Fn [v] -> Bool)] -> (Map k v))` | Keep entries by value | O(n) |
| `invert` | `(Fn [(Map k v)] -> (Map v k))` | Swap keys and values | O(n) |
| `group-vals` | `(Fn [(Map k v)] -> (Map v (Vec k)))` | Group keys by value | O(n) |
| `reduce-kv` | `(Fn [(Map k v) (Fn [acc k v] -> acc) acc] -> acc)` | Fold over key-value pairs | O(n) |
| `find` | `(Fn [(Map k v) (Fn [k v] -> Bool)] -> (Option (Tuple k v)))` | First matching entry | O(n) |
| `every?` | `(Fn [(Map k v) (Fn [k v] -> Bool)] -> Bool)` | All entries match | O(n) |
| `any?` | `(Fn [(Map k v) (Fn [k v] -> Bool)] -> Bool)` | Any entry matches | O(n) |

**Design rationale**: `map-keys`/`map-vals`/`filter-keys`/`filter-vals` avoid destructuring
in lambdas. `reduce-kv` gives direct access to both key and value. `invert` is a common
operation for building reverse lookup tables. `zipmap` and `rename-keys` live in builtins
because they're used frequently enough to warrant unqualified access.

---

### 3.9 `set` — Set Operations (New)

Set-specific operations beyond what builtins provide.

| Function | Signature | Description |
|----------|-----------|-------------|
| `of` | `(Fn [& a] -> (Set a))` | Construct from elements |
| `from-vec` | `(Fn [(Vec a)] -> (Set a))` | Build from vector (deduplicates) |
| `to-vec` | `(Fn [(Set a)] -> (Vec a))` | Convert to vector |
| `map` | `(Fn [(Set a) (Fn [a] -> b)] -> (Set b))` | Transform elements |
| `filter` | `(Fn [(Set a) (Fn [a] -> Bool)] -> (Set a))` | Keep matching elements |
| `reduce` | `(Fn [(Set a) (Fn [acc a] -> acc) acc] -> acc)` | Fold over elements |
| `every?` | `(Fn [(Set a) (Fn [a] -> Bool)] -> Bool)` | All elements match |
| `any?` | `(Fn [(Set a) (Fn [a] -> Bool)] -> Bool)` | Any element matches |
| `flat-map` | `(Fn [(Set a) (Fn [a] -> (Set b))] -> (Set b))` | Map and flatten |
| `partition` | `(Fn [(Set a) (Fn [a] -> Bool)] -> (Tuple (Set a) (Set a)))` | Split by predicate |
| `product` | `(Fn [(Set a) (Set b)] -> (Set (Tuple a b)))` | Cartesian product |

---

### 3.10 `option` — Option Combinators (New)

Rich operations on `(Option a)`. Inspired by Rust's `Option` methods and Gleam's `result` module.

**Relationship with builtin `map`/`filter`**: The builtin `map` and `filter` are
compiler-dispatched overloads that already work on Option (spec §5.11):

```nexl
(map inc (Some 1))           ;; => (Some 2) — builtin, collection-last
(filter pos? (Some -3))      ;; => None     — builtin, collection-last
```

The `option/*` module provides the same operations with **subject-first** ordering,
plus Option-specific combinators (`unwrap-or`, `flat-map`, `or-else`, etc.) that
have no builtin equivalent. Use builtins for one-off transforms; use `option/*`
when chaining multiple operations in a `->` pipeline.

All functions take the **Option as the first argument** (subject-first, for `->` threading):

```nexl
(-> (find-user id)
    (option/map :name)
    (option/filter (fn [n] (not (str/blank? n))))
    (option/unwrap-or "Anonymous"))
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `some?` | `(Fn [(Option a)] -> Bool)` | Is Some |
| `none?` | `(Fn [(Option a)] -> Bool)` | Is None |
| `unwrap` | `(Fn [(Option a)] -> a)` | Extract or panic |
| `unwrap-or` | `(Fn [(Option a) a] -> a)` | Extract or default |
| `unwrap-or-else` | `(Fn [(Option a) (Fn [] -> a)] -> a)` | Extract or compute default |
| `map` | `(Fn [(Option a) (Fn [a] -> b)] -> (Option b))` | Transform inner value |
| `flat-map` | `(Fn [(Option a) (Fn [a] -> (Option b))] -> (Option b))` | Chain fallible operations |
| `filter` | `(Fn [(Option a) (Fn [a] -> Bool)] -> (Option a))` | Keep if predicate matches |
| `or-else` | `(Fn [(Option a) (Fn [] -> (Option a))] -> (Option a))` | Fallback Option |
| `to-result` | `(Fn [(Option a) e] -> (Result a e))` | Convert to Result with error |
| `from-result` | `(Fn [(Result a e)] -> (Option a))` | Discard error info |
| `zip` | `(Fn [(Option a) (Option b)] -> (Option (Tuple a b)))` | Combine two Options |
| `values` | `(Fn [(Vec (Option a))] -> (Vec a))` | Collect only Some values |

**Design rationale**: `values` is used constantly in practice — filtering None from a list of
optional results. `flat-map` (also called `and-then` or `bind`) is the monadic operation that
enables chaining fallible lookups.

---

### 3.11 `result` — Result Combinators (New)

Rich operations on `(Result a e)`. Mirrors Option combinators where applicable.

**Relationship with builtin `map`**: Like Option, the builtin `map` already works
on Result via compiler-dispatched overloads (spec §5.11):

```nexl
(map inc (Ok 1))             ;; => (Ok 2) — builtin, collection-last
```

The `result/*` module provides subject-first ordering for `->` pipelines,
plus Result-specific operations (`map-err`, `unwrap-or-else`, `flat-map`,
`try-map`, `collect`, etc.) that have no builtin equivalent.

All functions take the **Result as the first argument** (subject-first, for `->` threading):

```nexl
(-> (io/read-file "config.toml")
    (result/map toml/parse)
    (result/map-err (fn [e] (str "config error: " e)))
    (result/unwrap-or default-config))
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `ok?` | `(Fn [(Result a e)] -> Bool)` | Is Ok |
| `err?` | `(Fn [(Result a e)] -> Bool)` | Is Err |
| `unwrap` | `(Fn [(Result a e)] -> a)` | Extract or panic |
| `unwrap-err` | `(Fn [(Result a e)] -> e)` | Extract error or panic |
| `unwrap-or` | `(Fn [(Result a e) a] -> a)` | Extract or default |
| `unwrap-or-else` | `(Fn [(Result a e) (Fn [e] -> a)] -> a)` | Extract or recover |
| `map` | `(Fn [(Result a e) (Fn [a] -> b)] -> (Result b e))` | Transform Ok value |
| `map-err` | `(Fn [(Result a e) (Fn [e] -> e2)] -> (Result a e2))` | Transform Err value |
| `flat-map` | `(Fn [(Result a e) (Fn [a] -> (Result b e))] -> (Result b e))` | Chain fallible ops |
| `or-else` | `(Fn [(Result a e) (Fn [e] -> (Result a e2))] -> (Result a e2))` | Recovery chain |
| `to-option` | `(Fn [(Result a e)] -> (Option a))` | Discard error info |
| `from-option` | `(Fn [(Option a) e] -> (Result a e))` | Add error context |
| `try-map` | `(Fn [(Vec a) (Fn [a] -> (Result b e))] -> (Result (Vec b) e))` | Map, fail on first error |
| `partition` | `(Fn [(Vec (Result a e))] -> (Tuple (Vec a) (Vec e)))` | Split Oks from Errs |
| `collect` | `(Fn [(Vec (Result a e))] -> (Result (Vec a) e))` | All Ok or first Err |

**Design rationale**: `try-map` and `collect` are essential for processing lists where any item
can fail. `partition` is useful when you want to process all items and collect both successes and
failures. These patterns appear in virtually every real-world application.

---

### 3.12 `iter` — Lazy Iteration (New)

Lazy sequences for composable, memory-efficient data processing.
`Iter` is a concrete ADT (not a protocol) defined in the language spec §5.12:

```nexl
(deftype Iter [a]
  | Done
  | (Yield a (Fn [] -> (Iter a))))
```

`Yield` carries the current value and a thunk that produces the next step.
`Done` signals the end of the sequence. All `Foldable` types can produce
an `(Iter a)` via the `iter` function; all `Buildable` types can be
constructed from one via `collect`.

Any type that implements the `Iterable` protocol can be used with `iter/*`
functions. Vec, Map, Set, Str, and Channel implement it out of the box.
See §7.2 for protocol details.

| Function | Signature | Description |
|----------|-----------|-------------|
| `from-vec` | `(Fn [(Vec a)] -> (Iter a))` | Lazy view over vector |
| `from-map` | `(Fn [(Map k v)] -> (Iter (Tuple k v)))` | Lazy view over map entries |
| `to-vec` | `(Fn [(Iter a)] -> (Vec a))` | Materialize to vector |
| `to-map` | `(Fn [(Iter (Tuple k v))] -> (Map k v))` | Materialize to map |
| `to-set` | `(Fn [(Iter a)] -> (Set a))` | Materialize to set |
| `range` | `(Fn [Int Int] -> (Iter Int))` | Lazy integer range |
| `repeat` | `(Fn [a] -> (Iter a))` | Infinite repetition |
| `iterate` | `(Fn [(Fn [a] -> a) a] -> (Iter a))` | Infinite unfolding |
| `unfold` | `(Fn [(Fn [s] -> (Option (Tuple a s))) s] -> (Iter a))` | General unfold |
| `map` | `(Fn [(Fn [a] -> b) (Iter a)] -> (Iter b))` | Lazy map |
| `filter` | `(Fn [(Fn [a] -> Bool) (Iter a)] -> (Iter a))` | Lazy filter |
| `take` | `(Fn [Int (Iter a)] -> (Iter a))` | Lazy take |
| `drop` | `(Fn [Int (Iter a)] -> (Iter a))` | Lazy drop |
| `take-while` | `(Fn [(Fn [a] -> Bool) (Iter a)] -> (Iter a))` | Lazy take-while |
| `drop-while` | `(Fn [(Fn [a] -> Bool) (Iter a)] -> (Iter a))` | Lazy drop-while |
| `flat-map` | `(Fn [(Fn [a] -> (Iter b)) (Iter a)] -> (Iter b))` | Lazy flat-map |
| `chain` | `(Fn [(Iter a) (Iter a)] -> (Iter a))` | Concatenate two iters |
| `zip` | `(Fn [(Iter a) (Iter b)] -> (Iter (Tuple a b)))` | Lazy zip |
| `enumerate` | `(Fn [(Iter a)] -> (Iter (Tuple Int a)))` | Add indices |
| `chunk` | `(Fn [Int (Iter a)] -> (Iter (Vec a)))` | Group into chunks |
| `reduce` | `(Fn [(Fn [acc a] -> acc) acc (Iter a)] -> acc)` | Consume and fold |
| `find` | `(Fn [(Fn [a] -> Bool) (Iter a)] -> (Option a))` | First match |
| `any?` | `(Fn [(Fn [a] -> Bool) (Iter a)] -> Bool)` | Short-circuit any |
| `all?` | `(Fn [(Fn [a] -> Bool) (Iter a)] -> Bool)` | Short-circuit all |
| `count` | `(Fn [(Iter a)] -> Int)` | Count elements (consumes) |
| `nth` | `(Fn [Int (Iter a)] -> (Option a))` | Nth element |
| `empty` | `(Iter a)` | Empty iterator |

#### Eager vs Lazy: When to Use Which

Nexl provides two parallel APIs for collection processing:
- **Eager** (builtins): `map`, `filter`, `reduce`, etc. — operate on concrete collections
- **Lazy** (`iter/*`): `iter/map`, `iter/filter`, etc. — build computation pipelines

They mirror Elixir's `Enum` vs `Stream`, Kotlin's `Collection` vs `Sequence`,
and Clojure's eager (`mapv`, `filterv`) vs lazy (`map`, `filter`) split.

**Use eager (builtins) when:**

```nexl
;; 1. Small/medium collections — no performance concern
(->> users
     (filter active?)
     (map :name)
     (sort))

;; 2. You need the full result immediately
(def names (->> users (map :name)))  ;; names is a Vec right now

;; 3. Simple 1-2 step pipelines — laziness overhead isn't worth it
(filter odd? (range 20))

;; 4. You need collection-specific operations (sort, group-by, frequencies)
;;    that require seeing all elements at once
(->> words (group-by str/lower) (map-vals count))
```

**Use lazy (`iter/*`) when:**

```nexl
;; 1. Large collections with early termination — avoid processing all 1M items
(->> (iter/range 0 1000000)
     (iter/filter even?)
     (iter/map (fn [n] (* n n)))
     (iter/take 10)
     (iter/to-vec))
;; Only computes 10 values, not 1,000,000

;; 2. Infinite sequences — eager would never terminate
(->> (iter/iterate inc 0)        ;; 0, 1, 2, 3, ...
     (iter/filter prime?)
     (iter/take 100)
     (iter/to-vec))
;; First 100 primes — impossible with eager `range`

;; 3. Multi-step pipelines on large data — avoid intermediate allocations
(->> (iter/from-vec huge-log-lines)    ;; don't copy
     (iter/filter (fn [l] (str/contains? l "ERROR")))
     (iter/map parse-log-entry)
     (iter/take-while (fn [e] (> (:timestamp e) cutoff)))
     (iter/to-vec))
;; One pass, no intermediate Vec at each step

;; 4. Generating sequences from computation (unfold, iterate)
(def fibs
  (iter/unfold
    (fn [[a b]] (Some [a [b (+ a b)]]))
    [0 1]))
;; Infinite Fibonacci sequence — pull values as needed
(->> fibs (iter/take 20) (iter/to-vec))

;; 5. Chaining multiple sources
(->> (iter/chain
     (iter/from-vec local-users)
     (iter/from-vec remote-users)
     (iter/filter active?)
     (iter/to-vec))
```

**The rule of thumb:**

| Situation | Use | Why |
|-----------|-----|-----|
| < 10,000 elements, simple pipeline | Eager (`map`, `filter`) | Simpler, no conversion overhead |
| Large data + `take`/`take-while` | Lazy (`iter/*`) | Short-circuits — doesn't process the rest |
| Infinite or generated sequences | Lazy (`iter/*`) | Eager would loop forever |
| 3+ chained transforms on large data | Lazy (`iter/*`) | One pass, no intermediate allocations |
| Need `sort`, `group-by`, `frequencies` | Eager (builtins) | These inherently need all elements |
| Reading lines from a file | Lazy (`iter/*`) | Process line-by-line without loading entire file |

**Converting between them:**

```nexl
;; Vec → Iter (free, just wraps)
(iter/from-vec my-vec)

;; Iter → Vec (materializes, costs O(n) memory)
(iter/to-vec my-iter)

;; Common pattern: go lazy for the pipeline, materialize at the end
(->> (iter/from-vec huge-dataset)
     (iter/filter valid?)
     (iter/map transform)
     (iter/to-vec))            ;; materialize only the final result
```

**What Iter does NOT support** (use eager instead):
- `sort`, `sort-by` — requires seeing all elements (use `iter/to-vec` then `sort`)
- `group-by`, `frequencies` — requires accumulating into a Map
- `reverse` — requires seeing all elements
- Random access (`get`, `nth` is O(n) on Iter)
- `count` on Iter consumes it (you can't use the iter after counting)

**Implementation**: `Iter` is a concrete ADT `| Done | (Yield a (Fn [] -> (Iter a)))` (spec §5.12).
This representation is simple, requires no runtime support beyond closures, and works naturally
with Nexl's existing type system. No special `Iterator` trait needed — `Foldable` and `Buildable`
protocols (spec §5.12) handle the conversion between concrete collections and lazy Iter.

---

### 3.13 `io` — File I/O and Console

All operations in this module perform effects: `Console` or `FileSystem`.

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `println` | `(Fn [& Any] -> Unit)` | Console | Print with newline |
| `print` | `(Fn [& Any] -> Unit)` | Console | Print without newline |
| `eprintln` | `(Fn [& Any] -> Unit)` | Console | **New.** Print to stderr |
| `eprint` | `(Fn [& Any] -> Unit)` | Console | **New.** Print to stderr (no newline) |
| `read-line` | `(Fn [] -> (Result Str Str))` | Console | Read line from stdin |
| `read-file` | `(Fn [Str] -> (Result Str Str))` | FileSystem | Read file as UTF-8 |
| `read-bytes` | `(Fn [Str] -> (Result (Vec Int) Str))` | FileSystem | **New.** Read file as bytes |
| `write-file` | `(Fn [Str Str] -> (Result Unit Str))` | FileSystem | Write string to file |
| `write-bytes` | `(Fn [Str (Vec Int)] -> (Result Unit Str))` | FileSystem | **New.** Write bytes to file |
| `append-file` | `(Fn [Str Str] -> (Result Unit Str))` | FileSystem | **New.** Append to file |
| `delete-file` | `(Fn [Str] -> (Result Unit Str))` | FileSystem | Delete file |
| `copy-file` | `(Fn [Str Str] -> (Result Unit Str))` | FileSystem | **New.** Copy file |
| `move-file` | `(Fn [Str Str] -> (Result Unit Str))` | FileSystem | **New.** Move/rename file |
| `file-exists?` | `(Fn [Str] -> Bool)` | FileSystem | Check existence |
| `dir?` | `(Fn [Str] -> Bool)` | FileSystem | **New.** Is directory |
| `file?` | `(Fn [Str] -> Bool)` | FileSystem | **New.** Is regular file |
| `read-dir` | `(Fn [Str] -> (Result (Vec Str) Str))` | FileSystem | List directory |
| `create-dir` | `(Fn [Str] -> (Result Unit Str))` | FileSystem | **New.** Create single directory |
| `create-dir-all` | `(Fn [Str] -> (Result Unit Str))` | FileSystem | Create directory tree |
| `temp-dir` | `(Fn [] -> Str)` | FileSystem | **New.** OS temporary directory |

---

### 3.14 `path` — Path Manipulation (New)

Cross-platform path operations. Pure (no effects) — operates on strings as paths.

| Function | Signature | Description |
|----------|-----------|-------------|
| `join` | `(Fn [& Str] -> Str)` | Join components with OS separator |
| `parent` | `(Fn [Str] -> (Option Str))` | Parent directory |
| `file-name` | `(Fn [Str] -> (Option Str))` | File name with extension |
| `stem` | `(Fn [Str] -> (Option Str))` | File name without extension |
| `extension` | `(Fn [Str] -> (Option Str))` | File extension (without dot) |
| `with-extension` | `(Fn [Str Str] -> Str)` | Replace extension |
| `normalize` | `(Fn [Str] -> Str)` | Normalize path (resolve `.`, `..`) |
| `absolute?` | `(Fn [Str] -> Bool)` | Is absolute path |
| `relative?` | `(Fn [Str] -> Bool)` | Is relative path |
| `separator` | `Str` | OS path separator (`/` or `\\`) |
| `components` | `(Fn [Str] -> (Vec Str))` | Split into components |
| `relative-to` | `(Fn [Str Str] -> (Option Str))` | Make relative to base |
| `starts-with?` | `(Fn [Str Str] -> Bool)` | Path prefix test |

**Design rationale**: Path operations are the most common source of platform-specific bugs.
Providing a dedicated module with cross-platform semantics (using the OS separator) prevents
string concatenation with hardcoded `/`. Inspired by Python's `pathlib` and Rust's `std::path`.

---

### 3.15 `json` — JSON (Keep, minor additions)

| Function | Signature | Description |
|----------|-----------|-------------|
| `encode` | `(Fn [Any] -> Str)` | Compact JSON |
| `decode` | `(Fn [Str] -> (Result Any Str))` | Parse JSON |
| `pretty` | `(Fn [Any] -> Str)` | Indented JSON (2-space) |
| `encode-sorted` | `(Fn [Any] -> Str)` | **New.** Compact JSON with sorted keys (deterministic) |

---

### 3.16 `csv` — CSV Parsing/Writing (New)

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `parse` | `(Fn [Str] -> (Result (Vec (Vec Str)) Str))` | — | Parse CSV string |
| `parse-with-headers` | `(Fn [Str] -> (Result (Vec (Map Keyword Str)) Str))` | — | Parse with header row |
| `encode` | `(Fn [(Vec (Vec Str))] -> Str)` | — | Encode to CSV string |
| `encode-with-headers` | `(Fn [(Vec Keyword) (Vec (Map Keyword Str))] -> Str)` | — | Encode with header row |

**Design rationale**: CSV is ubiquitous for data exchange. Including it in stdlib avoids
the "everyone reimplements CSV parsing incorrectly" problem. Header-aware parsing returns
maps keyed by keyword (matching Nexl's record/map idiom).

---

### 3.17 `toml` — TOML Parsing/Writing (New)

| Function | Signature | Description |
|----------|-----------|-------------|
| `parse` | `(Fn [Str] -> (Result Any Str))` | Parse TOML string to Nexl values |
| `encode` | `(Fn [Any] -> (Result Str Str))` | Encode Nexl values to TOML |
| `pretty` | `(Fn [Any] -> (Result Str Str))` | Encode with formatting |

**Design rationale**: TOML is Nexl's configuration format (project.nx is TOML-like).
Providing native TOML support avoids external dependencies for configuration files.

---

### 3.18 `http` — HTTP Client/Server (Enhance)

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `get` | `(Fn [Str] -> (Result Response Str))` | Net | HTTP GET |
| `post` | `(Fn [Str Str (Map Str Str)] -> (Result Response Str))` | Net | HTTP POST |
| `put` | `(Fn [Str Str (Map Str Str)] -> (Result Response Str))` | Net | **New.** HTTP PUT |
| `patch` | `(Fn [Str Str (Map Str Str)] -> (Result Response Str))` | Net | **New.** HTTP PATCH |
| `delete` | `(Fn [Str] -> (Result Response Str))` | Net | **New.** HTTP DELETE |
| `head` | `(Fn [Str] -> (Result Response Str))` | Net | **New.** HTTP HEAD |
| `request` | `(Fn [Request] -> (Result Response Str))` | Net | **New.** Generic request |
| `response` | `(Fn [Int Str (Map Str Str)] -> Response)` | — | Construct response |
| `status` | `(Fn [Response] -> Int)` | — | Extract status code |
| `body` | `(Fn [Response] -> Str)` | — | Extract body |
| `headers` | `(Fn [Response] -> (Map Str Str))` | — | Extract headers |
| `header` | `(Fn [Response Str] -> (Option Str))` | — | **New.** Get single header |
| `ok?` | `(Fn [Response] -> Bool)` | — | **New.** Status 200-299 |
| `serve` | `(Fn [Int (Fn [Request] -> Response)] -> Unit)` | Net | HTTP server |

**Types**:
```nexl
;; Request = {:method Str :url Str :headers (Map Str Str) :body Str}
;; Response = {:status Int :body Str :headers (Map Str Str)}
```

---

### 3.19 `uri` — URI Parsing (New)

| Function | Signature | Description |
|----------|-----------|-------------|
| `parse` | `(Fn [Str] -> (Result Uri Str))` | Parse URI string |
| `to-str` | `(Fn [Uri] -> Str)` | Render URI to string |
| `scheme` | `(Fn [Uri] -> (Option Str))` | Extract scheme (http, https) |
| `host` | `(Fn [Uri] -> (Option Str))` | Extract host |
| `port` | `(Fn [Uri] -> (Option Int))` | Extract port |
| `path` | `(Fn [Uri] -> Str)` | Extract path |
| `query` | `(Fn [Uri] -> (Option Str))` | Extract raw query string |
| `query-params` | `(Fn [Uri] -> (Map Str Str))` | Parse query parameters |
| `fragment` | `(Fn [Uri] -> (Option Str))` | Extract fragment |
| `encode` | `(Fn [Str] -> Str)` | Percent-encode |
| `decode` | `(Fn [Str] -> (Result Str Str))` | Percent-decode |

---

### 3.20 `db` — SQLite (Keep)

No changes needed — the current API is clean and sufficient.

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `open` | `(Fn [Str] -> (Result Db Str))` | Database | Open database |
| `close` | `(Fn [Db] -> Unit)` | Database | Close database |
| `execute` | `(Fn [Db Str (Vec Any)] -> (Result Int Str))` | Database | Execute DDL/DML |
| `query` | `(Fn [Db Str (Vec Any)] -> (Result (Vec (Map Keyword Any)) Str))` | Database | Execute SELECT |
| `transaction` | `(Fn [Db (Fn [] -> a)] -> (Result a Str))` | Database | **New.** Run in transaction |

---

### 3.21 `env` — Environment Variables (Keep)

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `get` | `(Fn [Str] -> (Option Str))` | Env | Get variable |
| `require` | `(Fn [Str] -> Str)` | Env | Get or panic |
| `all` | `(Fn [] -> (Map Keyword Str))` | Env | All variables |
| `load-dotenv` | `(Fn [] -> Unit)` | Env, FileSystem | Load .env file |
| `set` | `(Fn [Str Str] -> Unit)` | Env | **New.** Set variable |

---

### 3.22 `time` — Time and Duration (Enhance)

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `now` | `(Fn [] -> Int)` | Time | Unix milliseconds |
| `monotonic` | `(Fn [] -> Int)` | Time | Monotonic nanoseconds |
| `since` | `(Fn [Int] -> Int)` | Time | **New.** Millis since timestamp |
| `elapsed` | `(Fn [Int] -> Int)` | Time | **New.** Nanos since monotonic timestamp |
| `to-iso` | `(Fn [Int] -> Str)` | — | **New.** Unix ms → ISO 8601 string |
| `from-iso` | `(Fn [Str] -> (Result Int Str))` | — | **New.** ISO 8601 → Unix ms |
| `year` | `(Fn [Int] -> Int)` | — | **New.** Extract year from Unix ms |
| `month` | `(Fn [Int] -> Int)` | — | **New.** Extract month (1-12) |
| `day` | `(Fn [Int] -> Int)` | — | **New.** Extract day (1-31) |
| `hour` | `(Fn [Int] -> Int)` | — | **New.** Extract hour (0-23) |
| `minute` | `(Fn [Int] -> Int)` | — | **New.** Extract minute (0-59) |
| `second` | `(Fn [Int] -> Int)` | — | **New.** Extract second (0-59) |
| `day-of-week` | `(Fn [Int] -> Int)` | — | **New.** Day of week (0=Sun, 6=Sat) |
| `format` | `(Fn [Str Int] -> Str)` | — | **New.** Format with pattern |
| `parse` | `(Fn [Str Str] -> (Result Int Str))` | — | **New.** Parse with pattern |
| `millis` | `(Fn [Int] -> Int)` | — | Duration in millis (identity, documentation) |
| `seconds` | `(Fn [Int] -> Int)` | — | **New.** Seconds → millis |
| `minutes` | `(Fn [Int] -> Int)` | — | **New.** Minutes → millis |
| `hours` | `(Fn [Int] -> Int)` | — | **New.** Hours → millis |
| `days` | `(Fn [Int] -> Int)` | — | **New.** Days → millis |

**Design rationale**: Time is represented as Unix milliseconds (Int). No opaque DateTime type —
this matches Nexl's "data over objects" philosophy. Duration helpers (`seconds`, `minutes`, etc.)
are simple multipliers that make code self-documenting:
`(time/since (- (time/now) (time/hours 24)))`.

Date extraction functions use the proleptic Gregorian calendar.

---

### 3.23 `random` — Random Number Generation (Enhance)

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `bytes` | `(Fn [Int] -> (Vec Int))` | Random | N cryptographic random bytes |
| `int` | `(Fn [Int Int] -> Int)` | Random | **New.** Random int in [lo, hi) |
| `float` | `(Fn [] -> Float)` | Random | **New.** Random float in [0.0, 1.0) |
| `bool` | `(Fn [] -> Bool)` | Random | **New.** Random boolean |
| `choice` | `(Fn [(Vec a)] -> (Option a))` | Random | **New.** Random element |
| `shuffle` | `(Fn [(Vec a)] -> (Vec a))` | Random | **New.** Fisher-Yates shuffle |
| `sample` | `(Fn [Int (Vec a)] -> (Vec a))` | Random | **New.** N random elements (no repeats) |
| `uuid` | `(Fn [] -> Str)` | Random | **New.** UUID v4 string |
| `weighted-choice` | `(Fn [(Vec (Tuple a Float))] -> (Option a))` | Random | **New.** Weighted random selection |

**Design rationale**: All randomness is cryptographically sourced via `getrandom` crate.
For non-crypto PRNG (deterministic seeds), use `gen/` from the test module.

---

### 3.24 `crypto` — Cryptographic Operations (Enhance)

| Function | Signature | Description |
|----------|-----------|-------------|
| `sha256` | `(Fn [Str] -> Str)` | **New.** SHA-256 hex digest |
| `sha256-bytes` | `(Fn [(Vec Int)] -> (Vec Int))` | **New.** SHA-256 raw bytes |
| `sha512` | `(Fn [Str] -> Str)` | **New.** SHA-512 hex digest |
| `hmac-sha256` | `(Fn [Str Str] -> Str)` | **New.** HMAC-SHA256 hex digest |
| `blake3` | `(Fn [Str] -> Str)` | **New.** BLAKE3 hex digest |
| `constant-time=` | `(Fn [Str Str] -> Bool)` | Timing-safe comparison |
| `hash` | `(Fn [Str] -> Int)` | General-purpose hash (non-crypto) |
| `random-bytes` | `(Fn [Int] -> (Vec Int))` | **New.** Alias for random/bytes |
| `pbkdf2` | `(Fn [Str Str Int] -> Str)` | **New.** Password hash (salt, iterations) |
| `verify-pbkdf2` | `(Fn [Str Str Str Int] -> Bool)` | **New.** Verify password hash |
| `hex-encode` | `(Fn [(Vec Int)] -> Str)` | **New.** Bytes to hex string |
| `hex-decode` | `(Fn [Str] -> (Result (Vec Int) Str))` | **New.** Hex string to bytes |

**Design rationale**: SHA-256 and HMAC are required by virtually every web application
for JWT verification, webhook signatures, content hashing, etc. Including them in stdlib
avoids the "everyone depends on the same crypto crate" problem. BLAKE3 is included as a
modern, fast alternative. PBKDF2 covers password hashing without requiring bcrypt/argon2
dependencies.

---

### 3.25 `regex` — Regular Expressions (New)

| Function | Signature | Description |
|----------|-----------|-------------|
| `new` | `(Fn [Str] -> (Result Regex Str))` | Compile regex pattern |
| `matches?` | `(Fn [Regex Str] -> Bool)` | Full string match |
| `find` | `(Fn [Regex Str] -> (Option Match))` | First match |
| `find-all` | `(Fn [Regex Str] -> (Vec Match))` | All non-overlapping matches |
| `replace` | `(Fn [Regex Str Str] -> Str)` | Replace all matches |
| `replace-first` | `(Fn [Regex Str Str] -> Str)` | Replace first match |
| `split` | `(Fn [Regex Str] -> (Vec Str))` | Split by pattern |
| `captures` | `(Fn [Regex Str] -> (Option (Vec (Option Str))))` | Named/numbered capture groups |
| `escape` | `(Fn [Str] -> Str)` | Escape special regex chars |

**Types**:
```nexl
;; Match = {:start Int :end Int :text Str}
;; Regex is an opaque compiled pattern
```

**Literal syntax**: `#"pattern"` is syntactic sugar for `(regex/new "pattern")`.
No double-escaping needed: `#"\d+"` instead of `"\\d+"`. See §7.1 for details.

```nexl
;; All equivalent:
(regex/find-all #"\d+" "abc123def456")
(regex/find-all (regex/new "\\d+") "abc123def456")
```

**Design rationale**: Regex is a fundamental tool for text processing, input validation,
and parsing. Compiling to an opaque `Regex` type prevents repeated recompilation.
The `regex` crate (Rust) provides excellent Unicode support and guaranteed linear-time
matching (no catastrophic backtracking). The error type is `RegexError` (see §7.3).

---

### 3.26 `base64` — Base64 Encoding (New)

| Function | Signature | Description |
|----------|-----------|-------------|
| `encode` | `(Fn [Str] -> Str)` | Standard base64 encode |
| `decode` | `(Fn [Str] -> (Result Str Str))` | Standard base64 decode |
| `encode-url` | `(Fn [Str] -> Str)` | URL-safe base64 encode |
| `decode-url` | `(Fn [Str] -> (Result Str Str))` | URL-safe base64 decode |

---

### 3.27 `uuid` — UUID Generation (New)

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `v4` | `(Fn [] -> Str)` | Random | Random UUID |
| `v7` | `(Fn [] -> Str)` | Random, Time | **New.** Time-ordered UUID (sortable) |
| `parse` | `(Fn [Str] -> (Result Uuid Str))` | — | Parse UUID string |
| `to-str` | `(Fn [Uuid] -> Str)` | — | Render as standard format |
| `nil` | `Str` | — | The nil UUID (all zeros) |

**Design rationale**: UUID v4 (random) and v7 (time-sortable) cover the vast majority of
use cases. v7 is preferred for database primary keys (sorted, no index fragmentation).

---

### 3.28 `log` — Structured Logging (Keep)

No changes — the current design is solid.

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `debug` | `(Fn [Str] -> Unit)` | Console | Log at DEBUG |
| `info` | `(Fn [Str] -> Unit)` | Console | Log at INFO |
| `warn` | `(Fn [Str] -> Unit)` | Console | Log at WARN |
| `error` | `(Fn [Str] -> Unit)` | Console | Log at ERROR |
| `with` | `(Fn [(Map Keyword Any) (Fn [] -> a)] -> a)` | Console | Log with context fields |
| `set-level` | `(Fn [Str] -> Unit)` | Console | Set minimum level |
| `with-logger` | `(Fn [(Fn [Map] -> Unit) (Fn [] -> a)] -> a)` | Console | **New.** Custom log sink |
| `context` | `(Fn [] -> (Map Keyword Any))` | — | **New.** Current context fields |

---

### 3.29 `channel` — CSP-Style Channels (New)

Communicating Sequential Processes — the concurrency primitive.

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `new` | `(Fn [] -> (Channel a))` | — | Unbuffered channel |
| `buffered` | `(Fn [Int] -> (Channel a))` | — | Buffered channel |
| `send!` | `(Fn [(Channel a) a] -> Unit)` | — | Send value (blocks if full) |
| `recv!` | `(Fn [(Channel a)] -> (Option a))` | — | Receive value (blocks if empty) |
| `try-send!` | `(Fn [(Channel a) a] -> Bool)` | — | Non-blocking send |
| `try-recv!` | `(Fn [(Channel a)] -> (Option a))` | — | Non-blocking receive |
| `close!` | `(Fn [(Channel a)] -> Unit)` | — | Close channel |
| `closed?` | `(Fn [(Channel a)] -> Bool)` | — | Is channel closed |
| `select!` | Special form | — | Wait on multiple channels |
| `into-iter` | `(Fn [(Channel a)] -> (Iter a))` | — | Consume channel as lazy iter |

**Design rationale**: CSP channels (Go-style) are the safest concurrency primitive for
a language targeting WASM. They compose naturally with the effect system — channel operations
can be effect-tracked. `select!` is a special form because it needs compile-time support
for multiple channel patterns.

**Note**: Full implementation requires WASI threads or the component model async proposal.
In Stage 0, channels operate on OS threads via `std::sync::mpsc`.

---

### 3.30 `bit` — Bitwise Module Form (New)

Module-qualified bitwise operations with additional utilities.

| Function | Signature | Description |
|----------|-----------|-------------|
| `and` | `(Fn [Int Int] -> Int)` | Bitwise AND |
| `or` | `(Fn [Int Int] -> Int)` | Bitwise OR |
| `xor` | `(Fn [Int Int] -> Int)` | Bitwise XOR |
| `not` | `(Fn [Int] -> Int)` | Bitwise complement |
| `shift-left` | `(Fn [Int Int] -> Int)` | Left shift |
| `shift-right` | `(Fn [Int Int] -> Int)` | Arithmetic right shift |
| `count-ones` | `(Fn [Int] -> Int)` | Population count (Hamming weight) |
| `count-zeros` | `(Fn [Int] -> Int)` | Count zero bits |
| `leading-zeros` | `(Fn [Int] -> Int)` | Leading zero count |
| `trailing-zeros` | `(Fn [Int] -> Int)` | Trailing zero count |
| `rotate-left` | `(Fn [Int Int] -> Int)` | Bitwise rotate left |
| `rotate-right` | `(Fn [Int Int] -> Int)` | Bitwise rotate right |
| `test` | `(Fn [Int Int] -> Bool)` | Test bit at position |
| `set` | `(Fn [Int Int] -> Int)` | Set bit at position |
| `clear` | `(Fn [Int Int] -> Int)` | Clear bit at position |
| `toggle` | `(Fn [Int Int] -> Int)` | Toggle bit at position |

---

### 3.31 `sys` — System Interface (Enhance)

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `args` | `(Fn [] -> (Vec Str))` | — | Command-line arguments |
| `exit` | `(Fn [Int] -> Never)` | Process | Exit with status code |
| `os` | `(Fn [] -> Str)` | — | **New.** Operating system name |
| `arch` | `(Fn [] -> Str)` | — | **New.** CPU architecture |
| `cpu-count` | `(Fn [] -> Int)` | — | **New.** Number of CPUs |
| `cwd` | `(Fn [] -> Str)` | FileSystem | **New.** Current working directory |
| `home-dir` | `(Fn [] -> (Option Str))` | Env | **New.** User home directory |
| `exe-path` | `(Fn [] -> (Option Str))` | FileSystem | **New.** Path to current executable |

---

### 3.32 `process` — Child Processes (New)

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `run` | `(Fn [Str (Vec Str)] -> (Result Output Str))` | Process | Run and wait for completion |
| `run-with` | `(Fn [ProcessOpts] -> (Result Output Str))` | Process | Run with options |
| `spawn` | `(Fn [Str (Vec Str)] -> (Result Process Str))` | Process | Start without waiting |
| `wait` | `(Fn [Process] -> (Result Output Str))` | Process | Wait for spawned process |
| `kill` | `(Fn [Process] -> (Result Unit Str))` | Process | Kill spawned process |
| `stdin-write` | `(Fn [Process Str] -> (Result Unit Str))` | Process | Write to stdin |
| `pid` | `(Fn [Process] -> Int)` | — | Process ID |

**Types**:
```nexl
;; Output = {:exit-code Int :stdout Str :stderr Str}
;; ProcessOpts = {:cmd Str :args (Vec Str) :cwd (Option Str) :env (Option (Map Str Str)) :stdin (Option Str)}
```

---

### 3.33 `async` — Concurrency Primitives (Enhance)

| Function | Signature | Effect | Description |
|----------|-----------|--------|-------------|
| `sleep` | `(Fn [Int] -> Unit)` | Time | Sleep for N milliseconds |
| `spawn` | `(Fn [(Fn [] -> a)] -> (Future a))` | — | **New.** Run function concurrently |
| `await` | `(Fn [(Future a)] -> a)` | — | **New.** Block until future completes |
| `timeout` | `(Fn [Int (Fn [] -> a)] -> (Result a Str))` | Time | **New.** Run with timeout |
| `all` | `(Fn [(Vec (Future a))] -> (Vec a))` | — | **New.** Wait for all futures |
| `race` | `(Fn [(Vec (Future a))] -> a)` | — | **New.** Wait for first to complete |
| `defer` | `(Fn [(Fn [] -> Unit) (Fn [] -> a)] -> a)` | — | **New.** Guaranteed cleanup |

**Note**: `spawn`/`await`/`all`/`race` use OS threads in Stage 0. In WASM Component Model,
these map to `wasi:io/poll` and async lifting/lowering. The effect system tracks concurrent
effects through `Future` boundaries.

---

### 3.34 `test` — Testing Framework (Keep)

The M26 test framework is comprehensive. No changes needed for the stdlib spec.

---

### 3.35 `gen` — Property Test Generators (Keep)

The M26 generator system is sufficient.

---

## 4. Cross-Cutting Design Decisions

### 4.1 Parameter Order Summary

See §1.1 for the full threading convention. The summary:

| Category | Arg order | Threading | Examples |
|----------|-----------|-----------|----------|
| **Thing-operations** | subject first | `->` | `assoc`, `update`, `merge`, `str/trim`, `option/map`, `result/flat-map` |
| **Sequence-operations** | collection last | `->>` | `map`, `filter`, `reduce`, `sort-by`, `group-by`, `take`, `drop` |
| **Accessors** (1-arg) | subject only | either | `count`, `empty?`, `keys`, `vals`, `first`, `last` |
| **Constructors** | args only | N/A | `Some`, `Ok`, `Err`, `range`, `vec/of` |

**The rule**: "Am I transforming **this** thing?" → subject-first, use `->`.
"Am I processing **these** items?" → collection-last, use `->>`.

```nexl
;; -> for map manipulation (subject-first)
(-> response
    (assoc :cached true)
    (update :body json/decode))

;; ->> for sequence processing (collection-last)
(->> users
     (filter active?)
     (map :name)
     (sort)
     (take 10))

;; as-> when crossing the boundary
(as-> raw $
  (json/decode $)            ;; subject-first
  (result/unwrap $)
  (filter :active $)          ;; collection-last
  (map :email $))
```

### 4.2 Builtin Overloads vs Module Functions

The builtin `map`, `filter`, and `reduce` are **compiler-dispatched overloads** (spec §5.11)
that work polymorphically across Vec, Map, Set, List, Option, and Result:

```nexl
(map inc [1 2 3])        ;; => [2 3 4]   — Vec
(map inc (Some 1))       ;; => (Some 2)  — Option
(map inc (Ok 1))         ;; => (Ok 2)    — Result
(filter even? [1 2 3])   ;; => [2 4]     — Vec
(filter pos? (Some -3))  ;; => None      — Option
```

These builtins use **collection-last** ordering (for `->>` threading). The `option/*`,
`result/*`, `vec/*`, `map/*`, and `set/*` modules provide the same core operations
with **subject-first** ordering (for `->` threading), plus type-specific combinators
that have no builtin equivalent:

```nexl
;; Builtin: good for one-off transforms in ->> pipelines
(->> items (map :price) (filter pos?) (reduce + 0))

;; Module: good for -> pipelines chaining type-specific operations
(-> (find-user id)
    (option/map :name)
    (option/flat-map validate-name)   ;; no builtin equivalent
    (option/unwrap-or "Anonymous"))   ;; no builtin equivalent
```

**Rule**: Use builtins for quick transforms and `->>` pipelines. Use module-qualified
versions when chaining multiple operations on a specific type in a `->` pipeline,
or when you need type-specific operations like `result/map-err`, `option/flat-map`,
`vec/chunk`, etc.

### 4.3 Collection Return Type Preservation

When you `map` a Vec, you get a Vec. When you `filter` a Map, you get a Map.
When you `map` a Set, you get a Set. The operation preserves the container type.

**Exception**: Operations that change the structure (e.g., `group-by` on a Vec returns a Map)
clearly document the return type.

### 4.4 Nil Safety

Nexl has no null/nil. Every function that might not return a value uses `Option`.
Every function that might fail uses `Result`. This is non-negotiable.

| Instead of | Use |
|-----------|-----|
| Returning null | Return `(Option a)` |
| Throwing exception | Return `(Result a e)` |
| Sentinel value (-1, "") | Return `(Option a)` |
| Panic | Reserve for programmer errors only |

### 4.5 String Representation

All strings are UTF-8. The stdlib provides three levels of string inspection:

| Level | Unit | Use case | Function |
|-------|------|----------|----------|
| Byte | `Int` (0-255) | Binary protocols, hashing | `str/to-bytes`, `str/byte-count` |
| Codepoint | `Char` | Language processing, Unicode algorithms | `str/chars`, `str/char-count` |
| Grapheme | `Str` | User-visible text, display | `str/graphemes`, `str/grapheme-count` |

The unqualified `count` on strings returns **codepoint count** (most intuitive).
The unqualified `get` on strings returns **the nth codepoint as Char**.

### 4.6 Numeric Tower

```
Int ──→ Float (widening, lossless for small values)
Int ──→ Ratio (exact)
Ratio ──→ Float (lossy)
```

Cross-type arithmetic between `Int` and `Float` produces `Float`.
Cross-type arithmetic between fixed-width types (`Int32`, `U8`, etc.) is a **compile error**.
Use explicit `conv/->int` or `conv/->float` for conversions.

### 4.7 Structured Error Types

Every stdlib module that can fail defines a **module-specific error ADT** (see §7.3
for the full list). This enables pattern matching on specific failure modes:

```nexl
(match (io/read-file "config.toml")
  (Ok contents)                 (toml/parse contents)
  (Err (NotFound _))            (Ok default-config)       ;; handle specifically
  (Err (PermissionDenied info)) (panic (str "access denied: " (:path info))))
```

Every error ADT variant includes a `:message` field, so generic error handling
still works without knowing the specific error type:

```nexl
;; Generic — works with any stdlib error
(match (json/decode input)
  (Ok val) val
  (Err e)  (log/error (:message e)))
```

Application code may define its own error ADTs following the same pattern,
or use `Str` errors for simplicity.

---

## 5. Comparison to Other Languages

### What We Take From Each

| Language | What We Adopt | Why |
|----------|--------------|-----|
| **Clojure** | Persistent collections, `->` / `->>` split convention, naming (`?`, `!`) | Nexl is a Lisp — subject-first for things, collection-last for sequences |
| **Elixir** | Enum comprehensiveness, protocol pattern, pipe-chain aesthetics | Rich collection operations, but we keep Clojure's two-macro split over data-first-everywhere |
| **Rust** | Option/Result combinators, iterator chains, Big-O documentation | Type-safe error handling is non-negotiable |
| **Kotlin** | Comprehensive collection operations, `scan`, destination variants | "There's always a function for what you need" |
| **Go** | Batteries-included (HTTP, JSON, testing, crypto, logging) | Nexl programs should not need external deps for common tasks |
| **Python** | `itertools` patterns, `pathlib` design, `csv`/`json` in stdlib | Practical standard library for real-world data processing |
| **Gleam** | Result-based error handling, grapheme-aware strings, clean module design | Modern FP language with excellent API taste |
| **Roc** | Pure stdlib core, effects separated from data operations | Effect tracking means I/O is explicit — lean into it |
| **Zig** | Performance documentation, explicit resource management | Know what your code costs |
| **Swift** | Protocol-based collection hierarchy, progressive disclosure | Simple things simple, complex things possible |

### What We Deliberately Avoid

| Anti-pattern | Source | Why we skip it |
|-------------|--------|----------------|
| 700-function core namespace | Clojure | Overwhelming; use modules |
| Seq coercion (map on Map returns Seq) | Clojure | Loses type; we preserve container |
| Dead batteries | Python | Only include what we'll maintain |
| Too-lean stdlib | Rust | We include HTTP, JSON, crypto, logging |
| `if err != nil` verbosity | Go | `?` operator + Result combinators |
| Mutable-by-default collections | Most languages | Persistent by default |
| Multiple string types | Rust | One `Str` type (UTF-8) |
| Implicit numeric conversions | C, JavaScript | Cross-type arithmetic is a compile error |

---

## 6. Implementation Phases

### Phase 1 — Core Enrichment (Immediate)
Enhance existing modules with missing essentials:
- `empty?`, `not=`, `find`, `any?`, `all?`, `none?`, `distinct`, `flatten`, `partition`, `frequencies`
- `option/*` and `result/*` combinator modules
- `str/` additions (pad, repeat, capitalize, split-lines, case conversion)
- `math/` additions (sign, log2, log10, gcd, lcm, nan?, infinite?)
- `core/` additions (complement, tap, memoize, pipe)

### Phase 2 — New Essential Modules
- `vec/*`, `map/*`, `set/*` — dedicated collection modules
- `char/*` — character classification
- `path/*` — cross-platform paths
- `regex/*` — regular expressions
- `base64/*` — encoding
- `iter/*` — lazy iteration

### Phase 3 — Production Stack
- `crypto/*` — SHA-256, HMAC, PBKDF2, BLAKE3
- `http/*` — full HTTP verb support
- `uri/*` — URI parsing
- `csv/*`, `toml/*` — data formats
- `time/*` — date/time extraction and formatting
- `random/*` — full random number generation
- `uuid/*` — UUID generation

### Phase 4 — Concurrency
- `channel/*` — CSP channels
- `async/*` — futures, spawn, timeout
- `process/*` — child processes

---

## 7. Design Decisions (Resolved)

1. ~~**Should `map`/`filter` take function-first or collection-first?**~~
   **RESOLVED**: Keep function-first `(map f coll)` (Clojure convention). Sequence operations
   use `->>` (thread-last); thing-operations use `->` (thread-first). See §1.1.

2. ~~**Should we include a `regex` literal syntax?**~~
   **RESOLVED: Yes.** Add `#"pattern"` as syntactic sugar for `(regex/new "pattern")`.
   Regex is common enough that the literal form is worth the syntax cost. The literal
   compiles the pattern at read time (constant patterns) or at first use (dynamic).
   See §7.1 below for details.

3. ~~**Should `iter` be a protocol or a type?**~~
   **RESOLVED: Protocol.** `Iterable` is a protocol that any type can implement,
   inspired by Rust's `IntoIterator`. This allows user-defined types, channels,
   file readers, and future collection types to participate in lazy iteration
   without modifying their definitions. See §7.2 below for details.

4. ~~**Should error types be strings or structured?**~~
   **RESOLVED: Structured.** Each stdlib module defines its own error ADT with
   relevant context fields. This enables programmatic error inspection (e.g.,
   `JsonError` with `:line` and `:column` fields) and pattern matching on
   specific failure modes. See §7.3 below for details.

5. ~~**Where should `atom`/`deref`/`swap!`/`reset!` live?**~~
   **RESOLVED: Both.** Core operations (`atom`, `deref`, `swap!`, `reset!`) stay in
   builtins. A new `atom` module provides advanced operations (`compare-and-swap!`,
   `watch`, `validator`). See §7.4 below for details.

---

### 7.1 Regex Literal Syntax

```nexl
;; Literal form — compiled at read time
(def email-re #"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}")

;; Equivalent to:
(def email-re (regex/new "[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}"))

;; Usage — identical either way
(regex/matches? email-re "alice@example.com")  ;; true
(regex/find-all #"\d+" "abc123def456")         ;; [{:start 3 :end 6 :text "123"} ...]
```

**Advantages of the literal form**:
- No double-escaping: `#"\d+"` instead of `"\\d+"`
- Visual distinction: regex patterns stand out in code
- Read-time compilation: constant patterns are compiled once, not on every call

**Lexer rule**: `#"` opens a regex literal; `"` closes it. Backslash escapes
within the literal follow regex conventions, not string conventions. The reader
produces a `(regex/new "...")` call in the AST.

---

### 7.2 Iterable Protocol

```nexl
;; The Iterable protocol — any type can implement this
(defprotocol Iterable [a]
  "A type whose elements can be lazily iterated."
  (into-iter : (Fn [Self] -> (Iter a))))

;; Built-in implementations:
;; Vec, Map, Set, Str, Channel, Iter (identity) all implement Iterable

;; User-defined types can implement it:
(deftype FileLines {:path Str :handle Db})

(impl FileLines
  (Iterable Str)
  (into-iter [self]
    (iter/unfold
      (fn [h] (match (read-next-line h)
                (Some line) (Some [line h])
                None        None))
      (:handle self))))
```

**How it interacts with `iter/*` functions**:

All `iter/*` functions that accept an `(Iter a)` also accept any `Iterable`,
auto-calling `into-iter` if needed:

```nexl
;; These are equivalent:
(->> (iter/from-vec users) (iter/filter active?) (iter/to-vec))
(->> users (iter/filter active?) (iter/to-vec))  ;; Vec is Iterable, auto-converts
```

**Standard Iterable implementations**:

| Type | `into-iter` produces | Order |
|------|---------------------|-------|
| `(Vec a)` | `(Iter a)` | Index order |
| `(Map k v)` | `(Iter (Tuple k v))` | Insertion order |
| `(Set a)` | `(Iter a)` | Canonical order |
| `Str` | `(Iter Char)` | Codepoint order |
| `(Channel a)` | `(Iter a)` | Receive order (blocks until closed) |
| `(Iter a)` | `(Iter a)` | Identity |

---

### 7.3 Structured Error Types

Each stdlib module that can fail defines a specific error ADT:

```nexl
;; Module-specific error types
(deftype IoError
  | (NotFound {:path Str})
  | (PermissionDenied {:path Str})
  | (InvalidUtf8 {:path Str})
  | (Other {:message Str}))

(deftype JsonError
  | (SyntaxError {:line Int :column Int :message Str})
  | (UnexpectedType {:expected Str :got Str :path Str}))

(deftype RegexError
  | (InvalidPattern {:pattern Str :message Str :offset Int}))

(deftype DbError
  | (ConnectionFailed {:path Str :message Str})
  | (QueryFailed {:sql Str :message Str})
  | (ConstraintViolation {:table Str :constraint Str}))

(deftype HttpError
  | (ConnectionFailed {:url Str :message Str})
  | (Timeout {:url Str :ms Int})
  | (InvalidUrl {:url Str}))

(deftype CsvError
  | (ParseError {:line Int :message Str}))

(deftype TimeError
  | (InvalidFormat {:input Str :pattern Str}))

(deftype UriError
  | (InvalidUri {:input Str :message Str}))

(deftype CryptoError
  | (InvalidInput {:message Str}))
```

**Updated function signatures** (examples):
```nexl
;; Before (Str errors):
(defn read-file [path : Str] -> (Result Str Str) ! [FileSystem])

;; After (structured errors):
(defn read-file [path : Str] -> (Result Str IoError) ! [FileSystem])
```

**Pattern matching on errors**:
```nexl
(match (io/read-file "config.toml")
  (Ok contents)                 (toml/parse contents)
  (Err (NotFound _))            (Ok default-config)
  (Err (PermissionDenied info)) (panic (str "cannot read " (:path info))))
```

**Every error type has a `:message` field** accessible via keyword access `(:message e)`,
so generic error logging still works without pattern matching:

```nexl
(match (json/decode input)
  (Ok val) val
  (Err e)  (do (log/error (str "JSON parse failed: " (:message e)))
               default-value))
```

**Error type mapping**:

| Module | Error Type | Key Fields |
|--------|-----------|------------|
| `io` | `IoError` | `:path`, `:message` |
| `json` | `JsonError` | `:line`, `:column`, `:message` |
| `csv` | `CsvError` | `:line`, `:message` |
| `toml` | `TomlError` | `:line`, `:message` |
| `regex` | `RegexError` | `:pattern`, `:offset`, `:message` |
| `db` | `DbError` | `:sql`, `:table`, `:message` |
| `http` | `HttpError` | `:url`, `:ms`, `:message` |
| `time` | `TimeError` | `:input`, `:pattern` |
| `uri` | `UriError` | `:input`, `:message` |
| `crypto` | `CryptoError` | `:message` |
| `base64` | `Base64Error` | `:message` |
| `process` | `ProcessError` | `:cmd`, `:message` |

---

### 7.4 Atom Module

Core operations remain in builtins (used frequently, fundamental):

| Builtin | Signature | Description |
|---------|-----------|-------------|
| `atom` | `(Fn [a] -> (Atom a))` | Create mutable atom |
| `deref` | `(Fn [(Atom a)] -> a)` | Read current value |
| `swap!` | `(Fn [(Atom a) (Fn [a] -> a)] -> a)` | Update via function, return new value |
| `reset!` | `(Fn [(Atom a) a] -> a)` | Set value, return new value |

The `atom` module provides advanced operations:

| Function | Signature | Description |
|----------|-----------|-------------|
| `compare-and-swap!` | `(Fn [(Atom a) a a] -> Bool)` | CAS: set to new only if current = expected |
| `swap-vals!` | `(Fn [(Atom a) (Fn [a] -> a)] -> (Tuple a a))` | Return (old-value, new-value) pair |
| `reset-vals!` | `(Fn [(Atom a) a] -> (Tuple a a))` | Return (old-value, new-value) pair |
| `watch` | `(Fn [(Atom a) Keyword (Fn [Keyword a a] -> Unit)] -> Unit)` | Add watcher (key, old, new) |
| `unwatch` | `(Fn [(Atom a) Keyword] -> Unit)` | Remove watcher by key |
| `validator` | `(Fn [(Atom a) (Fn [a] -> Bool)] -> Unit)` | Set validation fn (rejects invalid swaps) |

**`compare-and-swap!`** is essential for lock-free algorithms:
```nexl
;; Atomically increment only if current value matches expected
(let [current (deref counter)]
  (atom/compare-and-swap! counter current (inc current)))
```

**`watch`** enables reactive patterns:
```nexl
(atom/watch app-state :logger
  (fn [key old new]
    (log/info (str "State changed: " (pr-str old) " → " (pr-str new)))))
```
