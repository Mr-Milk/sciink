//! Command-line contract with Inkscape (spec §C.3).
//!
//! Inkscape passes `--name=value` for every `.inx` parameter (bools as `true`/`false`,
//! optiongroups as their `value`, notebooks as the page name), `--id=ID` per selected
//! object, optionally `--selected-nodes=…`, and the input path last.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{ArgAction, Args, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ToolName {
    Flattener,
    Scaler,
    Homogenizer,
    TextGhoster,
    CombineByColor,
    FavoriteMarkers,
    About,
    FontProbe,
    TextHighlight,
    TextFix,
}

/// Arguments every tool accepts. Tools `#[command(flatten)]` this into their own struct.
#[derive(Debug, Args)]
pub struct Common {
    #[arg(long, value_enum)]
    pub tool: ToolName,
    /// Selected object ids, in Inkscape's selection order.
    #[arg(long = "id", action = ArgAction::Append)]
    pub ids: Vec<String>,
    /// `id:subpath:node` triples (Inkscape ≥ 1.2); accepted, unused for now.
    #[arg(long = "selected-nodes", action = ArgAction::Append, hide = true)]
    pub selected_nodes: Vec<String>,
    /// Write the result here instead of stdout (inkex-compatible; `-` = stdout).
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,
    /// Append timing/debug lines to this file (never shown to the user).
    #[arg(long, env = "SCIINK_LOG", hide = true)]
    pub log: Option<PathBuf>,
    /// Input SVG (Inkscape's temp file); stdin when absent.
    pub input: Option<PathBuf>,
}

/// Parses an `.inx` boolean. Inkscape sends `true`/`false`; upstream tests send `True`.
pub fn inx_bool(s: &str) -> Result<bool, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        other => Err(format!("expected true/false, got '{other}'")),
    }
}

/// The few arguments `main` needs before any tool has parsed argv.
#[derive(Debug, Default)]
pub struct Prescan {
    pub tool: Option<String>,
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub log: Option<PathBuf>,
}

/// Lenient scan of argv for `--tool`, `--output`/`-o`, `--log` (both `--k=v` and `--k v`)
/// and the last bare argument (the input path). Never fails.
pub fn prescan(argv: &[OsString]) -> Prescan {
    let args: Vec<String> = argv
        .iter()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let mut p = Prescan::default();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let (key, inline): (&str, Option<String>) = match a.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        if matches!(key, "--tool" | "--output" | "-o" | "--log") {
            let v = match inline {
                Some(v) => Some(v),
                None => {
                    i += 1;
                    args.get(i).cloned()
                }
            };
            match key {
                "--tool" => p.tool = v,
                "--output" | "-o" => p.output = v.map(PathBuf::from),
                _ => p.log = v.map(PathBuf::from),
            }
        } else if !a.starts_with('-') {
            p.input = Some(PathBuf::from(a));
        }
        i += 1;
    }
    p
}

pub const HELP: &str = "sciink — Inkscape extensions for scientific figures

Inkscape launches this binary through the .inx files in the same folder. For manual use:

    sciink --tool=<flattener|scaler|homogenizer|text-ghoster|combine-by-color|favorite-markers|about>
           [--<param>=<value>...] [--id=<object-id>...] [--output <file>] [input.svg]

The modified SVG is written to stdout (or --output); messages go to stderr.
Set SCIINK_LOG=<file> to append timing information.

Debug tools (Scientific ▸ Debug): font-probe, text-highlight, text-fix.";
