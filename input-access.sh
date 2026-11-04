#!/usr/bin/env bash
# Lets the logged-in user read touchpads and drawing tablets directly, so CUT
# can see finger contact and exact pen positions. Keyboards and mice stay root-only.
#   ./input-access.sh           install (asks for your sudo password)
#   ./input-access.sh --remove  undo (also takes back access given from CUT's Devices menu)
set -euo pipefail
RULE=/etc/udev/rules.d/70-cut-input.rules
# The rule's name from when the app was called scratchdeck.
OLD_RULE=/etc/udev/rules.d/70-scratchdeck-input.rules
# Single devices given access from CUT's Devices menu (X).
DEVICE_RULES=/etc/udev/rules.d/70-cut-devices.rules

if [[ "${1:-}" == "--remove" ]]; then
    sudo rm -f "$RULE" "$OLD_RULE" "$DEVICE_RULES"
else
    sudo tee "$RULE" >/dev/null <<'RULES'
# CUT: give the seat's active user read access to touchpads, tablets, tablet pads and gamepads only.
SUBSYSTEM=="input", KERNEL=="event*", ENV{ID_INPUT_TOUCHPAD}=="1", TAG+="uaccess"
SUBSYSTEM=="input", KERNEL=="event*", ENV{ID_INPUT_TABLET}=="1", TAG+="uaccess"
SUBSYSTEM=="input", KERNEL=="event*", ENV{ID_INPUT_TABLET_PAD}=="1", TAG+="uaccess"
SUBSYSTEM=="input", KERNEL=="event*", ENV{ID_INPUT_JOYSTICK}=="1", TAG+="uaccess"
RULES
fi
sudo udevadm control --reload
sudo udevadm trigger --subsystem-match=input --action=change
sleep 1
for d in /dev/input/event*; do
    n=$(cat "/sys/class/input/$(basename "$d")/device/name" 2>/dev/null || true)
    if [[ "$n" =~ [Tt]ouchpad|Wacom ]]; then
        if [[ -r "$d" ]]; then echo "readable: $n"; else echo "NOT readable: $n"; fi
    fi
done
