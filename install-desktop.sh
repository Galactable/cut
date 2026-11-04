#!/usr/bin/env bash
# Adds CUT to app launchers (rofi drun, desktop menus): a desktop entry plus the icon in
# the usual sizes, for this user only. No sudo.
#   ./install-desktop.sh          install
#   ./install-desktop.sh --remove undo
# From a downloaded release (cut-deck sits next to this script) the program is copied to
# ~/.local/bin, so the download folder can be deleted afterwards. From the source folder
# it's built first if needed and run from target/release.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
LOCAL_BIN="$HOME/.local/bin/cut-deck"
if [[ -f "$HERE/cut-deck" ]]; then
    PACKAGED=1
    BIN="$LOCAL_BIN"
else
    PACKAGED=0
    BIN="$HERE/target/release/cut-deck"
fi
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
    if [[ $PACKAGED == 1 ]]; then rm -f "$LOCAL_BIN"; fi
    refresh
    echo "removed CUT from the launcher"
    exit 0
fi

if [[ $PACKAGED == 1 ]]; then
    install -Dm755 "$HERE/cut-deck" "$LOCAL_BIN"
elif [[ ! -x "$BIN" ]]; then
    # rustup takes "+stable"; a distro-packaged cargo doesn't.
    if command -v rustup >/dev/null; then TC=+stable; else TC=; fi
    (cd "$HERE" && cargo $TC build --release)
fi

for s in $SIZES; do
    "$BIN" --write-icon "$ICONS/${s}x${s}/apps/cut.png" "$s"
done

TYPES="audio/wav audio/x-wav audio/mpeg audio/flac audio/ogg"
# CUT should show up under "Open with", not become the default player: note each
# type's current default so it can be put back after installing.
declare -A BEFORE
if command -v xdg-mime >/dev/null; then
    for t in $TYPES; do BEFORE[$t]=$(xdg-mime query default "$t" 2>/dev/null || true); done
fi

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
if command -v xdg-mime >/dev/null; then
    # xdg-mime writes ~/.config/mimeapps.list and fails if the folder isn't there yet.
    mkdir -p "${XDG_CONFIG_HOME:-$HOME/.config}"
    for t in $TYPES; do
        prev=${BEFORE[$t]:-}
        if [[ -n "$prev" && "$prev" != "cut.desktop" ]]; then
            xdg-mime default "$prev" "$t" || true
        fi
    done
fi
echo "CUT is in the launcher: $APPS/cut.desktop"
if [[ $PACKAGED == 1 ]]; then
    echo "CUT is installed at $LOCAL_BIN"
    case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) echo "(~/.local/bin isn't on your PATH: start CUT from the launcher, or add it to run cut-deck in a terminal)";; esac
fi
echo "Audio files still open in your usual player; CUT is under \"Open with\"."
