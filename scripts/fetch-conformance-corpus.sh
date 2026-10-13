#!/usr/bin/env sh
set -eu

root=${1:-target/conformance-corpus}
org_revision=75234089cd3f3229a9becf3ffb334b3df212a57e
worg_revision=fc5e668072052c75909eb8fba64a6c72c40780a8

fetch() {
    name=$1
    url=$2
    revision=$3
    destination="$root/$name"
    if [ -d "$destination/.git" ] && [ "$(git -C "$destination" rev-parse HEAD)" = "$revision" ]; then
        return
    fi
    rm -rf "$destination"
    git clone --filter=blob:none --no-checkout "$url" "$destination"
    git -C "$destination" fetch --depth=1 origin "$revision"
    git -C "$destination" checkout --detach FETCH_HEAD
}

mkdir -p "$root"
fetch org-mode https://git.savannah.gnu.org/git/emacs/org-mode.git "$org_revision"
fetch worg https://git.sr.ht/~bzg/worg "$worg_revision"
printf '%s\n' "$root"
