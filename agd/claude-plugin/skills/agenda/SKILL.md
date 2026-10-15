---
name: agenda
description: Read and safely update local org-mode agendas through the agd MCP server.
user-invocable: false
---

Use `agenda` or `query` before changing an item. Keep the returned `ref`; every write revalidates it. If a write returns `stale_ref`, read the item again and decide from the current state instead of blindly retrying.

Use `capture` for a new task and `set_state`, `plan`, or `edit` for an existing ref. Capture bodies are literal text. When a task comes from your work in a repository, pass `properties` such as `REPO`, `BRANCH` and `SOURCE` (an org link like `[[file:/path/to/file::42]]`) so the person can see where it came from. Never expose agenda data outside the local MCP session.
