//! One module per Inkscape menu entry. Each exposes
//! `pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String>`.

pub mod about;
pub mod combine_by_color;
pub mod flattener;
pub mod font_probe;
pub mod scaler;
pub mod text_fix;
pub mod text_ghoster;
pub mod text_highlight;

/// The first line of a clap error (its usage dump is noise in an Inkscape dialog).
pub(crate) fn first_line(e: clap::Error) -> String {
    e.to_string()
        .lines()
        .next()
        .unwrap_or("invalid arguments")
        .to_string()
}
