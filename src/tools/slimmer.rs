//! Slimmer (Plan 10): makes a whole document smaller and faster for Inkscape. No upstream
//! counterpart except the unused-definition step, which follows `dhelpers.py:990
//! clean_up_document`. Whole-document scope; every step that is on by default is rendering-exact
//! — the argument is in each step's doc comment, the guard tests in `tests/slimmer.rs` pin what
//! must stay. No `Ctx`: nothing here creates clips, every deletion is of something unreferenced by
//! construction, and the merge step repoints its own references.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt::Write as _;

use clap::Parser;

use crate::Output;
use crate::cli::{Common, inx_bool};
use crate::dom::{Doc, NodeId};
use crate::ops::cleanup::detach_tidy;

use super::first_line;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct SlimmerCli {
    #[command(flatten)]
    pub common: Common,
    /// The notebook page Inkscape reports; unused.
    #[arg(long, default_value = "Options")]
    pub tab: String,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub dedupstyles: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub removeempty: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub collapsegroups: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub pruneunused: bool,
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub mergedefs: bool,
    /// Significant digits for path and shape coordinates; 0 keeps them as written.
    #[arg(long, default_value_t = 0)]
    pub precision: u8,
    /// Show the report dialog.
    #[arg(long, value_parser = inx_bool, action = clap::ArgAction::Set, default_value = "true")]
    pub report: bool,
}

/// What a run did; every counter feeds one report line.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub sheets_removed: usize,
    pub sheet_moved: bool,
    pub empty_removed: usize,
    pub wrappers_collapsed: usize,
    pub defs_pruned: usize,
    pub prune_rounds: usize,
    pub containers_removed: usize,
    pub defs_merged: usize,
    pub attrs_repointed: usize,
    pub numbers_rounded: usize,
    /// Extra lines: a skipped step and why.
    pub notes: Vec<String>,
}

impl Report {
    fn changed(&self) -> bool {
        self.sheet_moved
            || self.sheets_removed
                + self.empty_removed
                + self.wrappers_collapsed
                + self.defs_pruned
                + self.containers_removed
                + self.defs_merged
                + self.numbers_rounded
                > 0
    }
}

fn style_elements(doc: &Doc) -> Vec<NodeId> {
    doc.descendants(doc.svg())
        .filter(|&n| doc.is_element(n) && doc.tag(n) == "style")
        .collect()
}

/// Step (a): `<style>` elements with identical text collapse onto the LAST copy. Same-precedence
/// conflicts are decided by source order, later wins (`style::DeclKey`); a duplicated rule reaches
/// its maximum order at the last copy, so deleting the earlier copies changes no winner — sheets
/// `A B A` render as A, and keeping the first would give B. Sheets with attributes beyond `id` and
/// `type="text/css"`, or containing an `@` rule (`@import`, `@media`: position- and
/// count-sensitive), are left alone. When exactly one sheet remains it moves to the front of the
/// root: position is irrelevant for a lone sheet, and out of a figure's nested `<defs>` deleting
/// that figure can no longer restyle the whole document. Returns (removed, moved).
pub fn dedup_stylesheets(doc: &mut Doc) -> (usize, bool) {
    let mut last: HashMap<String, NodeId> = HashMap::new();
    let mut eligible: Vec<(NodeId, String)> = Vec::new();
    for s in style_elements(doc) {
        let plain = doc.attrs(s).iter().all(|a| {
            a.name == "id" || (a.name == "type" && a.value.trim().eq_ignore_ascii_case("text/css"))
        });
        let key = doc.text_content(s).trim().to_string();
        if !plain || key.contains('@') {
            continue;
        }
        last.insert(key.clone(), s); // later copies overwrite: the last one survives
        eligible.push((s, key));
    }
    let mut removed = 0;
    for (s, key) in eligible {
        if last[&key] != s {
            detach_tidy(doc, s);
            removed += 1;
        }
    }
    let mut moved = false;
    let remaining = style_elements(doc);
    if remaining.len() == 1 {
        let only = remaining[0];
        let svg = doc.svg();
        if doc.parent(only) != Some(svg) {
            let first = doc.children(svg).find(|&c| doc.is_element(c));
            match first {
                Some(first) if first != only => doc.insert_before(only, first),
                _ => doc.append_child(svg, only),
            }
            moved = true;
        }
    }
    (removed, moved)
}

