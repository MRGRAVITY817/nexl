# Current Milestone: M13 — Native Backend (Cranelift)

**Goal:** Compile to native binaries (ELF/Mach-O) via Cranelift.

**Crates:** `nexl-ir`, `nexl-wasm` (extended), new `nexl-native` crate

**Spec sections to reference:**
- §12 Compilation Model (lines 2984+)
- §13 Value Representation (lines ~3100+)

**Key ADRs:**
- (none yet — native backend decisions TBD)

**Acceptance criteria:**
- IR → Cranelift IR → machine code (x86-64, aarch64)
- Native value representation (tagged pointers, unboxed numerics)
- Native memory management (Perceus RC)
- Native effect runtime (evidence vectors as native arrays)
- Tail calls via Cranelift
- `nexl build --target native` CLI

**When done:** Update this file to point to M14.

See `docs/todo-m13.md` for the task checklist.
See `milestones.md` for the full plan.
