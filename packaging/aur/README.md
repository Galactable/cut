# The AUR package (cut-deck)

`PKGBUILD` builds CUT from a tagged release's source. It installs the program, its launcher entry and icons, and the udev rule that lets the person at the computer read touchpads, tablets, tablet pads and gamepads (the same rule as `input-access.sh`). `options=('!lto')` is needed because the bundled MP3 encoder is C, and makepkg's link-time optimisation leaves it in a form Rust can't link.

## One-time setup

1. Make an account at https://aur.archlinux.org/register.
2. Add your SSH public key to the account (My Account → SSH Public Key). If you don't have a key yet, run `ssh-keygen -t ed25519` and paste in the contents of `~/.ssh/id_ed25519.pub`.
3. Clone the (empty) package repo to where the scripts look for it:

   ```bash
   git clone ssh://aur@aur.archlinux.org/cut-deck.git ~/aur/cut-deck
   ```

4. Publish the current version:

   ```bash
   packaging/aur/update.sh
   ```

## Every release after that

Nothing extra. `./release.sh` runs `packaging/aur/update.sh` once the GitHub release exists. That points the PKGBUILD at the new tag and its checksum, regenerates `.SRCINFO`, and pushes to the AUR.

To fix the package itself without a new CUT version, edit the PKGBUILD here and run `packaging/aur/update.sh` again. It raises `pkgrel` when the version hasn't changed.

To test-build locally: copy `PKGBUILD` and `cut-deck.install` into an empty folder and run `makepkg -f`.
