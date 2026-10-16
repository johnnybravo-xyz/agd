#!/bin/sh
# Renders the site's screenshots (site/content/*.webp) from the demo in
# docs/demo: drives agd in a tmux session, waits for each screen to settle,
# turns the captured ANSI text into SVG with freeze, rasterizes it with
# headless Chrome (freeze's own rasterizer has no font fallback, so glyphs
# like ↻ come out blank), and encodes it as webp.
#   cargo build --release
#   nix shell nixpkgs#charm-freeze nixpkgs#libwebp -c site/screenshots.sh
# tmux runs on its own server and Chrome with its own profile, so neither
# touches your sessions or your browser.
set -eu
cd "$(dirname "$0")/.."
out=site/content
work=$(mktemp -d)
export AGD="$PWD/target/release/agd"
chrome=${CHROME:-"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"}
esc=$(printf '\033')

t() { tmux -L agd-screenshots "$@"; }
start() { # start <session> <cols> <rows> [agd args]
  session=$1; cols=$2; rows=$3; shift 3
  t kill-session -t "$session" 2>/dev/null || true
  t new-session -d -s "$session" -x "$cols" -y "$rows" \
    "COLORTERM=truecolor docs/demo/demo.sh $*; sleep 60"
  sleep 4
}
keys() { t send-keys -t "$session" "$@"; sleep 1; }
shot() {
  # freeze ignores SGR 49 (default background), so a highlighted row's
  # background would run on to the end of the line; spell the default out.
  t capture-pane -p -e -t "$session" |
    sed "s/${esc}\[49m/${esc}[48;2;20;23;28m/g" |
    freeze --language ansi --window=false --padding 24 --margin 0 \
      --background "#14171c" --border.radius 0 --font.size 15 \
      --output "$work/$1.svg" >/dev/null
  size=$(sed -nE 's/.*<svg width="([0-9.]+)" height="([0-9.]+)".*/\1 \2/p' "$work/$1.svg")
  width=$(echo "$size" | awk '{ print int($1) }')
  height=$(echo "$size" | awk '{ print int($2) }')
  "$chrome" --headless=new --disable-gpu --hide-scrollbars \
    --user-data-dir="$work/chrome" \
    --window-size="$width,$height" --screenshot="$work/$1.png" \
    "file://$work/$1.svg" 2>/dev/null
  cwebp -quiet -q 90 "$work/$1.png" -o "$out/$1.webp"
}

start agd 150 30
shot today
keys j j j j j; keys Space; shot peek
keys Space; keys Tab; shot week
keys Tab; keys Tab; shot review
keys 1; keys T; shot triage
keys Escape; keys c; keys t
t send-keys -t agd -l "Renew passport due:fri on:+1d #personal !A"; sleep 1
shot capture
keys Escape; keys a; keys w; shot views
keys Escape; keys '?'; shot help
t kill-session -t agd

start agd 38 24 --pane
shot pane
t kill-server
