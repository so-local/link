---
name: lists
description: Add to, check off, show or clear items on the to-do and shopping lists.
---

# Lists

Files: `lists/todo.md` and `lists/shopping.md`, relative to Home. Each is one
checkbox item per line: `- [ ] item` and `- [x] item`. A to-do may end
with ` (due YYYY-MM-DD)`.

- Preserve the existing headings. New open items belong under Open,
  before Done, rather than at the end of the file under Done.
- Add: append a `- [ ]` line under Open, skipping an item already there.
- Check: change `[ ]` to `[x]` and move the matching line under Done.
  Preserve the other items and their status.
- Show: print only open items, a short list.
- Clear: remove checked lines (ask first before removing open ones).

Confirm in one line, such as "Added 2 to shopping.md."
