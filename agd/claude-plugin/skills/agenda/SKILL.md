---
name: agenda
description: Read and safely update the person's tasks, todos, agenda, deadlines, reminders, and Org files through the agd MCP server, which runs locally. Use when they ask what is due, scheduled, or overdue; want work captured, remembered, completed, rescheduled, or given a deadline; or when repository work produces a follow-up worth tracking.
user-invocable: false
---

# Work with the agenda

## Discover the workspace before writing

Call `describe` once before the first write. It returns capture templates and
their prompts, each file's TODO keywords, the allowed priority range, saved
views, tags in use, and accepted date forms.

## Read current state

Use `agenda` for dated work. It defaults to today and accepts `span`, `from`, or
a saved `view`. Use `query` for org-ql expressions; for example,
`(and (todo) (deadline :to 7))` finds open tasks due within seven days,
including overdue tasks.

Reads return at most 100 rows. A second content block reports omitted rows, so
increase `limit` or narrow the request when results are truncated. Set
`with_body: true` only when the task's notes are needed.

Before changing an existing item, read it through `agenda` or `query` and keep
its returned `ref`. Every write revalidates that ref. A ref without an Org
`:ID:` changes after each write; continue with `item.ref` from the mutation
result. If a write returns `ref_conflict`, inspect its `current` item and decide
from that state. Do not retry blindly.

## Choose the narrowest write tool

- `capture`: create a task through a `template_key` returned by `describe`.
  Omitting the key uses the first template. `title` and `body` are literal text;
  the body remains below the new heading.
- `set_state`: set `todo` to one of that file's declared TODO keywords.
- `plan`: set `scheduled` or `deadline`; pass `none` to remove one.
- `edit`: change title, tags, priority, properties, or notes.

When a result includes `notes`, report those notes to the person. They describe
the applied behavior, such as an agent inbox removing requested dates, and take
precedence over the original request wording.

## Preserve repository provenance

When a task comes from repository work, include properties that make its origin
clear:

- `REPO`: repository name;
- `BRANCH`: current branch;
- `SOURCE`: an Org file link such as `[[file:~/src/app/main.rs::42]]`.

Use `~` rather than an absolute home-directory prefix in `SOURCE`.

## Handle failures and privacy

A failed tool call sets `isError` and returns an `agd.error/v1` object with
`code` and `message`. `invalid_arguments` lists accepted arguments. Follow the
specific recovery in the message; never bypass a conflict by guessing.

Agenda data is private. Never expose it outside the local MCP session.
