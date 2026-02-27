# M20 — System Interface

## Deliverables

- [x] 1. `io/read-line` — read one line from stdin
  - Returns `(Result Str Str)` — `(Err "eof")` at end of input
  - Requires `Console` sandbox capability

- [x] 2. `sys/args` — access command-line arguments
  - Returns `(Vec Str)` — args after `nexl run <file>`
  - CLI passes trailing args via `nexl_runtime::sys::set_program_args`
  - Requires `Console` sandbox capability

- [x] 3. `sys/getenv` — read environment variables
  - `(sys/getenv "HOME")` → `(Option Str)`
  - Requires `Console` sandbox capability

- [x] 4. `sys/exit` — exit with status code
  - `(sys/exit 0)` / `(sys/exit 1)`

- [x] 5. `io/file-exists?` — check if path exists
  - Returns `Bool`
  - Requires `FileSystem` sandbox capability

- [x] 6. `io/read-dir` — list directory contents
  - Returns `(Result (Vec Str) Str)`
  - Requires `FileSystem` sandbox capability

- [x] 7. `io/delete-file` — delete a file
  - Returns `(Result Unit Str)`
  - Requires `FileSystem` sandbox capability

- [x] 8. Sandbox integration
  - All new operations check sandbox capabilities