/// Runs the enabled steps in order, one `Timer` phase each.
pub fn slim(doc: &mut Doc, o: &SlimmerCli, t: &mut crate::log::Timer) -> Report {
    let mut r = Report::default();
    if o.dedupstyles {
        let (n, moved) = dedup_stylesheets(doc);
        r.sheets_removed = n;
        r.sheet_moved = moved;
        t.phase("styles", || format!("removed={n} moved={moved}"));
    }
    r
}

fn human(bytes: usize) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1e6)
    } else if bytes >= 10_000 {
        format!("{:.1} kB", bytes as f64 / 1e3)
    } else {
        format!("{bytes} B")
    }
}

/// The dialog text: one line of totals, one per step, then the notes.
pub fn report_message(
    in_len: usize,
    out_len: usize,
    els_before: usize,
    els_after: usize,
    precision: u8,
    r: &Report,
) -> String {
    if !r.changed() && r.notes.is_empty() {
        return "Slimmer: nothing to do".to_string();
    }
    let pct = if in_len == 0 {
        0.0
    } else {
        (in_len as f64 - out_len as f64) / in_len as f64 * 100.0
    };
    let mut s = String::new();
    let _ = writeln!(
        s,
        "Slimmer: {} → {} ({}{:.0} %), {els_before} → {els_after} elements",
        human(in_len),
        human(out_len),
        if pct >= 0.0 { "-" } else { "+" },
        pct.abs()
    );
    let _ = writeln!(
        s,
        "  duplicate stylesheets removed: {}{}",
        r.sheets_removed,
        if r.sheet_moved {
            " (1 kept, moved to the document root)"
        } else {
            ""
        }
    );
    let _ = writeln!(
        s,
        "  empty or invisible elements removed: {}",
        r.empty_removed
    );
    let _ = writeln!(s, "  wrapper groups collapsed: {}", r.wrappers_collapsed);
    let _ = write!(s, "  unused definitions removed: {}", r.defs_pruned);
    if r.defs_pruned + r.containers_removed > 0 {
        let _ = write!(
            s,
            " in {} round(s) ({} emptied containers)",
            r.prune_rounds, r.containers_removed
        );
    }
    s.push('\n');
    let _ = write!(s, "  identical definitions merged: {}", r.defs_merged);
    if r.defs_merged > 0 {
        let _ = write!(s, " ({} attributes repointed)", r.attrs_repointed);
    }
    s.push('\n');
    if precision == 0 {
        let _ = writeln!(s, "  coordinate precision: unchanged");
    } else {
        let _ = writeln!(
            s,
            "  coordinate precision: {precision} significant digits, {} numbers changed",
            r.numbers_rounded
        );
    }
    for n in &r.notes {
        let _ = writeln!(s, "  {n}");
    }
    s.trim_end().to_string()
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = SlimmerCli::try_parse_from(argv).map_err(first_line)?;
    if cli.precision != 0 && !(4..=8).contains(&cli.precision) {
        return Err(format!(
            "precision must be 0 or 4–8 significant digits, got {}",
            cli.precision
        ));
    }
    let mut t = crate::log::Timer::new("slimmer");
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    let els_before = doc.element_count();
    t.phase("parse", || {
        format!("bytes={} elements={els_before}", input.len())
    });
    let report = slim(&mut doc, &cli, &mut t);
    let els_after = doc.element_count();
    let mut svg = Vec::new();
    doc.write(&mut svg);
    t.phase("write", || format!("bytes={}", svg.len()));
    t.total(|| format!("elements_removed={}", els_before.saturating_sub(els_after)));
    let messages = if cli.report {
        vec![report_message(
            input.len(),
            svg.len(),
            els_before,
            els_after,
            cli.precision,
            &report,
        )]
    } else {
        Vec::new()
    };
    Ok(Output { svg, messages })
}
