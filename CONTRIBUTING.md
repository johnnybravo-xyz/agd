# Contributing

Start with [ARCHITECTURE.md](ARCHITECTURE.md) for where things live and
[docs/DESIGN.md](docs/DESIGN.md) for how agd looks, reads and behaves.
[AGENTS.md](AGENTS.md) lists the product rules; they apply to people too.

## Set up

You need Rust 1.89 or later. Either use the Nix shell:

```bash
nix develop
```

or rustup and [just](https://github.com/casey/just). On macOS outside the
Nix shell, `scripts/cargo` links with Apple's clang and SDK; the justfile and
the Makefile use it for you.

Optional, for the checks that compare agd with the real editors: Docker (the
Emacs and Neovim oracles), the `claude` CLI (the Claude Code plugin's tests)
and [vhs](https://github.com/charmbracelet/vhs) (the demo GIFs).

## Develop

```bash
just              # list the commands
just run agenda --status
just build
just install      # a release build in ~/.local/bin, which the plugin runs
just plugin-dev   # Claude Code with the plugin from this checkout
```

To try agd without your own files, run the demo. It renders sample org
files into a temporary directory and points HOME and XDG there:

```bash
AGD=target/release/agd docs/demo/demo.sh          # the TUI
AGD=target/release/agd docs/demo/demo.sh --pane   # the narrow pane
```

Never point a work-in-progress agd at your real org files. Tests and manual
runs use a temporary root, with `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and
`HOME` set inside it, or `--config`.

## Check

```bash
just check        # fmt, clippy, tests, the plugin, nix flake check
just oracles      # agenda, edit and import against pinned Emacs and Neovim
```

CI runs the format, clippy and test steps on Linux, macOS and Windows; run
`just plugin` yourself when you change `agd/claude-plugin/`, and
`just oracles` when you change agenda output, an edit or the importer (it
needs Docker).

## Change things

Put behavior in `agd-core`, then expose it through the thinnest CLI, TUI or
MCP code that needs it.

- **A command**: a file in `agd/src/commands/`, registered in `main.rs`.
  Text output says things through `words.rs`; `--json` prints a schema from
  `agd-core/src/model.rs`; errors print through `main.rs`.
- **A key**: one entry in a table in `agd/src/tui/keys.rs`. Footers, Triage's
  chips and `?` read the same table. Check that the key means the same
  thing on every screen.
- **A config key**: the field in `agd-core/src/config/model.rs`, its check in
  `config/validation.rs` (name the key and the accepted values in the
  error), its place in `CONFIG_GROUPS` in `config/file.rs`, and the import
  if the editors have an equivalent.
- **An MCP tool**: a section in `agd/src/mcp/tools.rs` with a request struct
  that refuses unknown fields and describes every argument; add it to the
  server instructions if an agent needs to know about it.
- **An error**: a variant in `ErrorCode` (`agd-core/src/model.rs`), snake_case,
  with a message that says what happened and what to do.
- **A color or glyph**: `agd/src/tui/theme.rs`. The Claude Code pane and the
  CLI's terminal colors read from it.

Writes must keep untouched bytes exactly as they were. Test that with the
file's bytes before and after, including a refusal that leaves nothing
behind. Behavior that Org also has should match Org: add a case under
`conformance/` and record Emacs's answer with the oracle (`--update`).

## Commit

- One behavior per commit, with its tests, so each commit can be reviewed
  and reverted on its own.
- Sign commits (`git commit -S`); keep messages short and plain.
- Update the crate's `CHANGELOG.md` for anything a user or a library
  consumer would notice, and `README.md` when commands or installation
  change.

Releases use the Makefile (`make help`); see its header for the order.

## Report a security problem

See [SECURITY.md](SECURITY.md). Please don't open a public issue for it.
