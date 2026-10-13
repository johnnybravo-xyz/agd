# Launch announcement drafts

These drafts link to the stable releases page and make only claims backed by
the checked-in compatibility table. Keep that evidence-first framing; do not
lead with AI.

## Shared short version

**Agenda: a terminal Org agenda that proves its results against Emacs**

I built `agd`, a local-first Rust tool for people who keep Org files but want a
fast agenda in any terminal. Its agenda fixtures run through both pinned Emacs
Org and the native engine; releases publish the compatibility table instead of
claiming vague Org support.

The same binary provides a CLI, a focused Ratatui TUI, a narrow tmux/herdr pane,
and a stdio MCP server. Writes preserve untouched bytes, re-resolve the heading
under a lock, journal an inverse patch, and atomically replace the file. Stale
refs, duplicate IDs, dirty Emacs buffers, symlink targets, and unsafe imports
fail closed.

It works beside Emacs and nvim-orgmode rather than replacing either. Explicit
one-time probes can import the declarative configuration subset into a cold
TOML snapshot; arbitrary Lisp and Lua are never executed by the native engine.

Install: `cargo install agd --locked` or
`brew install johnnybravo-xyz/agd/agd`.

Demo and compatibility table: https://github.com/johnnybravo-xyz/agd/releases

## r/orgmode

**A terminal-first Org agenda with measured Emacs compatibility**

Agenda is for the part of my Org workflow that happens outside Emacs: checking
Today in a narrow pane, clearing slipped tasks, capturing one line, and letting
a coding agent make a reviewable task change.

The important bit is not the interface. Every supported agenda fixture is also
run through Emacs `org-batch-agenda-csv`; repeaters, deadline warnings, ranges,
inherited tags, carry-over, and done-item skipping are compared as structured
rows. Unsupported long-tail behavior—diary sexps, habits, arbitrary skip
functions—is listed plainly.

Writes use byte-span patches rather than serializing a parsed tree, so drawers,
comments, spacing, and unrelated text survive exactly. There is a journal and
guarded undo, and `--dry-run` shows the exact candidate diff.

I would value reports with the smallest Org fixture that differs from Emacs.
Please include the Emacs and Org versions and `agd agenda --csv` output.

https://github.com/johnnybravo-xyz/agd/releases

## r/emacs

**Agenda uses Emacs as its agenda oracle, not as a runtime dependency**

I have released Agenda, a terminal agenda and safe writer for ordinary Org
files. It does not try to clone the agenda buffer. A pinned Emacs/Org process
produces `org-batch-agenda-csv`, and the Rust engine must match those structured
rows for the supported subset.

Emacs remains the editor of record. Agenda detects dirty-buffer lock files,
revalidates source under its own sidecar lock, and refuses ambiguous or stale
writes. Enter in the TUI returns directly to Emacs, while a one-time explicit daemon
or batch probe can export declarative settings into a cold snapshot. Hooks,
advice, sexps, and function targets are retained as unsupported diagnostics,
not evaluated.

The repository includes the fixtures and probe scripts, so compatibility claims
are reproducible rather than screenshots.

https://github.com/johnnybravo-xyz/agd/releases

## nvim-orgmode discussion

**Agenda: local terminal agenda and safe writes beside nvim-orgmode**

Agenda reads the same Org files as nvim-orgmode and adds a fast Today/triage
TUI, deterministic JSON/CSV commands, and guarded local writes. It watches for
external edits and opens selected headings with direct Neovim argv rather than
embedding an editor.

The explicit `agd setup import nvim --profile ... --verify` probe runs against the
installed nvim-orgmode version on Neovim 0.12. It imports agenda files, TODO
keywords, tags, priorities, and declarative captures into a versioned cold
snapshot. Missing or incompatible runtime capabilities fail the import loudly.
Lua callbacks stay disabled and visible in the report. The setup-not-called
case is a hard error instead of guessed defaults.

I would especially welcome small fixtures for nvim-orgmode configuration shapes
that the probe should classify more accurately.

https://github.com/johnnybravo-xyz/agd/releases

## Show HN

**Show HN: Agenda – a terminal Org agenda tested against Emacs**

Agenda is a single local binary for Org agenda, capture, safe edits, a Ratatui
workflow, and stdio MCP. It has no server or account.

Two implementation choices make it different from another task-file parser:

1. Emacs is the executable specification. Pinned Org versions produce CSV rows
   for a fixture corpus, and the native engine compares dates, occurrence type,
   TODO state, inherited tags, priority, and ordering.
2. The parser is retained-source. Writes are minimal byte patches with fresh
   identity validation, a write-ahead inverse journal, fsync, atomic rename,
   and hash-guarded undo.

Coding-agent integrations are project-local and privacy-safe by default. No
agenda is injected into a session unless the user chooses a named filtered
profile. MCP is stdio only, and its writes are attributed in an Agent review
screen.

The compatibility table says exactly what is unsupported. I am most interested
in adversarial Org fixtures and crash/write-path review.

https://github.com/johnnybravo-xyz/agd/releases
