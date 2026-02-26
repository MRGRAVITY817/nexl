# M13 — Native Backend (Cranelift)

## Core
- [x] Create `nexl-native` crate with Cranelift dependencies
- [x] Native value representation (tagged pointers, heap objects) (§13.2)
- [x] IR → Cranelift IR translation (basic expressions, calls, branches)
- [x] Closure representation (code pointer + flat environment) (§13.2)
- [x] Perceus reference counting (inc/dec/drop) (§13.3)
- [x] Reuse analysis for in-place mutation (§13.3)
- [x] Evidence vectors as native arrays (§13.5)
- [x] Continuation capture (setjmp/longjmp or manual stack copy)
- [x] Tail calls via Cranelift
- [x] Object file emission (ELF/Mach-O)
- [x] `nexl build --target native` CLI integration

## Blocked
- [ ] (none)
