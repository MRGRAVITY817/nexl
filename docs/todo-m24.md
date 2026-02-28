# M24 — Hello Production Stack

## Deliverables

- [x] 1. **`json` module — production-grade upgrades**
  - `\uXXXX` Unicode escape sequences in strings
  - Proper stringification of special chars (`\n`, `\t`, control chars → `\uXXXX`)
  - `json/encode` + `json/decode` as primary API names (aliases)
  - `json/pretty` — indented output with configurable indent

- [x] 2. **`http` module — Request/Response types + client**
  - `Request` and `Response` record Values
  - `http/get url` → `(Result Response HttpError)`
  - `http/post url body headers` → `(Result Response HttpError)`
  - `http/response status body` → `Response`
  - `http/serve handler port` stub (Component Model backed in WASM)

- [x] 3. **`db` module — SQLite**
  - Add `rusqlite` (bundled) dependency to `nexl-stdlib`
  - `db/open path` → `(Result Db DbError)`
  - `db/query db sql params` → `(Result (Vec Map) DbError)`
  - `db/execute db sql params` → `(Result Int DbError)`
  - `db/close db` → Unit
  - `db/transaction db fn` → `(Result T DbError)`
  - Parameterized queries only (no string interpolation)
  - Row → `Vec<(Keyword, Value)>` Map

- [x] 4. **`test` module — enhanced testing**
  - `test/fail msg` — explicitly fail with message
  - `test/skip msg` — skip a test
  - `test/check name gen f` — simple property-based testing (run f N times)
  - `test/run-tests registry` — run a Vec of (name, thunk) pairs and report
  - Test registry support for `deftest` macro integration

- [x] 5. **`env` module — configuration**
  - New `env.rs` stdlib module
  - `env/get name` → `(Option Str)`
  - `env/require name` → `Str` (errors if missing)
  - `env/load-dotenv path` → Unit (loads `.env` file into process env)
  - `env/all` → Map of all env vars

- [x] 6. **`log` module — structured JSON logging**
  - JSON-formatted structured log lines on stderr
  - `log/with ctx body` — run body with additional context fields merged
  - `log/set-level level` — filter log level at runtime
  - Context accumulation via thread-local

- [ ] 7. **`nexl test` CLI command**
  - `nexl test [file]` — finds and runs `deftest` forms in source files
  - Evaluates file, collects tests from `*test-registry*` global
  - Reports pass/fail counts with timing
  - Exit code 0 on all pass, 1 on any failure
