#!/usr/bin/env bash
# Sets up a Wacom Intuos Pro for CUT (X11 / xf86-input-wacom).
#   ./tablet-setup.sh          scratch layout
#   ./tablet-setup.sh --reset  back to normal desktop behaviour
# Settings last until the tablet is unplugged or you log out; just run it again.
set -euo pipefail

dev() { xsetwacom --list devices | grep -i "$1" | sed -E 's/[[:space:]]+id:.*//' | head -1; }
PEN=$(dev "stylus")
TOUCH=$(dev "touch")
PAD=$(dev "pad")

if [[ -z "$PEN$TOUCH$PAD" ]]; then
    echo "No Wacom tablet found - is it plugged in?  (xsetwacom --list devices)" >&2
    exit 1
fi
echo "pen:   ${PEN:-none}"
echo "touch: ${TOUCH:-none}"
echo "pad:   ${PAD:-none}"

if [[ "${1:-}" == "--reset" ]]; then
    [[ -n "$TOUCH" ]] && { xinput enable "$TOUCH"; xsetwacom set "$TOUCH" Gesture on; xsetwacom set "$TOUCH" Mode Relative; }
    if [[ -n "$PAD" ]]; then
        for b in 1 2 3 8 9 10 11 12 13; do xsetwacom set "$PAD" Button "$b" "button +$b" || true; done
        xsetwacom set "$PAD" AbsWheelUp "button +4"
        xsetwacom set "$PAD" AbsWheelDown "button +5"
    fi
    echo "reset done"
    exit 0
fi

# Finger touch: CUT takes it from the desktop only while its window is focused
# (one fingertip = hand on the record) and gives it back when you switch away or quit,
# so here it's just left as a normal, enabled touch surface.
if [[ -n "$TOUCH" ]]; then
    xinput enable "$TOUCH"
fi

# ExpressKeys for the other hand. "+key" holds the key for as long as the button is held.
#   ring button: hold record (shift)
#   top three: hot cues 1-3 (tap = jump, hold 1 s = set)
#   4th: crossfader (held = cut; press I in the app for hamster)
#   below the ring: back to cue, record take, click, motor
if [[ -n "$PAD" ]]; then
    xsetwacom set "$PAD" Button 1 "key +shift"
    xsetwacom set "$PAD" Button 2 "key +F2"
    xsetwacom set "$PAD" Button 3 "key +F3"
    xsetwacom set "$PAD" Button 8 "key +F4"
    xsetwacom set "$PAD" Button 9 "key +space"
    xsetwacom set "$PAD" Button 10 "key c"
    xsetwacom set "$PAD" Button 11 "key r"
    xsetwacom set "$PAD" Button 12 "key t"
    xsetwacom set "$PAD" Button 13 "key m"
    # The touch ring scrolls by default, and scrolling scratches; make it volume instead.
    xsetwacom set "$PAD" AbsWheelUp "key Up"
    xsetwacom set "$PAD" AbsWheelDown "key Down"
fi

echo "done - one fingertip on the tablet = hand on record, ExpressKeys = hot cues, fader + transport, ring = volume"
