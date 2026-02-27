---
model: sonnet
---

Look up a specific part of the Nexl language spec.

## Arguments
$ARGUMENTS — A section number (e.g. "§2", "5.3"), topic keyword (e.g. "pattern matching", "effects"), or appendix letter (e.g. "D").

## Instructions

1. Determine which section of `nexl-spec.md` to read using the section index in `CLAUDE.md`:
   - If a section number is given, read that line range directly.
   - If a topic keyword is given, grep `nexl-spec.md` for the keyword to find the right section, then read the surrounding context.
   - If an appendix letter is given, read the corresponding appendix.
2. Read ONLY the relevant lines — never the entire spec.
3. Present a concise summary of what the spec says, including:
   - The key rules and semantics
   - Any code examples from the spec
   - Cross-references to related sections
   - Relevant ADRs from `decisions/` if applicable
4. Do NOT make any edits. This is a read-only lookup.
