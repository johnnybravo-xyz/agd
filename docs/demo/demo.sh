#!/bin/sh
# Opens agd on a sample org tree with dates relative to today, in a
# throwaway directory, with two agent changes waiting on the Agents tab:
# your own files, config and journal are not touched. Needs python3.
#
#   docs/demo/demo.sh            full TUI
#   docs/demo/demo.sh --pane     the narrow pane
set -eu
here=$(cd "$(dirname "$0")" && pwd)
agd=${AGD:-$(command -v agd || echo "$here/../../target/debug/agd")}
# Absolute, so it still runs after the seed below changes directory.
case $agd in */*) agd=$(cd "$(dirname "$agd")" && pwd)/$(basename "$agd") ;; esac
demo=$(mktemp -d "${TMPDIR:-/tmp}/agd-demo.XXXXXX")
python3 "$here/render.py" "$demo/org"
cat > "$demo/config.toml" <<CONFIG
roots = ["$demo/org"]

[[capture]]
key = "t"
desc = "Todo"
file = "$demo/org/inbox.org"
heading = "Inbox"
template = "* TODO %?\n:PROPERTIES:\n:CREATED:  %U\n:END:"

[[capture]]
key = "n"
desc = "Note"
file = "$demo/org/inbox.org"
heading = "Inbox"
template = "* %?\n%U"

[[view]]
key = "w"
desc = "Work focus"
blocks = [
  { kind = "agenda", span = "day" },
  { kind = "tags-todo", match = "work/NEXT|WAIT", header = "Next and waiting" },
]
CONFIG
export HOME="$demo/home" XDG_DATA_HOME="$demo/home/.local/share"
# The agents worked in a project of their own, which the Agents tab names.
mkdir -p "$HOME/code/app"
(cd "$HOME/code/app" && python3 "$here/seed_agents.py" "$agd" "$demo/config.toml")
exec "$agd" --config "$demo/config.toml" "$@"
