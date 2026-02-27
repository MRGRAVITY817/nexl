# M16 — Interoperability

## WASM Component Model
- [ ] Implement `(import-component ...)` — import foreign WASM components with type verification
- [ ] Implement `(export-component ...)` — export Nexl modules as WASM components
- [ ] Generate WIT interfaces from Nexl types
- [ ] Canonical ABI serialization at component boundaries

## WIT Resource Types
- [ ] Resource import/export
- [ ] Lifecycle verification (resources must be closed/transferred)

## Effect ↔ WIT Mapping
- [ ] Nexl effects → WIT interfaces for export
- [ ] WIT interfaces → Nexl effects for import

## C ABI FFI
- [ ] `(defextern name : Type "c_name")` — import C functions
- [ ] `:performs [Effect]` annotation on extern declarations
- [ ] `:unsafe` annotation → requires `Unsafe` capability
- [ ] Memory ownership: Nexl values pinned during C calls
- [ ] `(deftype-opaque CHandle Ptr :drop free-fn)` for C resource wrapping

## Exporting for C
- [ ] `(defn-export name ...)` → generates C-callable function with C ABI
- [ ] Automatic type marshaling

## Blocked
- [ ] (none)

## Done
