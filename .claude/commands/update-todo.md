---
model: haiku
---

Update the current milestone's todo list.

## Arguments
$ARGUMENTS — What to update (e.g. "check off span type", "add blocker: need reader macro design", "move string interp to blocked").

## Instructions

1. Read `docs/current-milestone.md` to get the active milestone number.
2. Read `docs/todo-m{N}.md`.
3. Apply the requested change:
   - **Check off**: Change `- [ ]` to `- [x]` for the specified item and move it to the "Done" section.
   - **Add task**: Add a new `- [ ]` item to the appropriate section (Todo, Blocked, Tests Needed).
   - **Move to blocked**: Move an item to the "Blocked" section with a note explaining why.
   - **Unblock**: Move an item from "Blocked" back to "Todo".
4. If checking off an item, verify the work is actually done:
   - Run `cargo test -p nexl-{relevant-crate}` to confirm tests pass.
   - If tests fail, do NOT check off the item. Report the failure instead.
5. Show the updated todo summary: done count / total count.
