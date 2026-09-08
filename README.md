# sciink

Fast, dependency-free Inkscape extensions for scientific figures — a Rust rewrite of
[Scientific-Inkscape](https://github.com/burghoff/Scientific-Inkscape) (Flattener, Scaler,
Homogenizer, Text Ghoster, Combine by Color, Favorite Markers).

Status: early development. Design specs live in `docs/spec/`.

## Developing

    cargo test
    dist/dev-install.sh      # symlink into Inkscape's user extensions dir, then restart Inkscape

Set `SCIINK_LOG=/tmp/sciink.log` in Inkscape's environment to get timing lines.
