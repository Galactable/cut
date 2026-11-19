# What's new in CUT

## 0.9.1
- `tablet-setup.sh` and the device setup files no longer say the Devices menu is on **X**: it's **File > Devices** now.

## 0.9.0
- Fewer keys. The keyboard now only has what you reach for while playing: **Space** (fader), **Shift** (hold), **M**, **C**, **F2 F3 F4**, **Up**/**Down**, **R**, **T**, **I**, **O**, **W**, **,** **.**, **S**, **A**, **Z**, **G**, **Y**, **B**, **F12**, **Backslash** (freeze the strip), **H**, **F11** and **Ctrl+Q**. Everything else (scratch feel, sound, save format, profiles, folders, devices, backing volume, pictures and the rest of the view) is in the menus, so a stray key can't change your setup mid-scratch. **J**, **K** and **L** no longer work the fader, and **Enter** and **Backspace** no longer double as motor and back to cue.
- Menu items that step a value (volume, sensitivity, smoothing, BPM, tempo, dim, strip zoom) keep the menu open, so you can click them again and again.
- Fixing the beat has its own mode. Choose **Backing > Fix the beat**: the beat keys (**/** to tap, **7** **8**, **;** **'**, **9** **0**, **1**) work, and a strip over the lower waveform lists them, until you press **Esc**. The rest of the time they do nothing, so a stray press can't move your beat.
- Remove a hot cue: hold its button (or **F2**-**F4**) for three seconds. At one second the cue is set as before; keep holding and its box empties in red, and at three seconds the cue is gone.
- More for your device buttons: whole sample, trainer strip on/off, freeze the strip, fix the beat and tap the beat can now go on any button in **File > Devices**. Device buttons no longer depend on the keyboard, so your setups keep working.
- The key guide (**H**) only shows the keys that still do something, and the menus show a key only for the commands that have one.
- Fixed: on X11, CUT could open twice as large as it should on laptops with a small, sharp screen (such as a 14" 1080p panel). It now only scales up on X11 when you've set `Xft.dpi`.

## 0.8.0
- CUT runs natively on Wayland. On a Wayland desktop (GNOME, KDE Plasma, Sway, Hyprland and others) it opens as a proper Wayland window instead of going through XWayland, and the touchpad and tablet work as they do on X11.
- On a touchpad, scratching with two fingers no longer also scrolls, and taps on the record no longer click, on Wayland too. The one difference from X11: with the pointer resting over another window while you scratch, that window may scroll.
- Sharp on high-resolution screens. With your desktop's scaling at 150% or more, CUT draws everything twice as large (three times from 250%), so text and the waveforms are no longer tiny. `CUT_SCALE=1` (or 2, 3) picks the size yourself.
- The launcher entry and icon are now called `cut-deck`, matching the window, so Wayland desktops show CUT's icon in the dock and taskbar. Run `./install-desktop.sh` again to update it; the old entry is removed.
- Not yet on Wayland: dropping a file onto the window. Press **O** to open one.
- `tablet-setup.sh` is for X11. On Wayland it says so and points you to the Devices menu (**X**), where the ExpressKeys are already set up.

## 0.7.0
- The trainer strip. Press **F12** and the lower waveform becomes a picture of your scratching against the beat: time runs right to left past a green "now" line, height is where the record is in the sample, and the upright lines are the beats, with each bar's first beat brightest. The line is green while your hand is on the record, lilac while it plays by itself and grey while the fader is closed, and a white dot marks every change of direction, so you can see whether your scratches land on the beat.
- **Backslash** freezes the strip so you can look at what you just played, and **backtick** (the key under Esc) changes how much it shows: 4, 8, 16 or 32 beats. All three are in the View menu.
- CUT finds the backing track's beat. When a backing track loads, its tempo and where its bars start are worked out, and the status bar shows its BPM next to BACKING.
- The click follows the track. While the backing track plays, **T** clicks on the track's own beat (the status bar says CLICK ON THE BEAT) with nothing to set. With the track paused it's the free click at your own BPM, as before.
- When the beat is found wrong, fix it by hand: play the track and tap **/** on the beat, starting on beat one (it follows you from the fourth tap); **7** and **8** halve and double the tempo; **;** and **'** move the beats 5 ms earlier or later (with Ctrl, a whole beat, to change which beat is one); **9** and **0** fine-tune the track's tempo by 0.1 BPM (with Ctrl, 1). **1** goes back to what CUT found. Your corrections are remembered for each track. They're all in the Backing menu.
- A picture on just the label. **2** switches your record picture (**F6**) between the whole record and only the label in the middle, with the grooves around it as usual. It's in the View menu as "Record image covers" and is remembered. It starts on the whole record, as before.

## 0.6.1
- Long files no longer freeze the window. Loading a file, and picking a long section of it (**A** for the whole sample, a long drag, or switching EQ, noise or loop), now happens in the background: the window stays live and what's on the record keeps playing until the new one is ready.
- A loading indicator. While that's going on, a green box over the waveform says "loading" with the file's name, or "pressing the record", with moving dots and a count of seconds.
- Long files load much faster. With vinyl noise on, putting an hour-long track on the record took about 20 seconds; it's now under 10, and picking the whole sample again takes about 3.
- An hour-long file needs noticeably less memory while its record is being pressed.

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
