# Test fonts

Vendored so `cargo test` measures text deterministically on every OS (spec §C.5).

| Files | Source | Licence |
|---|---|---|
| `DejaVuSans.ttf`, `DejaVuSans-Bold.ttf` | dejavu-fonts 2.37 | Bitstream Vera + Arev licence (`LICENSE-DejaVu.txt`) |
| `Roboto-Regular.ttf`, `Roboto-Bold.ttf` | googlefonts/roboto v2.138 (`roboto-android.zip`) | Apache-2.0 (`LICENSE-Roboto.txt`) |

Not shipped in release zips. Tests load them with `FontSystem::from_dirs`.
