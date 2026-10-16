#!/bin/sh
# Sets up docs/release/story.tape: a throwaway agenda from docs/demo/org, an
# `agd` wrapper that only sees it (put its directory first on PATH), the
# small project Claude Code works in, and two earlier Codex changes. Prints
# the throwaway directory.
#
# Claude Code's header shows the project's path, so the project lives at
# $STORY_PROJECT (default ~/code/ingest); a marker file keeps this script
# off a folder it did not create.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
agd=${AGD:-$here/../../../target/release/agd}
agd=$(cd "$(dirname "$agd")" && pwd)/$(basename "$agd")
run=$(mktemp -d "${TMPDIR:-/tmp}/agd-story.XXXXXX")
mkdir -p "$run/bin" "$run/config/agd" "$run/data"
python3 "$here/../render.py" "$run/org"
cat > "$run/config/agd/config.toml" <<CONFIG
roots = ["$run/org"]
start_view = "d"

[[capture]]
key = "t"
desc = "Todo"
file = "$run/org/inbox.org"
heading = "Inbox"
template = "* TODO %?\n:PROPERTIES:\n:CREATED:  %U\n:END:"

[[view]]
key = "d"
desc = "Dashboard"
blocks = [
  { kind = "agenda", span = "day" },
  { kind = "todo", match = "NEXT", header = "Next" },
  { kind = "todo", match = "WAIT", header = "Waiting" },
]
CONFIG
# Every agd the session starts, the mod's and the MCP server's, runs here.
cat > "$run/bin/agd" <<WRAP
#!/bin/sh
export XDG_CONFIG_HOME="$run/config" XDG_DATA_HOME="$run/data"
exec "$agd" "\$@"
WRAP
chmod +x "$run/bin/agd"

project=${STORY_PROJECT:-$HOME/code/ingest}
if [ -e "$project" ] && [ ! -e "$project/.agd-story" ]; then
  echo "$project exists and is not the story project" >&2
  exit 1
fi
mkdir -p "$project"
touch "$project/.agd-story"
cp -R "$here/ingest/." "$project/"
if [ ! -d "$project/.git" ]; then
  (cd "$project" && printf '.agd-story\n__pycache__/\n' > .gitignore && git init -q && git add -A &&
    git -c user.name=demo -c user.email=demo@example.com commit -q -m "ingest: dedupe and backfill")
fi
(cd "$project" && python3 "$here/seed_codex.py" "$run/bin/agd")
echo "$run"
