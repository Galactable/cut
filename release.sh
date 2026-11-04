#!/usr/bin/env bash
# Publishes the version in Cargo.toml: runs the tests, commits everything, tags it,
# pushes, and makes the GitHub release with that version's notes from CHANGELOG.md.
#   ./release.sh                     commit message "CUT x.y.z"
#   ./release.sh "what changed"      your own commit message
set -euo pipefail
cd "$(dirname "$0")"

V=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[[ -n "$V" ]] || { echo "no version in Cargo.toml" >&2; exit 1; }
if git rev-parse -q --verify "refs/tags/v$V" >/dev/null; then
    echo "v$V is already released - raise the version in Cargo.toml first" >&2
    exit 1
fi
# This version's notes: everything under "## $V" up to the next "## ".
NOTES=$(awk -v v="$V" '/^## /{on = ($2 == v); next} on' CHANGELOG.md | sed '/./,$!d')
[[ -n "$NOTES" ]] || { echo "CHANGELOG.md has no \"## $V\" section" >&2; exit 1; }

echo "== testing CUT $V"
if command -v rustup >/dev/null; then TC=+stable; else TC=; fi
cargo $TC test --release --quiet

echo "== publishing CUT $V"
git add -A
if ! git diff --cached --quiet; then
    git commit -m "${1:-CUT $V}"
fi
git tag "v$V"
git push
git push origin "v$V"
gh release create "v$V" --title "CUT $V" --notes "$NOTES" --latest
echo "released: https://github.com/Galactable/cut/releases/tag/v$V"
