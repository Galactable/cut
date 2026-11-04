# CUT

Scratch a sample with your fingers on a laptop touchpad or a Wacom tablet's touch surface, then record the takes for your DAW.

## Run

```bash
cd ~/Projects/cut && cargo +stable build --release
./target/release/cut-deck "path/to/sample.wav"
```

You can also start it with no file and drop a file onto the window or press **O**.

To add CUT to your app launcher (rofi drun, `$mod+d`) with its icon:

```bash
~/Projects/cut/install-desktop.sh
```

`--remove` takes it out again. Re-run it if you move the project folder.

## What makes it sound like vinyl

- **Your fingers control the record's position, not its speed.** It's the same as a hand on vinyl, so speed comes from how you move.
- **The record sits on a slipmat.** The motor keeps turning underneath, so letting go speeds the record up in about 12 ms, and motor start/stop spin up and down like a Technics.
- **It plays through a modelled cartridge and phono stage.** The groove is cut with RIAA EQ and read back as stylus velocity. Slow drags sound bassy and soft, fast cuts sound bright and punchy, and holding still is silent. **V** turns this off so you can compare.
- **Fast scratches don't alias.** The resampler's filter follows the speed, so you don't get the usual digital fizz.
- **Touch movement is smoothed.** Touch surfaces report 100-130 times a second. Each point is glided to and passed through a spring, so you hear motion instead of a stair-step buzz. **-** and **=** set how much smoothing there is.

## Other devices: DJ controllers, gamepads, dials, pads (X)

Press **X** (or Devices in the menu bar) to see every input device CUT can use, including ones plugged in while it's running. Touch any control and its device lights up in the list.

