# What's new in CUT

## 0.3.4
- Ready-made download: each release now has a `cut-<version>-linux-x86_64.tar.gz` that runs on current Ubuntu, Debian, Fedora, Arch and their relatives. Extract it and run `./install-desktop.sh`, with no building needed.
- On Arch, CachyOS and Manjaro, CUT is on the AUR as `cut-deck`, with touchpad and tablet access included.
- Share a device setup or report a problem from the issue forms on GitHub. Shared setups can become built-in for everyone with that device.
- The source install steps in the README are checked automatically on fresh Ubuntu, Fedora and Arch systems. `curl` was missing from the package list; it's added.
- `install-desktop.sh` works on a brand-new account that has no settings folder yet.

## 0.3.3
- Choosing a section on the top waveform is smoother. Dragging towards either end while zoomed in scrolls the waveform, and the start of your selection stays put while it does.
- On the touchpad, the top waveform never grabs the record: a click-and-drag there (with a thumb or second finger) selects instead of scratching or freezing the pointer.
- Touchpad taps work for switching windows again. CUT only turns tap-to-click off while the pointer is over its own window, not the whole time it's focused, so tapping another window switches to it.
- Learning a Wacom pad button no longer also picks up the pad's hidden "activity" signal. Setups that already have it are cleaned up automatically.
- Alt opens the menu bar only when pressed and released on its own, so Alt+Tab and other Alt shortcuts don't pop it open.
- The Devices menu no longer redraws constantly while a pen hovers over the tablet.
- Volume goes to 150% from a knob or slider too, the same as the keys. VOL turns red above 100%, where loud peaks start getting squashed (in takes too).
- Enter (motor) and Backspace (back to cue) are now listed in the key guide and menus.
- Device setup files only list what you've changed, so a touchpad's file no longer carries settings it doesn't use. Older files are tidied automatically.
- README corrections: fullscreen doesn't change the tablet's scratch area, and MIDI controllers still need their controls learned.

## 0.3.2
- You can always see whether the backing track goes into your takes. The status bar shows BACKING with "IN TAKES" in red, or "not in takes", and while recording, REC says "scratch + backing" or "scratch only".
- Starting a take with the backing track playing tells you which one you're recording.

## 0.3.1
- A take that stays completely silent (motor off, fader cut, nothing loaded) isn't kept, so accidental record presses don't leave empty files. CUT tells you when this happens.
- Installing CUT to the launcher no longer makes it the default app for audio files. They keep opening in your usual player, and CUT is under "Open with".

## 0.3.0
- Backing track: press Y to load a beat or song that plays under the record, with its own volume (5 and 6) and B to play or pause. It loops, and it restarts from the top whenever a take starts recording, so your take lines up with it at 0:00 in your DAW.
- Takes are the scratch alone by default, ready to drop over the beat. Press U to record the full mix with the backing track instead.
- Takes survive crashes: the file is saved as you record, so if CUT is closed, killed or crashes mid-take you keep everything up to the last half second. Quitting while recording finishes the take properly.
- Volume shows in the status bar.
- Wacom touch ring: lifting your finger and putting it down elsewhere no longer jumps the volume.
- Devices menu: touch surfaces light up in the list when touched, like other devices.

## 0.2.0
- Devices menu (X): CUT now finds input devices as you plug them in, including MIDI DJ controllers, gamepads, dials, foot pedals and tablet pads. Touch any control and its device lights up in the list.
- Guided setup: move what you want to scratch with, then press the buttons for the crossfader, hold, motor, cue and record. Every control runs exactly what its key does.
- Tune any device: sensitivity, scratch direction, one-turn calibration for jog wheels and rings, and any control can be learned again or cleared. Setups are saved per device in ~/.config/cut/devices.
- Touch surfaces can be switched between touchpad (two fingers scratch) and tablet (one finger scratches).
- Devices CUT can't read yet can be given access from the menu. The access covers only that one device.
- While CUT is focused it takes a set-up device from the desktop, so pad buttons don't also click, and gives it back when you switch away.
- Wacom tablet pads work with no setup script: ExpressKeys and the touch ring come set up like tablet-setup.sh.
- Version badge in the status bar. It says NEW after an update; click it or press F1 for these notes.

## 0.1.0
- First version: scratch with a touchpad, a Wacom tablet's touch surface or the mouse, with a modelled cartridge, slipmat and motor.
- Sample library, section selection and zoom, takes recorded to WAV or MP3, a click track and profiles.
