#!/usr/bin/env bash
# Points the AUR package at a released version and, if you have the AUR repo cloned,
# publishes it there.
#   packaging/aur/update.sh            version from Cargo.toml
#   packaging/aur/update.sh 0.3.4      a specific version (must already be tagged on GitHub)
# The AUR clone lives at $AUR_REPO (default ~/aur/cut-deck); see packaging/aur/README.md.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
V=${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)}
AUR_REPO=${AUR_REPO:-$HOME/aur/cut-deck}

URL="https://github.com/Galactable/cut/archive/v$V.tar.gz"
SUM=$(curl -fsSL "$URL" | sha256sum | cut -d' ' -f1) || { echo "v$V isn't on GitHub yet" >&2; exit 1; }

cd "$HERE"
OLD=$(sed -n 's/^pkgver=//p' PKGBUILD)
sed -i "s/^pkgver=.*/pkgver=$V/; s/^sha256sums=.*/sha256sums=('$SUM')/" PKGBUILD
# Same version again (a packaging fix): next pkgrel. New version: back to 1.
if [[ "$OLD" == "$V" ]]; then
    REL=$(( $(sed -n 's/^pkgrel=//p' PKGBUILD) + 1 ))
else
    REL=1
fi
sed -i "s/^pkgrel=.*/pkgrel=$REL/" PKGBUILD
makepkg --printsrcinfo > .SRCINFO
echo "PKGBUILD now builds cut-deck $V-$REL"

if [[ -d "$AUR_REPO/.git" ]]; then
    cp PKGBUILD .SRCINFO cut-deck.install "$AUR_REPO/"
    cd "$AUR_REPO"
    git add PKGBUILD .SRCINFO cut-deck.install
    if git diff --cached --quiet; then
        echo "AUR already has $V-$REL"
    else
        git commit -m "Update to $V-$REL"
        git push
        echo "published to the AUR: https://aur.archlinux.org/packages/cut-deck"
    fi
else
    echo "(no AUR clone at $AUR_REPO - see packaging/aur/README.md to set it up)"
fi
