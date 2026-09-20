`corpus/Simple_text.svg` is vendored verbatim from Scientific-Inkscape's test data
(`tests/data/svg/Simple_text.svg`, GPL-2.0-or-later — the same licence as this crate), so that at least one
end-to-end parse of a real Inkscape document runs on all three CI OSes; the rest of the upstream fixtures are
dev-machine-only (`tests/upstream`, see `tests/support/mod.rs`).

`edge/` holds hand-written documents for specific parser edge cases.
