# AUR package

`PKGBUILD` and `.SRCINFO` build agd from a release tag's source tarball. The
package will live at <https://aur.archlinux.org/packages/agd> once it is
published; until then, Arch users build it from this directory with
`makepkg -si`.

## Publish the first time

Needs an AUR account with your SSH public key added
(<https://aur.archlinux.org/account/>). New registrations were paused for a
while in October 2026; watch aur-general or the Arch news feed for reopening.

On Arch:

```sh
git clone ssh://aur@aur.archlinux.org/agd.git ~/aur-agd   # empty; the push creates the package
cp packaging/aur/PKGBUILD ~/aur-agd/
cd ~/aur-agd
makepkg --printsrcinfo > .SRCINFO
makepkg -si                     # build, run the tests, install
git add PKGBUILD .SRCINFO
git commit -m "agd X.Y.Z"
git push
```

## Update for a new release

After the release workflow has published tag `vX.Y.Z`:

1. `make aur VERSION=X.Y.Z` sets `pkgver`, `pkgrel=1` and the source
   tarball's checksum in both `PKGBUILD` and `.SRCINFO`; commit both files
   here as `aur: agd X.Y.Z`.
2. On Arch, check and build: `makepkg --printsrcinfo | diff - .SRCINFO`
   prints nothing, and `makepkg -si` builds, runs the tests and installs.
3. Copy both files into the AUR checkout and push, as above, with the commit
   message `agd X.Y.Z`.

Bump `pkgrel` instead of `pkgver` for a packaging-only change to the same
release.
