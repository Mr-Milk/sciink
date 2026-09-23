# Test fonts

Vendored so `cargo test` measures text deterministically on every OS (spec §C.5).

| Files | Source | Licence |
|---|---|---|
| `DejaVuSans.ttf`, `DejaVuSans-Bold.ttf` | dejavu-fonts 2.37 | Bitstream Vera + Arev licence (`LICENSE-DejaVu.txt`) |
| `Roboto-Regular.ttf`, `Roboto-Bold.ttf` | googlefonts/roboto v2.138 (`roboto-android.zip`) | Apache-2.0 (`LICENSE-Roboto.txt`) |

Tests load them with `FontSystem::from_dirs`. The DejaVu Sans files are also the shipped copies:
`dist/package.sh` copies `DejaVuSans.ttf`, `DejaVuSans-Bold.ttf` and `LICENSE-DejaVu.txt` from here
into the release zip's `fonts/` (no second copy in the repo). Roboto stays test-only, not shipped.
