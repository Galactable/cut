# Built-in font

CUT draws its text with FantasqueSansM Nerd Font Mono (Fantasque Sans Mono by Jany Belluz, with icons added by the Nerd Fonts project), built into the program so it looks the same everywhere. The font is under the SIL Open Font License 1.1: see `OFL.txt`.

These are cut-down copies of the full font: Latin, Greek, Cyrillic, punctuation, arrows and the icons CUT uses, about 60 KB each. To add an icon, put its code point in the list below and make them again from the full Nerd Font (version 3.x) with fontTools (`pip install fonttools`):

```bash
U="U+0020-007E,U+00A0-017F,U+0370-03FF,U+0400-04FF,U+2000-206F,U+20AC,U+2122,U+2190-21FF,U+F001,U+F005,U+F00C,U+F00D,U+F023,U+F040,U+F04B,U+F062,U+F071,U+F07B,U+F07C,U+F0A6,U+F0A7,U+F0C4,U+F0E7,U+F108,U+F111,U+F11B,U+F11C,U+F130,U+F1DE,U+F245"
for w in Regular Bold; do
  pyftsubset FantasqueSansMNerdFontMono-$w.ttf --unicodes="$U" --layout-features='' \
    --no-hinting --desubroutinize --output-file=FantasqueSansMNerdFontMono-$w.ttf
done
```

`cargo test` checks that every icon in the code is in both files.
