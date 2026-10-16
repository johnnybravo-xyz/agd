# Design

agd should feel like one tool, whether you meet it in the TUI, the narrow
pane, the CLI, Claude Code or through an agent. This page is the system it
follows. Each rule has one home in the code, linked below; change it there
and every surface follows.

## Principles

- **Org first.** Where Org or org-agenda has a word, key, format or
  behavior, agd uses it, and checks it against pinned Emacs where it can.
- **One meaning each.** A key, a color, a glyph, a word or an error code
  means one thing on every screen and surface.
- **Your files are yours.** A write changes only the bytes it means to,
  shows exactly what it will write first, and can be undone. Undo over
  confirmation.
- **Calm.** One accent color, color only for meaning, quiet when there is
  nothing to say.
- **Simple.** The smallest thing that works; no options without a person
  who needs them.

## Words

| Thing | agd says |
| --- | --- |
| Screens | Today, Week, Inbox, Agents, Triage, and your saved views |
| Today's sections | OVERDUE, SCHEDULE, DUE SOON, DOING, INBOX |
| The inbox | where your capture templates file new entries |
| An agent | by its client's name: `claude-code`, `codex` |
| Your own changes | `you` |
| A heading | heading (not headline) |
| A change | by its operation: capture, state change, schedule, deadline, priority, tags, edit, refile |

Counts use one plural rule (`1 marked row`, `2 marked rows`). The status
line reads `1 overdue · 3 today · 9 inbox · 2 unreviewed`, and every
surface counts the same way: overdue first, then what is scheduled or due
today (a deadline's early warning is DUE SOON, not today), each task once.
Today's sections count what is left to do; done rows are shown but not
counted.
An undo line names the operation it undid: `undid schedule · Pay rent`.

## Formats

| Fact | Shown as |
| --- | --- |
| A date | `Fri 16 Oct`, with the year when it isn't this year's |
| A date and time | `Fri 16 Oct 14:00` |
| When something happened | `17:22` today, else `Thu 15 Oct 17:22` |
| A countdown, an age, a missed deadline | `in 3d`, `12d`, `-2d` |
| A file | relative to its root: `projects/alpha.org:3` |
| A tag | `#work` (`:work:` with `notation = "org"`) |
| A priority | a pill; typed as `!A` (`#A` with `notation = "org"`) |
| A change line | `work.org:42  TODO → DONE · Pay rent · u undo` |

Org's own formats are never reworded: the `agd agenda` text and `--csv`
output match `org-batch-agenda` byte for byte, and files are written as
Org writes them. JSON (CLI `--json`, MCP) is for programs: absolute
paths, ISO dates, `HH:MM` times and error codes.

Home: [`agd/src/words.rs`](../agd/src/words.rs).

## Notation

The capture line and every list use one set of markers, chosen by
`notation` in `config.toml`:

| `notation` | Tag | Priority | Example |
| --- | --- | --- | --- |
| `"agd"` (default) | `#work` | `!A` | `Renew passport due:fri #personal !A` |
| `"org"` | `:work:` | `#A` | `Renew passport due:fri :personal: #A` |

`#` followed by digits stays text (`Fix #42`). The tags prompt adds and
removes with `+tag` and `-tag` in both notations.

## Keys

- A key means the same on every screen that has it.
- Keys follow org-agenda where it has one (`t`, `f b .`, `l`, `<`, `/`,
  `m * U B`, `C-c C-s`, `C-c C-w`); help shows its key beside ours.
- `j`/`k` move like `n`/`p` everywhere; in pickers and popups, where
  letters are typed, `C-n`/`C-j` and `C-p`/`C-k` move.
- `C-e` opens `$EDITOR` from the lists, Triage, Agents and the pane; in a
  popup with a text field it ends the line, so `$EDITOR` there is `C-o`.
- `esc` goes back one level; `q` quits.

Every key: [docs/KEYS.md](KEYS.md), generated from the one key table in
[`agd/src/tui/keys.rs`](../agd/src/tui/keys.rs); a test fails when they
drift apart.

## Look

| Role | Means |
| --- | --- |
| accent (blue) | focus, keys, scheduled rows, NEXT |
| red | lateness, and only lateness |
| amber | deadlines, priority, TODO |
| green | done, and in-progress states |
| yellow | WAIT |
| dim red | CANCELLED |
| muted | metadata: category, tags, ages, file:line, who captured |
| text | titles |

Keyword colors follow your `todo_keyword_colors`; without one, WAIT is
yellow and CANCELLED dim red, done states green, a sequence's first
keyword (TODO) amber and the rest blue. Fills only for state
(the Agenda badge, overdue badges, priority, parsed chips). Uppercase only
for section headers and pills; done rows are dim and struck through; no
italics. Glyphs each mean one thing: `▌` focus, `▸` next, `✓` done, `↻`
repeat, `●` agent, `*` marked, `→` destination or new state, `—` none,
`✗` error.

The TUI, the pane, the Claude Code pane (which reads the palettes from
`agd agenda --views --json`) and the CLI's terminal colors use one
palette, in dark, light and 16 colors, and respect `NO_COLOR`.

Home: [`agd/src/tui/theme.rs`](../agd/src/tui/theme.rs).

## Messages and errors

- People read sentences: what happened, and what to do next.
  `can't undo: Rotate staging certs changed after codex edited it
  (work.org:42); edit it by hand`.
- Names in messages are in single quotes: `unknown view 'x'; views: d, n`.
- Codes are for programs and appear only in JSON, one code per failure on
  every surface, named by one rule: `invalid_<thing>`, `unknown_<thing>`,
  `<thing>_not_found`, `<thing>_conflict`, `<thing>_failed`.

Home: `ErrorCode` in [`agd-core/src/model.rs`](../agd-core/src/model.rs).

## Names

- Config keys are snake_case; values reuse Org's names (`time-up`,
  `tags-todo`), and accepted values are checked at load with an error that
  lists them.
- One name per concept, the JSON schema's: `olpath`, `todo` (the keyword),
  `ref`, `template_key` (MCP), `scheduled`/`deadline` (`on:`/`due:` are
  the capture line's shorthands).
- Schemas are `agd.<record>/v1`, spelled only in `model.rs`.
