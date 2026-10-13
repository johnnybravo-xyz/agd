# Importing editor configuration

Import is a one-time probe, not a runtime dependency. It asks a specifically
selected Emacs or Neovim process for resolved values, validates them against
approved roots, and writes a versioned cold snapshot. Normal `agd` commands then
read only TOML and Org files.

## Precedence and trust

Configuration is merged in this order:

1. Built-in defaults.
2. The configured Emacs snapshot.
3. The configured Neovim snapshot.
4. Manual `config.toml` values.
5. `--root-directory` for the current invocation.

Snapshots cannot supply `files.roots`, import commands, or editor commands.
Manual configuration always owns those execution and privacy boundaries.
Agenda files emitted by a probe are canonicalized; an out-of-root file is
removed and reported as unsupported.

Snapshots use `agd.import/emacs/v1` or `agd.import/nvim/v1`, include the
editor and Org versions, source profile, host, extraction time, and a digest,
and are written atomically with mode `0600` on Unix.

## Emacs

A daemon is used only when configured and selected. For reproducible imports,
prefer an explicit batch profile:

```toml
[files]
roots = ["~/org"]

[imports.emacs]
snapshot = "imports/emacs.toml"
server = "personal"
prefer_daemon = true

[imports.emacs.batch_profiles.vanilla]
command = "emacs"
args = ["--quick"]

[imports.emacs.batch_profiles.doom]
command = "/home/me/.emacs.d/bin/doom"
args = ["run", "--"]
```

Preview the exact snapshot diff and verification report before writing:

```bash
agd import emacs --batch-profile vanilla --dry-run --verify
agd import emacs --batch-profile vanilla --stdout
agd import emacs --batch-profile vanilla --verify
```

`--server NAME` asks a specific running daemon (`emacsclient --socket-name`)
and overrides `imports.emacs.server`. A batch profile prints the command it is
about to run, because it executes your Emacs configuration.

Without `--batch-profile`, the configured daemon path is attempted. Failure is
explicit and recommends choosing a profile; `agd` never falls back to whichever
Emacs initialization happens to be available.

The fixed probe loads inside the chosen editor process and prints one marked
JSON payload. Startup chatter is ignored. A malformed or missing payload fails
the import.

## Neovim and nvim-orgmode

Neovim profiles are also executable plus argv arrays:

```toml
[imports.nvim]
snapshot = "imports/nvim.toml"
command = "nvim"
args = ["--headless"]

[imports.nvim.profiles.orgmode]
command = "nvim"
args = ["--headless", "-u", "/home/me/.config/nvim/init.lua"]
```

`-u` must point to the Neovim entrypoint that loads the plugin manager, normally
`~/.config/nvim/init.lua`. A Lazy.nvim plugin specification such as
`lua/plugins/orgmode.lua` returns configuration data to Lazy.nvim and is not a
standalone Neovim init file.

```bash
agd import nvim --profile orgmode --dry-run --verify
agd import nvim --profile orgmode --verify
```

The probe requires nvim-orgmode to be loaded and its setup state to be
available. A profile that has not called setup fails with a precise diagnostic
instead of manufacturing defaults.

## Verification output

`--verify` checks the snapshot through the native engine. Rows use three
statuses:

- `exact`: editor and native structured results agree.
- `difference`: a known representation or editor-only behavior differs; detail
  names both values.
- `unsupported`: the editor value cannot be represented safely or the fixture
  cannot be loaded inside the approved roots.

Verification includes core settings, capture entries, retained group/command
records, and agenda rows over the approved files. It never broadens roots just
to make a verification pass.

## Portable capture boundary

Supported capture destinations are file, file plus heading, outline path,
datetree, and ID. Supported template forms are literal text and `%?`, `%U`,
`%u`, `%T`, `%t`, `%i`, `%%`, `%^{prompt}`, and `%^g`, including file-based
template bodies. Titles, bodies and prompt answers are inserted literally;
escapes inside them are never expanded.
Dynamic function targets and `%(sexp)` are never evaluated. Such templates are
retained disabled with the original key and a diagnostic so they can be
replaced manually.

## Updating a snapshot

Editor configuration changes do not mutate snapshots automatically. Re-run the
same explicit import command, review `--dry-run --verify`, then write. The
manual config remains the final overlay, so local safety and privacy choices do
not disappear during refresh.
