# Current Milestone: M14 — Standard Library

**Goal:** Implement all §11.1 core modules.

**Crates:** existing crates + new stdlib modules

**Spec sections to reference:**
- §11 Standard Library (lines ~2600+)

**Key ADRs:**
- ADR-008: No HKT, compiler-dispatched map/filter
- ADR-011: Naming conventions (append/put/remove/each/etc.)

**Acceptance criteria:**
- core, str, math, conv, io, json, time, crypto, log, test, net, async modules
- All §11.1 functions implemented and tested
- `nexl` binary can compile real programs using the stdlib

**When done:** Update this file to point to M15.

See `docs/todo-m14.md` for the task checklist.
See `milestones.md` for the full plan.
