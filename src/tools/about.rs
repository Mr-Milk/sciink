//! Diagnostics: proves the Inkscape ↔ binary protocol works and reports what
//! the binary sees. Echoes the document unchanged.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::time::Instant;

use clap::Parser;

use crate::Output;
use crate::cli::Common;
use crate::dom::Doc;
use crate::text::fonts::FontSystem;

use super::first_line;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct AboutCli {
    #[command(flatten)]
    pub common: Common,
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    // clap's `Display` is a multi-line usage block; the dialog Inkscape shows
    // the user is one line, so keep only the first (the actual error message).
    let cli = AboutCli::try_parse_from(argv).map_err(first_line)?;
    // ponytail: test hook for the process boundary's panic path (see tests/cli.rs)
    if std::env::var_os("SCIINK_TEST_PANIC").is_some() {
        panic!("injected test panic");
    }
    let mut t = crate::log::Timer::new("about");
    let t0 = Instant::now();
    let doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let parse_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let (mut texts, mut paths) = (0usize, 0usize);
    for n in doc.descendants(doc.svg()).skip(1) {
        match doc.tag(n) {
            "text" => texts += 1,
            "path" => paths += 1,
            _ => {}
        }
    }
    let elements = doc.element_count();
    t.phase("parse", || format!("elements={elements}"));
    let mut r = String::new();
    let _ = writeln!(
        r,
        "sciink {} — {}",
        crate::version(),
        crate::paths::target_triple()
    );
    let _ = writeln!(r, "executable: {}", crate::paths::exe_path().display());
    let _ = writeln!(r, "extension dir: {}", crate::paths::inx_dir().display());
    let _ = writeln!(
        r,
        "document: {elements} elements ({texts} text, {paths} path), parsed in {parse_ms:.1} ms"
    );
    let mut fs = FontSystem::load();
    t.phase("fonts", || {
        format!("faces={} ms={:.0}", fs.face_count(), fs.load_ms())
    });
    let _ = writeln!(
        r,
        "fonts: {} faces in {:.0} ms",
        fs.face_count(),
        fs.load_ms()
    );
    match crate::paths::bundled_font_dir() {
        Some(d) => {
            let n = fs.faces().filter(|&k| fs.is_bundled(k)).count();
            let _ = writeln!(r, "bundled fonts: {} ({n} faces)", d.display());
        }
        None => {
            let _ = writeln!(r, "bundled fonts: not found");
        }
    }
    for fam in ["Arial", "DejaVu Sans", "sans-serif"] {
        let _ = writeln!(
            r,
            "{}",
            crate::tools::font_probe::describe_resolution(&mut fs, fam)
        );
    }
    let _ = writeln!(r, "selection: {} object(s)", cli.common.ids.len());
    t.total(String::new);
    Ok(Output {
        svg: input.to_vec(),
        messages: vec![r.trim_end().to_string()],
    })
}
