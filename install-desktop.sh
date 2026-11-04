#!/usr/bin/env bash
# Adds CUT to app launchers (rofi drun, desktop menus): a desktop entry plus the icon in
# the usual sizes, for this user only. No sudo.
#   ./install-desktop.sh          install (builds first if needed)
#   ./install-desktop.sh --remove undo
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
BIN="$HERE/target/release/cut-deck"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
SIZES="16 24 32 48 64 128 256"

refresh() {
    command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -f -t -q "$ICONS" || true
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$APPS" || true
}

if [[ "${1:-}" == "--remove" ]]; then
    rm -f "$APPS/cut.desktop"
    for s in $SIZES; do rm -f "$ICONS/${s}x${s}/apps/cut.png"; done
    refresh
    echo "removed CUT from the launcher"
    exit 0
fi

if [[ ! -x "$BIN" ]]; then
    # rustup takes "+stable"; a distro-packaged cargo doesn't.
    if command -v rustup >/dev/null; then TC=+stable; else TC=; fi
    (cd "$HERE" && cargo $TC build --release)
fi

for s in $SIZES; do
    "$BIN" --write-icon "$ICONS/${s}x${s}/apps/cut.png" "$s"
done

mkdir -p "$APPS"
cat > "$APPS/cut.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=CUT
GenericName=Vinyl scratch deck
Comment=Dig a sample, scratch it with your fingers, record takes for your DAW
Keywords=scratch;vinyl;turntable;dj;sample;turntablism;fl studio;
Exec=$BIN %f
Icon=cut
Terminal=false
Categories=AudioVideo;Audio;Music;
MimeType=audio/wav;audio/x-wav;audio/mpeg;audio/flac;audio/ogg;
StartupWMClass=cut-deck
EOF

refresh
echo "CUT is in the launcher: $APPS/cut.desktop"
