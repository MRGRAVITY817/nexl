# M30 — Production Stack & Data Formats

## Goal
Complete the production-ready module set: data formats (CSV, TOML), networking
utilities (URI, UUID, base64), enhanced crypto/HTTP/time/random, and low-level
modules (bit, atom, path). After this milestone, Nexl programs should not need
external dependencies for common tasks.

Reference: `docs/stdlib-spec.md`

## `path` Module (New — Rust)

- [x] **Cross-platform path operations** — 13 functions
  - Navigation: `join`, `parent`, `file-name`, `stem`, `extension`, `components`
  - Transforms: `with-extension`, `normalize`, `relative-to`
  - Predicates: `absolute?`, `relative?`, `starts-with?`
  - Constants: `separator`

## `uri` Module (New — Rust)

- [x] **URI parsing and construction** — 11 functions
  - `parse`, `to-str`: round-trip
  - Accessors: `scheme`, `host`, `port`, `path`, `query`, `query-params`, `fragment`
  - Encoding: `encode`, `decode`

## `csv` Module (New — Rust)

- [x] **CSV parsing/writing** — 4 functions
  - `parse`, `parse-with-headers`: string → data
  - `encode`, `encode-with-headers`: data → string
  - Header-aware parsing returns `(Vec (Map Keyword Str))`

## `toml` Module (New — Rust)

- [x] **TOML parsing/writing** — 3 functions
  - `parse`, `encode`, `pretty`
  - Backed by `toml` crate

## `base64` Module (New — Rust)

- [x] **Base64 encoding** — 4 functions
  - `encode`, `decode`: standard base64
  - `encode-url`, `decode-url`: URL-safe variant

## `uuid` Module (New — Rust)

- [x] **UUID generation** — 5 functions
  - `v4`: random UUID
  - `v7`: time-ordered UUID (sortable)
  - `parse`, `to-str`: round-trip
  - `nil`: the all-zeros UUID

## `bit` Module (New — Rust)

- [x] **Extended bitwise operations** — 16 functions
  - Core: `and`, `or`, `xor`, `not`, `shift-left`, `shift-right`
  - Counting: `count-ones`, `count-zeros`, `leading-zeros`, `trailing-zeros`
  - Rotation: `rotate-left`, `rotate-right`
  - Bit manipulation: `test`, `set`, `clear`, `toggle`

## `atom` Builtins

- [ ] **Atom builtins** — `atom`, `deref`, `swap!`, `reset!`
  - Requires `Rc<RefCell<Value>>` in nexl-runtime `Value` enum
  - Deferred: needs runtime changes before stdlib can implement

## Enhanced `crypto` (Rust)

- [x] **Cryptographic operations** — 10 new functions
  - Hashing: `sha256`, `sha256-bytes`, `sha512`, `blake3`
  - HMAC: `hmac-sha256`
  - Password: `pbkdf2`, `verify-pbkdf2`
  - Encoding: `hex-encode`, `hex-decode`
  - Alias: `random-bytes`

## Enhanced `http` (Rust)

- [x] **Full HTTP verb support** — 7 new functions
  - Methods: `put`, `patch`, `delete`, `head`
  - Generic: `request` (takes Request record)
  - Helpers: `header` (single header), `ok?` (status 200-299)

## Enhanced `time` (Rust)

- [x] **Date/time operations** — 16 new functions
  - Elapsed: `since`, `elapsed`
  - Formatting: `to-iso`, `from-iso`, `format`, `parse`
  - Extraction: `year`, `month`, `day`, `hour`, `minute`, `second`, `day-of-week`
  - Duration helpers: `seconds`, `minutes`, `hours`, `days`

## Enhanced `random` (Rust)

- [x] **Random number generation** — 8 new functions
  - Primitives: `int`, `float`, `bool`
  - Collections: `choice`, `shuffle`, `sample`
  - Convenience: `uuid` (UUID v4 string), `weighted-choice`

## Enhanced `env`, `json`, `db`

- [x] **`env/set`** — set an environment variable
- [x] **`json/encode-sorted`** — compact JSON with sorted keys
- [x] **`db/begin-transaction`, `db/commit-transaction`, `db/rollback-transaction`** — manual transaction control

## Structured Error Types

- [ ] **Module-specific error ADTs** — per stdlib-spec §7.3
  - `IoError`, `JsonError`, `RegexError`, `DbError`, `HttpError`
  - `CsvError`, `TimeError`, `UriError`, `CryptoError`, `Base64Error`, `ProcessError`
  - Every variant has `:message` field
  - Update existing module signatures to use structured errors
  - Deferred to M31 (requires evaluator changes for pattern matching on errors)
