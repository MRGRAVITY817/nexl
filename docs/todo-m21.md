# M21 — Multi-File Module Loading

## Deliverables

- [x] 1. Wire `eval_modules` into `nexl run`
  - Detect `(module ...)` declaration in the entry file
  - Resolve imports to file paths relative to project root
  - Read and parse all imported module files (transitively)
  - Topological sort and evaluate in dependency order
  - Fall back to single-file mode when no `(module ...)` is present

- [x] 2. Project root discovery
  - Walk up from entry file looking for `project.nxl`
  - Use `project.nxl` directory as the source root
  - Default to entry file's directory if no `project.nxl` found

- [x] 3. Circular dependency detection
  - Error with clear message showing the cycle
  - Already implemented in `nexl-modules` `topo_sort` — verify wiring

- [x] 4. LSP multi-file awareness
  - Skip `module`, `import`, `deftype` forms in type-checker (no false errors)
  - Full cross-module type inference and go-to-definition deferred to later milestone
