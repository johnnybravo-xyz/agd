+++
title = "agd"
template = "project.html"
description = "A local-first Org agenda for terminals and coding agents"
+++

<!-- Links and images are relative so the same content also serves from
     https://johnnybravo.xyz/projects/agd/. collect.sh copies agd.gif in. -->

`agd` brings the Org files and settings you already use with Emacs or
nvim-orgmode into a keyboard-first terminal UI. The same local agenda is
available to scripts and coding agents through a CLI and stdio MCP server. Org
remains the source of truth; writes preserve bytes outside the requested change.

![agd showing Today, Week, Agents, Triage, Capture, and saved views](agd.gif)

- **21 agenda fixtures** whose text and CSV match Emacs byte for byte
- **27 edit fixtures** whose resulting files match Emacs byte for byte
- **0 unrelated bytes** changed by a successful patch
- **1 binary** for the TUI, pane, CLI, and MCP server

```sh
cargo install agd --locked
brew install johnnybravo-xyz/agd/agd
nix run github:johnnybravo-xyz/agd
```

[Source](https://github.com/johnnybravo-xyz/agd) ·
[Documentation](docs/getting-started/) · [Keys](docs/keys/) ·
[Changelog](https://github.com/johnnybravo-xyz/agd/blob/main/agd/CHANGELOG.md)

## A focused terminal workflow

Today orders work by the decision it requires: OVERDUE, SCHEDULE, DUE SOON,
DOING, then INBOX. Week, saved views, and imported custom agenda commands use
the same Org-compatible engine.

![Today with the entry preview open](peek.webp)

Press `T` to process overdue and inbox work one card at a time. Each decision is
one key; `u` safely undoes one step or the complete pass.

![Triage](triage.webp)

Press `c`, or run `agd capture`, to file through an Org capture template:

```text
Renew passport due:fri #personal !A
```

The popup parses dates, tags, and priority and shows the exact Org text before
writing. Capture supports notes, meetings, datetrees, prompts, bodies, and
checklists.

![Capture with exact Org preview](capture.webp)

The Agents tab records the MCP client and project for every agent write. `a`
accepts a record without changing Org, `u` performs guarded undo, and `SPC`
shows the diff and task.

![Agent change review](review.webp)

`agd --pane` is a narrow companion for tmux and herdr. It shows timed items,
marks what is next, and summarizes overdue work.

![Narrow pane](pane.webp)

## Keep existing Org settings

```sh
agd setup import ~/.doom.d
# also ~/.config/emacs, ~/.emacs.d, and ~/.config/nvim
```

The importer runs `emacs -Q` or `nvim --clean`, reports every value it cannot
represent, and writes one flat `config.toml`. In-file directives continue to
win. Agenda and edit fixtures compare the supported subset byte for byte with
pinned Emacs 30.2 and Org 9.7; import fixtures also use pinned Neovim 0.12.6.

![Week using org-agenda-compatible prefixes](week.webp)

![Saved views imported from custom agenda commands](views.webp)

## Connect local coding agents

```sh
agd setup install --dry-run --detected
agd setup install --detected
```

Detection configures existing Codex, OpenCode, Pi, and oh-my-pi user
directories. Claude Code uses the `agd` plugin. Paseo reuses the configuration,
skills, and MCP servers of the provider it launches. Any local stdio client can
start `agd mcp`; GUI applications may need the binary's absolute path.

The server opens no port and collects no telemetry. It can access only Org files
under configured roots on the same machine. See the
[documentation](docs/getting-started/) for exact client setup and the local/cloud
boundary.

## Org files remain your data

All writes resolve identity again under lock, refuse stale or ambiguous targets,
patch only intended ranges, journal inverse patches, and atomically replace the
file. Multi-file writes are transactional. Dry runs create nothing, and undo
refuses bytes that changed or became ambiguous.

## Try the isolated demo

```sh
git clone https://github.com/johnnybravo-xyz/agd
cd agd
cargo build --release
AGD=target/release/agd docs/demo/demo.sh
```

The demo requires `python3`, uses temporary home, config, data, and Org
directories, and leaves personal files untouched.
