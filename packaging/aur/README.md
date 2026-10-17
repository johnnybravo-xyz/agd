# Arch User Repository package

`PKGBUILD` and `.SRCINFO` build `agd` from a tagged source archive. The package
will be available at <https://aur.archlinux.org/packages/agd> after its first
publication. Until then, Arch users can build directly from this directory with
`makepkg -si`.

## Prerequisites

Publishing requires:

- an AUR account;
- an SSH public key registered at <https://aur.archlinux.org/account/>;
- an Arch Linux environment for package generation and verification.

If AUR registration is unavailable, check Arch announcements and the
`aur-general` mailing list before attempting the first publication.

## First publication

Create the AUR repository and populate it from this checkout:

```sh
git clone ssh://aur@aur.archlinux.org/agd.git ~/aur-agd
cp packaging/aur/PKGBUILD ~/aur-agd/
cd ~/aur-agd
makepkg --printsrcinfo > .SRCINFO
makepkg -si
git add PKGBUILD .SRCINFO
git commit -m "agd X.Y.Z"
git push
```

The initially empty AUR repository becomes the package when the first commit is
pushed.

## Release update

After the release workflow publishes `vX.Y.Z`:

1. From the main repository, run `make aur VERSION=X.Y.Z`. The target sets
   `pkgver`, resets `pkgrel` to `1`, and updates the tagged archive checksum in
   both `PKGBUILD` and `.SRCINFO`.
2. Review both generated files and commit them in this repository with a subject
   such as `aur: agd X.Y.Z`.
3. In an Arch environment, verify generated metadata:

   ```sh
   makepkg --printsrcinfo | diff - .SRCINFO
   makepkg -si
   ```

   The first command should produce no output; the second builds, tests, and
   installs the package.
4. Copy `PKGBUILD` and `.SRCINFO` to the AUR checkout, commit them as
   `agd X.Y.Z`, and push.

For a packaging-only correction to an existing release, increment `pkgrel`
instead of `pkgver`.
