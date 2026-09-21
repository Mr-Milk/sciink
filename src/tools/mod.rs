//! One module per Inkscape menu entry. Each exposes
//! `pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String>`.

pub mod about;
pub mod font_probe;
pub mod text_fix;
pub mod text_highlight;
