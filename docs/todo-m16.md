# M16 — Interoperability

## WASM Component Model
- [x] Implement `(import-component ...)` — import foreign WASM components with type verification
- [x] Implement `(export-component ...)` — export Nexl modules as WASM components
- [x] Generate WIT interfaces from Nexl types
- [x] Canonical ABI serialization at component boundaries

## WIT Resource Types
- [x] Resource import/export
- [x] Lifecycle verification (resources must be closed/transferred)

## Effect ↔ WIT Mapping
- [x] Nexl effects → WIT interfaces for export
- [x] WIT interfaces → Nexl effects for import

## C ABI FFI
- [x] `(defextern name : Type "c_name")` — import C functions
- [x] `:performs [Effect]` annotation on extern declarations
- [x] `:unsafe` annotation → requires `Unsafe` capability
- [x] Memory ownership: Nexl values pinned during C calls
- [x] `(deftype-opaque CHandle Ptr :drop free-fn)` for C resource wrapping

## Exporting for C
- [x] `(defn-export name ...)` → generates C-callable function with C ABI
- [x] Automatic type marshaling

## Blocked
- [ ] (none)

## Done
