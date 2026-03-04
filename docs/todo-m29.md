# M29 — Collections, Iteration & Pattern Modules

## Goal
Add dedicated collection modules (`vec`, `map`, `set`), lazy iteration with the
`Iter` ADT and `Iterable` protocol, character classification, regex with literal
syntax, and threading macro variants. After this milestone, Nexl's data processing
story is complete.

Reference: `docs/stdlib-spec.md`

## `vec` Module (New — Mixed)

- [ ] **Vector operations** — ~24 functions
  - Constructors: `of`, `repeat`, `init` (Rust)
  - Slicing: `chunk`, `window`, `split-at`, `span` (Rust)
  - Mutation: `insert`, `remove-at`, `swap`, `rotate-left`, `rotate-right` (Rust)
  - Dedup: `dedup`, `dedup-by` (Nexl)
  - Insertion: `intersperse` (Nexl)
  - Folding: `scan`, `fold-right`, `sum`, `product` (Nexl)
  - Selection: `min-by`, `max-by` (Nexl)
  - Combinatorial: `permutations`, `combinations` (Rust)
  - Search: `binary-search` (Rust)
  - Conversion: `unzip` (Nexl)

## `map` Module (New — Mixed)

- [ ] **Map operations** — ~13 functions
  - Constructors: `of`, `from-entries` (Rust)
  - Access: `get-or` (Nexl)
  - Transforms: `map-keys`, `map-vals`, `filter-keys`, `filter-vals`, `invert` (Nexl)
  - Grouping: `group-vals` (Nexl)
  - Folding: `reduce-kv`, `find`, `every?`, `any?` (Nexl)

## `set` Module (New — Nexl)

- [ ] **Set operations** — ~11 functions
  - Constructors: `of`, `from-vec`, `to-vec` (Rust)
  - Transforms: `map`, `filter`, `flat-map` (Nexl)
  - Folding: `reduce`, `every?`, `any?` (Nexl)
  - Partitioning: `partition`, `product` (Nexl)

## `iter` Module (New — Nexl)

- [ ] **Iter ADT & Iterable protocol** — implement spec §5.12
  - `(deftype Iter [a] | Done | (Yield a (Fn [] -> (Iter a))))`
  - `(defprotocol Iterable [a] (into-iter : (Fn [Self] -> (Iter a))))`
  - Built-in `Iterable` impls for Vec, Map, Set, Str

- [ ] **Iter constructors** — `from-vec`, `from-map`, `range`, `repeat`, `iterate`, `unfold`, `empty`

- [ ] **Iter transforms** — `map`, `filter`, `take`, `drop`, `take-while`, `drop-while`, `flat-map`, `chain`, `zip`, `enumerate`, `chunk`

- [ ] **Iter consumers** — `to-vec`, `to-map`, `to-set`, `reduce`, `find`, `any?`, `all?`, `count`, `nth`

## `char` Module (New — Rust)

- [ ] **Character classification** — ~14 functions
  - Predicates: `alpha?`, `digit?`, `alphanumeric?`, `whitespace?`, `upper?`, `lower?`, `ascii?`, `control?`, `punctuation?`
  - Conversion: `to-upper`, `to-lower`, `to-int`, `from-int`, `to-str`

## Threading Macro Variants

- [ ] **`some->` / `some->>`** — Option chaining macros
  - Thread-first/last, unwrap Some, short-circuit on None
  - Auto-wrap plain values in Some

- [ ] **`ok->` / `ok->>`** — Result chaining macros
  - Thread-first/last, unwrap Ok, short-circuit on Err
  - Auto-wrap plain values in Ok

- [ ] **`cond->` / `cond->>`** — Conditional threading macros
  - Apply steps only when guard condition is true

## `regex` Module (New — Rust)

- [ ] **Regex functions** — ~9 functions
  - `new`, `matches?`, `find`, `find-all`, `replace`, `replace-first`, `split`, `captures`, `escape`
  - Match type: `{:start Int :end Int :text Str}`
  - Backed by Rust `regex` crate (linear-time, Unicode)

- [ ] **Regex literal syntax `#"pattern"`** — lexer + reader
  - `#"` opens regex literal, `"` closes
  - No double-escaping needed: `#"\d+"` instead of `"\\d+"`
  - Desugars to `(regex/new "...")` in AST
  - Read-time compilation for constant patterns
