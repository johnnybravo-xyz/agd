#!/usr/bin/env sh
set -eu

root=${1:-target/nvim-0.12.6}
archive=${root}.tar.gz
url=https://github.com/neovim/neovim/releases/download/v0.12.6/nvim-linux-x86_64.tar.gz
sha256=474430d53e6264f6d6dd18db42d6dc9df3a1b56ca9e88a325bbf860e1a811d87

if [ -x "$root/bin/nvim" ]; then
    "$root/bin/nvim" --version | grep -q '^NVIM v0.12.6$'
else
    mkdir -p "$(dirname "$archive")"
    curl -L --fail --output "$archive" "$url"
    printf '%s  %s\n' "$sha256" "$archive" | sha256sum -c -
    mkdir -p "$root"
    tar -xzf "$archive" -C "$root" --strip-components=1
fi

printf '%s\n' "$root/bin/nvim"
