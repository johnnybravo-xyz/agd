# agd

[![CI](https://github.com/johnnybravo-xyz/agd/actions/workflows/ci.yml/badge.svg)](https://github.com/johnnybravo-xyz/agd/actions/workflows/ci.yml)
[![agd on crates.io](https://img.shields.io/crates/v/agd.svg)](https://crates.io/crates/agd/0.0.1)
[![GitHub release](https://img.shields.io/github/v/release/johnnybravo-xyz/agd?filter=agd-v%2A)](https://github.com/johnnybravo-xyz/agd/releases/tag/agd-v0.0.1)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](https://github.com/johnnybravo-xyz/agd/blob/master/LICENSE)

`agd` is a local-first command-line tool for org-mode files. It searches and
reads an org knowledge base, renders agenda views, captures headings, and
updates TODO state and planning metadata without requiring Emacs at runtime.

Version [`0.0.1`](https://github.com/johnnybravo-xyz/agd/releases/tag/agd-v0.0.1)
is a functional fork baseline derived from
[`szaffarano/org-mcp-server`](https://github.com/szaffarano/org-mcp-server).
It exposes the inherited CLI as the
[`agd`](https://crates.io/crates/agd/0.0.1) package and executable. The shared
library is published as
[`agd-core`](https://crates.io/crates/agd-core/0.0.1); the workspace retains
the upstream-derived `org-core/` directory and `org-mcp-server` binary while
the product is consolidated.

## Install

Rust 1.89 or newer is required.

```bash
cargo install agd --locked
```

From a source checkout:

```bash
git clone https://github.com/johnnybravo-xyz/agd.git
cd agd
cargo install --path agd --locked
```

## Commands

```text
agd agenda         Agenda views and task management
agd capture        Capture a new heading
agd config         Initialize, inspect, or locate configuration
agd list           List org files
agd read           Read an org file
agd outline        Print an org file's heading outline
agd heading        Read one heading by outline path
agd element-by-id  Read an element by its ID property
agd search         Fuzzy-search the knowledge base
agd update-todo    Update heading state, planning, tags, properties, or body
```

Use `agd --help` and `agd <command> --help` for the complete argument surface.
Every command that returns structured data supports JSON where its `--format`
option is available.

### Examples

```bash
# Work against a directory without creating a config file.
agd --root-directory ~/org list
agd --root-directory ~/org search "project planning"

# Agenda views.
agd --root-directory ~/org agenda today
agd --root-directory ~/org agenda week --format json
agd --root-directory ~/org agenda list --states TODO,NEXT --tags work

# Capture a scheduled task.
agd --root-directory ~/org capture "Renew passport" \
  --todo-state TODO --priority A --scheduled "2026-10-15"

# Update a heading selected by its :ID: property.
agd --root-directory ~/org update-todo --id passport-renewal \
  --todo-state DONE

# Update a heading selected by file and outline path.
agd --root-directory ~/org update-todo \
  --file projects.org --heading "Personal/Passport" \
  --deadline "2026-10-30 -7d"
```

## Configuration

Create and inspect the default configuration:

```bash
agd config init
agd config path
agd config show
```

The default location is the platform configuration directory followed by
`agendum/config.toml`, for example:

- Linux: `~/.config/agendum/config.toml`
- macOS: `~/Library/Application Support/agendum/config.toml`

TOML, YAML, and JSON configuration files are supported. A minimal TOML file:

```toml
[org]
org_directory = "~/org"
org_default_notes_file = "inbox.org"
org_agenda_files = ["**/*.org"]
org_todo_keywords = ["TODO", "NEXT", "|", "DONE", "CANCELLED"]
org_auto_created_property = true
org_auto_closed_timestamp = true

[cli]
default_format = "plain"

[logging]
level = "info"
file = "~/.local/share/agendum/logs/agd.log"
```

CLI arguments override configuration values. Environment variables use the
`AGD_` prefix and `__` for nested keys:

```bash
AGD_ORG__ORG_DIRECTORY="$HOME/org" agd agenda today
AGD_CLI__DEFAULT_FORMAT=json agd search project
```

## Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo run -p agd -- --help
```

The workspace layout is:

- `agd/` — the published CLI package and `agd` executable.
- `org-core/` — the published `agd-core` library: parsing, discovery, agenda,
  search, capture, and update logic.
- `org-mcp-server/` — the upstream-derived stdio MCP server.
- `test-utils/` — shared fixtures and test helpers.

## Origin and license

This fork preserves the history and copyright notice of
[`szaffarano/org-mcp-server`](https://github.com/szaffarano/org-mcp-server).
The combined `agd` distribution and new contributions are licensed under
[GPL-3.0-or-later](https://github.com/johnnybravo-xyz/agd/blob/master/LICENSE).
Upstream-derived code remains available under its original
[MIT license](https://github.com/johnnybravo-xyz/agd/blob/master/LICENSE-MIT).
