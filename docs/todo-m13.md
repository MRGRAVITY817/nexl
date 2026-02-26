# M13 — Native Backend (Cranelift)

## Core
- [x] Create `nexl-native` crate with Cranelift dependencies
- [x] Native value representation (tagged pointers, heap objects) (§13.2)
- [ ] IR → Cranelift IR translation (basic expressions, calls, branches)
- [ ] Closure representation (code pointer + flat environment) (§13.2)
- [ ] Perceus reference counting (inc/dec/drop) (§13.3)
- [ ] Reuse analysis for in-place mutation (§13.3)
- [ ] Evidence vectors as native arrays (§13.5)
- [ ] Continuation capture (setjmp/longjmp or manual stack copy)
- [ ] Tail calls via Cranelift
- [ ] Object file emission (ELF/Mach-O)
- [ ] `nexl build --target native` CLI integration

## Blocked
- [ ] (none)
