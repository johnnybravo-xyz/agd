# Release tasks for agd. Development commands live in the justfile.
#
# A release, in order:
#   make version VERSION=0.2.0   set the version everywhere and date the changelogs
#   (commit)                     review the diff and commit it
#   make tag                     check the release, then sign and tag v$(VERSION)
#   make push                    push main and the tag: CI builds the binaries,
#                                updates the Homebrew tap and signs SHA256SUMS
#   make verify                  check the published SHA256SUMS signature
#   make publish                 publish agd-core, then agd, to crates.io
#   make aur                     point packaging/aur at the tagged tarball

SHELL := /bin/sh
.DEFAULT_GOAL := help

REMOTE ?= gh
REPO := johnnybravo-xyz/agd
CURRENT := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
VERSION ?= $(CURRENT)
TAG := v$(VERSION)
TODAY := $(shell date +%Y-%m-%d)

# cargo, with Apple's toolchain on macOS outside `nix develop` (see the script).
CARGO := scripts/cargo

.PHONY: help version release-check tag push publish verify aur

help: ## List the release targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{ printf "  %-14s %s\n", $$1, $$2 }'

version: ## Set VERSION=x.y.z in every manifest and date both changelogs
	@test "$(VERSION)" != "$(CURRENT)" || { echo "pass VERSION=x.y.z; $(CURRENT) is already the version"; exit 1; }
	perl -0pi -e 's/^version = "\Q$(CURRENT)\E"/version = "$(VERSION)"/m' Cargo.toml agd/Cargo.toml agd-core/Cargo.toml
	perl -pi -e 's/(agd-core = \{ version = )"\Q$(CURRENT)\E"/$$1"$(VERSION)"/' agd/Cargo.toml
	perl -pi -e 's/"version": "\Q$(CURRENT)\E"/"version": "$(VERSION)"/' agd/claude-plugin/.claude-plugin/plugin.json
	perl -pi -e 's/^## \[Unreleased\]$$/## [Unreleased]\n\n## [$(VERSION)] - $(TODAY)/' agd/CHANGELOG.md agd-core/CHANGELOG.md
	$(CARGO) update --workspace
	@echo "$(CURRENT) -> $(VERSION); review the diff and commit it"

release-check: ## Check that v$(VERSION) is ready to tag, changing nothing
	@git diff --quiet && git diff --cached --quiet || { echo "commit or stash your changes first"; exit 1; }
	@test "$$(git rev-parse --abbrev-ref HEAD)" = main || { echo "release from main"; exit 1; }
	@! git rev-parse -q --verify "refs/tags/$(TAG)" >/dev/null || { echo "$(TAG) already exists"; exit 1; }
	@for file in agd/Cargo.toml agd-core/Cargo.toml; do \
		grep -q '^version = "$(VERSION)"' $$file || { echo "$$file is not at $(VERSION)"; exit 1; }; \
	done
	@grep -q '"version": "$(VERSION)"' agd/claude-plugin/.claude-plugin/plugin.json || { echo "plugin.json is not at $(VERSION)"; exit 1; }
	@for file in agd/CHANGELOG.md agd-core/CHANGELOG.md; do \
		grep -q '^## \[$(VERSION)\]' $$file || { echo "$$file has no $(VERSION) entry"; exit 1; }; \
	done
	just check

tag: release-check ## Sign and tag v$(VERSION) at HEAD
	git tag -s $(TAG) -m "agd $(VERSION)"

push: ## Push main and v$(VERSION); CI builds, signs and updates the tap
	git push $(REMOTE) main $(TAG)

publish: ## Publish agd-core, then agd, to crates.io
	$(CARGO) publish -p agd-core
	$(CARGO) publish -p agd

verify: ## Check v$(VERSION)'s SHA256SUMS against its Sigstore bundle
	@directory=$$(mktemp -d) && \
	gh release download $(TAG) -R $(REPO) -p SHA256SUMS -p SHA256SUMS.sigstore.json -D $$directory && \
	cosign verify-blob --bundle $$directory/SHA256SUMS.sigstore.json \
		--certificate-identity-regexp '^https://github.com/$(REPO)/.github/workflows/sign-release.yml@' \
		--certificate-oidc-issuer https://token.actions.githubusercontent.com \
		$$directory/SHA256SUMS

aur: ## Point packaging/aur at v$(VERSION)'s source tarball
	@sum=$$(curl -fsSL https://github.com/$(REPO)/archive/refs/tags/$(TAG).tar.gz | shasum -a 256 | cut -d ' ' -f 1) && \
	perl -pi -e "s/^pkgver=.*/pkgver=$(VERSION)/; s/^pkgrel=.*/pkgrel=1/; s/^sha256sums=.*/sha256sums=('$$sum')/" packaging/aur/PKGBUILD && \
	perl -pi -e "s/pkgver = .*/pkgver = $(VERSION)/; s/pkgrel = .*/pkgrel = 1/; s/agd-[^:]*\.tar\.gz::/agd-$(VERSION).tar.gz::/; s{tags/v[^/]*\.tar\.gz}{tags/$(TAG).tar.gz}; s/sha256sums = .*/sha256sums = $$sum/" packaging/aur/.SRCINFO && \
	echo "packaging/aur at $(VERSION) ($$sum); commit it, then push it to the AUR"
