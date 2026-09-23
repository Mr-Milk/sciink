# Plan 9: Big-Document Performance, Bundled DejaVu, 0.2.0 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Flattener or Homogenizer run on one figure of a 50 MB, 62 000-element manuscript SVG costs ≤ 0.4 s of our time (today 1.1 s), the whole layer ≤ 2.5 s (today 6.2–7.4 s), the font system starts in ≤ 0.1 s with a warm cache file (today ~3 s after boot); DejaVu Sans ships in the zips; live preview is off everywhere; released as 0.2.0.

**Architecture:** Four cost classes, fixed in order and each measured with the per-phase timers Task 1 adds: (1) serialisation (byte-wise attribute escaping, buffer growth); (2) per-node cascade and DOM bookkeeping (cloning 354 universal-rule declarations per node, a `String` per id on every subtree move); (3) genuinely quadratic passes (`remove_duplicates`, `remove_white_rects`, `remove_kerning`'s nested-text check) plus a whole-document character table the Flattener builds by mistake; (4) font startup (1 022 files opened for metrics on every text run) — replaced by a validated on-disk cache, with DejaVu Sans bundled as the matplotlib fallback. Only class (3) changes an algorithm, and each replacement is a pure function over `&[kurbo::Rect]` tested against the verbatim pairwise code on seeded random input.

**Tech Stack:** Rust 2024 (rustc ≥ 1.85), clap 4.6, quick-xml 0.42, kurbo 0.13, fontdb 0.24 (`push_face_info`, `Source::File`), ttf-parser 0.25, rustybuzz 0.20; dev-deps roxmltree 0.21, resvg 0.48.1. **No new dependencies.**

**Spec:** the approved design `/Users/yzheng/.claude/plans/the-problem-with-https-github-com-burgho-prancy-llama.md` (Plan 9: findings, targets, tasks T1–T16, deviations), plus `docs/spec/02-geometry-tools.md` and `docs/spec/03-infrastructure.md` for the behaviour being preserved.

## Global Constraints

- Upstream (burghoff's Scientific-Inkscape, Python) parity by default. Every intentional difference goes under a heading `## Deliberate deviations (Plan 9)` in `docs/spec/03-infrastructure.md` (infrastructure) or `docs/spec/02-geometry-tools.md` (behaviour), with the reason. Task 16 writes those sections; earlier tasks note their deviation in the commit message.
- No new dependencies: std only for everything new (no serde, memchr, rand, hashing crates).
- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` green on every commit (309 tests at the start of this plan; the count only grows). **No existing assertion is ever loosened.** A changed output byte is wrong unless it is a listed deviation with its own new test. The single permitted assertion edit is the version string `sciink 0.1.0` → `sciink 0.2.0` in `tests/cli.rs` (Task 16).
- Every algorithmic replacement (Tasks 9, 10) is a pure function over `&[kurbo::Rect]`; the current pairwise implementation is copied verbatim into the test file as `*_ref` and the two are compared on seeded random input (a 64-bit LCG in the test; no `rand`).
- Instrumentation (Task 1) lands first; every later performance task names the `SCIINK_LOG` phase line that proves it worked.
- Exact values: duplicate tolerance `1e-6 · max(size_i, size_j)` with `size = max(width, height)`; `BoxGrid` side `clamp(ceil(sqrt(n)), 1, 512)`, `MAX_SPAN = 32`; `FONT_CACHE_FORMAT = 1`; cache file `fontcache-1.tsv`; environment variables `SCIINK_LOG`, `SCIINK_NO_FONT_CACHE`, `SCIINK_FONT_CACHE`, `SCIINK_NO_BUNDLED_FONTS`, `SCIINK_NO_SYSTEM_FONTS`, `SCIINK_FONT_DIRS`, `SCIINK_UPSTREAM_TESTS`, `SCIINK_SYSTEM_FONTS`, `SCIINK_BIG_SVG`.
- Tools stay silent on success: stderr carries only `warning: …` lines and error messages (Inkscape shows it as a dialog). Timing goes to `SCIINK_LOG` only.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. In this repository's worktree sessions the harness may refuse a plain `git` command; call `/usr/bin/git` with the same arguments in that case.
- Measurement file for acceptance: `/Users/yzheng/Documents/Manuscripts/MetPredict/metpredict_figures_clean.svg` (52.6 MB); selections `figure_1-3` (one figure, 3 018 descendants), `g660` (3 537 shapes), `g9740` (735 texts), `layer1` (everything, 39 023 descendants). Warm-cache numbers only; run each measurement twice and keep the second.

## File Structure

| File | Responsibility (new or changed) |
|---|---|
| `src/log.rs` | `Timer` (per-phase lines), `enabled()`, `elapsed_ms()`; `line()` gains `ms=` |
| `src/cli.rs` | `HELP` text describes the timing log |
| `src/tools/*.rs` | every tool emits `parse`, `selection`, its stages, `cleanup`, `write`, `total` |
| `src/dom.rs` | run-copying `escape_attr`/`escape_text`, `source_len` + sized output buffer, identical-write skip in `set_attr`, `unlink` + `after_move` (a move is one operation), `style_elements` counter, allocation-free id index maintenance, `selection` one-id fast path, `selection_ordered` in O(s) |
| `src/style.rs` | `cascaded_style` borrows instead of cloning; `Stylesheet` pre-merges lone `*{…}` rules and indexes declared property names; `sheet_value` short-circuit |
| `src/ops/cleanup.rs` | inline styles parsed only when they mention `clip-path`/`mask` |
| `src/ops/mod.rs` | `Ctx::set_text_roots`, `Ctx::char_table_els` |
| `src/text/table.rs` | `CharTable::el_count` |
| `src/tools/flattener.rs` | selection-scoped character table, timers, `duplicate_scan`/`background_scan` callers, phase functions `pub` |
| `src/tools/combine_by_color.rs` | `Ctx::for_roots`, one `selection` call |
| `src/text/kerning.rs` | linear nested-text check; (conditional) grid-filtered `external_merges` |
| `src/geom/grid.rs` (new) | `BoxGrid`, `duplicate_scan`, `background_scan` |
| `src/text/fontcache.rs` (new) | on-disk font-scan cache: path, TSV read/write, validation |
| `src/text/fonts.rs` | `ScanKey`, `scan_with_cache`, `from_cached`, `bundled` flag and tie-break, counters for tests |
| `src/paths.rs` | `data_dir`, `cache_dir`, `bundled_font_dir` |
| `src/tools/about.rs`, `src/tools/font_probe.rs` | report bundled fonts |
| `dist/package.sh`, `dist/test-package.sh`, `dist/dev-install.sh` | ship/symlink `fonts/` |
| `inx/{combine_by_color,text_ghoster,text_fix,text_highlight}.inx` | `needs-live-preview="false"` |
| `.github/workflows/{ci,release}.yml` | `ubuntu-24.04` |
| `tests/support/mod.rs` | `SCIINK_NO_FONT_CACHE=1` in `with_vendored_fonts`; `BigDoc` generator |
| `tests/flattener_sweeps.rs`, `tests/font_cache.rs`, `tests/big_document.rs`, `tests/bench.rs` (new) | property tests against the pairwise references; cache validity; synthetic big document; ignored phase bench |
| `README.md`, `docs/DEVELOPING.md`, `docs/spec/0{2,3}-*.md`, `CHANGELOG.md`, `Cargo.toml` | docs, deviations, 0.2.0 |

---

### Task 1: Per-phase timing through `SCIINK_LOG`

**Files:**
- Modify: `src/log.rs` (whole file), `src/cli.rs:102-111` (`HELP`), `src/tools/flattener.rs:225-278` (`run`, `finish`, `bbox_stage` signature), `src/tools/about.rs:24-91`, `src/tools/homogenizer.rs:540-623`, `src/tools/scaler.rs:719-782`, `src/tools/combine_by_color.rs:139-156`, `src/tools/text_ghoster.rs:92-115`, `src/tools/favorite_markers.rs:379-…` (`run`), `src/text/fonts.rs:100-104` (`load`), `docs/DEVELOPING.md` (environment table)
- Test: `tests/cli.rs` (append)

**Interfaces:**
- Produces: `pub fn log::enabled() -> bool`, `pub fn log::elapsed_ms() -> f64`, `pub struct log::Timer` with `Timer::new(tool: &'static str)`, `Timer::phase(&mut self, name: &str, detail: impl FnOnce() -> String)`, `Timer::total(&mut self, detail: impl FnOnce() -> String)`. Line format: `t=<unix secs> ms=<since start> tool=<tool> phase=<name> dt=<ms since previous phase> <detail>`. `remove_duplicates` and `remove_white_rects` return `usize` (removed count) from now on; `bbox_stage` takes `t: &mut Timer`.
- Consumed by every later task's "done when" line.

- [ ] **Step 1: Write the failing test**

Append to `tests/cli.rs` (it already has `bin()`, `tmp()` and `SIMPLE`; `simple.svg` contains an element with id `p`):

```rust
#[test]
fn flattener_logs_one_line_per_phase_with_durations() {
    let p = tmp("phases.svg", SIMPLE);
    let l = std::env::temp_dir().join(format!("sciink-test-{}-phases.log", std::process::id()));
    let _ = std::fs::remove_file(&l);
    let out = bin()
        .args(["--tool=flattener", "--id=p"])
        .arg("--log")
        .arg(&l)
        .arg(&p)
        .env("SCIINK_NO_SYSTEM_FONTS", "1")
        .env(
            "SCIINK_FONT_DIRS",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fonts"),
        )
        .output()
        .unwrap();
    assert!(out.status.success());
    let log = std::fs::read_to_string(&l).unwrap();
    for phase in ["parse", "selection", "workingset", "cleanup", "write", "total"] {
        assert!(
            log.contains(&format!("tool=flattener phase={phase} dt=")),
            "missing phase {phase} in:\n{log}"
        );
    }
    for line in log.lines().filter(|l| l.contains("phase=")) {
        let dt = line
            .split_whitespace()
            .find_map(|f| f.strip_prefix("dt="))
            .unwrap_or_else(|| panic!("no dt= in {line}"));
        let v: f64 = dt.parse().unwrap_or_else(|_| panic!("dt not a number in {line}"));
        assert!(v >= 0.0, "{line}");
        assert!(line.contains(" ms="), "{line}");
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --test cli flattener_logs_one_line_per_phase_with_durations`
Expected: FAIL — `missing phase parse` (no phase lines exist yet).

- [ ] **Step 3: Replace `src/log.rs`**

```rust
//! Optional append-only log file (`SCIINK_LOG` / `--log`). Never writes to stderr: stderr is the
//! dialog Inkscape shows the user. Every line is `t=<unix secs> ms=<since start> <fields>`;
//! tools emit one `phase=<name> dt=<ms>` line per stage through `Timer`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static LOG: OnceLock<Mutex<Option<File>>> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();

fn start() -> Instant {
    *START.get_or_init(Instant::now)
}

pub fn init(path: Option<&Path>) {
    start();
    let file = path.and_then(|p| OpenOptions::new().create(true).append(true).open(p).ok());
    let _ = LOG.set(Mutex::new(file));
}

/// True when `init` opened a log file.
pub fn enabled() -> bool {
    LOG.get()
        .and_then(|m| m.lock().ok())
        .is_some_and(|g| g.is_some())
}

/// Milliseconds since `init` (or since the first log call in this process).
pub fn elapsed_ms() -> f64 {
    start().elapsed().as_secs_f64() * 1000.0
}

pub fn line(msg: &str) {
    let Some(m) = LOG.get() else { return };
    let Ok(mut guard) = m.lock() else { return };
    let Some(f) = guard.as_mut() else { return };
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = writeln!(f, "t={secs} ms={:.1} {msg}", elapsed_ms());
}

/// Stopwatch that emits one `phase=` line per stage of a tool run. Costs one `Instant::now()`
/// per phase when logging is off; `detail` is only evaluated when it is on.
pub struct Timer {
    tool: &'static str,
    t0: Instant,
    last: Instant,
}

impl Timer {
    pub fn new(tool: &'static str) -> Timer {
        start();
        let now = Instant::now();
        Timer {
            tool,
            t0: now,
            last: now,
        }
    }

    /// `tool=<tool> phase=<name> dt=<ms since the previous phase> <detail>`.
    pub fn phase(&mut self, name: &str, detail: impl FnOnce() -> String) {
        let now = Instant::now();
        if enabled() {
            let dt = (now - self.last).as_secs_f64() * 1000.0;
            line(&format!(
                "tool={} phase={name} dt={dt:.1} {}",
                self.tool,
                detail()
            ));
        }
        self.last = now;
    }

    /// `phase=total` with `dt` = the whole run so far.
    pub fn total(&mut self, detail: impl FnOnce() -> String) {
        if enabled() {
            let dt = self.t0.elapsed().as_secs_f64() * 1000.0;
            line(&format!(
                "tool={} phase=total dt={dt:.1} {}",
                self.tool,
                detail()
            ));
        }
        self.last = Instant::now();
    }
}
```

- [ ] **Step 4: Instrument the Flattener**

In `src/tools/flattener.rs`, `run` becomes:

```rust
pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = FlattenerCli::try_parse_from(argv).map_err(first_line)?;
    let mut t = crate::log::Timer::new("flattener");
    let mut doc = Doc::parse(input).map_err(|e| e.to_string())?;
    t.phase("parse", || {
        format!("bytes={} elements={}", input.len(), doc.element_count())
    });
    let mut ctx = Ctx::new();
    let mut sel = doc.selection(&cli.common.ids);
    t.phase("selection", || {
        format!("ids={} sel={}", cli.common.ids.len(), sel.len())
    });
    if cli.tab == "Exclusions" {
        mark_exclusions(&mut doc, &sel, cli.markexc == 1);
        return finish(doc, ctx, false, t);
    }
    let opts = if cli.testmode {
        sel = duplicate_for_testmode(&mut doc, &sel);
        Options::testmode()
    } else {
        Options::from_cli(&cli)
    };
    let mut seld = working_set(&doc, &sel);
    t.phase("workingset", || format!("seld={}", seld.len()));
    if opts.deepungroup {
        let before = seld.len();
        move_defs_and_clips_to_root(&mut doc, &mut seld);
        t.phase("defsmove", || {
            format!("moved={}", before.saturating_sub(seld.len()))
        });
    }
    if groups(&doc, &seld).is_empty() && non_containers(&doc, &seld).is_empty() {
        return Err("No objects selected!".to_string());
    }
    if opts.deepungroup {
        let clones = seld.iter().filter(|&&n| doc.tag(n) == "use").count();
        unlink_clones(&mut doc, &mut ctx, &mut seld);
        t.phase("unlink", || format!("clones={clones}"));
        let ngroups = groups(&doc, &seld).len();
        deep_ungroup(&mut doc, &mut ctx, &seld, opts.removetextclips);
        t.phase("ungroup", || format!("groups={ngroups}"));
    }
    let mut ngs = non_containers(&doc, &seld);
    let wrects = if opts.removerectw || opts.reversions || opts.revertpaths {
        rect_passes(&mut doc, &mut ctx, &mut ngs, &opts)
    } else {
        Vec::new()
    };
    t.phase("rects", || format!("ngs={} wrects={}", ngs.len(), wrects.len()));
    if opts.fixtext {
        let before = ngs.len();
        text_phase(&mut doc, &mut ctx, &mut ngs, &opts);
        t.phase("text", || format!("ngs_in={before} ngs_out={}", ngs.len()));
    }
    if opts.removerectw || opts.removeduppaths {
        bbox_stage(&mut doc, &mut ctx, &ngs, &wrects, &opts, &mut t);
    }
    finish(doc, ctx, true, t)
}

/// End of every run: created-clip gc and dangling-reference sweep (`Ctx::finish`), whitespace and
/// `unlinked_clone` markers when the document was flattened (F:513–548, spec §B.3 step 9).
fn finish(
    mut doc: Doc,
    mut ctx: Ctx,
    flattened: bool,
    mut t: crate::log::Timer,
) -> Result<Output, String> {
    ctx.finish(&mut doc);
    if flattened {
        strip_whitespace(&mut doc);
        strip_attr(&mut doc, "unlinked_clone");
    }
    t.phase("cleanup", String::new);
    let messages = ctx.warn.0.iter().map(|w| format!("warning: {w}")).collect();
    let mut svg = Vec::new();
    doc.write(&mut svg);
    t.phase("write", || format!("bytes={}", svg.len()));
    t.total(String::new);
    Ok(Output { svg, messages })
}
```

`bbox_stage` gains the timer and logs each sub-stage; `remove_duplicates` and `remove_white_rects` return how many elements they removed (`removed.len()` and `deleted.len()` — add `-> usize` and return those values; nothing else changes in them):

```rust
pub(crate) fn bbox_stage(
    doc: &mut Doc,
    ctx: &mut Ctx,
    ngs: &[NodeId],
    wrects: &[NodeId],
    o: &Options,
    t: &mut crate::log::Timer,
) {
    let ngset: HashSet<NodeId> = attached(doc, ngs).into_iter().collect();
    let mut ngs2: Vec<NodeId> = doc
        .descendants(doc.svg())
        .filter(|n| ngset.contains(n) && is_drawn(doc, *n))
        .collect();
    let bbs = bb2(doc, ctx, &ngs2, true);
    t.phase("bbox", || format!("ngs2={} boxes={}", ngs2.len(), bbs.len()));
    if o.removeduppaths {
        let cands = ngs2.len();
        let removed = remove_duplicates(doc, ctx, &mut ngs2, &bbs);
        t.phase("dedup", || format!("cands={cands} removed={removed}"));
    }
    if o.removerectw {
        let cands = wrects.len();
        let removed = remove_white_rects(doc, ctx, &ngs2, &bbs, wrects);
        t.phase("whiterects", || format!("cands={cands} removed={removed}"));
    }
}
```

- [ ] **Step 5: Instrument the other tools**

Same pattern, fewer phases. In each `run`: `let mut t = crate::log::Timer::new("<tool>");` before `Doc::parse`; `t.phase("parse", || format!("bytes={} elements={}", input.len(), doc.element_count()))` right after it; `t.phase("selection", || format!("ids={} sel={}", cli.common.ids.len(), <sel>.len()))` after the selection; one `t.phase("<stage>", …)` per stage the tool has (Homogenizer: `fontsize`, `distortion`, `family`, `recentre`, `stroke`, `fuse`, `clips` after each `if cli.<option>` block; Scaler: `boxes` after `Boxes::compute`, `scale` after the plot loop; Combine by Color: `combine`; Text Ghoster: `ghost`; Favorite Markers: `apply`); and in each `finish` (or the tail of `run`): `t.phase("cleanup", String::new)` after `ctx.finish`, `t.phase("write", || format!("bytes={}", svg.len()))` after `doc.write`, `t.total(String::new)`. Thread `t` into `finish` as an extra `mut t: crate::log::Timer` parameter where a `finish` exists (Homogenizer, Scaler).

About (`src/tools/about.rs`): create `let mut t = crate::log::Timer::new("about");` before `Doc::parse`, replace the trailing `crate::log::line(&format!("tool=about phase=parse …"))` with `t.phase("parse", || format!("elements={elements}"))` placed right after `let elements = doc.element_count();`, add `t.phase("fonts", || format!("faces={} ms={:.0}", fs.face_count(), fs.load_ms()))` after `FontSystem::load()`, and `t.total(String::new)` before `Ok(…)`. (`tests/cli.rs:126` asserts the substring `tool=about phase=parse`; the new line contains it.)

`FontSystem::load()` (`src/text/fonts.rs`) logs once per load, so every caller is covered:

```rust
    pub fn load() -> FontSystem {
        let t0 = Instant::now();
        let scan = scanned();
        let fs = Self::from_entries(scan.0.clone(), scan.1.clone(), t0);
        crate::log::line(&format!(
            "phase=fonts ms={:.1} faces={} scans={}",
            fs.load_ms,
            fs.infos.len(),
            scan_count()
        ));
        fs
    }
```

- [ ] **Step 6: Fix the help text**

In `src/cli.rs` replace the line `Set SCIINK_LOG=<file> to append timing information.` with:

```
Set SCIINK_LOG=<file> (or pass --log <file>) to append one line per phase of a run with its
duration in milliseconds. Nothing is written to stderr, which Inkscape shows as a dialog.
```

In `docs/DEVELOPING.md`, the `SCIINK_LOG` row of the environment table becomes: `Append-only log: one line per phase of every run (\`tool=<tool> phase=<name> dt=<ms>\`), the Diagnostics summary and the source location of any internal error. stderr is the user's dialog, so nothing else is written there.`

- [ ] **Step 7: Run the tests**

Run: `cargo test`
Expected: PASS (310 tests; the new cli test included; `tests/cli.rs::about…` still finds `tool=about phase=parse`).

- [ ] **Step 8: Commit**

```bash
git add src/log.rs src/cli.rs src/tools src/text/fonts.rs docs/DEVELOPING.md tests/cli.rs
git commit -m "perf(log): per-phase SCIINK_LOG timing for every tool

Timer emits tool=<tool> phase=<name> dt=<ms> lines; FontSystem::load logs
the font scan; the help text now describes what the log contains.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Baseline (no code)

**Files:**
- Modify: this plan (append a section `## Appendix A — baseline (Task 2)` at the end)

- [ ] **Step 1: Build and measure**

```bash
cargo build --release
BIG=/Users/yzheng/Documents/Manuscripts/MetPredict/metpredict_figures_clean.svg
for sel in figure_1-3 g660 g9740 layer1; do
  rm -f /tmp/sciink-l; target/release/sciink --tool=flattener --id=$sel --log /tmp/sciink-l "$BIG" > /dev/null 2>/dev/null
  target/release/sciink --tool=flattener --id=$sel --log /tmp/sciink-l "$BIG" > /dev/null 2>/dev/null
  echo "== flattener $sel (second run)"; tail -n 14 /tmp/sciink-l
done
for sel in figure_1-3 layer1; do
  rm -f /tmp/sciink-l; target/release/sciink --tool=homogenizer --setfontsize=true --fontsize=7 --fontmodes=2 --id=$sel --log /tmp/sciink-l "$BIG" > /dev/null 2>/dev/null
  echo "== homogenizer $sel"; cat /tmp/sciink-l
done
rm -f /tmp/sciink-l; target/release/sciink --tool=about --log /tmp/sciink-l "$BIG" > /dev/null 2>/dev/null; echo "== about"; cat /tmp/sciink-l
```

- [ ] **Step 2: Record**

Paste the phase lines (second run of each) into `## Appendix A — baseline (Task 2)` of this plan, as a table `selection | phase | dt ms`. Note the machine and date. This is the number every later task is measured against.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/plans/2026-09-23-plan9-big-documents.md
git commit -m "docs(plan9): baseline phase timings

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Writer — run-copying escapes and a sized output buffer

**Files:**
- Modify: `src/dom.rs:108-127` (`Doc` struct), `:131-150` (`parse` literal), `:309-312` (`write` head), `:1001-1026` (`escape_text`, `escape_attr`)
- Test: `tests/dom.rs` (append)

**Interfaces:**
- Produces: `Doc.source_len: usize` (private; `0` for documents built in code). Output bytes are unchanged.

- [ ] **Step 1: Write the failing tests**

Append to `tests/dom.rs`:

```rust
/// The byte-at-a-time escapers this plan replaces, kept as the reference.
fn escape_text_ref(s: &str, out: &mut Vec<u8>) {
    for b in s.bytes() {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            _ => out.push(b),
        }
    }
}
fn escape_attr_ref(s: &str, out: &mut Vec<u8>) {
    for b in s.bytes() {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b'\n' => out.extend_from_slice(b"&#10;"),
            b'\r' => out.extend_from_slice(b"&#13;"),
            b'\t' => out.extend_from_slice(b"&#9;"),
            _ => out.push(b),
        }
    }
}

/// Every string of length ≤ 4 over the special bytes plus three ordinary chars, as an
/// attribute value and as text: the document must serialise exactly as the reference escapers say.
#[test]
fn escaping_is_unchanged_for_every_special_byte_position() {
    let alphabet: Vec<&str> = vec!["&", "<", ">", "\"", "\n", "\r", "\t", "a", "é", "𝄞"];
    let mut cases: Vec<String> = vec![String::new()];
    for len in 1..=4 {
        let mut next = Vec::new();
        for c in &cases {
            if c.chars().count() == len - 1 {
                for a in &alphabet {
                    next.push(format!("{c}{a}"));
                }
            }
        }
        cases.extend(next);
    }
    for s in &cases {
        // attribute: build the document from the escaped reference form so parse() sees the value `s`
        let mut esc_attr = Vec::new();
        escape_attr_ref(s, &mut esc_attr);
        let mut esc_text = Vec::new();
        // XML parsers may normalise a raw carriage return in text content, so that byte is only
        // exercised inside the attribute (where it is written as &#13;)
        if !s.contains('\r') {
            escape_text_ref(s, &mut esc_text);
        }
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"g\" data-v=\"{}\">{}</g></svg>",
            String::from_utf8(esc_attr).unwrap(),
            String::from_utf8(esc_text).unwrap()
        );
        let doc = Doc::parse(svg.as_bytes()).unwrap();
        let g = doc.by_id("g").unwrap();
        assert_eq!(doc.attr(g, "data-v"), Some(s.as_str()), "value round trip for {s:?}");
        let mut out = Vec::new();
        doc.write(&mut out);
        assert_eq!(String::from_utf8(out).unwrap(), svg, "serialisation for {s:?}");
    }
}

#[test]
fn a_large_attribute_round_trips_byte_for_byte() {
    let payload: String = (0..4_000_000u32)
        .map(|i| b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"[(i % 64) as usize] as char)
        .collect();
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><image id=\"i\" href=\"data:image/png;base64,{payload}\"/></svg>"
    );
    let doc = Doc::parse(svg.as_bytes()).unwrap();
    let mut out = Vec::new();
    doc.write(&mut out);
    assert_eq!(out.len(), svg.len());
    assert!(out == svg.as_bytes(), "large attribute changed");
}
```

- [ ] **Step 2: Run them to verify they pass against the current code (they are regression guards)**

Run: `cargo test --test dom escaping_is_unchanged_for_every_special_byte_position a_large_attribute_round_trips_byte_for_byte`
Expected: PASS (the current byte loop is the reference; the tests pin behaviour before the rewrite).

- [ ] **Step 3: Rewrite the escapers**

Replace `escape_text` and `escape_attr` in `src/dom.rs`:

```rust
/// Copies `s` into `out` escaping `& < > "` — runs of ordinary bytes are copied in bulk.
fn escape_text(s: &str, out: &mut Vec<u8>) {
    let b = s.as_bytes();
    let mut start = 0usize;
    for i in 0..b.len() {
        let rep: &[u8] = match b[i] {
            b'&' => b"&amp;",
            b'<' => b"&lt;",
            b'>' => b"&gt;",
            b'"' => b"&quot;",
            _ => continue,
        };
        out.extend_from_slice(&b[start..i]);
        out.extend_from_slice(rep);
        start = i + 1;
    }
    out.extend_from_slice(&b[start..]);
}

/// Attribute values additionally escape the whitespace characters an attribute cannot hold raw.
fn escape_attr(s: &str, out: &mut Vec<u8>) {
    let b = s.as_bytes();
    let mut start = 0usize;
    for i in 0..b.len() {
        let rep: &[u8] = match b[i] {
            b'&' => b"&amp;",
            b'<' => b"&lt;",
            b'>' => b"&gt;",
            b'"' => b"&quot;",
            b'\n' => b"&#10;",
            b'\r' => b"&#13;",
            b'\t' => b"&#9;",
            _ => continue,
        };
        out.extend_from_slice(&b[start..i]);
        out.extend_from_slice(rep);
        start = i + 1;
    }
    out.extend_from_slice(&b[start..]);
}
```

- [ ] **Step 4: Size the output buffer**

Add the field to `Doc` (after `next_auto_id`): `/// Byte length of the parsed source; \`write\` pre-sizes its buffer from it (0 for built documents). source_len: usize,` and set it in `parse`'s struct literal: `source_len: bytes.len(),`. At the top of `write`, before the stack is built:

```rust
        if out.is_empty() {
            out.reserve(self.source_len + self.source_len / 16);
        }
```

- [ ] **Step 5: Run the tests**

Run: `cargo test`
Expected: PASS (every round-trip test in `tests/dom.rs` unchanged).

- [ ] **Step 6: Measure**

Run the Task 2 Flattener command for `--id=figure_1-3` twice; the `phase=write` line must drop to ≤ 60 ms (baseline ≈ 100–250 ms on the 52 MB file). Record both numbers in the commit message.

- [ ] **Step 7: Commit**

```bash
git add src/dom.rs tests/dom.rs
git commit -m "perf(dom): bulk-copy escaping and a sized output buffer

escape_attr/escape_text copy runs of ordinary bytes with one extend_from_slice
instead of one push per byte; write() reserves the source length up front.
Byte-identical output (tests pin every special-byte position). write phase on
the 52 MB document: <before> ms -> <after> ms.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: Skip identical attribute writes

**Files:**
- Modify: `src/dom.rs:556-580` (`set_attr`)
- Test: `tests/dom.rs` (append)

**Interfaces:** none new. `set_attr(n, name, v)` with `v` equal to the current value is now a no-op (no generation bump), except for `id` when the index does not already point at `n`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn setting_an_attribute_to_its_current_value_bumps_no_generation() {
    let mut doc = Doc::parse(
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"r\" style=\"fill:red\" width=\"3\"/></svg>",
    )
    .unwrap();
    let r = doc.by_id("r").unwrap();
    let (g0, s0) = (doc.generation(), doc.style_generation());
    doc.set_attr(r, "style", "fill:red");
    doc.set_attr(r, "width", "3");
    doc.set_attr(r, "id", "r");
    assert_eq!(doc.generation(), g0, "identical writes must not bump generation");
    assert_eq!(doc.style_generation(), s0, "identical writes must not bump style_generation");
    doc.set_attr(r, "style", "fill:blue");
    assert!(doc.generation() > g0 && doc.style_generation() > s0);
}

#[test]
fn a_duplicate_id_keeps_pointing_at_the_first_node() {
    let mut doc = Doc::parse(
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"dup\" width=\"1\"/><rect id=\"dup\" width=\"2\"/></svg>",
    )
    .unwrap();
    let first = doc.by_id("dup").unwrap();
    assert_eq!(doc.attr(first, "width"), Some("1"));
    let g0 = doc.generation();
    doc.set_attr(first, "id", "dup");
    assert_eq!(doc.by_id("dup"), Some(first));
    assert_eq!(doc.generation(), g0);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test dom setting_an_attribute_to_its_current_value_bumps_no_generation a_duplicate_id_keeps_pointing_at_the_first_node`
Expected: FAIL — `identical writes must not bump generation`.

- [ ] **Step 3: Implement**

At the top of `Doc::set_attr`, right after `let value = value.into();`:

```rust
        if let Some(cur) = self.attr(n, name) {
            if cur == value && (name != "id" || self.ids.get(value.as_str()) == Some(&n)) {
                return; // nothing changes: no attribute write, no generation bump
            }
        }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/dom.rs tests/dom.rs
git commit -m "perf(dom): skip identical attribute writes

An unchanged value no longer rewrites the attribute or bumps the generations
(compose_style writes many unchanged styles during a deep ungroup). The id
index keeps its first-node-wins rule for duplicate ids.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 5: The cascade — stop cloning, fold universal rules, stop re-scanning

**Files:**
- Modify: `src/style.rs:362-386` (`Rule`, `Stylesheet`), `parse_stylesheet` (its final `sheet` return), `:652-693` (`cascaded_style`), `:795-813` (`sheet_value`); `src/ops/cleanup.rs:53-122` (`drop_dangling_refs`, `referenced_clip_ids`)
- Test: `tests/style.rs` (append), `tests/ops_cleanup.rs` (append)

**Interfaces:**
- Produces (private to `style.rs`): `Rule.universal: bool`; `Stylesheet { rules, universal_normal: Style, universal_important: Style, universal_order: usize, universal_folded: bool, props: HashSet<String> }`. `Stylesheet::rule_count()` unchanged. `cascaded_style` output (`Style.0` order and values) is byte-identical to before.

Why: on the 52 MB document every cascaded node applies 354 declarations from 177 `*{…}` `<style>` elements, cloning two `String`s each; `fix_css_clipmask` asks `sheet_value(n, "clip-path")` for every clipped child (~29 000 scans of 177 rules); `ops::cleanup` parses 35 963 inline styles per pass.

- [ ] **Step 1: Write the failing tests**

Append to `tests/style.rs` (it has `doc()`, `id()`, `NS`):

```rust
#[test]
fn a_sheet_of_many_universal_rules_cascades_to_the_same_style_as_one_merged_rule() {
    let mut rules = String::new();
    for i in 0..200 {
        // every rule sets stroke-linejoin; every 7th also stroke-width; every 50th an !important fill
        rules.push_str(&format!("*{{stroke-linejoin: {}; ", if i % 2 == 0 { "round" } else { "bevel" }));
        if i % 7 == 0 {
            rules.push_str(&format!("stroke-width: {i}px; "));
        }
        if i % 50 == 0 {
            rules.push_str(&format!("fill: #{i:02x}0000 !important; "));
        }
        rules.push_str("}\n");
    }
    let many = doc(&format!(
        r#"<svg {NS}><style>{rules}</style><g id="g" style="fill:blue"><rect id="r" stroke-width="1"/></g></svg>"#
    ));
    // the same declarations written as one rule, in the order a single pass would produce
    let one = doc(&format!(
        r#"<svg {NS}><style>*{{stroke-linejoin: bevel; stroke-width: 196px; fill: #960000 !important}}</style><g id="g" style="fill:blue"><rect id="r" stroke-width="1"/></g></svg>"#
    ));
    let a = many.cascaded_style(id(&many, "r"));
    let b = one.cascaded_style(id(&one, "r"));
    assert_eq!(a.to_css(), b.to_css());
    assert_eq!(
        a.to_css(),
        "stroke-width:196px;stroke-linejoin:bevel;fill:#960000",
        "presentation attribute first (position), universal rules override its value, !important last"
    );
    let ga = many.cascaded_style(id(&many, "g"));
    assert_eq!(ga.to_css(), "stroke-linejoin:bevel;stroke-width:196px;fill:#960000", "!important beats inline");
}

#[test]
fn universal_rules_still_lose_to_a_tag_rule_and_to_inline_style() {
    let d = doc(&format!(
        r#"<svg {NS}><style>*{{fill:red;stroke:red}} rect{{fill:green}} *{{stroke:blue}}</style><rect id="r" style="stroke:black"/><path id="p"/></svg>"#
    ));
    assert_eq!(d.cascaded_style(id(&d, "r")).to_css(), "fill:green;stroke:black");
    assert_eq!(d.cascaded_style(id(&d, "p")).to_css(), "fill:red;stroke:blue");
}

#[test]
fn a_presentation_attribute_still_loses_to_a_universal_rule() {
    let d = doc(&format!(
        r#"<svg {NS}><style>*{{fill:red}}</style><rect id="r" fill="blue" stroke="black"/></svg>"#
    ));
    // position from the attribute (first mention), value from the sheet
    assert_eq!(d.cascaded_style(id(&d, "r")).to_css(), "fill:red;stroke:black");
}

#[test]
fn a_descendant_universal_rule_disables_the_fold_but_not_the_result() {
    let d = doc(&format!(
        r#"<svg {NS}><style>*{{fill:red}} * *{{fill:green}} *{{fill:blue}}</style><g id="g"><rect id="r"/></g></svg>"#
    ));
    // source order decides among equal-specificity rules: red, green (matches r, not g), blue
    assert_eq!(d.cascaded_style(id(&d, "r")).to_css(), "fill:blue");
    assert_eq!(d.cascaded_style(id(&d, "g")).to_css(), "fill:blue");
    let d2 = doc(&format!(
        r#"<svg {NS}><style>*{{fill:red}} *{{fill:blue}} * *{{fill:green}}</style><g id="g"><rect id="r"/></g></svg>"#
    ));
    // `* *` matches every element below the root <svg>, so g (a child of the root) is green too
    assert_eq!(d2.cascaded_style(id(&d2, "r")).to_css(), "fill:green");
    assert_eq!(d2.cascaded_style(id(&d2, "g")).to_css(), "fill:green");
}

#[test]
fn sheet_value_returns_none_for_a_property_the_sheet_never_declares() {
    let d = doc(&format!(
        r#"<svg {NS}><style>*{{stroke-linejoin:round}} #r{{fill:red}}</style><rect id="r"/></svg>"#
    ));
    let r = id(&d, "r");
    assert_eq!(d.sheet_value(r, "fill").as_deref(), Some("red"));
    assert_eq!(d.sheet_value(r, "clip-path"), None);
    assert_eq!(d.sheet_value(r, "mask"), None);
}
```

Append to `tests/ops_cleanup.rs` (use its existing document/ctx helpers; if it has none, build them like `tests/ops_bbox.rs` does):

```rust
#[test]
fn ops_cleanup_still_drops_an_inline_clip_path_reference_to_a_deleted_id() {
    let mut d = sciink::dom::Doc::parse(
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><defs><clipPath id="c"><rect width="1" height="1"/></clipPath></defs><rect id="a" style="fill:red;clip-path:url(#c)"/><rect id="b" style="fill:blue"/></svg>"#
        )
        .as_bytes(),
    )
    .unwrap();
    let mut deleted = std::collections::HashSet::new();
    deleted.insert("c".to_string());
    sciink::ops::cleanup::drop_dangling_refs(&mut d, &deleted);
    let a = d.by_id("a").unwrap();
    let b = d.by_id("b").unwrap();
    assert_eq!(d.attr(a, "style"), Some("fill:red"));
    assert_eq!(d.attr(b, "style"), Some("fill:blue"), "untouched style is not re-serialised");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test style sheet_value_returns_none_for_a_property_the_sheet_never_declares a_sheet_of_many_universal_rules`
Expected: the `many universal rules` test PASSES already (it pins current behaviour) and `sheet_value_returns_none…` PASSES too — both are guards. The cleanup test passes as well. All three must stay green through Step 3–5; the point of this task is speed, verified in Step 7.

- [ ] **Step 3: Extend `Rule` and `Stylesheet`, fold universal rules at parse time**

In `src/style.rs`:

```rust
#[derive(Debug, Clone)]
struct Rule {
    selector: Selector,
    /// `(name, value, important)`
    decls: Vec<(String, String, bool)>,
    order: usize,
    /// A lone `*` compound: matches every element unconditionally.
    universal: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    rules: Vec<Rule>,
    /// The lone-`*` rules' declarations folded in source order (see `cascaded_style`); only
    /// meaningful when `universal_folded`.
    universal_normal: Style,
    universal_important: Style,
    /// `order` of the first lone-`*` rule (the folded block's position in the cascade).
    universal_order: usize,
    /// True when every rule of specificity (0,0,0) is a lone `*`, so those rules can be applied as
    /// one pre-merged block without changing the cascade order. A `* *` rule disables the fold.
    universal_folded: bool,
    /// Every property name declared anywhere in the sheet (`sheet_value` short-circuit).
    props: HashSet<String>,
}
```

Wherever `parse_stylesheet` constructs a `Rule`, add `universal: false` (it is computed below). Replace `parse_stylesheet`'s final `sheet` return with `finish_sheet(sheet)` and add:

```rust
/// Computes the per-rule `universal` flag, the folded universal blocks and the property index.
fn finish_sheet(mut sheet: Stylesheet) -> Stylesheet {
    let mut folded = true;
    let mut first: Option<usize> = None;
    for r in &mut sheet.rules {
        let c = &r.selector.parts[0].0;
        r.universal = r.selector.parts.len() == 1
            && c.tag.is_none()
            && c.id.is_none()
            && c.classes.is_empty();
        if r.selector.specificity == (0, 0, 0) && !r.universal {
            folded = false;
        }
        for (k, _, _) in &r.decls {
            sheet.props.insert(k.clone());
        }
    }
    if folded {
        for r in sheet.rules.iter().filter(|r| r.universal) {
            first.get_or_insert(r.order);
            for (k, v, imp) in &r.decls {
                if *imp {
                    sheet.universal_important.set_decl(k, v);
                } else {
                    sheet.universal_normal.set_decl(k, v);
                }
            }
        }
    }
    sheet.universal_folded = folded;
    sheet.universal_order = first.unwrap_or(0);
    sheet
}
```

(`parts` is never empty for a parsed rule; if the parser can produce an empty `parts`, guard with `r.selector.parts.first()` and treat it as non-universal.)

Why the fold is exact: in the sorted declaration list a lone-`*` rule carries key `(imp, false, (0,0,0), order+1)`. When every (0,0,0) rule is a lone `*`, the non-important ones form one contiguous block (after the presentation attributes `(false,false,(0,0,0),0)`, before every rule with higher specificity) and the important ones another (after all non-important declarations, before higher-specificity `!important` rules and inline `!important`). `Style::set_decl` gives a property the position of its first mention and the value of its last, so folding a block in source order and re-applying the folded list at the block's position produces the same `Style.0` — and the same `to_css()` bytes — as applying the rules one by one.

- [ ] **Step 4: Rewrite `cascaded_style` to borrow, and short-circuit `sheet_value`**

```rust
    /// The element's own declarations from all three sources, no inheritance.
    /// This is what `ungroup` pushes down onto children.
    pub fn cascaded_style(&self, n: NodeId) -> Style {
        // sort key: (important, is_inline, specificity, order) — later wins; the sort is stable,
        // so declarations pushed with equal keys keep their push order
        let sheet = self.stylesheet();
        let inline = self.attr(n, "style");
        let mut decls: Vec<(DeclKey, Cow<'_, str>, &str)> = Vec::new();
        for a in self.attrs(n) {
            if PRESENTATION_ATTRS.contains(&a.name.as_str()) {
                decls.push((
                    (false, false, (0, 0, 0), 0),
                    Cow::Borrowed(a.name.as_str()),
                    a.value.as_str(),
                ));
            }
        }
        if sheet.universal_folded {
            let key = (false, false, (0, 0, 0), sheet.universal_order + 1);
            for (k, v) in &sheet.universal_normal.0 {
                decls.push((key, Cow::Borrowed(k.as_str()), v.as_str()));
            }
            let key = (true, false, (0, 0, 0), sheet.universal_order + 1);
            for (k, v) in &sheet.universal_important.0 {
                decls.push((key, Cow::Borrowed(k.as_str()), v.as_str()));
            }
        }
        for rule in &sheet.rules {
            if (sheet.universal_folded && rule.universal) || !rule.selector.matches(self, n) {
                continue;
            }
            for (k, v, imp) in &rule.decls {
                decls.push((
                    (*imp, false, rule.selector.specificity, rule.order + 1),
                    Cow::Borrowed(k.as_str()),
                    v.as_str(),
                ));
            }
        }
        if let Some(inline) = inline {
            for (idx, decl) in split_declarations(inline).enumerate() {
                let Some((k, v)) = decl.split_once(':') else {
                    continue;
                };
                let k = k.trim();
                let (v, imp) = strip_important(v.trim());
                if k.is_empty() || v.is_empty() {
                    continue;
                }
                let k = if k.bytes().any(|b| b.is_ascii_uppercase()) {
                    Cow::Owned(k.to_ascii_lowercase())
                } else {
                    Cow::Borrowed(k)
                };
                decls.push(((imp, true, (0, 0, 0), idx), k, v));
            }
        }
        decls.sort_by_key(|d| d.0);
        let mut st = Style::default();
        for (_, k, v) in &decls {
            st.set_decl(k, v);
        }
        st
    }
```

Add `use std::borrow::Cow;` to the imports. In `sheet_value`, insert after `let sheet = self.stylesheet();`:

```rust
        if !sheet.props.contains(prop) {
            return None;
        }
```

- [ ] **Step 5: Stop parsing every inline style in `ops::cleanup`**

In `drop_dangling_refs`, wrap the inline-style block:

```rust
        if let Some(inline) = doc.attr(n, "style") {
            if !(inline.contains("clip-path") || inline.contains("mask")) {
                continue;
            }
            let mut st = Style::parse(inline);
            …unchanged…
        }
```

(`continue` is correct because the attribute checks above it have already run for this node.) In `referenced_clip_ids`, guard the same way:

```rust
        if let Some(inline) = doc.attr(n, "style") {
            if inline.contains("clip-path") || inline.contains("mask") {
                let st = Style::parse(inline);
                for att in ["clip-path", "mask"] {
                    if let Some(id) = st.get(att).and_then(url_id) {
                        out.insert(id.to_string());
                    }
                }
            }
        }
```

A property can only be present in a parsed style if its name appears in the text, so both filters are exact.

- [ ] **Step 6: Run the tests**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS with no edits to any existing test (`tests/style.rs`, `tests/ops_style.rs`, `tests/ops_clip.rs`, `tests/flattener*.rs`, `tests/homogenizer*.rs` all green).

- [ ] **Step 7: Measure**

Task 2 command, `--id=layer1`: `phase=ungroup` must be at most half the baseline; record before/after in the commit message. `--id=figure_1-3` `phase=ungroup` should drop as well.

- [ ] **Step 8: Commit**

```bash
git add src/style.rs src/ops/cleanup.rs tests/style.rs tests/ops_cleanup.rs
git commit -m "perf(style): borrow declarations, fold lone-* rules, index declared properties

cascaded_style no longer clones two Strings per declaration; the 177 matplotlib
*{...} rules of a multi-figure document are pre-merged once per sheet (exact:
they form one contiguous block in the cascade order); sheet_value returns None
without scanning when the sheet never declares the property; ops::cleanup only
parses inline styles that mention clip-path or mask. ungroup phase on the whole
layer: <before> ms -> <after> ms.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: DOM — a move is one operation, not a detach plus an attach

**Files:**
- Modify: `src/dom.rs:108-127` (`Doc` struct: `style_elements`), `:131-150` (`parse` literal), `:290-293` (`alloc`), `:662-676` (`set_tag`), `:756-782` (`detach`), `:799-833` (`append_child`, `insert_before`), `:906-943` (`after_attach` → `after_move`, `index_subtree`, `unindex_subtree`, `subtree_has_style`)
- Test: `tests/dom.rs` (append)

**Interfaces:** none public. Semantics preserved: after any move, `by_id` resolves every id to the same node as before; `generation` and `style_generation` advance by exactly one per move; `sheet_generation` advances only when the moved subtree contains a `<style>`.

Why: `ungroup` moves every child of every group with `insert_after`, which today calls `detach` (an `unindex_subtree` walk allocating a `String` per id) and then `after_attach` (an `index_subtree` walk allocating them all again), plus two `subtree_has_style` walks — on the 52 MB document that is on the order of half a million pointless allocations and four subtree walks per move.

- [ ] **Step 1: Write the failing tests**

Append to `tests/dom.rs`:

```rust
const MOVE_DOC: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"a\"><g id=\"b\"><rect id=\"c\"/><rect id=\"d\"/></g></g><g id=\"z\"/></svg>";

#[test]
fn moving_a_subtree_keeps_every_id_resolvable() {
    let mut doc = Doc::parse(MOVE_DOC.as_bytes()).unwrap();
    let (b, z) = (doc.by_id("b").unwrap(), doc.by_id("z").unwrap());
    let before: Vec<_> = ["a", "b", "c", "d", "z"].iter().map(|i| doc.by_id(i).unwrap()).collect();
    doc.append_child(z, b);
    let after: Vec<_> = ["a", "b", "c", "d", "z"].iter().map(|i| doc.by_id(i).unwrap()).collect();
    assert_eq!(before, after);
    assert_eq!(doc.parent(b), Some(z));
    doc.insert_before(b, doc.by_id("a").unwrap());
    let again: Vec<_> = ["a", "b", "c", "d", "z"].iter().map(|i| doc.by_id(i).unwrap()).collect();
    assert_eq!(before, again);
}

#[test]
fn moving_a_subtree_bumps_each_generation_exactly_once() {
    let mut doc = Doc::parse(MOVE_DOC.as_bytes()).unwrap();
    let (b, z) = (doc.by_id("b").unwrap(), doc.by_id("z").unwrap());
    let (g, s, sh) = (doc.generation(), doc.style_generation(), doc.sheet_generation());
    doc.append_child(z, b);
    assert_eq!(doc.generation(), g + 1);
    assert_eq!(doc.style_generation(), s + 1);
    assert_eq!(doc.sheet_generation(), sh, "no <style> moved");
    let mut with_style = Doc::parse(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"a\"><style id=\"s\">*{fill:red}</style></g><g id=\"z\"/></svg>".as_bytes(),
    )
    .unwrap();
    let (a, z) = (with_style.by_id("a").unwrap(), with_style.by_id("z").unwrap());
    let sh = with_style.sheet_generation();
    with_style.append_child(z, a);
    assert!(with_style.sheet_generation() > sh, "a moved <style> re-orders the sheet");
}

#[test]
fn detaching_then_reattaching_a_subtree_restores_the_id_index() {
    let mut doc = Doc::parse(MOVE_DOC.as_bytes()).unwrap();
    let (b, z) = (doc.by_id("b").unwrap(), doc.by_id("z").unwrap());
    doc.detach(b);
    assert_eq!(doc.by_id("b"), None);
    assert_eq!(doc.by_id("c"), None);
    doc.append_child(z, b);
    assert_eq!(doc.by_id("b"), Some(b));
    assert_eq!(doc.by_id("c").map(|c| doc.parent(c)), Some(Some(b)));
}

#[test]
fn a_duplicate_id_keeps_pointing_at_the_first_node_after_a_move() {
    let mut doc = Doc::parse(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"first\"><rect id=\"dup\" width=\"1\"/></g><g id=\"second\"><rect id=\"dup\" width=\"2\"/></g><g id=\"z\"/></svg>".as_bytes(),
    )
    .unwrap();
    let first_dup = doc.by_id("dup").unwrap();
    assert_eq!(doc.attr(first_dup, "width"), Some("1"));
    let (second, z) = (doc.by_id("second").unwrap(), doc.by_id("z").unwrap());
    doc.append_child(z, second);
    assert_eq!(doc.by_id("dup"), Some(first_dup), "moving the shadowed node changes nothing");
    let first = doc.by_id("first").unwrap();
    doc.append_child(z, first);
    assert_eq!(doc.by_id("dup"), Some(first_dup), "moving the indexed node keeps it indexed");
}
```

- [ ] **Step 2: Run them to verify the generation test fails**

Run: `cargo test --test dom moving_a_subtree_bumps_each_generation_exactly_once`
Expected: FAIL — generation advanced by 2 (detach + attach). The other three pass already and stay as guards.

- [ ] **Step 3: Implement**

Add to `Doc`: `/// Number of \`<style>\` elements ever created in this document (never decremented on delete); zero means \`subtree_has_style\` can answer without walking. style_elements: usize,` — initialise `style_elements: 0` in `parse`. In `alloc`:

```rust
    fn alloc(&mut self, kind: Kind) -> NodeId {
        if let Kind::Element { name, .. } = &kind {
            let local = name.rsplit_once(':').map(|(_, l)| l).unwrap_or(name);
            if local == "style" {
                self.style_elements += 1;
            }
        }
        self.nodes.push(Node::new(kind));
        (self.nodes.len() - 1) as NodeId
    }
```

In `set_tag`, after the rename: `if !was_style && local == "style" { self.style_elements += 1; }` (the `was_style && local != "style"` case leaves the counter alone — it is an upper bound).

Replace `detach`, `append_child`, `insert_before`, `after_attach`, `index_subtree`, `unindex_subtree`, `subtree_has_style`:

```rust
    /// Pointer surgery only: takes `n` out of its parent's child list, leaving the id index,
    /// the generations and the subtree untouched. `false` when `n` was already detached.
    fn unlink(&mut self, n: NodeId) -> bool {
        let Some(p) = self.nodes[n as usize].parent else {
            return false;
        };
        let (prev, next) = (self.nodes[n as usize].prev, self.nodes[n as usize].next);
        match prev {
            Some(x) => self.nodes[x as usize].next = next,
            None => self.nodes[p as usize].first = next,
        }
        match next {
            Some(x) => self.nodes[x as usize].prev = prev,
            None => self.nodes[p as usize].last = prev,
        }
        let node = &mut self.nodes[n as usize];
        node.parent = None;
        node.prev = None;
        node.next = None;
        true
    }

    /// Unlinks `n` from its parent (keeping its subtree). No-op if detached.
    pub fn detach(&mut self, n: NodeId) {
        if !self.unlink(n) {
            return;
        }
        self.unindex_subtree(n);
        if self.subtree_has_style(n) {
            self.bump_sheet();
        }
        // Detaching changes the (now former) ancestor chain the subtree saw;
        // see the comment in `after_move`.
        self.bump_style();
        self.bump();
    }

    /// Bookkeeping after `n` was linked in. The id index only learns material that was not in
    /// the document before: `index_subtree` is `or_insert`-only and no id changes during a move,
    /// so re-indexing a moved subtree would be a no-op. A moved `<style>` still bumps the sheet
    /// (the sheet concatenates `<style>` text in document order).
    fn after_move(&mut self, n: NodeId, was_attached: bool) {
        if !was_attached {
            self.index_subtree(n);
        }
        if self.subtree_has_style(n) {
            self.bump_sheet();
        }
        // Attaching changes the tree shape the attached subtree sees (its
        // ancestor chain, and thus which descendant/child selectors match),
        // regardless of whether a <style> element is involved.
        self.bump_style();
        self.bump();
    }

    pub fn append_child(&mut self, parent: NodeId, n: NodeId) {
        self.assert_can_attach(n, parent);
        let was = self.unlink(n);
        self.link_last(parent, n);
        self.after_move(n, was);
    }

    pub fn insert_before(&mut self, n: NodeId, anchor: NodeId) {
        self.assert_can_attach(n, anchor);
        let was = self.unlink(n);
        let p = self.nodes[anchor as usize]
            .parent
            .expect("anchor must be attached");
        let prev = self.nodes[anchor as usize].prev;
        {
            let node = &mut self.nodes[n as usize];
            node.parent = Some(p);
            node.prev = prev;
            node.next = Some(anchor);
        }
        self.nodes[anchor as usize].prev = Some(n);
        match prev {
            Some(x) => self.nodes[x as usize].next = Some(n),
            None => self.nodes[p as usize].first = Some(n),
        }
        self.after_move(n, was);
    }

    fn index_subtree(&mut self, n: NodeId) {
        let nodes: Vec<NodeId> = self.descendants(n).collect();
        for d in nodes {
            let Kind::Element { attrs, .. } = &self.nodes[d as usize].kind else {
                continue;
            };
            let Some(a) = attrs.iter().find(|a| a.name == "id") else {
                continue;
            };
            if !self.ids.contains_key(a.value.as_str()) {
                self.ids.insert(a.value.clone(), d);
            }
        }
    }

    fn unindex_subtree(&mut self, n: NodeId) {
        let nodes: Vec<NodeId> = self.descendants(n).collect();
        for d in nodes {
            let Kind::Element { attrs, .. } = &self.nodes[d as usize].kind else {
                continue;
            };
            let Some(a) = attrs.iter().find(|a| a.name == "id") else {
                continue;
            };
            if self.ids.get(a.value.as_str()) == Some(&d) {
                self.ids.remove(a.value.as_str());
            }
        }
    }

    fn subtree_has_style(&self, n: NodeId) -> bool {
        self.style_elements > 0
            && self
                .descendants(n)
                .any(|d| self.is_element(d) && self.tag(d) == "style")
    }
```

(`prepend_child`, `insert_after` and `replace` are unchanged: they delegate to the two functions above. The field-level borrows in `index_subtree`/`unindex_subtree` — `&self.nodes[..].kind` next to `self.ids.insert` — are disjoint fields, which the borrow checker accepts inside one method body; do not route them through `self.attr()`.)

- [ ] **Step 4: Run the tests**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS. If any existing test asserted a generation delta of two per move it was asserting an implementation detail; there is none in the suite today — if one appears, stop and report rather than editing it.

- [ ] **Step 5: Measure**

Task 2 command, `--id=layer1`: `phase=ungroup` ≤ 1 000 ms together with Task 5 (baseline ≈ 2 500 ms). Record in the commit message.

- [ ] **Step 6: Commit**

```bash
git add src/dom.rs tests/dom.rs
git commit -m "perf(dom): a move is one operation, not a detach plus an attach

insert_before/append_child unlink the node with pointer surgery and re-index
only material that was not in the document; the id index no longer allocates a
String per id per walk; subtree_has_style answers without walking when the
document has no <style>. ungroup phase on the whole layer: <before> -> <after> ms.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 7: Scope the Flattener's character table to what it measures (parity fix)

**Files:**
- Modify: `src/ops/mod.rs:83-133` (`Ctx`), `src/text/table.rs:13-19,42-…` (`CharTable`), `src/tools/flattener.rs` (`run`, `bbox_stage`, visibility of the phase functions), `src/tools/combine_by_color.rs:42-60,139-156`
- Test: `tests/flattener.rs` (append)

**Interfaces:**
- Produces: `Ctx::set_text_roots(&mut self, roots: Vec<NodeId>)` (ignored once the table exists), `Ctx::char_table_els(&self) -> Option<usize>`, `CharTable::el_count(&self) -> usize`; `flattener::{working_set, attached, non_containers, groups, move_defs_and_clips_to_root, unlink_clones, deep_ungroup, rect_passes, replace_fonts, text_phase, bbox_stage, remove_duplicates, remove_white_rects, mark_exclusions, duplicate_for_testmode}` become `pub`; `combine_by_color::candidates_from(doc: &Doc, roots: &[NodeId]) -> Vec<NodeId>` (and `candidates(doc, ids)` keeps working by calling it).

Why: `flattener::run` builds `Ctx::new()`, so the first text measurement in `bbox_stage` walks the whole document and measures every `<text>` (5 039 on the 52 MB file, ≈ 0.5 s of a 1.1 s one-figure run). Upstream measures only the bbox request set (`BB2(svg, ngs2)` → `make_char_table(els=tels)`). The bbox stage is the Flattener's only text measurement: `rect_passes` measures `RECT_TAGS` only, `revert_minus` a glyph path, and `text_phase`'s `remove_kerning` builds its own table over the selection's texts.

- [ ] **Step 1: Write the failing tests**

Append to `tests/flattener.rs` (it has `NS`, `DV`, `flatten`, `with_vendored_fonts`):

```rust
use sciink::dom::Doc;
use sciink::ops::Ctx;
use sciink::tools::flattener::{bbox_stage, deep_ungroup, non_containers, working_set, Options};

fn three_figures() -> String {
    let mut figs = String::new();
    for i in 0..3 {
        let y = i * 10;
        figs.push_str(&format!(
            r#"<g id="fig{i}"><g id="fig{i}g"><rect id="fig{i}r" x="0" y="{y}" width="10" height="5" style="fill:#ffffff"/><text id="fig{i}t1" x="1" y="{}" style="{DV};font-size:4px">a{i}</text><text id="fig{i}t2" x="5" y="{}" style="{DV};font-size:4px">b{i}</text></g></g>"#,
            y + 4,
            y + 4
        ));
    }
    format!(r#"<svg {NS} width="20" height="40"><g id="layer1">{figs}</g></svg>"#)
}

#[test]
fn the_character_table_covers_only_the_selected_figures_text() {
    with_vendored_fonts(|| {
        let mut doc = Doc::parse(three_figures().as_bytes()).unwrap();
        let sel = doc.selection(&["fig1".to_string()]);
        let mut ctx = Ctx::for_roots(sel.clone());
        let seld = working_set(&doc, &sel);
        deep_ungroup(&mut doc, &mut ctx, &seld, true);
        let ngs = non_containers(&doc, &seld);
        let mut t = sciink::log::Timer::new("test");
        bbox_stage(&mut doc, &mut ctx, &ngs, &[], &Options::testmode(), &mut t);
        assert_eq!(ctx.char_table_els(), Some(2), "fig1's two texts, not the document's six");
    });
}

#[test]
fn font_warnings_name_only_fonts_in_the_selection() {
    let svg = format!(
        r#"<svg {NS} width="20" height="20"><g id="fa"><text id="ta" x="1" y="5" style="font-family:'Nonexistent Family';font-size:4px">a</text></g><g id="fb"><text id="tb" x="1" y="15" style="{DV};font-size:4px">b</text></g></svg>"#
    );
    let (_, msgs) = flatten(&svg, &["--id=fb"]);
    assert!(
        !msgs.iter().any(|m| m.contains("Nonexistent Family")),
        "a font outside the selection must not be measured: {msgs:?}"
    );
    let (_, msgs) = flatten(&svg, &["--id=fa"]);
    assert!(msgs.iter().any(|m| m.contains("Nonexistent Family")), "{msgs:?}");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test flattener the_character_table_covers_only_the_selected_figures_text font_warnings_name_only_fonts_in_the_selection`
Expected: compile error (`bbox_stage` etc. are `pub(crate)`, `char_table_els` does not exist). After making the functions `pub` (Step 3) and before scoping, the first test FAILS with `Some(6)` and the second with the warning for `fb`.

- [ ] **Step 3: Implement**

`src/text/table.rs`: add `el_count: usize` to `CharTable`, set `el_count: els.len()` in the struct literal `build` returns, and add

```rust
    /// How many elements `build` was given (the table's scope).
    pub fn el_count(&self) -> usize {
        self.el_count
    }
```

`src/ops/mod.rs`, in `impl Ctx`:

```rust
    /// Restricts the character table to the text under `roots`. Ignored once the table exists —
    /// call it before the first measurement (the Flattener calls it with the bbox request set).
    pub fn set_text_roots(&mut self, roots: Vec<NodeId>) {
        if self.text.is_none() {
            self.text_roots = Some(roots);
        }
    }

    /// How many `<text>`/`<flowRoot>` elements the table covers; `None` before it is built.
    pub fn char_table_els(&self) -> Option<usize> {
        self.text.as_ref().map(|t| t.el_count())
    }
```

`src/tools/flattener.rs`: change every `pub(crate) fn` listed in Interfaces to `pub fn`. In `run`, replace `let mut ctx = Ctx::new();` + `let mut sel = …` with

```rust
    let mut sel = doc.selection(&cli.common.ids);
    let mut ctx = Ctx::for_roots(sel.clone());
```

(keep the `t.phase("selection", …)` line after it). In `bbox_stage`, between the `ngs2` collection and `bb2`:

```rust
    // upstream BB2(svg, ngs2) → make_char_table(els = tels): measure — and warn about the fonts
    // of — exactly the elements whose boxes are requested
    ctx.set_text_roots(ngs2.clone());
```

and extend the phase line: `t.phase("bbox", || format!("ngs2={} boxes={} table_texts={}", ngs2.len(), bbs.len(), ctx.char_table_els().unwrap_or(0)));`.

Why `set_text_roots(ngs2)` and not `for_roots(sel)` alone: `deep_ungroup` dissolves the selected `<g>` (the user's figure group is one), leaving the original root detached and its `descendants` empty; `ngs2` is the live set.

`src/tools/combine_by_color.rs`:

```rust
/// CBC:39–60 over the given roots (the selection and its descendants, document order,
/// deduplicated), keeping the path-like elements — not a skipped tag, and carrying `d`, `points`
/// or `x1`.
pub fn candidates_from(doc: &Doc, roots: &[NodeId]) -> Vec<NodeId> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for &root in roots {
        for n in doc.descendants(root) {
            if !doc.is_element(n) || SKIP_TAGS.contains(&doc.tag(n)) {
                continue;
            }
            let pathlike = ["d", "points", "x1"]
                .iter()
                .any(|a| doc.attr(n, a).is_some());
            if pathlike && seen.insert(n) {
                out.push(n);
            }
        }
    }
    out
}

pub fn candidates(doc: &Doc, ids: &[String]) -> Vec<NodeId> {
    candidates_from(doc, &doc.selection(ids))
}
```

and in `run`:

```rust
    let sel = doc.selection(&cli.common.ids);
    t.phase("selection", || format!("ids={} sel={}", cli.common.ids.len(), sel.len()));
    let mut ctx = Ctx::for_roots(sel.clone());
    if sel.is_empty() {
        messages.push("combine-by-color: nothing selected".to_string());
    } else {
        let els = candidates_from(&doc, &sel);
        combine_by_color(&mut doc, &mut ctx, &els, cli.lightnessth / 100.0);
        t.phase("combine", || format!("candidates={}", els.len()));
        ctx.finish(&mut doc);
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS; `tests/flattener_fixtures.rs` and `tests/invariance.rs` unchanged (they select whole layers, whose text sets equal the document's).

- [ ] **Step 5: Measure**

Task 2 command, `--id=figure_1-3`: `phase=bbox` ≤ 120 ms (baseline ≈ 600 ms) with `table_texts=154`; `--id=g660`: `table_texts=39`.

- [ ] **Step 6: Commit**

```bash
git add src/ops/mod.rs src/text/table.rs src/tools/flattener.rs src/tools/combine_by_color.rs tests/flattener.rs
git commit -m "fix(flattener): scope the character table to the measured elements

Upstream builds its table over the bbox request set (BB2(svg, ngs2)); ours
measured every text in the document. Ctx::set_text_roots(ngs2) restores parity
and drops ~0.5 s from every run on a 5 000-text document. Combine by Color
uses Ctx::for_roots and one selection walk. bbox phase, one figure:
<before> -> <after> ms.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: `remove_kerning`'s nested-text check in linear time

**Files:**
- Modify: `src/text/kerning.rs:733-747`
- Test: `tests/text_kerning.rs` (append)

**Interfaces:** none. Warning text (`<name>: nested in another selected text element; not edited`) and order unchanged.

- [ ] **Step 1: Write the failing test**

Append to `tests/text_kerning.rs` (it has `fonts()`, `NS`, `DV`, `Doc`, `Warnings`; `remove_kerning` and `KerningOptions` live in `sciink::text::kerning`):

```rust
#[test]
fn a_thousand_sibling_texts_are_all_edited_and_a_nested_one_is_skipped() {
    use sciink::text::kerning::{remove_kerning, KerningOptions};
    let mut body = String::new();
    for i in 0..1000 {
        body.push_str(&format!(
            r#"<text id="t{i}" x="{}" y="{}" style="{DV};font-size:4px">w{i} x</text>"#,
            (i % 40) * 20,
            (i / 40) * 8 + 5
        ));
    }
    body.push_str(&format!(
        r#"<text id="outer" x="0" y="300" style="{DV};font-size:4px">o<text id="inner" x="10" y="300">i</text></text>"#
    ));
    let svg = format!(r#"<svg {NS} width="900" height="400">{body}</svg>"#);
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let mut els: Vec<_> = (0..1000).map(|i| d.by_id(&format!("t{i}")).unwrap()).collect();
    els.push(d.by_id("outer").unwrap());
    els.push(d.by_id("inner").unwrap());
    let mut w = Warnings::default();
    let t0 = std::time::Instant::now();
    let out = remove_kerning(&mut d, &els, &KerningOptions::from_inx(true, true, true, true, 1), fonts(), &mut w);
    assert!(t0.elapsed().as_secs() < 20, "quadratic nested-text check");
    assert_eq!(out.len(), 1001, "1000 siblings + outer come back; inner is absorbed by outer");
    let nested: Vec<&String> = w.0.iter().filter(|m| m.contains("nested in another selected text")).collect();
    assert_eq!(nested.len(), 1, "{:?}", w.0);
    assert!(nested[0].contains("inner"), "{}", nested[0]);
}
```

(`KerningOptions::from_inx(removemanual, mergesubsuper, splitdistant, mergenearby, justification)` is the constructor `text_phase` uses; check its exact parameter order in `src/text/kerning.rs` and match it.)

- [ ] **Step 2: Run it to verify the current shape passes (guard)**

Run: `cargo test --test text_kerning a_thousand_sibling_texts_are_all_edited_and_a_nested_one_is_skipped`
Expected: PASS (1 002 texts × 1 002 ancestor walks is still fast in isolation; this test guards behaviour, the speed shows on the 5 039-text document).

- [ ] **Step 3: Implement**

Replace the loop at `src/text/kerning.rs:733-747`:

```rust
    // Membership in `cands` via a set: one ancestor walk per element instead of one per pair.
    // `cands` is deduplicated and `ancestors(e)` never yields `e`, so "some member is a strict
    // ancestor of e" is exactly the old `o != e && ancestors(e).any(|a| a == o)` over all `o`.
    let cset: HashSet<NodeId> = cands.iter().copied().collect();
    let mut tels: Vec<NodeId> = Vec::with_capacity(cands.len());
    for &e in &cands {
        if doc.ancestors(e).any(|a| cset.contains(&a)) {
            // the ancestor's parse already absorbed this element's characters
            warn.push(format!(
                "{}: nested in another selected text element; not edited",
                el_name(doc, e)
            ));
            continue;
        }
        tels.push(e);
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Measure**

Task 2 command, `--id=layer1`: `phase=text` drops (baseline ≈ 1 500 ms; expect ≤ 1 100 ms). Record in the commit message.

- [ ] **Step 6: Commit**

```bash
git add src/text/kerning.rs tests/text_kerning.rs
git commit -m "perf(text): linear nested-text check in remove_kerning

One ancestor walk per selected text against a HashSet of the candidates
instead of one walk per pair (25 M walks on a 5 039-text document).
text phase, whole layer: <before> -> <after> ms.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 9: Exact sweeps for the two O(k²) passes

**Files:**
- Create: `src/geom/grid.rs`
- Modify: `src/geom/mod.rs:5` (add `pub mod grid;`), `src/tools/flattener.rs` (`remove_duplicates`, `remove_white_rects`)
- Test: `tests/flattener_sweeps.rs` (new)

**Interfaces:**
- Produces: `geom::grid::BoxGrid { new(extent: Option<Rect>, n: usize), insert(&mut self, id: u32, r: Rect), query(&self, r: Rect, f: &mut dyn FnMut(u32) -> bool) }`; `geom::grid::duplicate_scan(boxes: &[Rect], is_dup: &mut dyn FnMut(usize, usize) -> bool) -> Vec<usize>`; `geom::grid::background_scan(boxes: &[Rect], white: &[bool]) -> Vec<usize>`. `remove_duplicates`/`remove_white_rects` keep their signatures (returning `usize` since Task 1).

Both passes keep their predicates verbatim; only the candidate enumeration changes, and both enumerations are supersets of the pairs the current loops test (proofs in the code comments below), visited in the same order.

- [ ] **Step 1: Write the failing tests**

Create `tests/flattener_sweeps.rs`:

```rust
//! The sweep replacements for the Flattener's all-pairs box tests must reproduce the pairwise
//! reference (copied verbatim from the pre-Plan-9 code) on random and adversarial input.

use kurbo::Rect;
use sciink::geom::grid::{background_scan, duplicate_scan, BoxGrid};
use sciink::geom::intersects;

fn duplicate_scan_ref(boxes: &[Rect], is_dup: &mut dyn FnMut(usize, usize) -> bool) -> Vec<usize> {
    let size = |r: &Rect| r.width().max(r.height());
    let equal = |i: usize, j: usize| -> bool {
        let (a, b) = (&boxes[i], &boxes[j]);
        if a.width() == 0.0 || a.height() == 0.0 || b.width() == 0.0 || b.height() == 0.0 {
            return false;
        }
        let tol = 1e-6 * size(a).max(size(b));
        (a.x0 - b.x0).abs() <= tol
            && (a.y0 - b.y0).abs() <= tol
            && (a.x1 - b.x1).abs() <= tol
            && (a.y1 - b.y1).abs() <= tol
    };
    let mut removed = Vec::new();
    let mut gone = std::collections::HashSet::new();
    for jj in (0..boxes.len()).rev() {
        for ii in 0..jj {
            if gone.contains(&ii) || !equal(ii, jj) {
                continue;
            }
            if is_dup(ii, jj) {
                gone.insert(ii);
                removed.push(ii);
            }
        }
    }
    removed
}

fn background_scan_ref(boxes: &[Rect], white: &[bool]) -> Vec<usize> {
    let mut deleted: Vec<usize> = Vec::new();
    for ii in 0..boxes.len() {
        if !white[ii] {
            continue;
        }
        let wb = boxes[ii];
        let behind = (0..ii).any(|k| !deleted.contains(&k) && intersects(boxes[k], wb));
        if !behind {
            deleted.push(ii);
        }
    }
    deleted
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn lattice_box(&mut self) -> Rect {
        // corners on a coarse lattice so exact duplicates are common; a jitter of 1e-9 (far below
        // the 1e-6 · size tolerance) on some boxes exercises the near-equal path
        let x0 = self.below(20) as f64;
        let y0 = self.below(20) as f64;
        let w = 1.0 + self.below(5) as f64;
        let h = 1.0 + self.below(5) as f64;
        let j = if self.below(4) == 0 { 1e-9 } else { 0.0 };
        Rect::new(x0 + j, y0, x0 + w, y0 + h + j)
    }
}

fn pseudo_dup(ii: usize, jj: usize) -> bool {
    (ii * 7 + jj * 13) % 3 != 0
}

#[test]
fn duplicate_scan_matches_the_pairwise_reference_on_random_boxes() {
    let mut rng = Lcg(42);
    for case in 0..200 {
        let boxes: Vec<Rect> = (0..300).map(|_| rng.lattice_box()).collect();
        let a = duplicate_scan_ref(&boxes, &mut pseudo_dup);
        let b = duplicate_scan(&boxes, &mut pseudo_dup);
        assert_eq!(a, b, "case {case}: removal sequences differ");
        assert!(!a.is_empty(), "case {case} exercised nothing");
    }
}

#[test]
fn duplicate_scan_matches_at_the_tolerance_edge() {
    let base = Rect::new(0.0, 0.0, 10.0, 10.0); // size 10 → tol 1e-5
    let tol = 1e-5;
    let mut boxes = vec![base];
    for d in [tol, tol * (1.0 + 1e-12), tol * (1.0 - 1e-12), -tol, 2.0 * tol] {
        boxes.push(Rect::new(base.x0 + d, base.y0, base.x1 + d, base.y1));
        boxes.push(Rect::new(base.x0, base.y0 + d, base.x1, base.y1 + d));
        boxes.push(Rect::new(base.x0 + d, base.y0 + d, base.x1 + d, base.y1 + d));
        boxes.push(Rect::new(base.x0, base.y0, base.x1 + d, base.y1));
    }
    let all = |_: usize, _: usize| true;
    assert_eq!(duplicate_scan_ref(&boxes, &mut { all }), duplicate_scan(&boxes, &mut { all }));
}

#[test]
fn duplicate_scan_ignores_degenerate_and_non_finite_boxes() {
    let mut rng = Lcg(7);
    let mut boxes: Vec<Rect> = (0..100).map(|_| rng.lattice_box()).collect();
    boxes.push(Rect::new(1.0, 1.0, 1.0, 5.0)); // zero width
    boxes.push(Rect::new(1.0, 1.0, 5.0, 1.0)); // zero height
    boxes.push(Rect::new(f64::NAN, 0.0, 1.0, 1.0));
    boxes.push(Rect::new(0.0, f64::NEG_INFINITY, 1.0, f64::INFINITY));
    boxes.push(Rect::new(-1e6, -1e6, 1e6, 1e6)); // 10⁶× the rest: inflates the bucket size
    boxes.extend((0..100).map(|_| rng.lattice_box()));
    let a = duplicate_scan_ref(&boxes, &mut pseudo_dup);
    let b = duplicate_scan(&boxes, &mut pseudo_dup);
    assert_eq!(a, b);
}

#[test]
fn background_scan_matches_the_pairwise_reference_on_random_boxes() {
    let mut rng = Lcg(99);
    for case in 0..200 {
        let n = 200;
        let mut boxes: Vec<Rect> = (0..n).map(|_| rng.lattice_box()).collect();
        let mut white: Vec<bool> = (0..n).map(|_| rng.below(3) == 0).collect();
        match case % 5 {
            0 => white.iter_mut().for_each(|w| *w = true),
            1 => white.iter_mut().for_each(|w| *w = false),
            2 => boxes[0] = Rect::new(-100.0, -100.0, 100.0, 100.0), // one box covering everything
            3 => boxes[10] = Rect::new(3.0, 3.0, 3.0, 3.0),          // zero-size box inside others
            _ => {}
        }
        assert_eq!(background_scan_ref(&boxes, &white), background_scan(&boxes, &white), "case {case}");
    }
}

#[test]
fn background_scan_respects_the_earlier_in_document_order_rule() {
    // w0 is deleted (nothing earlier); w1 overlaps only w0, which is gone → deleted too;
    // w2 overlaps the surviving path p → kept. Touching edges do not count (strict test).
    let boxes = vec![
        Rect::new(0.0, 0.0, 10.0, 10.0), // w0
        Rect::new(5.0, 5.0, 15.0, 15.0), // w1
        Rect::new(20.0, 0.0, 30.0, 10.0), // p
        Rect::new(25.0, 5.0, 35.0, 15.0), // w2
        Rect::new(30.0, 15.0, 40.0, 25.0), // w3: touches w2's corner only
    ];
    let white = vec![true, true, false, true, true];
    assert_eq!(background_scan(&boxes, &white), vec![0, 1, 4]);
    assert_eq!(background_scan_ref(&boxes, &white), vec![0, 1, 4]);
}

#[test]
fn box_grid_never_misses_an_overlap() {
    let mut rng = Lcg(2024);
    let boxes: Vec<Rect> = (0..2000).map(|_| rng.lattice_box()).collect();
    let extent = boxes.iter().copied().reduce(|a, b| a.union(b));
    let mut grid = BoxGrid::new(extent, boxes.len());
    for (i, r) in boxes.iter().enumerate() {
        grid.insert(i as u32, *r);
    }
    for _ in 0..10_000 {
        let q = rng.lattice_box();
        let mut seen = std::collections::HashSet::new();
        grid.query(q, &mut |id| {
            seen.insert(id);
            true
        });
        for (i, r) in boxes.iter().enumerate() {
            if intersects(*r, q) {
                assert!(seen.contains(&(i as u32)), "grid missed box {i} for query {q:?}");
            }
        }
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test flattener_sweeps`
Expected: compile error — `sciink::geom::grid` does not exist.

- [ ] **Step 3: Create `src/geom/grid.rs`**

```rust
//! Conservative spatial index for axis-aligned boxes, and the two Flattener sweeps built on it
//! (Plan 9 Task 9). `BoxGrid::query` visits a superset of the boxes overlapping the query and
//! never misses one; the callers keep `geom::intersects` / the tolerance test as ground truth.

use std::collections::HashMap;

use kurbo::Rect;

use super::intersects;

/// A box covering more than this many cells on an axis goes to the overflow list.
const MAX_SPAN: usize = 32;

fn finite(r: &Rect) -> bool {
    r.x0.is_finite() && r.y0.is_finite() && r.x1.is_finite() && r.y1.is_finite()
}

/// Uniform bucket grid over a fixed extent. Ids may be visited more than once by `query`.
pub struct BoxGrid {
    x0: f64,
    y0: f64,
    cw: f64,
    ch: f64,
    cols: usize,
    rows: usize,
    cells: Vec<Vec<u32>>,
    /// Boxes spanning too many cells, or not finite: checked on every query.
    large: Vec<u32>,
    /// Every inserted id, for queries that span too many cells.
    all: Vec<u32>,
}

impl BoxGrid {
    /// `extent` must contain every finite box inserted or queried; `n` sizes the grid
    /// (`clamp(ceil(sqrt(n)), 1, 512)` cells per axis; a zero-extent axis collapses to one cell).
    pub fn new(extent: Option<Rect>, n: usize) -> BoxGrid {
        let side = ((n as f64).sqrt().ceil() as usize).clamp(1, 512);
        let (x0, y0, w, h) = match extent {
            Some(r) if finite(&r) => (r.x0, r.y0, r.width(), r.height()),
            _ => (0.0, 0.0, 0.0, 0.0),
        };
        let cols = if w > 0.0 { side } else { 1 };
        let rows = if h > 0.0 { side } else { 1 };
        BoxGrid {
            x0,
            y0,
            cw: if w > 0.0 { w / cols as f64 } else { 1.0 },
            ch: if h > 0.0 { h / rows as f64 } else { 1.0 },
            cols,
            rows,
            cells: vec![Vec::new(); cols * rows],
            large: Vec::new(),
            all: Vec::new(),
        }
    }

    /// Cell range of `r`, or `None` when it is not finite or spans more than `MAX_SPAN` cells.
    fn span(&self, r: Rect) -> Option<(usize, usize, usize, usize)> {
        if !finite(&r) {
            return None;
        }
        let col = |x: f64| (((x - self.x0) / self.cw).floor().max(0.0) as usize).min(self.cols - 1);
        let row = |y: f64| (((y - self.y0) / self.ch).floor().max(0.0) as usize).min(self.rows - 1);
        let (c0, c1) = (col(r.x0.min(r.x1)), col(r.x0.max(r.x1)));
        let (r0, r1) = (row(r.y0.min(r.y1)), row(r.y0.max(r.y1)));
        if c1 - c0 + 1 > MAX_SPAN || r1 - r0 + 1 > MAX_SPAN {
            return None;
        }
        Some((c0, c1, r0, r1))
    }

    pub fn insert(&mut self, id: u32, r: Rect) {
        self.all.push(id);
        match self.span(r) {
            Some((c0, c1, r0, r1)) => {
                for row in r0..=r1 {
                    for col in c0..=c1 {
                        self.cells[row * self.cols + col].push(id);
                    }
                }
            }
            None => self.large.push(id),
        }
    }

    /// Calls `f` for every candidate (a superset of the boxes overlapping `r`); stops when `f`
    /// returns `false`. Two finite boxes that overlap share a point, and that point maps to the
    /// same cell for both (clamping is monotone), so a gridded box is always found; overflow boxes
    /// are always visited; an oversize query falls back to every id.
    pub fn query(&self, r: Rect, f: &mut dyn FnMut(u32) -> bool) {
        let Some((c0, c1, r0, r1)) = self.span(r) else {
            for &id in &self.all {
                if !f(id) {
                    return;
                }
            }
            return;
        };
        for &id in &self.large {
            if !f(id) {
                return;
            }
        }
        for row in r0..=r1 {
            for col in c0..=c1 {
                for &id in &self.cells[row * self.cols + col] {
                    if !f(id) {
                        return;
                    }
                }
            }
        }
    }
}

/// The Flattener's duplicate pass (F:422–497) as a pure index walk. Visits exactly the pairs
/// `(ii, jj)`, `ii < jj`, whose boxes are equal within `1e-6 · max(size_ii, size_jj)` on every
/// coordinate (`size = max(width, height)`, degenerate boxes never equal), in upstream's order —
/// `jj` descending, `ii` ascending within each `jj`, skipping an `ii` already removed — and asks
/// `is_dup(ii, jj)`; an `ii` for which it returns `true` is removed. Returns the removals in order.
///
/// Candidates come from a 4-D bucket index with cell `2 · 1e-6 · maxsize`: if `equal(i, j)` then
/// every coordinate differs by at most `1e-6 · maxsize = cell / 2`, so the floor quotients differ
/// by at most one and `key(i)` lies in the 3⁴ neighbourhood of `key(j)` — no pair is missed.
/// Saturating casts or one giant box only add candidates; `equal` still decides.
pub fn duplicate_scan(boxes: &[Rect], is_dup: &mut dyn FnMut(usize, usize) -> bool) -> Vec<usize> {
    let usable = |r: &Rect| finite(r) && r.width() != 0.0 && r.height() != 0.0;
    let size = |r: &Rect| r.width().max(r.height());
    let equal = |i: usize, j: usize| -> bool {
        let (a, b) = (&boxes[i], &boxes[j]);
        if a.width() == 0.0 || a.height() == 0.0 || b.width() == 0.0 || b.height() == 0.0 {
            return false;
        }
        let tol = 1e-6 * size(a).max(size(b));
        (a.x0 - b.x0).abs() <= tol
            && (a.y0 - b.y0).abs() <= tol
            && (a.x1 - b.x1).abs() <= tol
            && (a.y1 - b.y1).abs() <= tol
    };
    let maxsize = boxes
        .iter()
        .filter(|r| usable(r))
        .map(size)
        .fold(0.0_f64, f64::max);
    let mut removed = Vec::new();
    if !(maxsize > 0.0) {
        return removed;
    }
    let cell = 2.0 * 1e-6 * maxsize;
    let key = |r: &Rect| -> [i64; 4] {
        [
            (r.x0 / cell).floor() as i64,
            (r.y0 / cell).floor() as i64,
            (r.x1 / cell).floor() as i64,
            (r.y1 / cell).floor() as i64,
        ]
    };
    let mut buckets: HashMap<[i64; 4], Vec<u32>> = HashMap::new();
    for (i, r) in boxes.iter().enumerate() {
        if usable(r) {
            buckets.entry(key(r)).or_default().push(i as u32);
        }
    }
    let mut gone = vec![false; boxes.len()];
    let mut cands: Vec<u32> = Vec::new();
    for jj in (0..boxes.len()).rev() {
        if !usable(&boxes[jj]) {
            continue;
        }
        let k = key(&boxes[jj]);
        cands.clear();
        for d0 in -1..=1 {
            for d1 in -1..=1 {
                for d2 in -1..=1 {
                    for d3 in -1..=1 {
                        let nk = [
                            k[0].saturating_add(d0),
                            k[1].saturating_add(d1),
                            k[2].saturating_add(d2),
                            k[3].saturating_add(d3),
                        ];
                        if let Some(v) = buckets.get(&nk) {
                            cands.extend(v.iter().copied().filter(|&i| (i as usize) < jj));
                        }
                    }
                }
            }
        }
        cands.sort_unstable();
        cands.dedup();
        for &ii in &cands {
            let ii = ii as usize;
            if gone[ii] || !equal(ii, jj) {
                continue;
            }
            if is_dup(ii, jj) {
                gone[ii] = true;
                removed.push(ii);
            }
        }
    }
    removed
}

/// The Flattener's white-rectangle pass (F:499–509) as a pure index walk: the positions `ii` with
/// `white[ii]` that no EARLIER, still-alive box strictly intersects (`geom::intersects`); a
/// returned position is dead for the positions after it. Returns the deletions in ascending order.
pub fn background_scan(boxes: &[Rect], white: &[bool]) -> Vec<usize> {
    let extent = boxes
        .iter()
        .filter(|r| finite(r))
        .copied()
        .reduce(|a, b| a.union(b));
    let mut grid = BoxGrid::new(extent, boxes.len());
    let mut deleted = Vec::new();
    for ii in 0..boxes.len() {
        if white[ii] {
            let wb = boxes[ii];
            let mut behind = false;
            grid.query(wb, &mut |k| {
                if intersects(boxes[k as usize], wb) {
                    behind = true;
                    false
                } else {
                    true
                }
            });
            if !behind {
                deleted.push(ii);
                continue; // never inserted: it is gone for every later position
            }
        }
        grid.insert(ii as u32, boxes[ii]);
    }
    deleted
}
```

Add `pub mod grid;` after `pub mod path;` in `src/geom/mod.rs`.

- [ ] **Step 4: Run the sweep tests**

Run: `cargo test --test flattener_sweeps`
Expected: PASS (all six).

- [ ] **Step 5: Use the scans in the Flattener**

`remove_duplicates` keeps its filter/box preparation and `is_dup` body; only the loop changes (`delete_up` stays inside the callback, so nothing is deferred):

```rust
pub fn remove_duplicates(
    doc: &mut Doc,
    ctx: &mut Ctx,
    ngs2: &mut Vec<NodeId>,
    bbs: &HashMap<NodeId, Rect>,
) -> usize {
    let inside = shape_inside_targets(doc);
    let els: Vec<NodeId> = ngs2
        .iter()
        .copied()
        .filter(|&n| {
            RECT_TAGS.contains(&doc.tag(n)) && bbs.contains_key(&n) && !inside.contains(&n)
        })
        .collect();
    let boxes: Vec<Rect> = els.iter().map(|n| bbs[n]).collect();
    let size = |r: &Rect| r.width().max(r.height());
    let mut sfs: Vec<Option<crate::ops::style::StrokeFill>> = vec![None; els.len()];
    let mut paths: Vec<Option<kurbo::BezPath>> = vec![None; els.len()];
    let removed = crate::geom::grid::duplicate_scan(&boxes, &mut |ii, jj| {
        if sfs[jj].is_none() {
            sfs[jj] = Some(strokefill(doc, els[jj]));
        }
        if sfs[ii].is_none() {
            sfs[ii] = Some(strokefill(doc, els[ii]));
        }
        let (my, oth) = (sfs[jj].as_ref().unwrap(), sfs[ii].as_ref().unwrap());
        if my.stroke_is_url || my.fill_is_url || (my.stroke.is_none() && my.fill.is_none()) {
            return false;
        }
        if let Some(s) = &my.stroke {
            if s.alpha != 1.0 || !oth.stroke.as_ref().is_some_and(|o| same_rgba(s, o)) {
                return false;
            }
        }
        if let Some(f) = &my.fill {
            if f.alpha != 1.0 || !oth.fill.as_ref().is_some_and(|o| same_rgba(f, o)) {
                return false;
            }
        }
        if !style_eq(&doc.specified_style(els[jj]), &doc.specified_style(els[ii])) {
            return false;
        }
        if paths[jj].is_none() {
            if let Some(pp) = shape_path(doc, els[jj]) {
                paths[jj] = Some(doc.composed_transform(els[jj]) * pp.path);
            }
        }
        if paths[ii].is_none() {
            if let Some(pp) = shape_path(doc, els[ii]) {
                paths[ii] = Some(doc.composed_transform(els[ii]) * pp.path);
            }
        }
        let (Some(gj), Some(gi)) = (&paths[jj], &paths[ii]) else {
            return false;
        };
        let tol = 1e-6 * size(&boxes[ii]).max(size(&boxes[jj]));
        if !(path_eq(gj, gi, tol) || path_eq(gj, &reverse(gi), tol)) {
            return false;
        }
        delete_up(doc, ctx, els[ii]);
        true
    });
    let gone: HashSet<NodeId> = removed.iter().map(|&i| els[i]).collect();
    ngs2.retain(|n| !gone.contains(n));
    removed.len()
}

pub fn remove_white_rects(
    doc: &mut Doc,
    ctx: &mut Ctx,
    ngs2: &[NodeId],
    bbs: &HashMap<NodeId, Rect>,
    wrects: &[NodeId],
) -> usize {
    let ngs3: Vec<NodeId> = ngs2
        .iter()
        .copied()
        .filter(|n| doc.parent(*n).is_some() && bbs.contains_key(n))
        .collect();
    let white: HashSet<NodeId> = wrects.iter().copied().collect();
    let boxes: Vec<Rect> = ngs3.iter().map(|n| bbs[n]).collect();
    let flags: Vec<bool> = ngs3.iter().map(|n| white.contains(n)).collect();
    // the test reads only `bbs`, never the document, so deleting after the scan is equivalent
    let deleted = crate::geom::grid::background_scan(&boxes, &flags);
    for &ii in &deleted {
        delete_up(doc, ctx, ngs3[ii]);
    }
    deleted.len()
}
```

(The old `equal` closure and the `removed: HashSet<usize>` in `remove_duplicates` go away; `size` stays for the path tolerance.)

- [ ] **Step 6: Run everything**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS; `tests/flattener.rs` duplicate/white-rectangle tests and `tests/invariance.rs::duplicate_removal_is_visually_invariant_on_text_tests` unchanged.

- [ ] **Step 7: Measure**

Task 2 command, `--id=layer1`: `phase=dedup` + `phase=whiterects` ≤ 300 ms combined (baseline ≈ 1 700 ms). Record in the commit message.

- [ ] **Step 8: Commit**

```bash
git add src/geom/grid.rs src/geom/mod.rs src/tools/flattener.rs tests/flattener_sweeps.rs
git commit -m "perf(flattener): bucket and grid sweeps replace the all-pairs box tests

geom::grid::duplicate_scan enumerates equal-box candidates from a 4-D bucket
index (cell = 2e-6 * maxsize, so the tolerance window is one neighbouring
bucket) and background_scan tests white rectangles against an incremental
BoxGrid of the earlier surviving boxes. Predicates and visiting order are
unchanged; property tests compare both against the verbatim pairwise code.
dedup + whiterects, whole layer: <before> -> <after> ms.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 10: `external_merges` candidate filter — only if Task 16's measurement needs it (not executed: text phase was 223.3 ms)

**Files:**
- Modify: `src/text/kerning.rs:318-381` (`external_merges`, the `for (j, w2) in chks.iter().enumerate()` loop)
- Test: `tests/text_kerning.rs` (append)

**Interfaces:** none. Merge results are byte-identical: candidates are visited in ascending `j`, which is today's order, and `perform_merges` breaks ties with a strict `<` (first candidate wins), so the order matters and is preserved.

Skip this task when, after Task 9, the Task 2 command for `--id=layer1` shows `phase=text ≤ 1 100 ms`. The controller decides at Task 16 and records the decision in Appendix B; if skipped, leave this section in the plan marked `(not executed: text phase was <n> ms)`.

- [ ] **Step 1: Write the failing test (a golden captured before the change)**

Append to `tests/text_kerning.rs`:

```rust
#[test]
fn external_merges_finds_the_same_candidates_with_and_without_the_grid() {
    use sciink::text::kerning::{remove_kerning, KerningOptions};
    // 400 short texts on a grid plus adjacent fragments that must merge ("Ver" + "tical" etc.)
    let mut body = String::new();
    for i in 0..400 {
        let (x, y) = ((i % 20) * 30, (i / 20) * 12 + 6);
        body.push_str(&format!(r#"<text id="g{i}" x="{x}" y="{y}" style="{DV};font-size:4px">n{i}</text>"#));
    }
    for k in 0..10 {
        let y = 300 + k * 12;
        body.push_str(&format!(
            r#"<text id="l{k}" x="0" y="{y}" style="{DV};font-size:5px">Ver</text><text id="r{k}" x="8.7" y="{y}" style="{DV};font-size:5px">tical{k}</text>"#
        ));
    }
    let svg = format!(r#"<svg {NS} width="700" height="500">{body}</svg>"#);
    let before = support::text_positions(&svg);
    let mut d = Doc::parse(svg.as_bytes()).unwrap();
    let els: Vec<_> = d.descendants(d.svg()).filter(|&n| d.is_element(n) && d.tag(n) == "text").collect();
    let mut w = Warnings::default();
    remove_kerning(&mut d, &els, &KerningOptions::from_inx(true, true, true, true, 4), fonts(), &mut w);
    let mut out = Vec::new();
    d.write(&mut out);
    let out = String::from_utf8(out).unwrap();
    let merged = roxmltree::Document::parse(&out).unwrap();
    let texts = merged.descendants().filter(|n| n.has_tag_name("text")).count();
    assert_eq!(texts, 410, "each Ver+tical pair merged into one element, the grid untouched");
    support::assert_same_positions(&before, &support::text_positions(&out), 1e-6, "external merges");
    // GOLDEN: run this test once BEFORE changing external_merges, paste `out` into
    // tests/data/edge/external_merges_golden.svg, then enable the comparison below.
    // assert_eq!(out, include_str!("data/edge/external_merges_golden.svg"));
}
```

Run it before the code change; write `out` to `tests/data/edge/external_merges_golden.svg` (the test can do so under `if std::env::var_os("SCIINK_WRITE_GOLDEN").is_some()`); uncomment the `assert_eq!`; the test now pins the exact output.

- [ ] **Step 2: Implement**

In `external_merges`, replace the inner `for (j, w2) in chks.iter().enumerate()` with a grid query. Before the outer loop:

```rust
    let extent = chks.iter().map(|c| c.bb).reduce(|a, b| a.union(b));
    let mut grid = crate::geom::grid::BoxGrid::new(extent, chks.len());
    for (i, c) in chks.iter().enumerate() {
        grid.insert(i as u32, c.bb);
    }
    let mut js: Vec<u32> = Vec::new();
```

and at the top of each `i` iteration:

```rust
        js.clear();
        grid.query(w.bb_big, &mut |j| {
            js.push(j);
            true
        });
        js.sort_unstable();
        js.dedup();
        for &j in &js {
            let j = j as usize;
            let w2 = &chks[j];
            if i == j || (w.angle - w2.angle).abs() >= 0.001 || !intersects(w.bb_big, w2.bb) {
                continue;
            }
            …the existing body, unchanged…
        }
```

- [ ] **Step 3: Run the tests, measure, commit**

Run: `cargo test` — PASS, golden byte-equal. Task 2 command `--id=layer1`: `phase=text` before/after in the commit message.

```bash
git add src/text/kerning.rs tests/text_kerning.rs tests/data/edge/external_merges_golden.svg
git commit -m "perf(text): grid-filtered external merge candidates

Same candidate set and order (ascending j) as the all-pairs loop; the golden
output and the appearance oracle are unchanged.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 11: Selection helpers

**Files:**
- Modify: `src/dom.rs:868-889` (`selection`, `selection_ordered`)
- Test: `tests/dom.rs` (append)

**Interfaces:** unchanged signatures; `selection_ordered` is O(s), `selection` returns without walking for a single id.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn selection_ordered_keeps_inkscape_order_and_drops_repeats() {
    let mut body = String::new();
    for i in 0..5000 {
        body.push_str(&format!("<rect id=\"r{i}\"/>"));
    }
    let doc = Doc::parse(format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{body}</svg>").as_bytes()).unwrap();
    let mut ids: Vec<String> = (0..5000).rev().map(|i| format!("r{i}")).collect();
    ids.push("r4999".to_string()); // repeat
    ids.push("missing".to_string());
    let t0 = std::time::Instant::now();
    let sel = doc.selection_ordered(&ids);
    assert!(t0.elapsed().as_secs() < 5, "quadratic selection_ordered");
    assert_eq!(sel.len(), 5000);
    assert_eq!(sel[0], doc.by_id("r4999").unwrap());
    assert_eq!(sel[4999], doc.by_id("r0").unwrap());
}

#[test]
fn selection_of_one_id_matches_the_general_path() {
    let doc = Doc::parse(MOVE_DOC.as_bytes()).unwrap();
    let one = doc.selection(&["c".to_string()]);
    let two = doc.selection(&["d".to_string(), "c".to_string()]);
    assert_eq!(one, vec![doc.by_id("c").unwrap()]);
    assert_eq!(two, vec![doc.by_id("c").unwrap(), doc.by_id("d").unwrap()], "document order");
    assert!(doc.selection(&["nope".to_string()]).is_empty());
}
```

- [ ] **Step 2: Run them (guards; both pass today), then implement**

```rust
    /// Nodes for the given ids in document order; unknown ids are dropped. One pre-order walk of
    /// the document (1–2 ms for 60 000 nodes); a rank index maintained across every attach and
    /// detach would cost more than it saves — deliberately not done (Plan 9).
    pub fn selection(&self, ids: &[String]) -> Vec<NodeId> {
        if let [one] = ids {
            return self.by_id(one).into_iter().collect();
        }
        let wanted: std::collections::HashSet<NodeId> =
            ids.iter().filter_map(|i| self.by_id(i)).collect();
        self.descendants(self.svg)
            .filter(|n| wanted.contains(n))
            .collect()
    }

    /// Nodes for the given ids in the order given (Inkscape's selection order), each once;
    /// unknown ids are dropped. The Scaler's match target is the FIRST selected object.
    pub fn selection_ordered(&self, ids: &[String]) -> Vec<NodeId> {
        let mut seen: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        let mut out: Vec<NodeId> = Vec::new();
        for id in ids {
            if let Some(n) = self.by_id(id) {
                if seen.insert(n) {
                    out.push(n);
                }
            }
        }
        out
    }
```

- [ ] **Step 3: Run the tests and commit**

Run: `cargo test` — PASS.

```bash
git add src/dom.rs tests/dom.rs
git commit -m "perf(dom): O(s) selection_ordered and a one-id selection fast path

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 12: Persistent font-scan cache

**Files:**
- Create: `src/text/fontcache.rs`
- Modify: `src/text/mod.rs` (`pub mod fontcache;`), `src/paths.rs`, `src/text/fonts.rs` (`FaceInfo`, `ScanKey`, `scanned`, `scan_entries`, new `scan_fresh`/`from_cached`/`scan_with_cache`, counters), `src/tools/favorite_markers.rs:215-227` (`store_path` onto `paths::data_dir`), `tests/support/mod.rs` (`SCIINK_NO_FONT_CACHE=1`), `docs/DEVELOPING.md`
- Test: `tests/font_cache.rs` (new)

**Interfaces:**
- Produces: `paths::data_dir() -> Option<PathBuf>` (`$INKSCAPE_PROFILE_DIR/sciink`, not created), `paths::cache_dir() -> Option<PathBuf>` (data dir, else `<temp>/sciink-<user>`, created, 0700 on unix); `fontcache::FONT_CACHE_FORMAT: u32 = 1`, `fontcache::CachedFace { info: FaceInfo, families: Vec<String>, post_script_name: String, monospaced: bool }`, `fontcache::cache_path() -> Option<PathBuf>`, `fontcache::read(path: &Path, key: &ScanKey) -> Option<Vec<CachedFace>>`, `fontcache::write(path: &Path, key: &ScanKey, faces: &[CachedFace])`; `fonts::ScanKey { pub system: bool, pub dirs: Vec<PathBuf>, pub bundled: Option<PathBuf> }` (Clone, PartialEq, Eq, Hash, Debug), `FaceInfo` gains `pub bundled: bool` and derives `PartialEq`, `FontSystem::scan_with_cache(key: &ScanKey, cache: Option<&Path>) -> FontSystem` (bypasses the process memo; the tests' entry point), `fonts::face_open_count() -> usize`, `fonts::cache_events() -> (usize, usize, usize)` = (hits, misses, writes). `FontSystem::from_dirs` never touches the disk cache.
- Task 13 fills `ScanKey.bundled` and `FaceInfo.bundled`; here they are `None`/`false`.

Why: `scan_entries` opens all 1 022 font files for metrics on every text-touching run (0.17–0.4 s warm, ~3 s after boot). fontdb 0.24's `Database::push_face_info(FaceInfo)` accepts face metadata without parsing the file (`FaceInfo { id: ID::dummy(), source: Source::File(path), … }`), and our seven metrics are pure functions of the file, so both can be cached on disk and validated by file size + mtime plus the mtime of every directory holding a cached file.

- [ ] **Step 1: `src/paths.rs`**

Append:

```rust
/// `$INKSCAPE_PROFILE_DIR/sciink` — Inkscape exports the variable for extensions. Not created here.
pub fn data_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("INKSCAPE_PROFILE_DIR")?).join("sciink"))
}

/// Where caches live: the data dir, else a per-user directory under the system temp dir. Created
/// on demand (0700 on unix); `None` when it cannot be created.
pub fn cache_dir() -> Option<PathBuf> {
    let dir = data_dir().unwrap_or_else(|| {
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "user".to_string());
        std::env::temp_dir().join(format!("sciink-{user}"))
    });
    std::fs::create_dir_all(&dir).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    Some(dir)
}
```

`favorite_markers::store_path` becomes (same paths as before):

```rust
    if let Some(dir) = crate::paths::data_dir() {
        return dir.join("favorite_markers.svg");
    }
    crate::paths::inx_dir().join("favorite_markers.svg")
```

- [ ] **Step 2: `src/text/fontcache.rs`**

```rust
//! Persistent font-scan cache (Plan 9). One TSV file holding fontdb's face metadata and our
//! metrics per face, validated against the size and mtime of every cached font file and the
//! mtime of every directory holding one. Any doubt → the caller rescans and rewrites. Std only.
//!
//! Lines (tab-separated; `\`, TAB and LF in values are escaped as `\\`, `\t`, `\n`):
//! `H sciink-fontcache <format> <crate version>` · `K <system 0|1> <bundled dir or ->` ·
//! `D <dir>` per `SCIINK_FONT_DIRS` entry · `S <file> <len> <mtime secs> <mtime nanos>` per font
//! file · `R <dir> <secs> <nanos>` per directory · `F <path> <index> <weight> <style 0|1|2>
//! <stretch 1..9> <mono 0|1> <post_script_name> <bundled 0|1> <upem> <asc> <desc> <ascmax>
//! <descmax> <xheight> <capheight> <family>…` per face, in scan order.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::fonts::{FaceInfo, FontStyle, ScanKey};

/// Bump when `face_metrics` or the stored fields change — not with the crate version, so a patch
/// release does not force a full rescan.
pub const FONT_CACHE_FORMAT: u32 = 1;

/// A face as fontdb needs it (`fontdb::FaceInfo` minus id and source) plus our `FaceInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedFace {
    pub info: FaceInfo,
    pub families: Vec<String>,
    pub post_script_name: String,
    pub monospaced: bool,
}

/// `SCIINK_NO_FONT_CACHE=1` → none; `SCIINK_FONT_CACHE=<path>` → that file; else
/// `<cache dir>/fontcache-<format>.tsv`.
pub fn cache_path() -> Option<PathBuf> {
    if std::env::var_os("SCIINK_NO_FONT_CACHE").is_some_and(|v| v == "1") {
        return None;
    }
    if let Some(p) = std::env::var_os("SCIINK_FONT_CACHE") {
        return Some(PathBuf::from(p));
    }
    crate::paths::cache_dir().map(|d| d.join(format!("fontcache-{FONT_CACHE_FORMAT}.tsv")))
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unesc(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next()? {
            '\\' => out.push('\\'),
            't' => out.push('\t'),
            'n' => out.push('\n'),
            _ => return None,
        }
    }
    Some(out)
}

fn path_field(p: &Path) -> String {
    esc(&p.to_string_lossy())
}

fn mtime(m: &fs::Metadata) -> (u64, u32) {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| (d.as_secs(), d.subsec_nanos()))
        .unwrap_or((0, 0))
}

fn style_code(s: FontStyle) -> u8 {
    match s {
        FontStyle::Normal => 0,
        FontStyle::Italic => 1,
        FontStyle::Oblique => 2,
    }
}

fn style_from(c: &str) -> Option<FontStyle> {
    match c {
        "0" => Some(FontStyle::Normal),
        "1" => Some(FontStyle::Italic),
        "2" => Some(FontStyle::Oblique),
        _ => None,
    }
}

fn parse_face(f: &[&str]) -> Option<CachedFace> {
    if f.len() < 17 {
        return None;
    }
    let num = |s: &str| s.parse::<f64>().ok().filter(|v| v.is_finite());
    let families: Vec<String> = f[16..].iter().map(|s| unesc(s)).collect::<Option<_>>()?;
    let info = FaceInfo {
        family: families.first()?.clone(),
        path: Some(PathBuf::from(unesc(f[1])?)),
        index: f[2].parse().ok()?,
        weight: f[3].parse().ok()?,
        style: style_from(f[4])?,
        width: f[5].parse().ok()?,
        upem: num(f[9])?,
        ascent: num(f[10])?,
        descent: num(f[11])?,
        ascent_max: num(f[12])?,
        descent_max: num(f[13])?,
        x_height: num(f[14])?,
        cap_height: num(f[15])?,
        bundled: f[8] == "1",
    };
    Some(CachedFace {
        info,
        families,
        post_script_name: unesc(f[7])?,
        monospaced: f[6] == "1",
    })
}

/// The cached faces when the file is valid for `key` and every stat still matches; `None` on any
/// mismatch, missing file, unparsable line or duplicate face.
pub fn read(path: &Path, key: &ScanKey) -> Option<Vec<CachedFace>> {
    let text = fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next()?.split('\t').collect();
    if head.len() < 3
        || head[0] != "H"
        || head[1] != "sciink-fontcache"
        || head[2].parse::<u32>().ok()? != FONT_CACHE_FORMAT
    {
        return None;
    }
    let mut key_seen = false;
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut faces: Vec<CachedFace> = Vec::new();
    let mut seen: HashSet<(PathBuf, u32)> = HashSet::new();
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        match *f.first()? {
            "K" => {
                if f.len() != 3 {
                    return None;
                }
                let system = f[1] == "1";
                let bundled = if f[2] == "-" {
                    None
                } else {
                    Some(PathBuf::from(unesc(f[2])?))
                };
                if system != key.system || bundled != key.bundled {
                    return None;
                }
                key_seen = true;
            }
            "D" => {
                if f.len() != 2 {
                    return None;
                }
                dirs.push(PathBuf::from(unesc(f[1])?));
            }
            "S" => {
                if f.len() != 5 {
                    return None;
                }
                let m = fs::metadata(PathBuf::from(unesc(f[1])?)).ok()?;
                let want = (f[3].parse::<u64>().ok()?, f[4].parse::<u32>().ok()?);
                if m.len() != f[2].parse::<u64>().ok()? || mtime(&m) != want {
                    return None;
                }
            }
            "R" => {
                if f.len() != 4 {
                    return None;
                }
                let m = fs::metadata(PathBuf::from(unesc(f[1])?)).ok()?;
                let want = (f[2].parse::<u64>().ok()?, f[3].parse::<u32>().ok()?);
                if mtime(&m) != want {
                    return None;
                }
            }
            "F" => {
                let face = parse_face(&f)?;
                if !seen.insert((face.info.path.clone()?, face.info.index)) {
                    return None;
                }
                faces.push(face);
            }
            _ => return None,
        }
    }
    if !key_seen || dirs != key.dirs {
        return None;
    }
    Some(faces)
}

/// Writes the cache atomically (`<path>.tmp-<pid>` then rename). Every failure is swallowed: a
/// cache must never fail a tool run. Faces without a file path (in-memory sources) disable the
/// write, because a partial cache would change which faces exist.
pub fn write(path: &Path, key: &ScanKey, faces: &[CachedFace]) {
    if faces.iter().any(|f| f.info.path.is_none()) {
        return;
    }
    let mut out = String::new();
    out.push_str(&format!(
        "H\tsciink-fontcache\t{FONT_CACHE_FORMAT}\t{}\n",
        env!("CARGO_PKG_VERSION")
    ));
    out.push_str(&format!(
        "K\t{}\t{}\n",
        key.system as u8,
        key.bundled.as_deref().map(path_field).unwrap_or_else(|| "-".to_string())
    ));
    for d in &key.dirs {
        out.push_str(&format!("D\t{}\n", path_field(d)));
    }
    let mut files: BTreeSet<PathBuf> = BTreeSet::new();
    let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
    for f in faces {
        if let Some(p) = &f.info.path {
            files.insert(p.clone());
            if let Some(d) = p.parent() {
                dirs.insert(d.to_path_buf());
            }
        }
    }
    dirs.extend(key.dirs.iter().cloned());
    dirs.extend(key.bundled.iter().cloned());
    for p in &files {
        let Ok(m) = fs::metadata(p) else { return };
        let (s, n) = mtime(&m);
        out.push_str(&format!("S\t{}\t{}\t{s}\t{n}\n", path_field(p), m.len()));
    }
    for d in &dirs {
        let Ok(m) = fs::metadata(d) else { continue };
        let (s, n) = mtime(&m);
        out.push_str(&format!("R\t{}\t{s}\t{n}\n", path_field(d)));
    }
    for f in faces {
        let Some(p) = &f.info.path else { return };
        let i = &f.info;
        out.push_str(&format!(
            "F\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:?}\t{:?}\t{:?}\t{:?}\t{:?}\t{:?}\t{:?}",
            path_field(p),
            i.index,
            i.weight,
            style_code(i.style),
            i.width,
            f.monospaced as u8,
            esc(&f.post_script_name),
            i.bundled as u8,
            i.upem,
            i.ascent,
            i.descent,
            i.ascent_max,
            i.descent_max,
            i.x_height,
            i.cap_height
        ));
        for fam in &f.families {
            out.push('\t');
            out.push_str(&esc(fam));
        }
        out.push('\n');
    }
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    if fs::write(&tmp, out).is_ok() && fs::rename(&tmp, path).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}
```

(`{:?}` on an `f64` prints the shortest string that parses back to the same value, so metrics survive the round trip bit for bit; `parse_face` rejects non-finite values.)

- [ ] **Step 3: Wire the cache into `src/text/fonts.rs`**

Add `pub bundled: bool` to `FaceInfo` (set `bundled: false` in `scan_entries` for now) and `#[derive(Debug, Clone, PartialEq)]` on it. Replace the `ScanKey` type alias and the statics:

```rust
/// What a scan covers: the system fonts (unless `SCIINK_NO_SYSTEM_FONTS=1`), the `SCIINK_FONT_DIRS`
/// directories in order, and the bundled font directory (Task 13; `None` until then).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScanKey {
    pub system: bool,
    pub dirs: Vec<PathBuf>,
    pub bundled: Option<PathBuf>,
}

type Scan = (fontdb::Database, Vec<(fontdb::ID, FaceInfo)>);
static SCANS: OnceLock<Mutex<HashMap<ScanKey, Arc<Scan>>>> = OnceLock::new();
static SCAN_COUNT: AtomicUsize = AtomicUsize::new(0);
static FACE_OPENS: AtomicUsize = AtomicUsize::new(0);
static CACHE_HITS: AtomicUsize = AtomicUsize::new(0);
static CACHE_MISSES: AtomicUsize = AtomicUsize::new(0);
static CACHE_WRITES: AtomicUsize = AtomicUsize::new(0);

/// Font files opened for metrics in this process (tests).
pub fn face_open_count() -> usize {
    FACE_OPENS.load(Ordering::SeqCst)
}

/// `(cache hits, cache misses, cache writes)` in this process (tests).
pub fn cache_events() -> (usize, usize, usize) {
    (
        CACHE_HITS.load(Ordering::SeqCst),
        CACHE_MISSES.load(Ordering::SeqCst),
        CACHE_WRITES.load(Ordering::SeqCst),
    )
}

fn scan_key() -> ScanKey {
    let system = std::env::var_os("SCIINK_NO_SYSTEM_FONTS").is_none_or(|v| v != "1");
    let dirs = std::env::var_os("SCIINK_FONT_DIRS")
        .map(|d| std::env::split_paths(&d).collect())
        .unwrap_or_default();
    ScanKey {
        system,
        dirs,
        bundled: None,
    }
}

/// The filesystem scan itself: fontdb enumerates the faces, `scan_entries` opens each for metrics.
fn scan_fresh(key: &ScanKey) -> Scan {
    SCAN_COUNT.fetch_add(1, Ordering::SeqCst);
    let mut db = fontdb::Database::new();
    if key.system {
        db.load_system_fonts();
    }
    for d in &key.dirs {
        db.load_fonts_dir(d);
    }
    if let Some(b) = &key.bundled {
        db.load_fonts_dir(b);
    }
    let entries = FontSystem::scan_entries(&db, key.bundled.as_deref());
    (db, entries)
}

/// A scan rebuilt from cached faces: fontdb takes the metadata as given (`push_face_info`) and our
/// metrics come from the cache, so no font file is opened.
fn from_cached(faces: Vec<super::fontcache::CachedFace>) -> Scan {
    let mut db = fontdb::Database::new();
    let mut entries = Vec::with_capacity(faces.len());
    for f in faces {
        let Some(path) = f.info.path.clone() else { continue };
        let id = db.push_face_info(fontdb::FaceInfo {
            id: fontdb::ID::dummy(),
            source: fontdb::Source::File(path),
            index: f.info.index,
            families: f
                .families
                .into_iter()
                .map(|n| (n, fontdb::Language::English_UnitedStates))
                .collect(),
            post_script_name: f.post_script_name,
            style: match f.info.style {
                FontStyle::Normal => fontdb::Style::Normal,
                FontStyle::Italic => fontdb::Style::Italic,
                FontStyle::Oblique => fontdb::Style::Oblique,
            },
            weight: fontdb::Weight(f.info.weight),
            stretch: stretch_from_number(f.info.width),
            monospaced: f.monospaced,
        });
        entries.push((id, f.info));
    }
    (db, entries)
}

fn stretch_from_number(n: u16) -> fontdb::Stretch {
    use fontdb::Stretch as S;
    match n {
        1 => S::UltraCondensed,
        2 => S::ExtraCondensed,
        3 => S::Condensed,
        4 => S::SemiCondensed,
        6 => S::SemiExpanded,
        7 => S::Expanded,
        8 => S::ExtraExpanded,
        9 => S::UltraExpanded,
        _ => S::Normal,
    }
}

/// The faces of a fresh scan in cache form; `None` when a face has no file path.
fn to_cached(scan: &Scan) -> Option<Vec<super::fontcache::CachedFace>> {
    scan.1
        .iter()
        .map(|(id, info)| {
            let f = scan.0.face(*id)?;
            info.path.as_ref()?;
            Some(super::fontcache::CachedFace {
                info: info.clone(),
                families: f.families.iter().map(|(n, _)| n.clone()).collect(),
                post_script_name: f.post_script_name.clone(),
                monospaced: f.monospaced,
            })
        })
        .collect()
}

/// Read the cache for `key` when there is one, else scan and (try to) write it.
fn scan_with_cache_raw(key: &ScanKey, cache: Option<&std::path::Path>) -> Scan {
    if let Some(p) = cache {
        if let Some(faces) = super::fontcache::read(p, key) {
            CACHE_HITS.fetch_add(1, Ordering::SeqCst);
            return from_cached(faces);
        }
        CACHE_MISSES.fetch_add(1, Ordering::SeqCst);
    }
    let scan = scan_fresh(key);
    if let Some(p) = cache {
        if let Some(faces) = to_cached(&scan) {
            super::fontcache::write(p, key, &faces);
            CACHE_WRITES.fetch_add(1, Ordering::SeqCst);
        }
    }
    scan
}

/// The scan for the current environment, from the process memo, the disk cache, or fresh.
fn scanned() -> Arc<Scan> {
    let key = scan_key();
    let cache = SCANS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = guard.get(&key) {
        return s.clone();
    }
    let path = super::fontcache::cache_path();
    let scan = Arc::new(scan_with_cache_raw(&key, path.as_deref()));
    guard.insert(key, scan.clone());
    scan
}
```

In `impl FontSystem`: `load()` is unchanged (it calls `scanned()`); `from_dirs` calls `Self::scan_entries(&db, None)`; add

```rust
    /// Scan `key` through the disk cache at `cache` (read if valid, else scan and write). Bypasses
    /// the process-wide memo — the entry point the font-cache tests use.
    pub fn scan_with_cache(key: &ScanKey, cache: Option<&std::path::Path>) -> FontSystem {
        let t0 = Instant::now();
        let (db, entries) = scan_with_cache_raw(key, cache);
        Self::from_entries(db, entries, t0)
    }
```

`scan_entries` gets a second parameter `bundled: Option<&std::path::Path>`, counts opens, and fills the flag:

```rust
    fn scan_entries(
        db: &fontdb::Database,
        bundled: Option<&std::path::Path>,
    ) -> Vec<(fontdb::ID, FaceInfo)> {
        …existing loop; inside it, before `db.with_face_data`:
            FACE_OPENS.fetch_add(1, Ordering::SeqCst);
        …and in the `FaceInfo` literal:
                    bundled: matches!((&path, bundled), (Some(p), Some(b)) if p.starts_with(b)),
        …the sort key becomes (Task 13 relies on this position):
            let ka = (&a.1.family, a.1.weight, a.1.style as u8, a.1.width, a.1.bundled as u8, &a.1.path, a.1.index);
            let kb = (&b.1.family, b.1.weight, b.1.style as u8, b.1.width, b.1.bundled as u8, &b.1.path, b.1.index);
    }
```

The `fonts` log line from Task 1 gains `cached=`: `crate::log::line(&format!("phase=fonts ms={:.1} faces={} scans={} cache_hits={}", fs.load_ms, fs.infos.len(), scan_count(), CACHE_HITS.load(Ordering::SeqCst)))`.

`src/text/mod.rs`: add `pub mod fontcache;`. `tests/support/mod.rs`, inside the `Once` in `with_vendored_fonts`: `std::env::set_var("SCIINK_NO_FONT_CACHE", "1");` so no ordinary test ever touches a cache file (`from_dirs` never does anyway).

- [ ] **Step 4: Write the tests**

Create `tests/font_cache.rs`. The counters are process-global, so every test holds one lock to serialise the binary's tests:

```rust
//! The on-disk font-scan cache: written cold, read warm with no font file opened, invalidated by
//! any change to the font files or their directories, and never able to fail a scan.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use sciink::text::fontcache::{cache_path, read, FONT_CACHE_FORMAT};
use sciink::text::fonts::{cache_events, face_open_count, FontSystem, ScanKey};

static SERIAL: Mutex<()> = Mutex::new(());

fn vendored() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")
}

/// A fresh directory with copies of the vendored fonts (so tests may touch, add and remove files),
/// and a cache file path inside a sibling directory.
fn sandbox(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("sciink-fc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let fonts = root.join("fonts");
    std::fs::create_dir_all(&fonts).unwrap();
    for e in std::fs::read_dir(vendored()).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "ttf") {
            std::fs::copy(e.path(), fonts.join(e.file_name())).unwrap();
        }
    }
    (fonts, root.join("cache").join("fontcache-test.tsv"))
}

fn key(fonts: &Path) -> ScanKey {
    ScanKey {
        system: false,
        dirs: vec![fonts.to_path_buf()],
        bundled: None,
    }
}

fn touch(p: &Path) {
    let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
    f.set_modified(SystemTime::now() + Duration::from_secs(5)).unwrap();
}

#[test]
fn a_cold_scan_writes_the_cache_and_a_warm_scan_opens_no_font_file() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, cache) = sandbox("cold-warm");
    let (h0, m0, w0) = cache_events();
    let fresh = FontSystem::scan_with_cache(&key(&fonts), Some(&cache));
    assert_eq!(fresh.face_count(), 4);
    assert_eq!(cache_events(), (h0, m0 + 1, w0 + 1), "cold: one miss, one write");
    assert!(cache.is_file());
    let opens = face_open_count();
    let warm = FontSystem::scan_with_cache(&key(&fonts), Some(&cache));
    assert_eq!(cache_events(), (h0 + 1, m0 + 1, w0 + 1), "warm: one hit, no write");
    assert_eq!(face_open_count(), opens, "the warm scan opened no font file");
    assert_eq!(warm.face_count(), 4);
}

#[test]
fn metrics_from_the_cache_equal_metrics_from_a_fresh_scan() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, cache) = sandbox("metrics");
    let fresh = FontSystem::scan_with_cache(&key(&fonts), None);
    FontSystem::scan_with_cache(&key(&fonts), Some(&cache)); // writes
    let mut cached = FontSystem::scan_with_cache(&key(&fonts), Some(&cache)); // reads
    assert_eq!(fresh.face_count(), cached.face_count());
    for k in fresh.faces() {
        let (a, b) = (fresh.face_info(k), cached.face_info(k));
        assert_eq!(a, b, "face {k:?} differs between fresh and cached scans");
        for (x, y) in [
            (a.upem, b.upem),
            (a.ascent, b.ascent),
            (a.descent, b.descent),
            (a.ascent_max, b.ascent_max),
            (a.descent_max, b.descent_max),
            (a.x_height, b.x_height),
            (a.cap_height, b.cap_height),
        ] {
            assert_eq!(x.to_bits(), y.to_bits(), "metric not bit-identical for {k:?}");
        }
    }
    assert_eq!(fresh.family_faces("dejavu sans"), cached.family_faces("dejavu sans"));
    // the cached system can still measure: resolving a family finds a face
    let spec = sciink::text::fonts::FontSpec::from_style(&sciink::style::Style::parse("font-family:'DejaVu Sans'"));
    assert!(cached.resolve(&spec).is_some());
}

#[test]
fn touching_adding_or_removing_a_font_file_invalidates_the_cache() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, cache) = sandbox("invalidate");
    let k = key(&fonts);
    FontSystem::scan_with_cache(&k, Some(&cache));
    let (h, m, w) = cache_events();
    touch(&fonts.join("DejaVuSans.ttf"));
    FontSystem::scan_with_cache(&k, Some(&cache));
    assert_eq!(cache_events(), (h, m + 1, w + 1), "a touched file forces a rescan and rewrite");
    std::fs::copy(vendored().join("Roboto-Bold.ttf"), fonts.join("Extra.ttf")).unwrap();
    let fs = FontSystem::scan_with_cache(&k, Some(&cache));
    assert_eq!(cache_events(), (h, m + 2, w + 2), "a new file changes the directory mtime");
    assert_eq!(fs.face_count(), 5);
    std::fs::remove_file(fonts.join("Extra.ttf")).unwrap();
    let fs = FontSystem::scan_with_cache(&k, Some(&cache));
    assert_eq!(cache_events(), (h, m + 3, w + 3), "a removed file is noticed");
    assert_eq!(fs.face_count(), 4);
    FontSystem::scan_with_cache(&k, Some(&cache));
    assert_eq!(cache_events(), (h + 1, m + 3, w + 3), "stable again: a hit");
}

#[test]
fn a_corrupt_cache_file_is_ignored_and_rewritten() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, cache) = sandbox("corrupt");
    let k = key(&fonts);
    FontSystem::scan_with_cache(&k, Some(&cache));
    let good = std::fs::read_to_string(&cache).unwrap();
    let corruptions: Vec<String> = vec![
        good[..good.len() / 2].to_string(),                                    // truncated mid-line
        good.replacen("\t2048", "\tnot-a-number", 1),                           // garbage number
        good.replacen(&format!("\t{FONT_CACHE_FORMAT}\t"), "\t999\t", 1),       // wrong format
        String::new(),                                                          // empty
        (0..1_000_000u32).map(|i| (b'!' + (i % 90) as u8) as char).collect(),  // 1 MB of noise
    ];
    for (i, c) in corruptions.iter().enumerate() {
        std::fs::write(&cache, c).unwrap();
        assert!(read(&cache, &k).is_none(), "corruption {i} accepted");
        let (h, m, w) = cache_events();
        let fs = FontSystem::scan_with_cache(&k, Some(&cache));
        assert_eq!(fs.face_count(), 4, "corruption {i}");
        assert_eq!(cache_events(), (h, m + 1, w + 1), "corruption {i}: rescanned and rewritten");
        assert!(read(&cache, &k).is_some(), "corruption {i}: rewritten file is valid");
    }
}

#[test]
fn a_different_scan_key_does_not_reuse_the_cache() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, cache) = sandbox("key");
    FontSystem::scan_with_cache(&key(&fonts), Some(&cache));
    let other = ScanKey {
        system: false,
        dirs: vec![fonts.clone(), fonts.parent().unwrap().to_path_buf()],
        bundled: None,
    };
    let (h, m, w) = cache_events();
    FontSystem::scan_with_cache(&other, Some(&cache));
    assert_eq!(cache_events(), (h, m + 1, w + 1));
    assert!(read(&cache, &key(&fonts)).is_none(), "the file now belongs to the other key");
}

#[test]
fn an_unwritable_cache_location_does_not_fail_the_scan() {
    let _g = SERIAL.lock().unwrap();
    let (fonts, _) = sandbox("unwritable");
    let bad = PathBuf::from("/nonexistent-sciink-dir/deeper/fontcache.tsv");
    let fs = FontSystem::scan_with_cache(&key(&fonts), Some(&bad));
    assert_eq!(fs.face_count(), 4);
}

#[test]
fn the_cache_path_honours_the_environment_switches() {
    let _g = SERIAL.lock().unwrap();
    // SAFETY: tests in this binary are serialised by SERIAL and restore the variables.
    unsafe {
        std::env::set_var("SCIINK_NO_FONT_CACHE", "1");
        assert_eq!(cache_path(), None);
        std::env::remove_var("SCIINK_NO_FONT_CACHE");
        std::env::set_var("SCIINK_FONT_CACHE", "/tmp/x.tsv");
        assert_eq!(cache_path(), Some(PathBuf::from("/tmp/x.tsv")));
        std::env::remove_var("SCIINK_FONT_CACHE");
    }
    assert!(cache_path().is_some_and(|p| p.ends_with(format!("fontcache-{FONT_CACHE_FORMAT}.tsv"))));
}
```

(`File::set_modified` is stable since Rust 1.75; `FontSpec::from_style` and `Style::parse` exist already. If `FaceInfo` does not yet derive `PartialEq` when you get here, add it — Step 3 asks for it.)

- [ ] **Step 5: Run everything**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS; every pre-existing test unchanged (they go through `from_dirs` or `with_vendored_fonts`, which now sets `SCIINK_NO_FONT_CACHE=1`).

- [ ] **Step 6: Measure**

```bash
cargo build --release
rm -f "$HOME/Library/Application Support/org.inkscape.Inkscape/config/inkscape/sciink/fontcache-1.tsv" /tmp/sciink-$USER/fontcache-1.tsv
target/release/sciink --tool=about tests/data/edge/simple.svg 2>&1 >/dev/null | grep fonts:   # cold: full scan + write
target/release/sciink --tool=about tests/data/edge/simple.svg 2>&1 >/dev/null | grep fonts:   # warm: ≤ 30 ms
```

Record both `fonts:` lines in the commit message and add `SCIINK_NO_FONT_CACHE`, `SCIINK_FONT_CACHE` rows to the `docs/DEVELOPING.md` environment table plus the one-line rule under Conventions: *bump `FONT_CACHE_FORMAT` in `src/text/fontcache.rs` when `face_metrics` or the cached fields change.*

- [ ] **Step 7: Commit**

```bash
git add src/text/fontcache.rs src/text/fonts.rs src/text/mod.rs src/paths.rs src/tools/favorite_markers.rs tests/support/mod.rs tests/font_cache.rs docs/DEVELOPING.md
git commit -m "feat(fonts): persistent face-metric cache

fontdb metadata and our metrics per face are cached in
\$INKSCAPE_PROFILE_DIR/sciink/fontcache-1.tsv (temp-dir fallback), validated by
every cached file's size and mtime and every containing directory's mtime, and
loaded through push_face_info without opening a font file. Cold start on this
Mac: <before> ms -> <after> ms. Deviation from upstream (rescans every run).

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 13: Bundle DejaVu Sans (Book, Bold)

**Files:**
- Modify: `src/paths.rs` (`bundled_font_dir`), `src/text/fonts.rs` (`scan_key`, `is_bundled`), `src/tools/about.rs` (`bundled fonts:` line), `src/tools/font_probe.rs:28-42` (`describe_face`), `dist/package.sh`, `dist/test-package.sh`, `dist/dev-install.sh`, `README.md`, `dist/README-dist.txt`, `docs/DEVELOPING.md`, `tests/fonts/README.md`
- Test: `tests/text_fonts.rs` (append)

**Interfaces:**
- Produces: `paths::bundled_font_dir() -> Option<PathBuf>` (`<inx dir>/fonts` when it exists), `FontSystem::is_bundled(&self, k: FaceKey) -> bool`; `ScanKey.bundled` is set only when system fonts are scanned and `SCIINK_NO_BUNDLED_FONTS` is not `1`.
- Source of the shipped files: `tests/fonts/DejaVuSans.ttf`, `tests/fonts/DejaVuSans-Bold.ttf`, `tests/fonts/LICENSE-DejaVu.txt` — packaged from there, not moved (no second copy in the repo, no test churn). Roboto stays test-only.

Why: matplotlib's default font is DejaVu Sans, which is not installed on most macOS and Windows machines, so text there is measured with a substitute (Verdana on the user's Mac). Shipping the two faces (1.4 MB, Bitstream Vera + DejaVu licence) makes the fallback exact. An installed DejaVu Sans must still win: `scan_entries` sorts `bundled` after `width` and before `path` (Task 12), so a face of identical family/weight/style/width that is not bundled precedes the bundled copy and `pick` returns it.

- [ ] **Step 1: Write the failing tests**

Append to `tests/text_fonts.rs`:

```rust
use sciink::text::fonts::ScanKey;

fn bundled_copy(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sciink-bundled-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts/DejaVuSans.ttf");
    std::fs::copy(src, dir.join("DejaVuSans.ttf")).unwrap();
    dir
}

#[test]
fn an_installed_face_wins_a_tie_against_a_bundled_face_of_the_same_metadata() {
    let bundled = bundled_copy("tie");
    let mut fs = FontSystem::scan_with_cache(
        &ScanKey {
            system: false,
            dirs: vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")],
            bundled: Some(bundled.clone()),
        },
        None,
    );
    assert_eq!(fs.face_count(), 5, "4 vendored + 1 bundled copy of DejaVu Sans Book");
    let spec = sciink::text::fonts::FontSpec::from_style(&sciink::style::Style::parse(
        "font-family:'DejaVu Sans'",
    ));
    let k = fs.resolve(&spec).expect("DejaVu Sans resolves");
    assert!(!fs.is_bundled(k), "the non-bundled face wins the tie");
    assert!(fs.face_info(k).path.as_ref().unwrap().starts_with(env!("CARGO_MANIFEST_DIR")));
}

#[test]
fn is_bundled_reports_the_source_directory() {
    let bundled = bundled_copy("flag");
    let fs = FontSystem::scan_with_cache(
        &ScanKey {
            system: false,
            dirs: vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fonts")],
            bundled: Some(bundled.clone()),
        },
        None,
    );
    let flagged: Vec<_> = fs.faces().filter(|&k| fs.is_bundled(k)).collect();
    assert_eq!(flagged.len(), 1);
    assert!(fs.face_info(flagged[0]).path.as_ref().unwrap().starts_with(&bundled));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test text_fonts an_installed_face_wins is_bundled_reports`
Expected: compile error — `is_bundled` does not exist.

- [ ] **Step 3: Implement**

`src/paths.rs`:

```rust
/// `<inx dir>/fonts` when it exists — the release zips ship DejaVu Sans there.
pub fn bundled_font_dir() -> Option<PathBuf> {
    let d = inx_dir().join("fonts");
    d.is_dir().then_some(d)
}
```

`src/text/fonts.rs`, in `scan_key()`:

```rust
    let bundled = if system && std::env::var_os("SCIINK_NO_BUNDLED_FONTS").is_none_or(|v| v != "1") {
        crate::paths::bundled_font_dir()
    } else {
        None
    };
    ScanKey { system, dirs, bundled }
```

(`SCIINK_NO_SYSTEM_FONTS=1` therefore disables the bundle too, which keeps `with_vendored_fonts` deterministic without a new variable.) In `impl FontSystem`:

```rust
    /// True when the face comes from the bundled font directory shipped next to the `.inx` files.
    pub fn is_bundled(&self, k: FaceKey) -> bool {
        self.infos[k.0 as usize].bundled
    }
```

`src/tools/font_probe.rs`, `describe_face`: build the string as today, then `if fs.is_bundled(k) { s.push_str(" [bundled]"); }`.

`src/tools/about.rs`, after the `fonts:` line:

```rust
    match crate::paths::bundled_font_dir() {
        Some(d) => {
            let n = fs.faces().filter(|&k| fs.is_bundled(k)).count();
            let _ = writeln!(r, "bundled fonts: {} ({n} faces)", d.display());
        }
        None => {
            let _ = writeln!(r, "bundled fonts: not found");
        }
    }
```

`dist/package.sh`, after the `LICENSE` copy:

```bash
mkdir -p "$stage/sciink/fonts"
cp "$here/tests/fonts/DejaVuSans.ttf" "$here/tests/fonts/DejaVuSans-Bold.ttf" "$here/tests/fonts/LICENSE-DejaVu.txt" "$stage/sciink/fonts/"
```

`dist/test-package.sh`, before `echo "PACKAGE-OK $zip"`:

```bash
for f in DejaVuSans.ttf DejaVuSans-Bold.ttf LICENSE-DejaVu.txt; do
  test -f "$tmp/sciink/fonts/$f" || { echo "FAIL: fonts/$f missing"; exit 1; }
done
grep -q 'bundled fonts: .*(2 faces)' "$tmp/err.txt" || { echo "FAIL: about does not see the bundled fonts"; cat "$tmp/err.txt"; exit 1; }
```

`dist/dev-install.sh`, after the `.inx` loop:

```bash
mkdir -p "$EXT/fonts"
for f in DejaVuSans.ttf DejaVuSans-Bold.ttf LICENSE-DejaVu.txt; do ln -sfn "$PWD/tests/fonts/$f" "$EXT/fonts/$f"; done
```

Docs: `README.md` Fonts section gets the sentence *DejaVu Sans (Book and Bold) is bundled, so matplotlib's default font measures correctly even where it is not installed; an installed copy takes precedence.* and the License section *DejaVu Sans is redistributed under the Bitstream Vera and DejaVu licences (`fonts/LICENSE-DejaVu.txt` in the release).* `dist/README-dist.txt` gets the same licence line. `docs/DEVELOPING.md`: environment table row `SCIINK_NO_BUNDLED_FONTS=1` (skip the bundled directory), packaging section mentions `fonts/`, `tests/fonts/README.md` notes that the DejaVu files are also the shipped copies (`dist/package.sh`).

- [ ] **Step 4: Run the tests and the package check**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test && cargo build --release && dist/package.sh macos-universal target/release/sciink && dist/test-package.sh dist/out/sciink-macos-universal.zip`
Expected: all green; `PACKAGE-OK`.

- [ ] **Step 5: Commit**

```bash
git add src/paths.rs src/text/fonts.rs src/tools/about.rs src/tools/font_probe.rs dist/package.sh dist/test-package.sh dist/dev-install.sh README.md dist/README-dist.txt docs/DEVELOPING.md tests/fonts/README.md tests/text_fonts.rs
git commit -m "feat(fonts): bundle DejaVu Sans (Book, Bold) as the matplotlib fallback

The zips carry fonts/ next to the .inx files; the scan loads it after the
system fonts and an installed face of identical metadata sorts ahead of the
bundled copy. Diagnostics and Font Probe say when a bundled face is used.
Deviation from upstream, which bundles nothing.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 14: Small fixes — live preview off, runner pin, `<use>` bbox tests, docs

**Files:**
- Modify: `inx/combine_by_color.inx:15`, `inx/text_ghoster.inx:12`, `inx/text_fix.inx:16`, `inx/text_highlight.inx:14` (`needs-live-preview="true"` → `"false"`), `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `docs/DEVELOPING.md`, `README.md`, `CHANGELOG.md`
- Test: `tests/ops_bbox.rs` (append), `tests/cli.rs` or a new `tests/inx.rs` (append)

**Interfaces:** none.

- [ ] **Step 1: Write the failing tests**

Append to `tests/ops_bbox.rs` (helpers `doc`, `id`, `NS`, `rect_close`, `bbox`, `VISUAL` exist):

```rust
#[test]
fn a_use_inside_a_clip_path_measures_its_target_under_defs() {
    with_vendored_fonts(|| {
        let d = doc(&format!(
            r#"<svg {NS}><defs><rect id="t" x="2" y="2" width="4" height="4"/><clipPath id="c"><use href="#t"/></clipPath></defs><rect id="r" x="0" y="0" width="10" height="10" clip-path="url(#c)"/></svg>"#
        ));
        let mut ctx = Ctx::new();
        let b = bbox(&d, &mut ctx, id(&d, "r"), VISUAL).expect("clipped box");
        assert!(rect_close(b, 2.0, 2.0, 6.0, 6.0), "{b:?}");
    });
}

#[test]
fn a_use_whose_href_does_not_resolve_has_no_box() {
    with_vendored_fonts(|| {
        // dhelpers.py:1505-1521: a dangling clone contributes nothing, so a clipPath made of one
        // clips everything away (Other_tests.svg's image270 is such a case)
        let d = doc(&format!(
            r#"<svg {NS}><defs><clipPath id="c"><use href="#missing"/></clipPath></defs><rect id="r" x="0" y="0" width="10" height="10" clip-path="url(#c)"/><use id="u" href="#missing"/></svg>"#
        ));
        let mut ctx = Ctx::new();
        assert_eq!(bbox(&d, &mut ctx, id(&d, "u"), VISUAL), None);
        assert_eq!(bbox(&d, &mut ctx, id(&d, "r"), VISUAL), None);
    });
}
```

Add to `tests/cli.rs`:

```rust
#[test]
fn no_inx_file_enables_live_preview() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("inx");
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let text = std::fs::read_to_string(e.path()).unwrap();
        assert!(
            text.contains(r#"needs-live-preview="false""#) && !text.contains(r#"needs-live-preview="true""#),
            "{}: live preview must be off (upstream has it off everywhere; each preview re-runs the tool and reloads the document)",
            e.path().display()
        );
    }
}
```

- [ ] **Step 2: Run them**

Run: `cargo test --test ops_bbox a_use_inside a_use_whose --test cli no_inx_file_enables_live_preview`
Expected: the two bbox tests PASS already (they pin behaviour that works and the parity gap); the inx test FAILS on four files.

- [ ] **Step 3: Flip live preview and pin the runner**

In the four `.inx` files: `<effect needs-live-preview="false">`.

`.github/workflows/ci.yml`: `check.runs-on: ubuntu-24.04`; test matrix `os: [ubuntu-24.04, macos-latest, windows-latest]`. `.github/workflows/release.yml`: matrix entry `os: ubuntu-24.04`, **and** the musl build step's guard `if: matrix.os == 'ubuntu-24.04'` (leaving it at `ubuntu-latest` silently skips the Linux build; the release job's `test … -eq 3` would then fail late), **and** `release.runs-on: ubuntu-24.04`.

- [ ] **Step 4: Docs**

`docs/DEVELOPING.md`, Known gaps: delete the three bullets this plan fixes (whole-document character tables, the first-run font scan, DejaVu not bundled); replace the `ops::bbox` `<use>` bullet with:

> - A `<use>` whose `href` does not resolve has no bounding box, so a `clipPath` made of one clips its element away entirely (upstream behaves the same; `Other_tests.svg`'s `image270` is such a case, which is why the clip-region oracle counts 10 of 11 clipped elements).

Add a section `## Performance on large documents` to `docs/DEVELOPING.md`:

> Inkscape writes the whole document to a temporary file, runs the extension, reads the result back and re-renders it. On a 50 MB SVG that round trip takes seconds before and after our binary runs, and we do not control it; time Extensions ▸ Scientific ▸ Diagnostics on the file to see your own floor (Diagnostics itself does ~0.3 s of work). Two things keep it small: link raster images instead of embedding them (one manuscript we measured carried 27 MB of base64 in 93 `<image>` elements), and run the tools per figure rather than on a whole layer. Our own share is logged per phase with `SCIINK_LOG`; on a 62 000-element document the Flattener takes ≈ <Task 16 number> s on one figure and ≈ <Task 16 number> s on the whole layer.

and the short form in `README.md` under a `## Large documents` heading (three sentences: the round trip is Inkscape's, link rasters, work per figure). `CHANGELOG.md`: start `## 0.2.0 (unreleased)` with bullets for live preview off, the runner pin, the `<use>` gap correction; Task 16 completes it.

- [ ] **Step 5: Run the tests and commit**

Run: `cargo test` — PASS.

```bash
git add inx .github/workflows docs/DEVELOPING.md README.md CHANGELOG.md tests/ops_bbox.rs tests/cli.rs
git commit -m "fix(inx): live preview off everywhere; ci: pin ubuntu-24.04; docs: large documents

Upstream ships needs-live-preview=false on every tool; a preview re-runs the
tool and reloads the document on each keystroke, which on a 50 MB file is the
freeze this release fixes. The <use>-in-clipPath known gap was a dangling href
(upstream parity) and is now pinned by tests instead of listed as a bug.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 15: Synthetic big document, invariants, phase bench

**Files:**
- Modify: `tests/support/mod.rs` (append `BigDoc`)
- Test: `tests/big_document.rs` (new), `tests/bench.rs` (new)

**Interfaces:**
- Produces: `support::BigDoc { figures, groups_per_figure, shapes, texts, clones, style_rules, image_kb }` with `Default` and `fn svg(&self) -> String`. Deterministic (seeded LCG), compact XML (no indentation text nodes, so `strip_whitespace` leaves untouched figures byte-identical), needs no upstream data. Figure `i` is `<g id="fig{i}">` under `<g id="layer1">`; each figure starts with a white background `<rect id="bg{i}">`, has `shapes` paths (every fifth an exact duplicate of the previous one, so the duplicate pass has work), `texts` DejaVu Sans texts (every other one with a `dx` list for manual kerning), `clones` `<use href="#marker">`, and `groups_per_figure` levels of nested `<g transform="translate(…)" clip-path="url(#clip{i})">`; `style_rules` copies of `<style>*{stroke-linejoin: round; stroke-linecap: butt}</style>`; `image_kb` KB of base64-shaped text in one `<image href="data:image/png;base64,…">`.

- [ ] **Step 1: Add `BigDoc` to `tests/support/mod.rs`**

```rust
/// A deterministic many-figure document in the shape of a multi-panel matplotlib export.
pub struct BigDoc {
    pub figures: usize,
    pub groups_per_figure: usize,
    pub shapes: usize,
    pub texts: usize,
    pub clones: usize,
    pub style_rules: usize,
    pub image_kb: usize,
}

impl Default for BigDoc {
    fn default() -> Self {
        BigDoc {
            figures: 100,
            groups_per_figure: 3,
            shapes: 30,
            texts: 4,
            clones: 5,
            style_rules: 20,
            image_kb: 0,
        }
    }
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

impl BigDoc {
    /// The document as compact XML: no whitespace between elements.
    pub fn svg(&self) -> String {
        let mut rng = Lcg(0x5c11_9e11);
        let cols = 10usize;
        let (fw, fh) = (100.0, 80.0);
        let rows = self.figures.div_ceil(cols).max(1);
        let mut s = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
            cols as f64 * fw,
            rows as f64 * fh,
            cols as f64 * fw,
            rows as f64 * fh
        );
        for _ in 0..self.style_rules {
            s.push_str("<style>*{stroke-linejoin: round; stroke-linecap: butt}</style>");
        }
        s.push_str("<defs><path id=\"marker\" d=\"M0 0h1v1h-1z\"/>");
        for i in 0..self.figures {
            let (x, y) = ((i % cols) as f64 * fw, (i / cols) as f64 * fh);
            s.push_str(&format!(
                "<clipPath id=\"clip{i}\"><rect x=\"{x}\" y=\"{y}\" width=\"{fw}\" height=\"{fh}\"/></clipPath>"
            ));
        }
        s.push_str("</defs>");
        if self.image_kb > 0 {
            let payload: String = (0..self.image_kb * 1024)
                .map(|k| b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"[k % 64] as char)
                .collect();
            s.push_str(&format!(
                "<image id=\"img\" x=\"0\" y=\"0\" width=\"10\" height=\"10\" href=\"data:image/png;base64,{payload}\"/>"
            ));
        }
        s.push_str("<g id=\"layer1\">");
        for i in 0..self.figures {
            let (x, y) = ((i % cols) as f64 * fw, (i / cols) as f64 * fh);
            s.push_str(&format!("<g id=\"fig{i}\" transform=\"translate({x},{y})\">"));
            for lvl in 0..self.groups_per_figure {
                s.push_str(&format!(
                    "<g id=\"fig{i}g{lvl}\" transform=\"translate(0.5,0.5)\" clip-path=\"url(#clip{i})\">"
                ));
            }
            s.push_str(&format!(
                "<rect id=\"bg{i}\" x=\"0\" y=\"0\" width=\"{fw}\" height=\"{fh}\" style=\"fill:#ffffff;stroke:none\"/>"
            ));
            let mut last = String::new();
            for k in 0..self.shapes {
                let d = if k % 5 == 4 && !last.is_empty() {
                    last.clone()
                } else {
                    let (px, py) = (rng.below(90) as f64, rng.below(70) as f64);
                    format!("M{px} {py}l{} {}l{} {}z", rng.below(9) + 1, rng.below(9) + 1, rng.below(9) + 1, rng.below(9) + 1)
                };
                last = d.clone();
                s.push_str(&format!(
                    "<path id=\"fig{i}p{k}\" d=\"{d}\" style=\"fill:#{:02x}{:02x}{:02x};stroke:none\"/>",
                    rng.below(200) + 20,
                    rng.below(200) + 20,
                    rng.below(200) + 20
                ));
            }
            for k in 0..self.clones {
                s.push_str(&format!(
                    "<use id=\"fig{i}u{k}\" href=\"#marker\" x=\"{}\" y=\"{}\"/>",
                    rng.below(90),
                    rng.below(70)
                ));
            }
            for k in 0..self.texts {
                let dx = if k % 2 == 1 { " dx=\"0 0.2 0.1 0.3 0.2\"" } else { "" };
                s.push_str(&format!(
                    "<text id=\"fig{i}t{k}\" x=\"{}\" y=\"{}\" style=\"font-family:'DejaVu Sans';font-size:4px\"{dx}>label {i}-{k}</text>",
                    rng.below(80) + 2,
                    rng.below(60) + 8
                ));
            }
            for _ in 0..self.groups_per_figure {
                s.push_str("</g>");
            }
            s.push_str("</g>");
        }
        s.push_str("</g></svg>");
        s
    }
}
```

- [ ] **Step 2: Write the tests**

Create `tests/big_document.rs`:

```rust
//! The default pipelines on a many-figure document without upstream data: they finish, touch only
//! the selection, measure only the selection's text, and carry a multi-megabyte raster untouched.

mod support;

use std::ffi::OsString;

use support::{with_vendored_fonts, BigDoc};

fn args(v: &[&str]) -> Vec<OsString> {
    std::iter::once("sciink").chain(v.iter().copied()).map(OsString::from).collect()
}

fn run(tool: &str, svg: &str, extra: &[&str]) -> (String, Vec<String>) {
    let mut a = vec![format!("--tool={tool}")];
    a.extend(extra.iter().map(|s| s.to_string()));
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let out = with_vendored_fonts(|| sciink::run(&args(&a), svg.as_bytes())).unwrap();
    (String::from_utf8(out.svg).unwrap(), out.messages)
}

/// The exact source text of the element with `id`, taken from the document's own bytes.
fn slice_of<'a>(doc: &'a roxmltree::Document<'a>, id: &str) -> &'a str {
    let n = doc
        .descendants()
        .find(|n| n.attribute("id") == Some(id))
        .unwrap_or_else(|| panic!("no element {id}"));
    &doc.input_text()[n.range()]
}

#[test]
fn flattener_defaults_survive_a_big_document() {
    let svg = BigDoc::default().svg();
    let before = roxmltree::Document::parse(&svg).unwrap().descendants().count();
    let (out, msgs) = run("flattener", &svg, &["--id=layer1"]);
    let d = roxmltree::Document::parse(&out).expect("output parses");
    assert!(d.descendants().count() < before, "duplicates and backgrounds removed");
    assert!(
        !msgs.iter().any(|m| m.contains("nest") || m.contains("internal error")),
        "{msgs:?}"
    );
}

#[test]
fn flattener_on_one_figure_leaves_the_others_byte_identical() {
    let svg = BigDoc::default().svg();
    let (out, _) = run("flattener", &svg, &["--id=fig7"]);
    let (a, b) = (
        roxmltree::Document::parse(&svg).unwrap(),
        roxmltree::Document::parse(&out).unwrap(),
    );
    for id in ["fig0", "fig6", "fig8", "fig99"] {
        assert_eq!(slice_of(&a, id), slice_of(&b, id), "{id} changed");
    }
    assert_ne!(slice_of(&a, "fig7"), slice_of(&b, "fig7"), "fig7 was flattened");
}

#[test]
fn homogenizer_font_size_on_one_figure_leaves_the_others_byte_identical() {
    let svg = BigDoc::default().svg();
    let (out, _) = run(
        "homogenizer",
        &svg,
        &["--id=fig7", "--setfontsize=true", "--fontsize=6", "--fontmodes=2"],
    );
    let (a, b) = (
        roxmltree::Document::parse(&svg).unwrap(),
        roxmltree::Document::parse(&out).unwrap(),
    );
    for id in ["fig0", "fig6", "fig8", "fig99"] {
        assert_eq!(slice_of(&a, id), slice_of(&b, id), "{id} changed");
    }
    assert_ne!(slice_of(&a, "fig7"), slice_of(&b, "fig7"));
}

#[test]
fn a_multi_megabyte_base64_image_round_trips_byte_for_byte() {
    let big = BigDoc {
        figures: 3,
        image_kb: 4096,
        ..BigDoc::default()
    };
    let svg = big.svg();
    let payload_in = roxmltree::Document::parse(&svg)
        .unwrap()
        .descendants()
        .find(|n| n.attribute("id") == Some("img"))
        .unwrap()
        .attribute("href")
        .unwrap()
        .to_string();
    let (out, _) = run(
        "flattener",
        &svg,
        &["--id=fig0", "--fixtext=false", "--removeduppaths=false", "--removerectw=false", "--deepungroup=false"],
    );
    let d = roxmltree::Document::parse(&out).unwrap();
    let payload_out = d
        .descendants()
        .find(|n| n.attribute("id") == Some("img"))
        .unwrap()
        .attribute("href")
        .unwrap();
    assert_eq!(payload_out.len(), payload_in.len());
    assert!(payload_out == payload_in, "the image payload changed");
}
```

Create `tests/bench.rs`:

```rust
//! Phase timings on the synthetic document and, when `SCIINK_BIG_SVG` names a file, on that file.
//! Opt-in: `cargo test --release --test bench -- --ignored --nocapture`.

mod support;

use std::process::Command;

fn log_for(input: &std::path::Path, args: &[&str]) -> String {
    let log = std::env::temp_dir().join(format!("sciink-bench-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let status = Command::new(env!("CARGO_BIN_EXE_sciink"))
        .args(args)
        .arg("--log")
        .arg(&log)
        .arg(input)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read_to_string(&log).unwrap_or_default()
}

fn total_ms(log: &str) -> f64 {
    log.lines()
        .filter(|l| l.contains("phase=total"))
        .filter_map(|l| l.split_whitespace().find_map(|f| f.strip_prefix("dt=")))
        .filter_map(|v| v.parse::<f64>().ok())
        .last()
        .unwrap_or(f64::NAN)
}

#[test]
#[ignore = "timing report; run with --release --ignored --nocapture"]
fn phase_timings() {
    let synthetic = std::env::temp_dir().join(format!("sciink-bench-{}.svg", std::process::id()));
    std::fs::write(&synthetic, support::BigDoc::default().svg()).unwrap();
    let mut inputs: Vec<(String, std::path::PathBuf, &str)> = vec![
        ("synthetic".into(), synthetic.clone(), "fig0"),
    ];
    if let Some(p) = std::env::var_os("SCIINK_BIG_SVG") {
        inputs.push(("SCIINK_BIG_SVG".into(), p.into(), "figure_1-3"));
    }
    for (name, path, figure) in inputs {
        for args in [
            vec!["--tool=flattener", "--id=layer1"],
            vec!["--tool=flattener", &format!("--id={figure}")],
            vec!["--tool=homogenizer", "--setfontsize=true", "--fontsize=7", "--fontmodes=2", "--id=layer1"],
        ] {
            let args: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();
            let log = log_for(&path, &args);
            println!("== {name} {}\n{log}", args.join(" "));
            let t = total_ms(&log);
            assert!(t < 60_000.0, "{name} {}: total {t} ms", args.join(" "));
        }
    }
}
```

(If `vec![…, &format!(…)]` fights the borrow checker, build the three argument lists as `Vec<String>` first and map to `&str` inside the loop.)

- [ ] **Step 3: Run**

Run: `cargo test --test big_document` — PASS (each test a few seconds in debug). `cargo test --release --test bench -- --ignored --nocapture` prints the phase logs.

- [ ] **Step 4: Commit**

```bash
git add tests/support/mod.rs tests/big_document.rs tests/bench.rs
git commit -m "test: synthetic big document, selection-isolation invariants, phase bench

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---
### Task 16: Re-measure, document the deviations, release 0.2.0

**Files:**
- Modify: `docs/spec/03-infrastructure.md` (append `## Deliberate deviations (Plan 9)`), `docs/spec/02-geometry-tools.md` (append `## Deliberate deviations (Plan 9)`), `CHANGELOG.md`, `docs/DEVELOPING.md` (the two `<Task 16 number>` placeholders), `Cargo.toml` (`version = "0.2.0"`), `Cargo.lock` (regenerated by cargo), `tests/cli.rs:12,32` (`sciink 0.1.0` → `sciink 0.2.0`), this plan (Appendix B)

- [ ] **Step 1: Re-measure**

Run the Task 2 commands again (release build, second run of each). Fill Appendix B of this plan with a before/after table per selection and phase. Decide Task 10 from `phase=text` on `--id=layer1` (execute it when > 1 100 ms) and record the decision. Acceptance: `phase=total` ≤ 400 ms for `--id=figure_1-3` and `--id=g660`; ≤ 2 500 ms for `--id=layer1`; Homogenizer on `figure_1-3` ≤ 300 ms; warm font start (`fonts:` line of `--tool=about`) ≤ 30 ms. If a budget is missed, the log names the phase; the follow-up candidates, in order, are: memoise `Doc::transform`/`composed_transform` (a new `xform_generation` bumped only by `transform` writes plus a `structure_generation` bumped by attach/detach, cached in `Doc.caches` with the walk-up-to-a-cached-ancestor pattern `specified_style` uses), tag predicates as `matches!` instead of slice `contains` in `ops::bbox`, a `shape_path` cache in `remove_duplicates`, then Task 10. Do none of these speculatively; open a ruling in the SDD ledger and add a task only when the numbers demand it.

- [ ] **Step 2: Spec deviations**

Append to `docs/spec/03-infrastructure.md`:

```markdown
## Deliberate deviations (Plan 9)

1. **Persistent font-scan cache.** Face metadata and metrics are cached under
   `$INKSCAPE_PROFILE_DIR/sciink/fontcache-1.tsv` (per-user temp directory when the variable is
   unset) and validated by the size and mtime of every cached font file plus the mtime of every
   directory holding one; a font installed into a brand-new top-level font directory is the only
   change not noticed. `SCIINK_NO_FONT_CACHE=1` disables the cache, `SCIINK_FONT_CACHE=<path>` moves
   it. Upstream rescans on every run. Cached faces report English (US) for every family name; only the
   names are used.
2. **Bundled DejaVu Sans (Book, Bold)** ships in `<extension dir>/fonts` and is scanned after the
   system fonts; a face of identical family, weight, style and width that is not bundled sorts ahead
   of the bundled copy, so an installed DejaVu Sans always wins. `SCIINK_NO_SYSTEM_FONTS=1` disables
   the bundle too; `SCIINK_NO_BUNDLED_FONTS=1` disables only it. Upstream bundles nothing.
3. **Per-phase timing** is written to `SCIINK_LOG` (`tool=… phase=… dt=…`); upstream has no
   equivalent. Nothing reaches stderr.
4. **`needs-live-preview="false"` on every tool** — this matches upstream; Plans 5–7 had enabled it
   on four tools and that was the deviation.
```

Append to `docs/spec/02-geometry-tools.md`:

```markdown
## Deliberate deviations (Plan 9)

- **Deviation removed:** the Flattener's character table covers only the elements whose boxes the
  bbox stage requests (`ngs2`), matching upstream's `BB2(svg, ngs2)` → `make_char_table(els=tels)`.
  Plans 6–8 built it over the whole document; that was the deviation (and ≈ 0.5 s per run on a
  5 000-text document).
- The duplicate-path and white-rectangle passes enumerate candidates through a bucket index and a
  uniform grid (`geom::grid`) instead of all pairs. Same predicates, same visiting order, same
  results (property-tested against the pairwise code); upstream is O(k²) in numpy.
- `Doc::selection` and the bbox stage keep one whole-document walk each to recover document order
  (1–2 ms on 62 000 nodes) — a rank index that every attach/detach would invalidate was rejected.
- `ops::bbox` gives no box to a `<use>` whose `href` does not resolve (upstream `dhelpers.py:1505–1521`
  does the same); this was misfiled as a bug in 0.1.0's known gaps.
```

- [ ] **Step 3: CHANGELOG, version, docs numbers**

`CHANGELOG.md` `## 0.2.0 (unreleased)` — complete it:

```markdown
## 0.2.0 (unreleased)

Large documents no longer freeze Inkscape for our part of the run. On a 52 MB, 62 000-element
manuscript the Flattener went from <before> s to <after> s on one figure and from <before> s to
<after> s on the whole layer (Homogenizer: <before> → <after> s); Inkscape's own save/reload of
such a file is unchanged and documented in `docs/DEVELOPING.md`.

- Flattener: the character table covers only the measured elements (upstream parity; it measured
  every text in the document); duplicate and white-rectangle removal use index sweeps instead of
  all-pairs tests; the nested-text check in kerning removal is linear.
- Style cascade: universal `*{…}` rules are pre-merged per stylesheet; declarations are no longer
  cloned per node; `clip-path`/`mask` sheet lookups short-circuit.
- DOM: a move re-indexes nothing; identical attribute writes are skipped; the writer copies attribute
  values in bulk and pre-sizes its buffer.
- Fonts: the scan is cached on disk (`fontcache-1.tsv`; ≈ 3 s → tens of ms after boot); DejaVu Sans
  (Book, Bold) is bundled as the matplotlib fallback, an installed copy takes precedence; Diagnostics
  and Font Probe show `[bundled]`.
- Live preview is off on every tool, as upstream.
- `SCIINK_LOG` records one line per phase with its duration; new switches `SCIINK_NO_FONT_CACHE`,
  `SCIINK_FONT_CACHE`, `SCIINK_NO_BUNDLED_FONTS`.
- CI and release builds pinned to `ubuntu-24.04`.
- Known-gap correction: a `<use>` with a dangling `href` has no box (parity), not a bbox bug.
```

Fill the two `<Task 16 number>` placeholders in `docs/DEVELOPING.md`. `Cargo.toml`: `version = "0.2.0"`; run `cargo build` so `Cargo.lock` follows; `tests/cli.rs`: the two `sciink 0.1.0` assertions become `sciink 0.2.0` (a version bump, not a loosened assertion).

- [ ] **Step 4: Full verification**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test
cargo test --release --test invariance -- --ignored --nocapture
SCIINK_SYSTEM_FONTS=1 cargo test --test text_fixtures --test text_tools --test text_ghoster --test flattener_fixtures --test homogenizer_fixtures -- --ignored --test-threads=1
cargo build --release && dist/package.sh macos-universal target/release/sciink && dist/test-package.sh dist/out/sciink-macos-universal.zip && dist/test-install.sh dist/out/sciink-macos-universal.zip
SCIINK_BIG_SVG=/Users/yzheng/Documents/Manuscripts/MetPredict/metpredict_figures_clean.svg cargo test --release --test bench -- --ignored --nocapture
```

Expected: all green; `PACKAGE-OK`; the bench prints the phase logs.

- [ ] **Step 5: Commit**

```bash
git add docs/spec/02-geometry-tools.md docs/spec/03-infrastructure.md CHANGELOG.md docs/DEVELOPING.md Cargo.toml Cargo.lock tests/cli.rs docs/superpowers/plans/2026-09-23-plan9-big-documents.md
git commit -m "release: 0.2.0 candidate — deviations, changelog, measured numbers

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

- [ ] **Step 6: After the branch is reviewed and merged (controller, not implementer)**

`gh workflow run release.yml --ref main` dry run → three artifacts, each containing `fonts/`; the user runs `dist/dev-install.sh`, restarts Inkscape once and does the manual pass on the 50 MB file (Diagnostics timing for the docs; one figure through the Flattener with no freeze; a dialog field edit re-runs nothing; Font Probe shows `[bundled]` for DejaVu on a machine without it); then the CHANGELOG heading is dated and `git tag v0.2.0 && git push origin v0.2.0` publishes.

---

## Appendix A — baseline (Task 2)

| selection | tool | phase | dt ms |
|---|---|---|---|
| figure_1-3 | flattener | parse | 60.5 |
| figure_1-3 | flattener | selection | 1.8 |
| figure_1-3 | flattener | workingset | 0.2 |
| figure_1-3 | flattener | defsmove | 0.3 |
| figure_1-3 | flattener | unlink | 97.4 |
| figure_1-3 | flattener | ungroup | 181.0 |
| figure_1-3 | flattener | rects | 6.6 |
| figure_1-3 | flattener | fonts | 183.9 |
| figure_1-3 | flattener | text | 214.7 |
| figure_1-3 | flattener | fonts | 0.3 |
| figure_1-3 | flattener | bbox | 392.5 |
| figure_1-3 | flattener | dedup | 5.2 |
| figure_1-3 | flattener | whiterects | 0.1 |
| figure_1-3 | flattener | cleanup | 30.8 |
| figure_1-3 | flattener | write | 81.1 |
| figure_1-3 | flattener | total | 1072.2 |
| g660 | flattener | parse | 61.9 |
| g660 | flattener | selection | 2.1 |
| g660 | flattener | workingset | 0.3 |
| g660 | flattener | defsmove | 0.1 |
| g660 | flattener | unlink | 0.1 |
| g660 | flattener | ungroup | 108.7 |
| g660 | flattener | rects | 13.0 |
| g660 | flattener | fonts | 158.4 |
| g660 | flattener | text | 170.2 |
| g660 | flattener | fonts | 0.3 |
| g660 | flattener | bbox | 438.0 |
| g660 | flattener | dedup | 13.0 |
| g660 | flattener | whiterects | 0.1 |
| g660 | flattener | cleanup | 3.2 |
| g660 | flattener | write | 80.0 |
| g660 | flattener | total | 890.8 |
| g9740 | flattener | parse | 60.4 |
| g9740 | flattener | selection | 1.6 |
| g9740 | flattener | workingset | 0.2 |
| g9740 | flattener | defsmove | 0.1 |
| g9740 | flattener | unlink | 0.1 |
| g9740 | flattener | ungroup | 45.7 |
| g9740 | flattener | rects | 21.0 |
| g9740 | flattener | fonts | 177.0 |
| g9740 | flattener | text | 320.8 |
| g9740 | flattener | fonts | 0.3 |
| g9740 | flattener | bbox | 356.8 |
| g9740 | flattener | dedup | 2.4 |
| g9740 | flattener | whiterects | 1.9 |
| g9740 | flattener | cleanup | 28.3 |
| g9740 | flattener | write | 80.1 |
| g9740 | flattener | total | 919.4 |
| layer1 | flattener | parse | 63.9 |
| layer1 | flattener | selection | 1.6 |
| layer1 | flattener | workingset | 2.2 |
| layer1 | flattener | defsmove | 7.7 |
| layer1 | flattener | unlink | 247.2 |
| layer1 | flattener | ungroup | 2018.2 |
| layer1 | flattener | rects | 244.1 |
| layer1 | flattener | fonts | 167.3 |
| layer1 | flattener | text | 1087.9 |
| layer1 | flattener | fonts | 0.4 |
| layer1 | flattener | bbox | 1170.2 |
| layer1 | flattener | dedup | 532.8 |
| layer1 | flattener | whiterects | 542.0 |
| layer1 | flattener | cleanup | 96.3 |
| layer1 | flattener | write | 115.0 |
| layer1 | flattener | total | 6129.1 |
| figure_1-3 | homogenizer | parse | 58.0 |
| figure_1-3 | homogenizer | selection | 1.3 |
| figure_1-3 | homogenizer | fonts | 173.9 |
| figure_1-3 | homogenizer | fontsize | 225.6 |
| figure_1-3 | homogenizer | fonts | 0.4 |
| figure_1-3 | homogenizer | recentre | 19.5 |
| figure_1-3 | homogenizer | cleanup | 0.1 |
| figure_1-3 | homogenizer | write | 80.9 |
| figure_1-3 | homogenizer | total | 385.5 |
| layer1 | homogenizer | parse | 58.1 |
| layer1 | homogenizer | selection | 2.6 |
| layer1 | homogenizer | fonts | 159.5 |
| layer1 | homogenizer | fontsize | 1754.6 |
| layer1 | homogenizer | fonts | 0.3 |
| layer1 | homogenizer | recentre | 529.8 |
| layer1 | homogenizer | cleanup | 0.1 |
| layer1 | homogenizer | write | 81.6 |
| layer1 | homogenizer | total | 2426.8 |
| - | about | parse | 60.2 |
| - | about | fonts | 172.9 |
| - | about | fonts | 173.0 |
| - | about | total | 234.8 |

Apple M-series Mac, macOS, warm cache — 2026-09-23. About `phase=fonts`: ms=173, faces=1022.

## Appendix B — after (Task 16)

Apple M-series Mac, macOS, release build (`cargo build --release`), same file
(`/Users/yzheng/Documents/Manuscripts/MetPredict/metpredict_figures_clean.svg`, read-only) — 2026-09-24.
Same methodology as Appendix A: `--log /tmp/sciink-l`, `rm -f` before each run, run twice, second run
recorded. The persistent font-scan cache (Task 12) was already warm from earlier work in this worktree
for every row below except the dedicated cold/warm measurement, which deletes it first.

### Before/after per selection and phase

| selection | tool | phase | before ms (Task 2) | after ms (Task 16) |
|---|---|---|---|---|
| figure_1-3 | flattener | parse | 60.5 | 47.6 |
| figure_1-3 | flattener | selection | 1.8 | 0.4 |
| figure_1-3 | flattener | workingset | 0.2 | 0.1 |
| figure_1-3 | flattener | defsmove | 0.3 | 0.3 |
| figure_1-3 | flattener | unlink | 97.4 | 11.7 |
| figure_1-3 | flattener | ungroup | 181.0 | 17.6 |
| figure_1-3 | flattener | rects | 6.6 | 4.6 |
| figure_1-3 | flattener | fonts | 183.9 | 15.7 |
| figure_1-3 | flattener | text | 214.7 | 25.8 |
| figure_1-3 | flattener | fonts | 0.3 | 0.3 |
| figure_1-3 | flattener | bbox | 392.5 | 18.1 |
| figure_1-3 | flattener | dedup | 5.2 | 17.5 |
| figure_1-3 | flattener | whiterects | 0.1 | 0.2 |
| figure_1-3 | flattener | cleanup | 30.8 | 6.5 |
| figure_1-3 | flattener | write | 81.1 | 61.9 |
| figure_1-3 | flattener | total | 1072.2 | 212.4 |
| g660 | flattener | parse | 61.9 | 48.4 |
| g660 | flattener | selection | 2.1 | 0.5 |
| g660 | flattener | workingset | 0.3 | 0.2 |
| g660 | flattener | defsmove | 0.1 | 0.1 |
| g660 | flattener | unlink | 0.1 | 0.1 |
| g660 | flattener | ungroup | 108.7 | 11.6 |
| g660 | flattener | rects | 13.0 | 11.4 |
| g660 | flattener | fonts | 158.4 | 3.4 |
| g660 | flattener | text | 170.2 | 9.1 |
| g660 | flattener | fonts | 0.3 | 0.3 |
| g660 | flattener | bbox | 438.0 | 23.7 |
| g660 | flattener | dedup | 13.0 | 22.6 |
| g660 | flattener | whiterects | 0.1 | 0.2 |
| g660 | flattener | cleanup | 3.2 | 3.1 |
| g660 | flattener | write | 80.0 | 61.2 |
| g660 | flattener | total | 890.8 | 192.3 |
| g9740 | flattener | parse | 60.4 | 49.1 |
| g9740 | flattener | selection | 1.6 | 0.4 |
| g9740 | flattener | workingset | 0.2 | 0.1 |
| g9740 | flattener | defsmove | 0.1 | 0.1 |
| g9740 | flattener | unlink | 0.1 | 0.1 |
| g9740 | flattener | ungroup | 45.7 | 6.3 |
| g9740 | flattener | rects | 21.0 | 3.3 |
| g9740 | flattener | fonts | 177.0 | 3.5 |
| g9740 | flattener | text | 320.8 | 32.0 |
| g9740 | flattener | fonts | 0.3 | 0.4 |
| g9740 | flattener | bbox | 356.8 | 18.3 |
| g9740 | flattener | dedup | 2.4 | 13.1 |
| g9740 | flattener | whiterects | 1.9 | 0.2 |
| g9740 | flattener | cleanup | 28.3 | 5.5 |
| g9740 | flattener | write | 80.1 | 61.1 |
| g9740 | flattener | total | 919.4 | 189.6 |
| layer1 | flattener | parse | 63.9 | 51.5 |
| layer1 | flattener | selection | 1.6 | 0.6 |
| layer1 | flattener | workingset | 2.2 | 1.8 |
| layer1 | flattener | defsmove | 7.7 | 7.8 |
| layer1 | flattener | unlink | 247.2 | 64.6 |
| layer1 | flattener | ungroup | 2018.2 | 191.2 |
| layer1 | flattener | rects | 244.1 | 128.8 |
| layer1 | flattener | fonts | 167.3 | 3.4 |
| layer1 | flattener | text | 1087.9 | 223.3 |
| layer1 | flattener | fonts | 0.4 | 0.3 |
| layer1 | flattener | bbox | 1170.2 | 226.7 |
| layer1 | flattener | dedup | 532.8 | 37.7 |
| layer1 | flattener | whiterects | 542.0 | 2.4 |
| layer1 | flattener | cleanup | 96.3 | 13.3 |
| layer1 | flattener | write | 115.0 | 84.5 |
| layer1 | flattener | total | 6129.1 | 1034.2 |
| figure_1-3 | homogenizer | parse | 58.0 | 51.3 |
| figure_1-3 | homogenizer | selection | 1.3 | 0.5 |
| figure_1-3 | homogenizer | fonts | 173.9 | 3.7 |
| figure_1-3 | homogenizer | fontsize | 225.6 | 14.4 |
| figure_1-3 | homogenizer | fonts | 0.4 | 0.3 |
| figure_1-3 | homogenizer | recentre | 19.5 | 7.3 |
| figure_1-3 | homogenizer | cleanup | 0.1 | 0.0 |
| figure_1-3 | homogenizer | write | 80.9 | 61.0 |
| figure_1-3 | homogenizer | total | 385.5 | 134.6 |
| layer1 | homogenizer | parse | 58.1 | 52.8 |
| layer1 | homogenizer | selection | 2.6 | 1.5 |
| layer1 | homogenizer | fonts | 159.5 | 3.5 |
| layer1 | homogenizer | fontsize | 1754.6 | 228.9 |
| layer1 | homogenizer | fonts | 0.3 | 0.4 |
| layer1 | homogenizer | recentre | 529.8 | 123.2 |
| layer1 | homogenizer | cleanup | 0.1 | 0.0 |
| layer1 | homogenizer | write | 81.6 | 64.7 |
| layer1 | homogenizer | total | 2426.8 | 471.3 |
| - | about | parse | 60.2 | 51.1 |
| - | about | fonts | 172.9 | 3.5 |
| - | about | fonts | 173.0 | 3.6 |
| - | about | total | 234.8 | 56.3 |

Notes on the table above: `bbox phase=…table_texts=` measured 156 (figure_1-3), 39 (g660), 735 (g9740),
5078 (layer1) — the figure_1-3 and g660 figures match the Task 7 ruling exactly (156 and 39). Homogenizer
`fontsize`/`recentre` measured `tels=154` (figure_1-3) and `tels=5039` (layer1). About `phase=fonts`:
ms=3, faces=1022 (was ms=173, faces=1022 in Appendix A — same face count, cache warm).

### Totals summary (speedup)

| selection | tool | before total ms | after total ms | speedup |
|---|---|---|---|---|
| figure_1-3 | flattener | 1072.2 | 212.4 | ≈5.0× |
| g660 | flattener | 890.8 | 192.3 | ≈4.6× |
| g9740 | flattener | 919.4 | 189.6 | ≈4.8× |
| layer1 | flattener | 6129.1 | 1034.2 | ≈5.9× |
| figure_1-3 | homogenizer | 385.5 | 134.6 | ≈2.9× |
| layer1 | homogenizer | 2426.8 | 471.3 | ≈5.2× |
| - | about | 234.8 | 56.3 | ≈4.2× |

### Font start (cold vs warm)

Dedicated measurement, separate from the matrix above: removed
`<TMPDIR>/sciink-<user>/fontcache-1.tsv` (on this machine,
`/var/folders/6h/dzlbdtr909b4t0tk4ggwdznc0000gp/T/sciink-yzheng/fontcache-1.tsv`, since
`INKSCAPE_PROFILE_DIR` is unset outside Inkscape), then ran
`target/release/sciink --tool=about tests/data/edge/simple.svg 2>&1 >/dev/null | grep -E 'fonts:|bundled fonts:'`
twice:

```
cold: fonts: 1022 faces in 287 ms      (scan + cache write)
      bundled fonts: not found
warm: fonts: 1022 faces in 18 ms       (cache read)
      bundled fonts: not found
```

`bundled fonts: not found` is expected here: `target/release/sciink` run directly has no `fonts/`
directory beside the executable (that only exists inside the packaged extension layout produced by
`dist/package.sh`, verified separately in Step 4 below). Warm font start 18 ms ≤ the 30 ms budget.

### Acceptance targets (plan Step 1)

| target | budget | measured | result |
|---|---|---|---|
| `phase=total`, flattener, `--id=figure_1-3` | ≤ 400 ms | 212.4 ms | MET |
| `phase=total`, flattener, `--id=g660` | ≤ 400 ms | 192.3 ms | MET |
| `phase=total`, flattener, `--id=layer1` | ≤ 2 500 ms | 1034.2 ms | MET |
| Homogenizer, `--id=figure_1-3` | ≤ 300 ms | 134.6 ms | MET |
| warm font start (`fonts:` line of `--tool=about`) | ≤ 30 ms | 18 ms | MET |

All five budgets are met. No phase named a budget miss, so none of the Step 1 follow-up candidates
(memoising `Doc::transform`/`composed_transform`, `matches!` tag predicates in `ops::bbox`, a
`shape_path` cache in `remove_duplicates`, Task 10) were implemented — consistent with the plan's
"do none of these speculatively" instruction.

### Task 10 decision

Trigger: execute Task 10 only if `phase=text` on `--id=layer1` exceeds 1 100 ms after Task 9. The
fresh measurement above (Before/after table, `layer1 | flattener | text`) is **223.3 ms**, well under
the 1 100 ms trigger (it was 402 ms after Task 8, per the ledger; Task 9 does not touch the text phase
and the number fell further, plausibly from cumulative effects of Tasks 3–6 and 11 on the shared DOM/
selection/write paths). Task 10 is confirmed **not executed**; the plan's `### Task 10` heading is
marked `(not executed: text phase was 223.3 ms)`.

### Corrections carried from execution (not written into the Task bodies)

Per the controller's ledger, four Task N briefs had defects discovered and fixed during
implementation; the corrections are not retrofitted into those Task sections above (per Global
Constraints) and are instead recorded here, alongside the fresh numbers that confirm them:

- **Task 9** (non-finite-box window proof): the plan's window proof for the grid sweep was
  incomplete for non-finite coordinates — see the Task 9 ruling below for the exact reasoning
  (`1e-6·inf = inf`, `inf <= inf` holds). The implementer's fix (non-finite boxes stay candidates in
  both roles) is unchanged by this task; `dedup`/`whiterects` numbers above (e.g. layer1: 532.8→37.7 ms
  and 542.0→2.4 ms) reflect the corrected behaviour.
- **Task 15** (`BigDoc` generator and assertion changes): three plan defects in the synthetic-document
  generator and its invariant assertions were found and fixed (duplicate-path style reuse and the
  100×80 cell containment; clip-rect coordinates and the single-outermost-group `clip-path`; the
  "element count strictly drops" proxy replaced by specific structural assertions). See the three
  Task 15 rulings below.
- **Task 7** (`table_texts`): the plan's done-when line said `table_texts=154`; the measured value is
  **156** for figure_1-3, confirmed again in the Before/after table above (`bbox` row, figure_1-3) and
  by this run's g660 figure of 39 (also matching the Task 7 ruling).
  The 154 vs. 156 gap is the plan's estimate versus the measured, verified value; 156 is correct.
- **Task 8** (return-count of `write_clean_text`): the plan expected 1001; the verified count is
  **1002** (1000 unmerged siblings + the rewritten `outer` + one split-off; the old `outer`/`inner`
  NodeIds are not returned). See the two Task 8 rulings below for the full reasoning.

### Rulings mirrored from execution

One bullet per `Ruling:` line in `.superpowers/sdd/2026-09-23-plan9-big-documents/progress.md`
(Task 1 through Task 15), verbatim, with the `Task N:` prefix trimmed into a label (the two
pre-flight rulings predate Task 1 and carry no task number):

- **Pre-flight:** Ruling: test-only verbatim copies of replaced implementations (`escape_*_ref`,
  `duplicate_scan_ref`, `background_scan_ref`, the external_merges golden) are required by the plan's
  Global Constraints as the comparison oracle — a reviewer flagging them as duplication is adjudicated
  against this ruling, not fixed — costs nothing if wrong beyond ~150 lines of test code.
- **Pre-flight:** Ruling: the plan's `<before>`/`<after>` placeholders in commit messages are to be
  filled with the measured numbers by the implementer, not left literal — a commit with a literal
  placeholder is a review finding — costs a reworded commit if wrong.
- **Task 1:** Ruling: keep the debug-tool timers (parse/write/total) — the plan's File Structure row
  says every tool emits phases and the dispatch asked for it explicitly; harmless log lines — costs
  three trivial hunks to revert if wrong.
- **Task 1:** second sonnet reviewer stalled the same way. Ruling: switch every task review in this
  plan to the bounded checklist method (fixed YES/NO items with file:line evidence, no hand-tracing,
  at most one focused test), haiku first — the Plan 5 lesson; a sonnet free-form review of a 40 KB
  diff stalls — costs a coarser review per task, offset by the opus whole-branch review at the end.
- **Task 1:** checklist review returned in 70 s: 14/14 YES, Spec compliance PASS, Task quality
  Approved, no findings. Ruling: every review package is copied to `review-taskN.diff` (or
  `review-taskN-fixR.diff`) before dispatch — three reviewers stalled on the `..`-named path and none
  on the plain name — costs one `cp` per review.
- **Task 2:** Ruling: implementer's `rm -f` between runs instead of `tail -n 14` accepted — each run
  emits 16 phase lines (fonts + defsmove), so the brief's tail would have dropped parse/selection; the
  plan's Step 1 command is documentation, the appendix is the deliverable — costs nothing.
- **Task 3:** Ruling: the brief's "done when write ≤ 60 ms (from ≈ 250)" is not met because the
  design's 250 ms estimate was wrong — the measured baseline was 81 ms; the change is correct and
  byte-identical, a 15 % gain; accepted, no SWAR follow-up unless Task 16 shows write on the critical
  path — costs ~10 ms per run on a 50 MB file if wrong.
- **Task 3:** review 10/11 YES; one Important finding: commit trailer read "Claude Haiku 4.5"
  (implementers substitute their own model name). Ruling: controller amends the trailer on unpushed
  task commits (message-only, code untouched) instead of a fix round — costs nothing; every dispatch
  now states the trailer verbatim with "do not substitute your model name", and the review checklist
  keeps the trailer item.
- **Task 5:** implemented — DONE, commit b93a4e1, 320 tests, trailer correct. ungroup layer1 2018 →
  253 ms (8×), figure_1-3 181 → 22 ms; implementer byte-diffed (SHA-256) flattener+homogenizer output
  on the 50 MB doc pre/post: identical. Ruling: the clippy-driven removal of a no-op `format!` in the
  brief's cleanup test is accepted (assertions unchanged) — costs nothing.
- **Task 6:** review 11/12 YES; the one NO (item 10, commit body not visible in the package) is a
  false positive — the controller verified the full message (numbers 252.9 -> 202.3 ms, Fable
  trailer) with /usr/bin/git log before the review. Ruling: closed as not-a-defect; the commit-message
  item leaves the reviewer checklist (the controller checks it) — costs nothing.
- **Task 7:** Ruling: 156 texts in figure_1-3's bbox set is the measured value; the plan's
  "table_texts=154" done-when line is read as "the figure's own texts, not the document's 5 039" —
  costs nothing.
- **Task 8:** Ruling: the plan's 1001 assumed `outer` gets rewritten (detaching `inner`); 1002 means
  neither is edited (a `<text>` nested in a `<text>` is presumably not parsed). The implementer must
  verify and pin the reason in the test (outer unchanged, inner still its child) — the count follows
  the verified facts, not the other way round; the missing guard run is accepted (the test is a
  behaviour test, passing after the change) — costs one small commit.
- **Task 8:** Ruling: the plan's 1001 expected the split-distant option not to split any of the test
  texts; one does split, so 1002 is the true count; the test's factual assertions (inner not returned)
  are what matters — costs nothing.
- **Task 9:** Ruling: the implementer's overflow list for non-finite boxes is correct and the plan's
  window proof was incomplete — an infinite coordinate makes the reference's tolerance `1e-6·inf = inf`
  and `inf <= inf` holds, so such a box equals almost every other; non-finite boxes must stay
  candidates in both roles (the degenerate-box property test caught it). Plan proof gets a correction
  note at Task 16 — costs nothing.
- **Task 9:** Ruling: the per-case `!a.is_empty()` sanity check in the random-box test was over-strict
  (12/200 seed-42 cases legitimately remove nothing, by the reference's own behaviour); an aggregate
  guard is right but `total_removed > 0` is too weak → pre-review fix: `cases_with_removals >= 150`
  (188 observed). Equivalence assertions untouched — costs nothing.
- **Task 10:** Ruling: not executed — its trigger (text phase on layer1 > 1 100 ms after Task 9) is
  not met (402 ms after Task 8; Task 9 does not touch the text phase); Task 16 re-checks the number
  and marks the plan section "(not executed: text phase was <n> ms)" — costs a later small task if the
  final numbers disagree.
- **Task 12:** implemented — DONE, commit 7350f70, 342 tests, trailer correct. fonts: cold 247 ms
  (scan + cache write) → warm 8 ms. Ruling: the two literal deviations (kept `scan_count()`,
  `path.clone()` in the fontdb literal) are accepted — the brief's snippet omitted a function tests
  still need, and the clone is the compiling form — costs nothing.
- **Task 13:** implemented — DONE, commit 155adc8, 344 tests, PACKAGE-OK, trailer correct; live check
  on this Mac: `DejaVu Sans → DejaVu Sans (DejaVuSans.ttf) [bundled]`, opt-out falls back to Verdana.
  Ruling: the brief's Step 2 command (two positional test filters) was invalid cargo syntax — the
  implementer's compile check is the equivalent; the stale DEVELOPING Known-gaps bullet belongs to
  Task 14 (the implementer's spawned follow-up chip task_fb3c1372 was dismissed as redundant) — costs
  nothing.
- **Task 14:** implemented — DONE, commit f4e02cb, 347 tests, trailer correct. Ruling: the brief's two
  ops_bbox tests needed `&mut d` and `r##"…"##` to compile — accepted as transcription fixes
  (assertions unchanged); CHANGELOG/doc wording where the brief gave topics is the implementer's —
  costs nothing.
- **Task 15:** Ruling: plan defects, fixed in the test design — (a) the fig7 `assert_ne!` is replaced
  by "fig7's group was dissolved" (deep ungroup dissolves the selected group, as tests/flattener.rs:228
  already pins); (b) `BigDoc::svg` must make the duplicate path reuse the previous style too (else no
  duplicates), and keep shapes, clones and labels inside their 100 × 80 cell (else earlier figures'
  spill keeps later backgrounds); the count-drops assertion stays and gains "no bg* remains" and "no
  fig* group remains" — costs nothing; Task 16 mirrors this into the plan appendix.
- **Task 15:** Ruling: another brief defect — clip rects go to local figure coordinates (0,0,fw,fh)
  and `clip-path` is applied on the outermost nested group only; count assertion unchanged — costs
  nothing; mirrored at Task 16.
- **Task 15:** Ruling: the brief's "element count strictly drops" was a wrong proxy — replaced by the
  specific effects: no fig* groups, no bg* rects, no <use>, paths_after == paths_before −
  (shapes/5)·figures + clones·figures; total count not asserted (comment explains the clipPath
  copies) — costs nothing; mirrored at Task 16.
- **Task 15:** implemented — DONE after 3 pre-review rounds, commit cc93f04, 351 passed + 8 ignored
  (bench), trailer correct. Path arithmetic 3001 − 600 + 500 = 2901 held exactly. Ruling: four tests in
  big_document.rs (the fifth in the brief's list, the T7 character-table test, already lives in
  tests/flattener.rs) — costs nothing.

