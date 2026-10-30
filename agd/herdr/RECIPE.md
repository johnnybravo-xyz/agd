# agd agenda panel

Build a native Herdr side panel for the local [agd](https://github.com/johnnybravo-xyz/agd) Org agenda.

## Behavior

- Dock on the right at 340 px the first time Herdr sees the mod.
- Run `agd agenda --span day --json` on load and once a minute.
- Show unfinished rows in agd's order, with timing, category, tags, priority, and overdue emphasis.
- Keep the last successful agenda visible when refresh fails, and show the error at the bottom.
- Provide **Refresh**, **Open** (`agd`), and **+** (`agd capture -i`) actions.
- Follow Herdr's active theme. Keep frequent actions immediate; do not animate them.

## Access

The mod makes no network requests. It runs the local `agd` executable, which reads the Org roots in agd's configuration. The Open and Capture actions start local terminal tabs through Herdr.
