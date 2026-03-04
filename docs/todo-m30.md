# M30 — Production Stack & Data Formats

## Goal
Complete the production-ready module set: data formats (CSV, TOML), networking
utilities (URI, UUID, base64), enhanced crypto/HTTP/time/random, and low-level
modules (bit, atom, path). After this milestone, Nexl programs should not need
external dependencies for common tasks.

Reference: `docs/stdlib-spec.md`

## `path` Module (New — Rust)

- [ ] **Cross-platform path operations** — ~13 functions
  - Navigation: `join`, `parent`, `file-name`, `stem`, `extension`, `components`
  - Transforms: `with-extension`, `normalize`, `relative-to`
  - Predicates: `absolute?`, `relative?`, `starts-with?`
  - Constants: `separator`

## `uri` Module (New — Rust)

- [ ] **URI parsing and construction** — ~11 functions
  - `parse`, `to-str`: round-trip
  - Accessors: `scheme`, `host`, `port`, `path`, `query`, `query-params`, `fragment`
  - Encoding: `encode`, `decode`

## `csv` Module (New — Rust)

- [ ] **CSV parsing/writing** — ~4 functions
  - `parse`, `parse-with-headers`: string → data
  - `encode`, `encode-with-headers`: data → string
  - Header-aware parsing returns `(Vec (Map Keyword Str))`

## `toml` Module (New — Rust)

- [ ] **TOML parsing/writing** — ~3 functions
  - `parse`, `encode`, `pretty`
  - Backed by `toml` crate

## `base64` Module (New — Rust)

- [ ] **Base64 encoding** — ~4 functions
  - `encode`, `decode`: standard base64
  - `encode-url`, `decode-url`: URL-safe variant

## `uuid` Module (New — Rust)

- [ ] **UUID generation** — ~5 functions
  - `v4`: random UUID
  - `v7`: time-ordered UUID (sortable)
  - `parse`, `to-str`: round-trip
  - `nil`: the all-zeros UUID

## `bit` Module (New — Rust)

- [ ] **Extended bitwise operations** — ~16 functions
  - Core: `and`, `or`, `xor`, `not`, `shift-left`, `shift-right`
  - Counting: `count-ones`, `count-zeros`, `leading-zeros`, `trailing-zeros`
  - Rotation: `rotate-left`, `rotate-right`
  - Bit manipulation: `test`, `set`, `clear`, `toggle`

## `atom` Module (New — Rust)

- [ ] **Advanced atom operations** — ~6 functions
  - `compare-and-swap!`: CAS for lock-free algorithms
  - `swap-vals!`, `reset-vals!`: return (old, new) pair
  - `watch`, `unwatch`: reactive state observation
  - `validator`: reject invalid state transitions

## Enhanced `crypto` (Rust)

- [ ] **Cryptographic operations** — ~8 new functions
  - Hashing: `sha256`, `sha256-bytes`, `sha512`, `blake3`
  - HMAC: `hmac-sha256`
  - Password: `pbkdf2`, `verify-pbkdf2`
  - Encoding: `hex-encode`, `hex-decode`

## Enhanced `http` (Rust)

- [ ] **Full HTTP verb support** — ~6 new functions
  - Methods: `put`, `patch`, `delete`, `head`
  - Generic: `request` (takes Request record)
  - Helpers: `header` (single header), `ok?` (status 200-299)

## Enhanced `time` (Rust)

- [ ] **Date/time operations** — ~15 new functions
  - Elapsed: `since`, `elapsed`
  - Formatting: `to-iso`, `from-iso`, `format`, `parse`
  - Extraction: `year`, `month`, `day`, `hour`, `minute`, `second`, `day-of-week`
  - Duration helpers: `seconds`, `minutes`, `hours`, `days`

## Enhanced `random` (Rust)

- [ ] **Random number generation** — ~7 new functions
  - Primitives: `int`, `float`, `bool`
  - Collections: `choice`, `shuffle`, `sample`
  - Convenience: `uuid` (alias for uuid/v4), `weighted-choice`

## Structured Error Types

- [ ] **Module-specific error ADTs** — per stdlib-spec §7.3
  - `IoError`, `JsonError`, `RegexError`, `DbError`, `HttpError`
  - `CsvError`, `TimeError`, `UriError`, `CryptoError`, `Base64Error`, `ProcessError`
  - Every variant has `:message` field
  - Update existing module signatures to use structured errors
