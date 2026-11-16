# What's new in CUT

## 0.6.0
- Your own background. Press **F5** and choose a picture (PNG, JPG or WebP): it fills the window behind the waveforms and the record. **F7** sets how much it's dimmed, from 0% to 80% in steps of 20, so the waveforms and key guide stay easy to read. It starts at 60%.
- Your own picture on the record. Press **F6** and choose one: it covers the whole record and turns with it, so you can see the spin at a glance.
- Pressing **F5** or **F6** again takes that picture away; press once more to choose another. Both are remembered for next time.
- **F8** turns the record's groove rings off, for a plain flat record or a clean picture (with them on, the rings show faintly over your picture), and **F9** turns the green cue line off. Both are on unless you change them.
- All five are in the View menu and the key guide. With no pictures chosen and nothing switched off, CUT looks exactly as it did.

## 0.5.0
- Record what's playing. Press **W** and CUT records whatever your computer is playing: a YouTube video, a film in Plex, anything. Press **W** again and it's on the record, ready to scratch. Drag the top waveform to pick your cut and **S** saves it to the library as usual, so there's no more recording somewhere else, importing and cutting.
- The silence before and after the sound is trimmed off, so you can press **W** first and then start the video. The whole recording is also kept in a `Captures` folder inside your samples folder.
- CUT goes quiet while it records, so the record, the click and the backing track don't end up in the recording. The status bar shows a timer, and the level meter shows what's being picked up. A recording stops by itself after 10 minutes.
- It's in the File menu, and any device button can start and stop it from the Devices menu (X). It needs `parec`, which most desktops already have (package `pulseaudio-utils`, or `libpulse` on Arch).
- The smoothing amount is back in view: the status bar shows SMOOTH with its value next to the input and sensitivity, so you can see what **-** and **=** are doing as you press them.

## 0.4.0
- Hot cues. The top three ExpressKeys on a Wacom Intuos Pro (or F2, F3, F4 on the keyboard) are hot cues 1-3. Hold one for a second to set that cue where the needle is in your section. A box next to the needle fills up while you hold, and a numbered marker appears on both waveforms. Tap the key to jump straight to that cue.
- Hot cues are cleared when you load another file. Picking a new section keeps the ones still inside it.
- The 4th ExpressKey, next to the touch ring, is now the crossfader, and the button in the ring holds the record. Below the ring are back to cue, record take, click and motor. `tablet-setup.sh` uses the same layout.
- Hot cues can also be given to any device button from the Devices menu (X).

## 0.3.4
- Ready-made download: each release now has a `cut-<version>-linux-x86_64.tar.gz` that runs on current Ubuntu, Debian, Fedora, Arch and their relatives. Extract it and run `./install-desktop.sh`, with no building needed.
- On Arch, CachyOS and Manjaro, CUT can be built as a proper package (`makepkg -si` in `packaging/aur`), with touchpad and tablet access included. It's coming to the AUR as `cut-deck` once AUR sign-ups reopen.
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
