#!/usr/bin/env sh
set -eu

root=${1:-target/nvim-0.12.6}
archive=${root}.tar.gz
url=https://github.com/neovim/neovim/releases/download/v0.12.6/nvim-linux-x86_64.tar.gz
sha256=474430d53e6264f6d6dd18db42d6dc9df3a1b56ca9e88a325bbf860e1a811d87

if [ -x "$root/bin/nvim" ]; then
    "$root/bin/nvim" --version | grep -q '^NVIM v0.12.6$'
else
    curl -L --fail --output "$archive" "$url"
    printf '%s  %s\n' "$sha256" "$archive" | sha256sum -c -
    mkdir -p "$root"
    tar -xzf "$archive" -C "$root" --strip-components=1
fi

plugin_root=${2:-target/nvim-orgmode-0.7.5}
plugin_commit=359538284a21c1edac20cb8a0b6b772ca098098e
if [ ! -d "$plugin_root/.git" ]; then
    git clone --depth=1 --branch 0.7.5 https://github.com/nvim-orgmode/orgmode.git "$plugin_root"
fi
test "$(git -C "$plugin_root" rev-parse HEAD)" = "$plugin_commit"
printf '%s\n%s\n' "$root/bin/nvim" "$plugin_root"
