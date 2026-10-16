# Development commands for agd. Release tasks live in the Makefile.

coverage_dir := "coverage"

# cargo, with Apple's toolchain on macOS outside `nix develop` (see the script).
cargo := "scripts/cargo"

# List the commands
_default:
    @just --list

# === Build and run ===

# Build every crate and target
build:
    {{ cargo }} build --workspace --all-targets

# Run agd with arguments, such as `just run agenda --status`
run *args:
    {{ cargo }} run -q -p agd -- {{ args }}

# Install a release build to ~/.local/bin, the agd Claude Code's plugin runs
install:
    #!/usr/bin/env sh
    set -eu
    # Inside the Nix shell on macOS the binary would link a libiconv from the
    # Nix store, which garbage collection can remove.
    if [ "{{ os() }}" = macos ] && [ -n "${IN_NIX_SHELL:-}" ]; then
        echo "run just install outside nix develop on macOS, so agd links the system libiconv" >&2
        exit 1
    fi
    {{ cargo }} install --path agd --locked --root "$HOME/.local" --force

# Open Claude Code with the plugin loaded from this checkout
plugin-dev:
    claude --plugin-dir ./agd/claude-plugin

# === Checks ===

# Run every repository check, as AGENTS.md lists them
check: fmt-check lint test plugin
    nix flake check

# Run the workspace tests
test:
    {{ cargo }} test --workspace --all-targets --all-features

# Run clippy with warnings as errors
lint:
    {{ cargo }} clippy --workspace --all-targets --all-features -- -D warnings

# Format the code
fmt:
    {{ cargo }} fmt --all

# Check the formatting without changing it
fmt-check:
    {{ cargo }} fmt --all --check

# Validate and test the Claude Code plugin
plugin:
    claude plugin validate --strict ./agd/claude-plugin
    claude plugin test ./agd/claude-plugin

# Compare agenda, edit and import behavior with the pinned Emacs and Neovim (needs Docker)
oracles:
    python3 scripts/emacs-agenda-oracle.py
    python3 scripts/emacs-edit-oracle.py
    python3 scripts/emacs-import-oracle.py
    python3 scripts/nvim-import-oracle.py

# Record the README demo GIF from docs/release/agd.tape (needs vhs)
demo:
    vhs docs/release/agd.tape

# === Coverage ===

# Generate the HTML coverage report
coverage-html:
    {{ cargo }} llvm-cov --workspace --all-features --html --output-dir {{ coverage_dir }}/html
    @echo "Open {{ coverage_dir }}/html/index.html"

# Show the coverage summary
coverage-summary:
    {{ cargo }} llvm-cov --workspace --all-features --summary-only

# Write LCOV coverage for CI
coverage-ci:
    mkdir -p {{ coverage_dir }}
    {{ cargo }} llvm-cov --workspace --all-features --lcov --output-path {{ coverage_dir }}/lcov.info

# Write JSON coverage
coverage-json:
    mkdir -p {{ coverage_dir }}
    {{ cargo }} llvm-cov --workspace --all-features --json --output-path {{ coverage_dir }}/coverage.json

# Generate every coverage format
coverage: coverage-html coverage-ci coverage-json coverage-summary

# Remove build artifacts and coverage reports
clean:
    {{ cargo }} clean
    rm -rf {{ coverage_dir }}/
