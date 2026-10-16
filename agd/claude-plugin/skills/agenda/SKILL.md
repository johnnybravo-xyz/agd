---
name: agenda
description: Read and safely update the person's tasks, todos, agenda, deadlines, reminders and org-mode (.org) files through the agd MCP server. Use when they ask what is due, scheduled or overdue, want a task captured, remembered, marked done, rescheduled or given a deadline, or when your work turns up a follow-up worth tracking.
user-invocable: false
---

Call `describe` once before writing: it lists the capture templates (and their prompts), each file's TODO keywords, the priority range, saved views, tags in use and the date grammar.

Read with `agenda` (today unless you pass `span`, `from` or a saved `view`) or `query` with org-ql, such as `(and (todo) (deadline :to 7))` for open tasks due within a week, overdue ones included. Reads return at most 100 rows; a second content block says how many were left out, so pass `limit` or narrow the query. Pass `with_body: true` only when you need an item's notes.

Use `agenda` or `query` before changing an item, and keep the returned `ref`; every write revalidates it. A ref without an `:ID:` changes with every write, so use the `item.ref` the write returns. If a write returns `ref_conflict`, read its `current` item and decide from that state instead of blindly retrying.

Use `capture` for a new task, with a `template_key` from `describe` (left out, the first template). The title and `body` are literal text, and the body is kept under the new heading. When the result carries `notes` (for example, agent captures go to an inbox without dates), tell the person what they say rather than what you asked for. Use `set_state` with `todo` set to one of the file's TODO keywords, `plan` for `scheduled` and `deadline` (`none` removes one), and `edit` for the title, tags, priority, properties or a note.

When a task comes from your work in a repository, pass `properties` such as `REPO`, `BRANCH` and `SOURCE` (an org link like `[[file:/path/to/file::42]]`) so the person can see where it came from.

A failed call sets `isError` and answers `agd.error/v1` with a `code` and `message`; `invalid_arguments` lists the arguments the tool accepts. Never expose agenda data outside the local MCP session.
