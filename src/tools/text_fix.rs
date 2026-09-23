//! Debug tool: run the Flattener's text pipeline (spec §A.1 stages 2–12) on the selected text, with
//! the Flattener's own option names, so the pipeline can be exercised from Inkscape before the
//! Flattener exists.

use std::ffi::OsString;

use clap::Parser;

use crate::Output;
use crate::cli::{Common, inx_bool};
use crate::dom::Doc;
use crate::text::Warnings;
use crate::text::fonts::FontSystem;
use crate::text::kerning::{KerningOptions, remove_kerning};

use super::first_line;
use super::font_probe::text_elements;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct TextFixCli {
    #[command(flatten)]
    pub common: Common,
    // clap-derive infers `ArgAction::SetTrue` (a valueless flag) for any `bool` field unless told
    // otherwise; `action = Set` is required for the `--flag=true|false` syntax Inkscape sends.
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removemanualkerning: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub mergesubsuper: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub splitdistant: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub mergenearby: bool,
    /// 1 = centre, 2 = left, 3 = right, 4 = unchanged (upstream's optiongroup values).
    #[arg(long, default_value_t = 1)]
    pub justification: u8,
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = TextFixCli::try_parse_from(argv).map_err(first_line)?;
    let mut t = crate::log::Timer::new("text-fix");
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    t.phase("parse", || {
        format!("bytes={} elements={}", input.len(), doc.element_count())
    });
    let els = text_elements(&doc, &cli.common.ids);
    if els.is_empty() {
        return Err("select at least one text element (or a group containing text)".to_string());
    }
    let opts = KerningOptions::from_inx(
        cli.removemanualkerning,
        cli.mergesubsuper,
        cli.splitdistant,
        cli.mergenearby,
        cli.justification,
    );
    let mut warn = Warnings::default();
    let out = remove_kerning(&mut doc, &els, &opts, FontSystem::load(), &mut warn);
    let mut messages = vec![format!(
        "text-fix: {} text elements in, {} out",
        els.len(),
        out.len()
    )];
    messages.extend(warn.0.iter().map(|w| format!("warning: {w}")));
    let mut svg = Vec::new();
    doc.write(&mut svg);
    t.phase("write", || format!("bytes={}", svg.len()));
    t.total(String::new);
    Ok(Output { svg, messages })
}
