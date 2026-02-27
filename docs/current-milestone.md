# Current Milestone: M16 — Interoperability

**Goal:** WASM Component Model, WIT generation, C FFI.

**Crates:** `nexl-wasm`, `nexl-cli`, plus potential new crate for WIT generation

**Spec sections to reference:**
- §15 Interoperability (FFI, Component Model)
- §15.3 C ABI FFI
- §15.4 Exporting for C

**Key design points:**
- WASM Component Model for importing/exporting modules
- WIT interface generation from Nexl types
- Effect ↔ WIT mapping (Nexl effects → WIT interfaces)
- C FFI via `defextern` with `:performs` and `:unsafe` annotations
- Memory ownership: Nexl values pinned during C calls

**Acceptance criteria:**
- `(import-component ...)` imports foreign WASM components with type verification
- `(export-component ...)` exports Nexl modules as WASM components
- WIT resource types with lifecycle verification
- `(defextern name : Type "c_name")` imports C functions
- `(defn-export name ...)` generates C-callable functions

**When done:** Update this file to point to M17.

See `docs/todo-m16.md` for the task checklist.
See `milestones.md` for the full plan.
