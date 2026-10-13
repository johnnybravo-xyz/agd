# agd repository instructions

`agd` is a Rust 2024 workspace for local-first org-mode tooling. The published
`agd` crate is the CLI. The `org-core/` directory publishes the `agd-core`
business-logic crate, and the upstream-derived `org-mcp-server` crate exposes
part of that logic over stdio MCP.

## Workspace

```text
agd-core  <--  agd
agd-core  <--  org-mcp-server
test-utils <-- agd-core (dev), agd (dev), org-mcp-server (dev)
```

- `agd/` — CLI entry point, command implementations, and CLI integration tests.
- `org-core/` — published `agd-core`: parsing, discovery, agenda, capture, search, and updates.
- `org-mcp-server/` — MCP protocol adapter and integration tests.
- `test-utils/` — shared deterministic fixtures.

New business behavior belongs in `org-core` and is exposed through `agd` and/or
`org-mcp-server`. Shared dependency versions belong in the root `Cargo.toml`.

## Build and validation

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo run -p agd -- --help
cargo run -p agd -- --root-directory test-utils/fixtures list
cargo run -p agd -- --root-directory test-utils/fixtures search project
```

Do not cancel builds or tests while Cargo is compiling dependencies. After a
CLI change, run the actual affected command against `test-utils/fixtures/` or a
temporary org directory in addition to automated tests.

## Configuration

The default configuration is the platform config directory followed by
`agendum/config.toml`. Resolution order is defaults, config file, environment,
then CLI flags. Environment variables use the `AGD_` prefix and `__` separator,
for example `AGD_ORG__ORG_DIRECTORY`.

`OrgConfig` and `LoggingConfig` are defined in `org-core/src/config.rs`.
CLI-only configuration is defined in `agd/src/config.rs`.

## Code conventions

- Format with rustfmt before committing.
- Prefer standard-library imports before external crate imports.
- Use captured formatting such as `"{value}"` rather than positional formatting.
- Return typed errors instead of panicking in runtime code.
- Serialize environment-mutating tests with `serial_test`.
- Keep tests deterministic and use shared fixtures for consumer-visible behavior.

## Important implementation details

- `OrgMode` in `org-core/src/org_mode/types.rs` is the central service object.
- `OrgMode::capture_append` is the capture write path. It validates input,
  serializes concurrent writers with a sidecar lock, writes through a temporary
  file, fsyncs, and atomically renames.
- `agd update-todo` can select a heading by `:ID:` or by file plus outline path.
- The MCP server uses `rmcp` and currently retains the `org-mcp-server` binary
  name while the workspace is consolidated.

See `README.md` for the supported command surface and licensing. The combined
fork is GPL-3.0-or-later; `LICENSE-MIT` preserves the upstream notice.
