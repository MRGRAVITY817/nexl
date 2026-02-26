# M8 — WASM Backend

## IR Design (nexl-ir crate)
- [x] Create nexl-ir crate — ANF IR node types (Module, FuncDef, Block, LetBind, Atom, Rhs, Tail, MatchArm)
- [x] IR lowering pass — lower typed AST → nexl-ir (closures → env structs, match → decision trees, `?` → jumps)

## WASM Codegen (nexl-wasm crate)
- [x] Create nexl-wasm crate scaffold with wasm-encoder dependency
- [x] Codegen: functions → WASM functions
- [x] Codegen: closures → code pointer + env struct in linear memory
- [x] Codegen: ADTs → tagged unions in linear memory
- [x] Codegen: strings → pointer + length in linear memory

## Memory Management (nexl-memory crate)
- [x] Create nexl-memory crate — Perceus RC data structures (ref-count header, alloc/free)
- [x] dup/drop insertion pass over IR
- [x] Reuse analysis: uniquely-owned values mutated in-place

## Effect Runtime
- [x] Evidence vectors as WASM linear memory arrays
- [x] Tail-resumptive handlers → direct function calls through evidence

## Tail Calls
- [x] loop/recur → WASM `loop`/`br`
- [ ] General tail calls → WASM `return_call`

## Output
- [ ] End-to-end: compile a trivial Nexl program to a runnable .wasm file (Wasmtime)

## Blocked
- [ ] (none yet)
