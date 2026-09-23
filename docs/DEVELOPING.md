# Developing sciink

Everything a contributor needs: building, tests, the upstream oracles, the Inkscape dev loop,
packaging and releases. The behaviour of every tool is specified in [`docs/spec/`](spec/); the
plans in [`docs/superpowers/plans/`](superpowers/plans/) record how each part was built and every
ruling made along the way.

## Prerequisites

- Rust stable, 1.85 or newer (`rust-toolchain.toml` selects stable with `rustfmt` and `clippy`).
- Inkscape 1.2 or newer for the manual loop.
- `gh` (GitHub CLI) for release dry runs; optional.

## Build and test

```sh
cargo test
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings   # what CI runs first
```

Tests measure text with the vendored fonts in `tests/fonts` (DejaVu Sans, Roboto) so results are
identical on every OS; `tests/support/mod.rs` sets `SCIINK_NO_SYSTEM_FONTS=1` and
`SCIINK_FONT_DIRS` for you.

## Upstream fixtures and oracles

The `*_fixtures` and `invariance` tests compare sciink against Scientific-Inkscape's test data
(`tests/data` of its `dev` branch, pinned at `292ae77ef73e7b2d3b4a06c11768f2e1aad15bae`, about
33 MB). It is not vendored. Make it available either way:

```sh
git clone https://github.com/burghoff/Scientific-Inkscape.git ../Scientific-Inkscape
git -C ../Scientific-Inkscape checkout 292ae77ef73e7b2d3b4a06c11768f2e1aad15bae
ln -s "$PWD/../Scientific-Inkscape/tests" tests/upstream      # git-ignored
# or: export SCIINK_UPSTREAM_TESTS=$PWD/../Scientific-Inkscape/tests/data
```

Without it those tests print `SKIP: …` and pass. With it, `cargo test` also runs every oracle that
works with the vendored fonts.

The oracles against upstream's reference outputs need the fonts of the machine that produced them
(Arial, Avenir, Tahoma, …) and are `#[ignore]`d. Run them with the installed fonts:

```sh
SCIINK_SYSTEM_FONTS=1 cargo test --test text_fixtures --test text_tools --test text_ghoster --test flattener_fixtures --test homogenizer_fixtures -- --ignored --test-threads=1
```

The full-size visual-invariance check on `Acid_tests.svg` (18 000 elements) is slow in a debug build
and also opt-in:

```sh
cargo test --release --test invariance -- --ignored --nocapture
```

## Running the binary by hand

Inkscape runs the binary with the `.inx` parameters as flags, one `--id` per selected object and the
document as the last argument; the modified SVG goes to stdout and every message to stderr, which
Inkscape shows as a dialog. Any error echoes the input unchanged, so the document is never damaged.

```sh
cargo build --release
target/release/sciink --tool=flattener --id=layer1 figure.svg > out.svg
target/release/sciink --tool=scaler --tab=correction --id=g5224 figure.svg --output out.svg
target/release/sciink --help
```

Tools: `flattener`, `scaler`, `homogenizer`, `text-ghoster`, `combine-by-color`, `favorite-markers`,
`about`, and the debug tools `font-probe`, `text-highlight`, `text-fix`. Booleans are `true`/`false`
(`True`, `1` and `0` are accepted); omitted parameters take their defaults; unknown flags are errors,
because the `.inx` files and the binary ship together.

## Inkscape dev loop

```sh
dist/dev-install.sh
```

builds the release binary and symlinks it and `inx/*.inx` into Inkscape's user extensions directory
(macOS and Linux). Restart Inkscape once: it reads `.inx` files only at startup. After that every
`cargo build --release` takes effect on the next Apply, because the binary is re-executed per run.
On Windows, package a zip (below) and install it with `.\install.ps1 -Zip dist\out\sciink-windows-x64.zip`.

Extensions ▸ Scientific ▸ Diagnostics shows the installed version, the font count and scan time and
the resolutions of a few families; Debug ▸ Font Probe lists the face used for every family in the
document; Debug ▸ Text Highlight draws the text engine's measurements; Debug ▸ Text Fix runs the
Flattener's text pipeline alone on the selection.

## Environment variables

| Variable | Effect |
|---|---|
| `SCIINK_FONT_DIRS` | Extra font directories, separated by the platform's path-list separator (`:` / `;`). |
| `SCIINK_NO_SYSTEM_FONTS=1` | Skip the system font scan; only `SCIINK_FONT_DIRS` is used. |
| `SCIINK_LOG=<file>` | Append-only log: one line per phase of every run (`tool=<tool> phase=<name> dt=<ms>`), the Diagnostics summary and the source location of any internal error. stderr is the user's dialog, so nothing else is written there. |
| `INKSCAPE_PROFILE_DIR` | Set by Inkscape. Favorite Markers stores its templates in `$INKSCAPE_PROFILE_DIR/sciink/favorite_markers.svg`, falling back to the `.inx` directory. |
| `SCIINK_UPSTREAM_TESTS` | Tests: the upstream `tests/data` directory (alternative to the `tests/upstream` symlink). |
| `SCIINK_SYSTEM_FONTS=1` | Tests: run the `#[ignore]`d oracles against the installed fonts. |
| `SCIINK_GIT_SHA`, `SCIINK_TARGET` | Build time, set by `release.yml`; shown by `--version` and Diagnostics. |

