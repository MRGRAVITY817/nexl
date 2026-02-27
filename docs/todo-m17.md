# M17 — Optimization

## Inlining
- [x] Identify small/leaf functions eligible for inlining
- [x] Inline small functions at call sites in IR

## Escape Analysis
- [x] Analyze closures for escape (can they be stack-allocated?)
- [x] Analyze collections for escape (Vec/Map that don't leave scope)
- [x] Stack-allocate non-escaping values

## Perceus Reuse Analysis
- [x] Track unique ownership of persistent data structures
- [x] In-place mutation for uniquely-owned values (functional-but-fast)

## Dead Code Elimination
- [x] Identify unreachable definitions
- [x] Remove dead code from IR before codegen

## Constant Folding
- [x] Evaluate constant arithmetic at compile time
- [x] Fold constant conditionals

## Specialization
- [x] Monomorphize polymorphic functions at known call sites

## WASM GC Backend (Optional)
- [x] Emit WASM GC types instead of linear memory management
- [x] Target browsers/Wasmtime with GC support

## Arena Mode
- [x] `--gc none` flag for short-lived WASM plugins

## Blocked
- (none)

## Done
- All items complete
