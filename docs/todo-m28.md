# M28 — Stdlib Core & Enrichment

## Goal
Enrich existing Rust stdlib modules with missing essentials, add the `option`,
`result`, and `core` modules, and build the infrastructure for writing stdlib
modules in Nexl. This milestone lays the foundation for all subsequent stdlib work.

Reference: `docs/stdlib-spec.md`

## Infrastructure

- [x] **Nexl-written stdlib loading** — evaluator can load `.nx` files as stdlib modules
  - Stdlib `.nx` files embedded via `include_str!` or loaded from a `stdlib/` directory
  - Evaluated after Rust NativeFn registration, before user code
  - Module-qualified names (`option/map`, `result/flat-map`) work from `.nx` definitions
  - Test: a simple `.nx` stdlib function is callable from user code

## Builtins — Arithmetic & Comparison

- [x] **New arithmetic builtins** — `inc`, `dec`, `rem`, `quot`
  - `inc`/`dec`: polymorphic over Numeric types
  - `rem`: sign of dividend (truncated division)
  - `quot`: truncated division quotient

- [x] **New comparison builtins** — `not=`, `compare`, `clamp`
  - `not=`: complement of `=`
  - `compare`: returns `:lt`, `:eq`, `:gt` (matches Ord protocol, spec §5.11)
  - `clamp`: restrict value to [lo, hi] range

## Builtins — Collections

- [x] **Polymorphic accessors** — `empty?`, `nth`, `get-in`
  - `empty?`: O(1) for Vec/Map/Set, O(1) for Str (check byte length)
  - `nth`: alias for `get` on indexed collections
  - `get-in`: nested access via key path vector

- [x] **Structural transforms** — `assoc-in`, `update`, `update-in`, `conj`, `into`, `concat`, `empty`
  - `conj`: polymorphic append (Vec end, Set add, Map takes [k v] pair)
  - `into`: pour elements from source into destination collection
  - `concat`: concatenate two collections of same type
  - `empty`: return empty collection of same type
  - `assoc-in`/`update`/`update-in`: nested path operations

- [x] **Map builtins** — `merge`, `merge-with`, `select-keys`, `rename-keys`, `zipmap`, `entries`
  - `merge`: variadic, rightmost wins
  - `merge-with`: conflict resolver function
  - `select-keys`/`rename-keys`: submap operations

- [x] **Set builtins** — `dissoc`, `disj`, `union`, `intersection`, `difference`, `symmetric-difference`, `subset?`, `superset?`, `disjoint?`
  - `dissoc`: remove key(s) from Map (variadic)
  - `disj`: remove element(s) from Set (variadic)

- [x] **Higher-order sequence functions** — `reject`, `keep`, `some`, `every?`, `any?`, `not-any?`, `not-every?`
  - `remove`: complement of filter (keep where pred is false)
  - `keep`: map + filter-None in one pass
  - `some`: first non-None result of applying f

- [ ] **More HOFs** — `find`, `find-index`, `map-indexed`, `reduce-indexed`, `sort-with`, `distinct`, `flatten`, `frequencies`, `partition-by`, `interleave`, `interpose`, `zip`, `zip-with`

- [ ] **String builtins** — `pr-str`
  - Readable representation with quotes, escapes

## `str` Module Enrichment

- [ ] **String functions** — ~20 new functions (Rust)
  - `split-first`, `split-lines`: additional split variants
  - `capitalize`, `title`: case transforms
  - `replace-first`, `last-index-of`: search variants
  - `pad-start`, `pad-end`, `repeat`, `reverse`: formatting
  - `byte-count`, `char-count`, `grapheme-count`: explicit cost model
  - `from-chars`, `from-code-points`, `to-bytes`, `from-bytes`: conversions
  - `kebab-case`, `snake-case`, `camel-case`: code generation helpers

## `math` Module Enrichment

- [ ] **Math constants** — `tau`, `inf`, `neg-inf`, `nan`

- [ ] **Math functions** — ~15 new functions (Rust)
  - `sign`, `truncate`, `cbrt`: basic operations
  - `log2`, `log10`, `exp2`: logarithms
  - `sinh`, `cosh`, `tanh`: hyperbolic functions
  - `nan?`, `infinite?`, `finite?`: float classification
  - `gcd`, `lcm`, `divmod`: integer math

## `conv` Module Enrichment

- [ ] **New conversions** — `->bool`, `->char`
  - `->bool`: 0/0.0/""/false → false, all else → true
  - `->char`: codepoint Int to Char (None if invalid)

## `core` Module (New — Nexl)

- [ ] **Higher-order utilities** — ~12 functions (written in Nexl)
  - `identity`, `comp`, `comp*`, `pipe`: function composition
  - `partial`, `constantly`, `complement`: function builders
  - `juxt`, `apply`: function application
  - `memoize`, `trampoline`: advanced patterns
  - `tap`: debug logging in pipelines (subject-first)

## `option` Module (New — Nexl)

- [x] **Option combinators** — ~13 functions (written in Nexl)
  - Predicates: `some?`, `none?`
  - Extraction: `unwrap`, `unwrap-or`, `unwrap-or-else`
  - Transforms: `map`, `flat-map`, `filter` (all subject-first for `->`)
  - Chaining: `or-else`, `zip`
  - Conversion: `to-result`, `from-result`
  - Collection: `values` (filter None from Vec of Options)

## `result` Module (New — Nexl)

- [ ] **Result combinators** — ~15 functions (written in Nexl)
  - Predicates: `ok?`, `err?`
  - Extraction: `unwrap`, `unwrap-err`, `unwrap-or`, `unwrap-or-else`
  - Transforms: `map`, `map-err`, `flat-map` (all subject-first for `->`)
  - Chaining: `or-else`
  - Conversion: `to-option`, `from-option`
  - Batch: `try-map`, `partition`, `collect`
