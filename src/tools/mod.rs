//! One module per Inkscape menu entry. Each exposes
//! `pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String>`.

pub mod about;
