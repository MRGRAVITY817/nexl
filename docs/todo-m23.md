# M23 — WASI Integration & Interop

## Deliverables

- [x] 1. **wasmtime runtime integration**
  - Add `wasmtime` dependency to `nexl-cli`
  - `nexl run --wasm` compiles to WASM and executes via wasmtime
  - Basic WASI linker setup (no interfaces yet — just engine + store + instantiate)
  - Prerequisite for all WASI interface work

- [x] 2. **`wasi:cli` — command-line basics**
  - `wasi:cli/stdin`, `stdout`, `stderr` — read/write
  - `wasi:cli/environment` — args and env vars
  - `wasi:cli/exit` — process exit codes
  - `nexl run --wasm` programs can print to stdout and read args

- [x] 3. **`wasi:clocks` + `wasi:random`**
  - `wasi:clocks/wall-clock` — current time
  - `wasi:clocks/monotonic-clock` — elapsed time / sleep
  - `wasi:random` — cryptographic and insecure random bytes
  - Expose as stdlib: `(time/now)`, `(random/bytes n)`

- [x] 4. **`wasi:filesystem`**
  - Open, read, write, stat, readdir, close
  - Expose as stdlib: `(fs/read-file path)`, `(fs/write-file path content)`
  - Preopened directory sandboxing (WASI capability model)

- [x] 5. **`wit-import` — generate Nexl bindings from WIT files**
  - `(wit-import "path/to/interface.wit")` → typed Nexl functions
  - Resource types → opaque Nexl types with `:drop` hooks
  - WIT lists/records/variants → Nexl Vec/records/ADTs
  - Builds on existing `wit.rs` and `canonical_abi.rs`

- [x] 6. **`wit-export` — expose Nexl modules as WIT interfaces**
  - `(export-component :wit "my-service.wit")` on a module
  - Nexl types → WIT types (records, variants, enums, resources)
  - Effect declarations → WIT imported interfaces
  - Canonical ABI serialization at component boundaries

- [x] 7. **Effect ↔ WASI capability mapping**
  - `:performs [Net]` ↔ `wasi:http` import
  - `:performs [FileSystem]` ↔ `wasi:filesystem` import
  - Module without `:performs [Net]` cannot import `wasi:http`
  - Effect system enforces sandboxing at the WASM level

- [x] 8. **`wasi:http` + `wasi:sockets`**
  - `wasi:http/outgoing-handler` — HTTP client requests
  - `wasi:http/incoming-handler` — HTTP server handler
  - `wasi:sockets` — TCP client and server
  - Expose as stdlib: `(http/get url)`, `(http/serve handler port)`

- [ ] 9. **Component composition — practical test**
  - Import a real Rust component (regex engine or crypto library)
  - Export a Nexl component consumable from another language
  - Compose two Nexl components via `wasm-tools compose`
  - Document the full workflow in `docs/component-composition.md`

- [ ] 10. **WASI 0.3 async readiness** (design only, gate behind flag)
  - Map Nexl's `Concurrent` effect to WASI async I/O
  - Non-blocking HTTP, filesystem, and socket design doc
  - Gate behind `--experimental-wasi3` flag; no runtime changes until spec is final
