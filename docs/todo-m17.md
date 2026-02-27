# M17 — Optimization

## Inlining
- [ ] Identify small/leaf functions eligible for inlining
- [ ] Inline small functions at call sites in IR

## Escape Analysis
- [ ] Analyze closures for escape (can they be stack-allocated?)
- [ ] Analyze collections for escape (Vec/Map that don't leave scope)
- [ ] Stack-allocate non-escaping values

## Perceus Reuse Analysis
- [ ] Track unique ownership of persistent data structures
- [ ] In-place mutation for uniquely-owned values (functional-but-fast)

## Dead Code Elimination
- [ ] Identify unreachable definitions
- [ ] Remove dead code from IR before codegen

## Constant Folding
- [ ] Evaluate constant arithmetic at compile time
- [ ] Fold constant conditionals

## Specialization
- [ ] Monomorphize polymorphic functions at known call sites

## WASM GC Backend (Optional)
- [ ] Emit WASM GC types instead of linear memory management
- [ ] Target browsers/Wasmtime with GC support

## Arena Mode
- [ ] `--gc none` flag for short-lived WASM plugins

## Blocked
- [ ] (none)

## Done
