# Current Milestone: M18 — Content Addressing + Self-Hosting Preparation

**Goal:** Incremental compilation via content hashing. Define the kernel subset for self-hosting.

**Crates:** `nexl-pkg`, `nexl-ir`, `nexl-cli`, plus potential new crates

**Spec sections to reference:**
- §12 Compilation Model (§12.3 content addressing)
- §14.3 Structured REPL protocol

**Key design points:**
- Hash every top-level definition after type inference
- On-disk definition store: hash → compiled artifact + type + effect row + deps
- Recompile only when hash or dependency hash changes
- JSON-based machine-readable REPL protocol
- Define kernel subset for Stage 1 (no macros, all types annotated)
- Stage 0 → Stage 1 bootstrap proof-of-concept

**Acceptance criteria:**
- Content-addressed definition store caches compiled artifacts
- Warm builds skip unchanged definitions
- Structured REPL protocol with JSON responses
- Kernel subset documented and verified sufficient
- Small Nexl kernel-subset program compiles with Stage 0

**When done:** Update this file to point to next milestone (or declare Stage 0 complete).

See `docs/todo-m18.md` for the task checklist.
See `milestones.md` for the full plan.