- **Guided setup** walks you through it. First, move what you want to scratch with (a jog wheel, dial, touch ring, knob or stick) and press Enter. Then turn it exactly once around to calibrate a record turn. Then press the buttons for the crossfader, hold record (or touch a jog's top plate), motor, back to cue and record take, and set the volume control. **Tab** skips a step.
- Every other key command can be given a control too: pick its row and press the control. **Del** clears a row.
- **Sensitivity** (← →) and **scratch direction** are per device, on top of the controller **[ ]** setting.
- MIDI controllers work with no setup. Relative jogs (64-centred or two's complement), absolute knobs and faders, notes and CC buttons are told apart while you learn them.
- While CUT is focused it takes a set-up device from the desktop, so its buttons don't also type or click. It gives the device back when you switch away. You can turn this off per device.
- Touch surfaces have one setting: work as a **touchpad** (two fingers scratch, one is the cursor) or a **tablet** (one finger scratches).
- A device marked **needs access** can be unlocked from the menu (asks for your password). This writes a udev rule for that one device only, by vendor, product and name. `./input-access.sh --remove` takes all of it back.
- Setups are plain text files in `~/.config/cut/devices/`, one per device model, so you can copy them to another machine or share one for a controller.

## Wacom Intuos Pro

The ExpressKeys and touch ring work as soon as the tablet is plugged in. The pad comes set up with the layout below, and you can change it under **X**. `tablet-setup.sh` still works if you prefer the desktop to handle the keys:

```bash
~/Projects/cut/tablet-setup.sh
```

- **One fingertip** on the tablet's touch surface is your hand on the record. While CUT is focused the desktop ignores the touch surface, so it's scratch-only; switch away or quit and it's a normal touch surface again. Quick taps work for stops and skips.
- A grab holds until every finger lifts, so a finger skipping off mid-stroke doesn't let go.
- Keep the pen off the tablet: the tablet ignores touch while a pen is near it. CUT doesn't use the pen.
- The top 4 ExpressKeys are the crossfader: any of them held cuts the sound. **I** flips this to hamster mode, where held opens it. 
- The remaining ExpressKeys are motor, back to cue, record take, hold record and click. The touch ring is volume.
- To undo the tablet setup: `./tablet-setup.sh --reset`.

Press **F11** for fullscreen, so the whole tablet surface is scratch area. **TAB** switches between left/right, up/down and circular platter motion.

## Laptop touchpad

One-time setup, so CUT can see your fingers directly (asks for your sudo password):

```bash
~/Projects/cut/input-access.sh
```

This only gives read access to touchpads, tablets, tablet pads and gamepads. Keyboards and mice stay root-only.

- Two fingers down puts your hand on the record. Move them to scratch, hold them still to hold the record, lift to let go.
- One finger is the normal cursor. While two fingers hold the record, the desktop pauses the touchpad, so nothing scrolls or clicks outside the app.
- Touch only scratches while the CUT window is focused.
- Without the setup, two-finger scrolling still scratches, but pausing counts as letting go.

## Feel

- Each surface (touchpad, tablet touch, mouse) has its own sensitivity and scratch motion, and **[ ]** and **TAB** change the one you used last. They're saved in `~/.config/CUT/settings.txt`.
- **[** and **]** change sensitivity (shown as x1.00). Controllers share one setting, and each device has its own on top of it (see **X**). At x1.00 a quarter-second scratch is about 1.6 cm on the touchpad and 5 cm on the tablet.
- **-** and **=** change smoothing: lower feels heavier, higher feels snappier.
- **TAB** switches the scratch motion: AUTO (follows whatever line you scratch along, right/up = forward), left/right, up/down, or PLATTER (circle round the centre of the tablet, clockwise = forward, one turn = 1.8 s like a real 33). Tablet touch defaults to platter, and a dot on the on-screen record shows where your finger is.

## Start-up and view

- CUT opens in full view, paused, with an empty record on the platter. Press **M** to start it: the empty run-out groove plays hiss, crackle and a scuff that ticks once per turn until you load a sample.
- **H** shows or hides the key guide.

## Library and takes

- **F** opens the folders menu: choose where saved samples and takes go, browse to a folder or make a new one, and choose whether to move your existing files there. Takes kept inside the samples folder move with it.

- Drag on the top waveform to choose the section that goes on the record. Grab an edge of the selection to move just that edge. **A** selects the whole file.
- Zoom the top waveform for precise selection: scroll over it (two fingers on the touchpad, or the mouse wheel) to zoom around the pointer, scroll sideways to pan, and **Z** jumps to the section and back out. The edge times and zoom level show while zoomed.
- The section normally sits on the record with 1 s of blank groove before it (so you can pull back past the start) and 3 s after. **G** makes it loop seamlessly instead, with no gaps.
- **S** saves the section (clean) to `<samples folder>/<Folder>/` (default `~/Music/CUT`).
- **,** and **.** step to the previous and next sample in the same folder.
- **R** starts or stops a take of CUT's own output (after the fader and volume; never the click or other apps), saved to the takes folder (default `~/Music/CUT/Takes`, 32-bit float WAV).
- **T** turns on the click, and **9** / **0** change the BPM (hold Ctrl for steps of 10). With the click on, a take starts on the next bar, so it lines up when you drop it at a bar in FL. The click is never recorded.

## Version and what's new

The status bar shows the version in its bottom right corner. After an update it says **NEW**: click it or press **F1** for the release notes (`CHANGELOG.md`).

## Running on other Linux systems

CUT runs on any Linux desktop with X11. On Wayland it runs through XWayland (most Wayland desktops include it), but the touchpad and touch features that use `xinput` don't work there.

- **Sound:** CUT uses PipeWire's JACK support, or a JACK server, when one is running. Otherwise it plays through ALSA, which also covers PulseAudio systems through ALSA's pulse plugin. The only library it needs is `libasound` (alsa-lib), which every desktop distro installs. JACK is loaded only if it's there.
- **Needed for parts of the app:** `zenity` (open and save dialogs), `xinput` (touchpad handling), `pkexec` with a polkit agent (giving access from the Devices menu; otherwise it prints the `sudo` command to run), and `xsetwacom` (only for `tablet-setup.sh`).
- **Building:** Rust (rustup), a C compiler, and the ALSA headers: `alsa-lib` on Arch/CachyOS, `libasound2-dev` on Debian/Ubuntu, `alsa-lib-devel` on Fedora.

## Latency

Use wired headphones or the laptop speakers. Bluetooth headphones add 150 ms or more, which makes tight scratching impossible. The app asks PipeWire for a 128-sample buffer (about 3 ms).

## License

CUT is released into the public domain under [CC0 1.0](LICENSE). Copy it, change it and pass it on: no permission or credit needed.

The libraries CUT is built with keep their own licenses.