## Repository layout

```
src/main.rs, cli.rs      process contract: argv parsing, --tool dispatch, stdout/stderr, panic → document unchanged
src/dom.rs, style.rs     lossless XML DOM with id index; CSS cascade with `font:` shorthand
src/geom/                affines, path parsing and serialisation, bounding-box algebra, number formatting
src/ops/                 style composition, clip/mask merging, ungroup/unlink, bounding boxes, fuse and global_transform, cleanup
src/text/                font discovery and metrics (fontdb, rustybuzz, ttf-parser), text parsing, layout, editing, kerning removal, writer
src/tools/               one file per menu entry
inx/                     the menu entries; `@VERSION@` is substituted when packaging
dist/                    package.sh, test-package.sh, test-install.sh, dev-install.sh, README-dist.txt (the README inside the zip)
tests/                   integration tests; support/ (fixture and font helpers), fonts/ (vendored), data/ (edge cases), upstream/ (symlink, git-ignored)
docs/spec/               00 overview, 01 text engine, 02 geometry and tools, 03 infrastructure: the authority on behaviour
docs/superpowers/plans/  one implementation plan per part, with the rulings made during execution
```

## Conventions

- Behaviour follows Scientific-Inkscape, constants included. Every intentional difference is recorded
  under "Deliberate deviations" in [`spec/02-geometry-tools.md`](spec/02-geometry-tools.md) or
  [`spec/01-text-engine.md`](spec/01-text-engine.md), with the reason.
- Tools are silent on success. stderr carries only `warning: …` lines and error messages, because
  Inkscape shows it as a dialog.
- Every number written to the document goes through `num::fmt` (8 significant digits), so output is
  byte-stable across runs and platforms.
- Tests never loosen a tolerance to pass; a mismatch with upstream is either a bug or a documented
  deviation with its own test.

## Packaging

```sh
cargo build --release
dist/package.sh macos-universal target/release/sciink        # or windows-x64 / linux-x64
dist/test-package.sh dist/out/sciink-macos-universal.zip     # layout, exec bit, version banner, pipe contract
dist/test-install.sh dist/out/sciink-macos-universal.zip     # install.sh offline: install, upgrade, uninstall
```

A zip contains `sciink/{*.inx, bin/sciink[.exe], README.txt, LICENSE}` and is unzipped straight into
the extensions directory. Each OS zips its own binary in CI so the exec bit survives.

## CI

`ci.yml` runs on pushes to `main` and on pull requests: `cargo fmt --check`, `cargo clippy -D warnings`
and `cargo test` on Linux, then tests, a release build, a pipe smoke test, packaging and the installer
tests on Linux, macOS and Windows (`install.ps1` under both PowerShell 7 and Windows PowerShell 5.1).

## Releasing

1. Set `version` in `Cargo.toml` and date the section in `CHANGELOG.md`; commit and push; wait for CI.
2. Tag and push the tag:

   ```sh
   git tag vX.Y.Z && git push origin vX.Y.Z
   ```

   `release.yml` refuses a tag that does not match the Cargo version, builds macOS universal
   (`lipo`, ad-hoc `codesign`), Windows x64 (static CRT) and Linux x64 (musl, static), smoke-tests every
   zip and publishes a GitHub release with generated notes. A tag with a suffix (`v0.2.0-beta.1`) is
   published as a pre-release.
3. Dry run without publishing: `gh workflow run release.yml --ref <branch>`. The publish job is
   tag-gated, so only the builds run and the zips appear as workflow artifacts.

The installers download `releases/latest`; `SCIINK_VERSION=vX.Y.Z` / `-Version vX.Y.Z` pins one.

## Known gaps

- `ops::bbox` yields no box for a `<use>` inside a `clipPath` whose target sits under `<defs>`.
- The Flattener and Combine by Color build character tables for the whole document, not just the
  selection.
- Favorite Markers re-saves a template among the store's direct children only; a store re-saved by
  Inkscape can reorder the template list.
- The first run after boot scans every installed font file (about 3 s for 1000 faces on macOS);
  later runs take a few hundred milliseconds. A persistent scan cache is the planned fix.
- DejaVu Sans is not bundled, so matplotlib's default font is measured with a substitute where it is
  not installed.
- Inkscape 1.4's headless `--actions` route runs the tools with an empty selection (`select-by-id`
  and `select-all` do not reach the extension), so script the binary directly instead.
- Scientific-Inkscape's Autoexporter and Gallery Viewer are not ported.
