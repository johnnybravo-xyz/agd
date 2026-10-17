#!/bin/sh
# Copies the repository's canonical guide and generated key reference into the
# site, alongside the existing release posts and demo GIF:
#   docs/getting-started.md -> site/content/docs/getting-started.md
#   docs/KEYS.md            -> site/content/docs/keys.md
#   docs/release/posts/*.md -> site/content/posts/*.md
#   docs/release/agd.gif    -> site/content/agd.gif
# A file's leading "# Title" becomes its front matter title; docs also get the
# one-line description listed below. Posts are dated by the commit that added
# them, so this needs full git history. The copies are gitignored; run this
# before `zola build` in site/.
set -eu
cd "$(dirname "$0")/.."
content=site/content

rm -f \
  "$content/docs/importing.md" \
  "$content/docs/compatibility.md" \
  "$content/docs/safety.md"

page() { # page <source> <target> <extra front matter line>
  title=$(sed -n '1s/^# //p' "$1" | sed 's/"/\\"/g')
  if [ -z "$title" ]; then
    echo "collect.sh: $1 does not start with '# Title'" >&2
    exit 1
  fi
  { printf '+++\ntitle = "%s"\n%s\n+++\n' "$title" "$3"; tail -n +2 "$1"; } > "$2"
  echo "collect.sh: $1 -> $2"
}

# source|target|description, in the order the site lists them
weight=0
while IFS='|' read -r source target description; do
  weight=$((weight + 1))
  page "docs/$source" "$content/docs/$target" "weight = $weight
description = \"$description\""
done <<'EOF'
getting-started.md|getting-started.md|Install, configure, import, connect agents, and understand compatibility and safety
KEYS.md|keys.md|Generated keyboard reference for the full TUI, pane, Triage, Agents, and popups
EOF

for post in docs/release/posts/*.md; do
  date=$(git log --diff-filter=A --follow --format=%as -- "$post" | tail -n 1)
  if [ -z "$date" ]; then
    echo "collect.sh: no commit adds $post; is the history shallow?" >&2
    exit 1
  fi
  page "$post" "$content/posts/$(basename "$post")" "date = $date"
done

cp docs/release/agd.gif "$content/agd.gif"
echo "collect.sh: docs/release/agd.gif -> $content/agd.gif"
