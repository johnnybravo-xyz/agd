#!/bin/sh
# Copies the repository's docs into the site so each has one source:
#   docs/{IMPORTING,COMPATIBILITY,SAFETY}.md -> site/content/docs/<name>.md
#   docs/release/posts/*.md                  -> site/content/posts/*.md
#   docs/release/agd.gif                     -> site/content/agd.gif
# A file's leading "# Title" becomes its front matter title; docs also get
# the one-line description listed below. Posts are dated by the commit that
# added them, so this needs full git history. The copies are gitignored; run
# this before `zola build` in site/.
set -eu
cd "$(dirname "$0")/.."
content=site/content

page() { # page <source> <target> <extra front matter line>
  title=$(sed -n '1s/^# //p' "$1" | sed 's/"/\\"/g')
  if [ -z "$title" ]; then
    echo "collect.sh: $1 does not start with '# Title'" >&2
    exit 1
  fi
  { printf '+++\ntitle = "%s"\n%s\n+++\n' "$title" "$3"; tail -n +2 "$1"; } > "$2"
  echo "collect.sh: $1 -> $2"
}

# name|description, in the order the site lists them
weight=0
while IFS='|' read -r name description; do
  weight=$((weight + 1))
  lower=$(echo "$name" | tr '[:upper:]' '[:lower:]')
  page "docs/$name.md" "$content/docs/$lower.md" "weight = $weight
description = \"$description\""
done <<'EOF'
IMPORTING|Bring settings over from a running Emacs or nvim-orgmode, and verify them
COMPATIBILITY|What matches Emacs exactly, and what is reported instead
SAFETY|Roots, locks, the journal, atomic writes and undo
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
