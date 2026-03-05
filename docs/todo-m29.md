# M29 — Collections, Iteration & Pattern Modules

## Goal
Add dedicated collection modules (`vec`, `map`, `set`), lazy iteration with the
`Iter` ADT and `Iterable` protocol, character classification, regex with literal
syntax, and threading macro variants. After this milestone, Nexl's data processing
story is complete.

Reference: `docs/stdlib-spec.md`

## Memory Safety (Completed)

- [x] **Recursion depth limit** — thread-local `CALL_DEPTH` counter with RAII guard (`MAX_CALL_DEPTH = 10_000`) in `nexl-eval`
- [x] **Unbounded allocation guards** — `range` (10M), `vec/repeat` (10M), `vec/init` (10M), `vec/permutations` (10 elements), `vec/combinations` (20 elements)
- [x] **Rc cycle Debug fix** — `Value::Function` Debug no longer recurses into captures/module_captures

## `vec` Module (New — Mixed)

- [x] **Vector operations** — ~24 functions
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

- [x] **Map operations** — ~13 functions
  - Constructors: `of`, `from-entries` (Rust)
  - Access: `get-or` (Nexl)
  - Transforms: `map-keys`, `map-vals`, `filter-keys`, `filter-vals`, `invert` (Nexl)
  - Grouping: `group-vals` (Nexl)
  - Folding: `reduce-kv`, `find`, `every?`, `any?` (Nexl)

## `set` Module (New — Nexl)

- [x] **Set operations** — ~11 functions
  - Constructors: `of`, `from-vec`, `to-vec` (Rust)
  - Transforms: `map`, `filter`, `flat-map` (Nexl)
  - Folding: `reduce`, `every?`, `any?` (Nexl)
  - Partitioning: `partition`, `product` (Nexl)

## `iter` Module (New — Nexl)

- [x] **Iter ADT** — `(deftype Iter [a] | Done | (Yield a (Fn [] -> (Iter a))))` via deftype
- [x] **Iter constructors** — `empty`, `singleton`, `from-vec`, `from-map`, `from-set`, `range`, `range-from`, `range-by`, `repeat`, `iterate`, `unfold`
- [x] **Iter transforms** — `map`, `filter`, `take`, `drop`, `take-while`, `drop-while`, `flat-map`, `chain`, `zip`, `enumerate`, `chunk`
- [x] **Iter consumers** — `to-vec`, `to-map`, `to-set`, `reduce`, `find`, `any?`, `all?`, `count`, `nth`, `first`
- [x] **Bug fix**: module-qualified recursive closures now resolve correctly (eval_apply adds self to module alias)

## `char` Module (New — Rust)

- [x] **Character classification** — ~14 functions
  - Predicates: `alpha?`, `digit?`, `alphanumeric?`, `whitespace?`, `upper?`, `lower?`, `ascii?`, `control?`, `punctuation?`
  - Conversion: `to-upper`, `to-lower`, `to-int`, `from-int`, `to-str`

## Threading Macro Variants

- [x] **`some->` / `some->>`** — Option chaining macros
  - Thread-first/last, short-circuit on None; threads whole Option via let+match

- [x] **`ok->` / `ok->>`** — Result chaining macros
  - Thread-first/last, short-circuit on Err; threads whole Result via let+match

- [x] **`cond->` / `cond->>`** — Conditional threading macros
  - Apply steps only when guard condition is true

## `regex` Module (New — Rust)

- [x] **Regex functions** — ~9 functions
  - `new`, `matches?`, `find`, `find-all`, `replace`, `replace-first`, `split`, `captures`, `escape`
  - Match type: `{:start Int :end Int :text Str}`
  - Backed by Rust `regex` crate (linear-time, Unicode)

- [x] **Regex literal syntax `#"pattern"`** — lexer + reader
  - `#"` opens regex literal, `"` closes
  - No double-escaping needed: `#"\d+"` instead of `"\\d+"`
  - Desugars to `(regex/new "...")` in AST
  - Read-time compilation for constant patterns
