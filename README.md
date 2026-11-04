# CUT

A vinyl scratch deck for Linux. Load a sample, then scratch it with your fingers on a laptop touchpad, a drawing tablet's touch surface, a MIDI DJ controller's jog wheel, or nearly any other input device. Load a beat as a backing track to scratch over, then record your takes as WAV or MP3 to drop into your DAW.

It's modelled on a real turntable: your hand controls the record's position, the platter spins on a slipmat, and the sound goes through a modelled cartridge and phono stage.

## Install

CUT runs on Linux with an X11 desktop (see [Running on other Linux systems](#running-on-other-linux-systems)). Build it from source:

**1. Install the build tools and libraries**

```bash
# Arch, CachyOS, Manjaro
sudo pacman -S --needed base-devel git alsa-lib zenity xorg-xinput
# Debian, Ubuntu, Mint, Pop!_OS
sudo apt install build-essential git pkg-config libasound2-dev zenity xinput
# Fedora
sudo dnf install gcc git pkgconf-pkg-config alsa-lib-devel zenity xinput
```

**2. Install Rust** (skip this if you already have it)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**3. Download and build CUT**

```bash
git clone https://github.com/Galactable/cut.git
cd cut
cargo build --release
```

**4. Optional extras**

```bash
./install-desktop.sh   # adds CUT to your app launcher, with its icon
./input-access.sh      # lets CUT read touchpads, tablets and gamepads directly (asks for your password)
```

Audio files keep opening in your usual player; CUT shows up under "Open with". `./install-desktop.sh --remove` takes CUT out of the launcher again. Re-run it if you move the folder.

**Update** to the newest version later:

```bash
cd cut && git pull && cargo build --release
```

## Run

```bash
./target/release/cut-deck "path/to/sample.wav"
```

Or start it from your app launcher. You can also start it with no file and drop a file onto the window, or press **O**.

## Quick start: scratch a sample over a beat

1. **Load the sample you want to scratch.** Press **O** (or drop the file onto the window). Drag across the top waveform to pick the part that goes on the record: a word, a stab, an "ahh".
2. **Load the beat.** Press **Y** and choose a beat or a whole song. It plays under the record and loops.
3. **Start both.** Press **B** to play the beat and **M** to start the record turning.
4. **Scratch.** Put your hand on the record (two fingers on a touchpad, one on a tablet, a jog wheel, or a mouse drag) and cut with the crossfader (**Space**, **J**, **K**, **L**, or the buttons you set up under **X**). The crossfader only cuts the record, so the beat keeps playing.
5. **Balance the levels.** **Up**/**Down** set the record's volume, and **5**/**6** set the beat's.
6. **Record a take.** Press **R** to start and **R** again to stop. The beat jumps back to its start the moment recording begins, so the take and the beat begin together.
7. **Use it in your DAW.** Put the beat on one track and the take on another, both starting at the same point. They line up. The take is the scratch alone, so you can mix, EQ and edit it separately.

To keep the beat in the take as well (for a practice clip or a quick share), press **U** before recording.

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
- MIDI controllers show up without any access setup; learn their controls here like any other device. Relative jogs (64-centred or two's complement), absolute knobs and faders, notes and CC buttons are told apart while you learn them.
- While CUT is focused it takes a set-up device from the desktop, so its buttons don't also type or click. It gives the device back when you switch away. You can turn this off per device.
- Touch surfaces have one setting: work as a **touchpad** (two fingers scratch, one is the cursor) or a **tablet** (one finger scratches).
- A device marked **needs access** can be unlocked from the menu (asks for your password). This writes a udev rule for that one device only, by vendor, product and name. `./input-access.sh --remove` takes all of it back.
- Setups are plain text files in `~/.config/cut/devices/`, one per device model, so you can copy them to another machine or share one for a controller. They only list what you've set: controls, plus `grab=false`, `sens=` or `reverse=true` if you changed those from the defaults.

## Wacom Intuos Pro

The ExpressKeys and touch ring work as soon as the tablet is plugged in, once CUT can read the tablet (run `./input-access.sh` once, or unlock the pad from **X**). The pad comes set up with the layout below, and you can change it under **X**. `tablet-setup.sh` still works if you prefer the desktop to handle the keys:

```bash
./tablet-setup.sh
```

- **One fingertip** on the tablet's touch surface is your hand on the record. While CUT is focused the desktop ignores the touch surface, so it's scratch-only; switch away or quit and it's a normal touch surface again. Quick taps work for stops and skips.
- A grab holds until every finger lifts, so a finger skipping off mid-stroke doesn't let go.
- Keep the pen off the tablet: the tablet ignores touch while a pen is near it. CUT doesn't use the pen.
- The top 3 ExpressKeys and the button in the middle of the touch ring are the crossfader: any of them held cuts the sound. **I** flips this to hamster mode, where held opens it.
- The 4th key is motor, and the 4 keys below the ring are back to cue, record take, hold record and click. The touch ring is volume. Change any of it under **X**.
- To undo the tablet setup: `./tablet-setup.sh --reset`.

The whole touch surface is scratch area whatever size the window is; CUT reads the tablet directly. **TAB** switches between AUTO, left/right, up/down and circular platter motion.

## Laptop touchpad

One-time setup from the `cut` folder, so CUT can see your fingers directly (asks for your sudo password):

```bash
./input-access.sh
```

This only gives read access to touchpads, tablets, tablet pads and gamepads. Keyboards and mice stay root-only.

- Two fingers down puts your hand on the record. Move them to scratch, hold them still to hold the record, lift to let go.
- One finger is the normal cursor. While two fingers hold the record, the desktop pauses the touchpad, so nothing scrolls or clicks outside the app.
- Touch only scratches while the CUT window is focused.
- Tap-to-click is off while the pointer is over CUT (a tap is a quick record stop there). Move the pointer onto another window and tapping works again, so you can tap it to switch.
- Without the setup, two-finger scrolling still scratches, but pausing counts as letting go.

## Feel

- Each input (touchpad, tablet touch, mouse, controllers) has its own sensitivity, and the touch surfaces and mouse have their own scratch motion. **[ ]** and **TAB** change the one you used last. They're saved in `~/.config/cut/settings.txt`.
- **[** and **]** change sensitivity (shown as x1.00). Controllers share one setting, and each device has its own on top of it (see **X**). At x1.00 a quarter-second scratch is about 1.6 cm on the touchpad and 5 cm on the tablet.
- **-** and **=** change smoothing: lower feels heavier, higher feels snappier.
- **Up** / **Down** set the record's volume, up to 150%. VOL turns red above 100%: from there CUT's limiter squashes loud peaks, in your takes too, so keep it at 100% or below for clean takes. A knob or slider set up under **X** covers the same range.
- **TAB** switches the scratch motion: AUTO (follows whatever line you scratch along, right/up = forward), left/right, up/down, or PLATTER (circle round the centre of the tablet, clockwise = forward, one turn = 1.8 s like a real 33). Tablet touch defaults to platter, and a dot on the on-screen record shows where your finger is.

## Start-up and view

- CUT opens in full view, paused, with an empty record on the platter. Press **M** (or **Enter**) to start it: the empty run-out groove plays hiss, crackle and a scuff that ticks once per turn until you load a sample.
- **H** shows or hides the key guide. **Alt** (pressed and released on its own) or **F10** opens the menu bar, where every command is also listed with its key.
- **P** opens profiles: save your current feel and mixer settings under a name, and load them back later (for example one per controller or per routine).
- **Ctrl+Q** quits. Quitting mid-take finishes the take first.

## Library and takes

- **F** opens the folders menu: choose where saved samples and takes go, browse to a folder or make a new one, and choose whether to move your existing files there. Takes kept inside the samples folder move with it.

- **C** (or **Backspace**) puts the needle back on the cue, the start of the section.
- Drag on the top waveform to choose the section that goes on the record. Grab an edge of the selection to move just that edge. **A** selects the whole file.
- When zoomed in, drag towards either end of the waveform and it scrolls that way, faster the further out you go, so a selection can run past what's on screen.
- On the touchpad, the top waveform is for selecting: a finger there never holds or scratches the record, so you can click and drag with two fingers or your thumb.
- Zoom the top waveform for precise selection: scroll over it (two fingers on the touchpad, or the mouse wheel) to zoom around the pointer, scroll sideways to pan, and **Z** jumps to the section and back out. The edge times and zoom level show while zoomed.
- The section normally sits on the record with 1 s of blank groove before it (so you can pull back past the start) and 3 s after. **G** makes it loop seamlessly instead, with no gaps.
- **S** saves the section (clean) to `<samples folder>/<Folder>/` (default `~/Music/CUT`).
- **,** and **.** step to the previous and next sample in the same folder.
- **R** starts or stops a take of CUT's own output (after the fader and volume; never the click or other apps, and the backing track only if you press **U**), saved to the takes folder (default `~/Music/CUT/Takes`). Takes are 32-bit float WAV; **E** switches saved sections and takes to 320 kbps MP3. The file is saved as you go, so a crash or a forced quit mid-take keeps everything up to the last half second. A take that stays completely silent (motor off, or the fader cut the whole time) isn't kept.
- **T** turns on the click, and **9** / **0** change the BPM (hold Ctrl for steps of 10). With the click on, a take starts on the next bar, so it lines up when you drop it at a bar in FL. The click is never recorded.

## Backing track

A backing track is a beat or song that plays under the record while you scratch. The record is what you scratch; the backing track just plays.

| Key | What it does |
| --- | --- |
| **Y** | Load a backing track (WAV, MP3, FLAC, OGG, AIFF, M4A) |
| **B** | Play or pause it (loads one first if none is loaded) |
| **5** / **6** | Its volume down / up, separate from the record's |
| **U** | Record it into takes too (off: takes are the scratch alone) |

- **It's not on the crossfader.** Cuts, transforms and flares only cut the record, like a real mixer with the beat on the other channel.
- **It loops,** so a short loop or a long song both work.
- **It restarts when a take starts.** The moment recording begins, the backing track jumps to its start, so every take begins at the same point in the beat. In your DAW, line the take up with the start of the beat and you're done.
- **With the click on** (**T**), recording waits for the next bar, and the backing track restarts on that same bar. Set the click to the beat's tempo with **9** / **0** if you want to hear it count you in.
- **Takes are the scratch alone by default,** clean for mixing. Press **U** for the full mix instead. The status bar always shows which: **IN TAKES** (red) or *not in takes* next to BACKING, and **scratch + backing** or *scratch only* next to REC while you record.
- **The status bar** shows **BACKING** with its volume while it plays (or *paused*), and whether it's going into takes.
- **It's remembered:** next time you open CUT, the last backing track is loaded again, paused.
- **Put it on a controller:** under **X**, pick any device and give "Backing track play/pause" a button.

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
