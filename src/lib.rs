//! sciink — Inkscape extensions for scientific figures, as one fast binary.
//! Inkscape launches `sciink --tool=<name> --param=value… --id=… input.svg`
//! and reads the modified SVG from stdout (see docs/spec/03-infrastructure.md §C.3).

pub mod cli;
pub mod dom;
pub mod geom;
pub mod log;
pub mod num;
pub mod ops;
pub mod paths;
pub mod style;
pub mod text;
pub mod tools;

use std::ffi::OsString;

/// What a tool produces: the document to hand back to Inkscape and messages for the user.
pub struct Output {
    pub svg: Vec<u8>,
    pub messages: Vec<String>,
}

/// Version string shown to users: Cargo version plus the git SHA baked in by CI.
pub fn version() -> String {
    match option_env!("SCIINK_GIT_SHA") {
        Some(sha) => format!("{} ({sha})", env!("CARGO_PKG_VERSION")),
        None => env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Runs the tool named by `--tool=` in `argv` on `input`. Pure: no I/O besides logging.
pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let pre = cli::prescan(argv);
    let tool = pre.tool.ok_or_else(|| {
        "missing --tool=<name> (this binary is meant to be launched by Inkscape through its .inx files)".to_string()
    })?;
    match tool.as_str() {
        "about" => tools::about::run(argv, input),
        "font-probe" => tools::font_probe::run(argv, input),
        "text-highlight" => tools::text_highlight::run(argv, input),
        "text-fix" => tools::text_fix::run(argv, input),
        "combine-by-color" => tools::combine_by_color::run(argv, input),
        "text-ghoster" => tools::text_ghoster::run(argv, input),
        "flattener" => tools::flattener::run(argv, input),
        "scaler" => tools::scaler::run(argv, input),
        "homogenizer" => tools::homogenizer::run(argv, input),
        "favorite-markers" => Err(format!("the {tool} tool is not implemented yet")),
        other => Err(format!("unknown tool '{other}'")),
    }
}
