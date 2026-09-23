# Appendix C — Infrastructure: DOM, style, CLI, packaging, tests

## C.0 Facts found on this machine that shaped the design
- Fixtures (11 files): all start `<?xml version="1.0" encoding="UTF-8" standalone="no"?>`; no DOCTYPE, no
  entities, no CDATA, no PIs; comments in Acid (24), Text_tests (6), Text_tests_dx (1); `<style>` sheets use
  only `*{}` and `.clsN,.clsM{}`; prefixes exactly `svg inkscape sodipodi xlink rdf cc dc`; only `&lt; &gt;
  &amp; &quot;`; id-less elements only `rdf:RDF cc:Work dc:format dc:type`; presentation attrs few
  (`clip-path` 1481, `opacity` 427, `class` 332, `fill` 16, `stroke` 11). Upstream test data (≈33 MB) is on
  the public `dev` branch at `292ae77ef73e7b2d3b4a06c11768f2e1aad15bae`.
- Upstream refs: lxml single-line, `{:.6g}` numbers; comparison rounds non-transform decimals to 0 places,
  transforms to 3 (abcd)/0 (ef), positional id normalization, ±1 on x/y, `Cambria Math`≈`Cambria`, ±1 height.
  Homogenizer refs have md5 names: `5a83c7…` = Other_tests.svg, `b1a8ca…` = Other_tests_nonuniform.svg.
- Fonts present: Arial, Tahoma, Verdana, Helvetica, Avenir, Avenir Next, Roboto (+Light), Times New Roman,
  Courier New. **Missing**: DejaVu Sans (204 uses in Acid), Calibri, Cambria(+Math), Franklin Gothic Book.
- Inkscape 1.4.4 actions: `select-by-id`, `select-all`, `<ext-id>.noprefs`, `export-filename`, `export-do`;
  `export-filename:-` mirrors to stderr, so export to a file. `--pdf-poppler`, `--pdf-font-strategy`,
  `--export-type=svg` exist.
- **Bundled Python is dead here**: `…/Python.framework/Versions/3.10/bin/python3.10` exits 137 (SIGKILL);
  `codesign -vv` reports a modified `gi/overrides/GioUnix.py` (+ `.si-backup` — an older Scientific-Inkscape
  patched it) while the bundle carries `com.apple.quarantine`. Core Python extensions run headlessly return
  the document unchanged. Upstream benchmark is blocked until the user repairs this.
- Windows: Inkscape spawns via GLib's GUI-subsystem helper without `CREATE_NO_WINDOW`, so a console-subsystem
  exe **flashes a console**; use `#![cfg_attr(windows, windows_subsystem = "windows")]` — inherited pipe
  handles still work (that is how `pythonw` works). Ink/Stitch does the same.
- quick-xml ≥ 0.38 emits `Event::GeneralRef` for every `&name;`; no entity-name validation.
- Toolchain: rustc/cargo 1.93, only `aarch64-apple-darwin`; `uv` present (no local matplotlib → `uv run
  --with matplotlib`); no R; `rsvg-convert`, `magick`/`compare`, `pdftocairo`, `lipo`, `zip`, `gh` (auth'd).

## C.1 Editable DOM — `src/dom.rs` (~700 LOC), `src/num.rs` (~60)
```rust
pub type NodeId = u32;                 // arena index; detached nodes are tombstones
pub struct Doc { nodes: Vec<Node>, root: NodeId /*synthetic Document*/, svg: NodeId, ids: HashMap<String, NodeId>,
                 next_auto_id: u32, gen: Cell<u64>, style_cache: RefCell<StyleCache> }
struct Node { parent, first, last, prev, next: Option<NodeId>, kind: Kind }
enum Kind { Document, Element { name: QName, attrs: Vec<Attr>, self_closing: bool }, Text(String),
            CData(String), Comment(String), PI { target, data }, DocType(String), Decl(String) }
pub struct QName { prefix: Option<Box<str>>, local: Box<str> }   // literal prefix, never resolved
pub struct Attr { name: String /* "inkscape:label" as written */, value: String }
```
- Parse: strip BOM; UTF-8 only; quick-xml default config (`expand_empty_elements=false`, no trimming); explicit
  stack (iterative); merge Text+GeneralRef runs (char refs + 5 predefined; other names → `Unsupported`);
  root must be `svg`; known namespace URIs must be bound to standard prefixes or default, else `Unsupported`
  (`ponytail:` ceiling — no namespace resolution; `svg:path` ≡ `path`).
- Lossless: decl, DOCTYPE, comments, PIs, CDATA, attr order, prefixes, all whitespace text, self-closing form.
  Normalized: `"` quotes, canonical escaping. Parse→write of every fixture must be byte-identical (test).
- Serialization: hand-rolled iterative writer (~120 LOC), never pretty-prints.
- `num::fmt(f64)`: 8 significant digits (`{:.7e}` → parse → Display → strip zeros), `-0`→`0`, NaN/inf → `0` +
  log. Everything that writes numbers goes through it → byte-stable across runs/OSes.
- Ids: `ensure_id(n)` assigns `sciink-N` only when a tool needs one; never bulk-assign. Duplicate ids: first
  wins + warning. `deep_clone` drops ids in the copy.
- API: `parse, write, svg, defs (create+prepend if absent), by_id, ensure_id, selection(ids) (document order,
  unknown ids logged), parent/children/next_sibling/prev_sibling/ancestors/descendants (pre-order, iterative),
  tail(n) (following Text sibling, for lxml ports), new_element/new_text/deep_clone, append/prepend/
  insert_before/insert_after/detach/replace, tag/is_element/is_text, attr/set_attr/remove_attr/attrs/href,
  text/set_text/text_content`. Mutations bump `gen`, keep `ids` consistent (moves preserve ids).
  Target: Acid_tests (18.6k elements) parse+write < 60 ms.

## C.2 Style cascade — `src/style.rs` (~650 LOC)
- Precedence: UA defaults → presentation attrs → `<style>` rules by (importance, specificity, order) → inline
  `style=""`; then inheritance (+ `inherit`); computed step: `font-size` → px (`medium`=12px as Inkscape;
  %/em/ex/smaller/larger vs parent), `font-weight` keywords → 100..900 (bolder/lighter relative), lengths
  (`stroke-width`, `letter-spacing`, `word-spacing`) → user units. `font:` and `marker` shorthands expanded.
- Selector subset (sized from matplotlib `*{…}`, svglite `.svglite line, .svglite text {…}` in CDATA,
  Illustrator `.st0{…}`, Inkscape PDF import `style=` only): `*`, type, `.class`, `#id`, compound, comma
  lists, descendant and `>` combinators, `/* */`, `@media/@font-face/@import` skipped, `!important`. Others
  drop the rule + log. Own parser ~200 LOC; no CSS crates.
- `Style(Vec<(String, String)>)` ordered; `to_string()` = `k:v;` in order (Inkscape's format).
- Property table `(name, initial, inherited)` per SVG 1.1/2, cross-checked with Inkscape's
  `inkex/properties.py:626-935`. Inherited: clip-rule, color, color-*, cursor, direction, fill, fill-opacity,
  fill-rule, font*, glyph-orientation-*, image-rendering, letter-spacing, line-height, marker*, paint-order,
  pointer-events, shape-rendering, stroke*, text-align, text-anchor, text-rendering, visibility, white-space,
  word-spacing, writing-mode. Not inherited: alignment-baseline, baseline-shift, clip, clip-path, display,
  dominant-baseline, filter, flood-*, inline-size, mask, opacity, overflow, shape-inside, stop-*,
  text-decoration, unicode-bidi, vector-effect. Initials: `fill:black stroke:none stroke-width:1
  font-size:medium font-family:sans-serif text-anchor:start opacity:1 stroke-linecap:butt
  stroke-linejoin:miter stroke-miterlimit:4 display:inline visibility:visible clip-path/mask/filter/
  marker-*:none direction:ltr white-space:normal color:black`.
- API: `cascaded_style(n)` (own declarations, no inheritance — what ungroup pushes down), `computed_style(n)
  -> Rc<Style>` (cached), `computed(n, prop)`, `set_style(n, prop, val)` (writes into `style=""`, removes a
  same-named presentation attr, bumps gen), `remove_style`.
- Cache: per-node `(gen, Rc<Style>)`; recompute on gen mismatch by walking ancestors (depth ≤ ~15). Sheet
  matches precomputed into `HashMap<NodeId, Vec<(importance, specificity, order, decl)>>`, rebuilt only when
  a `<style>`, `class` or `id` changes. Upstream applies CSS only to elements with ids and ignores specificity
  (`cache.py:1073-1164`); we do it correctly (golden budgets absorb divergences).

## C.3 CLI & `.inx` contract — `src/cli.rs` (~200), `src/main.rs` (~150)
Inkscape argv: `[…/extensions/sciink/bin/sciink, --tool=flattener, --tab=Options, --deepungroup=true, …,
--id=a, --id=b, --selected-nodes=a:0:3, /tmp/ink_ext_XXXX.svg]`. Bools `true`/`false` (accept
`True`/`1`/`0` too — upstream tests pass `True`), floats as text, optiongroups as `value`, notebooks as page
`name`, strings possibly empty. Every `.inx` param is always passed, any order.
```rust
pub fn tool_from_argv(argv) -> Option<ToolName>;      // scan for --tool=<x>
#[derive(clap::Args)] pub struct Common {
    #[arg(long, value_enum)] tool: ToolName,           // flattener scaler homogenizer text-ghoster combine-by-color favorite-markers about
    #[arg(long = "id", action = Append)] ids: Vec<String>,
    #[arg(long = "selected-nodes", action = Append, hide = true)] selected_nodes: Vec<String>,  // accepted, unused v1
    #[arg(long, short = 'o')] output: Option<PathBuf>, // inkex-compatible; "-" = stdout
    #[arg(long, env = "SCIINK_LOG", hide = true)] log: Option<PathBuf>,
    input: Option<PathBuf>,                            // default stdin
}
// per tool: #[derive(clap::Parser)] struct FlattenerCli { #[command(flatten)] common: Common, #[arg(long, default_value="Options")] tab: String,
//   #[arg(long, value_parser = inx_bool, action = Set, default_value_t = true)] deepungroup: bool, … justification: u8, markexc: u8,
//   #[arg(long, value_parser = inx_bool, hide = true, default_value_t = false)] testmode: bool, debugparser: bool }
```
Two-stage parse: find tool → `ToolCli::try_parse_from(argv)`. Unknown args are errors (the `.inx` and binary
ship together; mismatch should be loud).
```rust
#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    // help/version → stdout, exit 0
    let input = read_input(&argv);                    // failure → stderr, exit 1 (nothing to echo)
    let out = match catch_unwind(|| run(&argv, &input)) {   // run: parse → tool → Doc::write
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e))    => { eprintln!("sciink {tool}: {e}\nThe document was left unchanged."); input.clone() }
        Err(_)        => { eprintln!("sciink {tool}: internal error (a bug): {msg}\nThe document was left unchanged. Set SCIINK_LOG=<file> and report the log."); input.clone() }
    };
    write_output(&out, &output);                      // stdout BufWriter+flush, BrokenPipe ignored; --output: temp+rename
}
```
`profile.release` keeps `panic = "unwind"`; panic hook stores message+location. All tree walks iterative.
stderr carries only: fatal message (doc echoed unchanged), user-facing tool warnings (e.g. fonts not
installed), the About report. Exit 0 whenever output was produced. Debug/timing → `SCIINK_LOG=/path`
(`tool=flattener phase=parse ms=12 elements=18607`). Env: `SCIINK_FONT_DIRS`, `SCIINK_NO_SYSTEM_FONTS=1`.

**About/Diagnostics tool** (`--tool=about`, ~80 LOC): stderr `sciink 0.1.0 (sha, target)`, exe path, `.inx`
dir, `fonts: N faces in M ms`, resolutions of `Arial`/`DejaVu Sans`/`sans-serif`, element count; echoes doc.
Its `.inx` has no visible params → Inkscape runs it immediately and shows the dialog. `sciink --version`
prints the first line (`CARGO_PKG_VERSION` + `option_env!("SCIINK_GIT_SHA")` from CI).

**`.inx` files**: `inx/{flattener,scaler,homogenizer,text_ghoster,combine_by_color,favorite_markers,about}.inx`.
Param names/defaults/pages/option values copied verbatim from `$SI/*.inx`; differences: ids `org.sciink.*`,
hidden `tool` param, version label (`@VERSION@` substituted by `dist/package.sh`), `<object-type>all`,
`needs-live-preview` (true for text-ghoster, favorite-markers, combine-by-color, homogenizer, scaler; false
for flattener), `<script><command location="inx">bin/sciink</command></script>` (Windows zip: `bin/sciink.exe`,
rewritten by the packager). Skeleton (flattener):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<inkscape-extension xmlns="http://www.inkscape.org/namespace/inkscape/extension">
    <name>Flattener</name>
    <id>org.sciink.flattener</id>
    <param name="tool" type="string" gui-hidden="true">flattener</param>
    <param name="tab" type="notebook">
        <page name="Options" gui-text="Main options"> …deepungroup fixtext revertpaths removeduppaths removerectw… </page>
        <page name="Options2" gui-text="Text fix options"> …splitdistant mergenearby removemanualkerning mergesubsuper reversions removetextclips justification setreplacement replacement… </page>
        <page name="Exclusions" gui-text="Exclusions"> …markexc… </page>
    </param>
    <effect needs-live-preview="false"><object-type>all</object-type><effects-menu><submenu name="Scientific"/></effects-menu></effect>
    <script><command location="inx">bin/sciink</command></script>
</inkscape-extension>
```
If upstream is also installed, the `Scientific` submenu shows duplicate names — README says remove
`scientific_inkscape/` (its Python is dead on this Mac anyway) or rename our submenu.

## C.4 Packaging & release
Repo layout:
```
Cargo.toml Cargo.lock rust-toolchain.toml .cargo/config.toml LICENSE (GPL-2.0-or-later; upstream is GPL-2+) README.md CHANGELOG.md
src/ main.rs cli.rs dom.rs style.rs num.rs log.rs paths.rs fonts.rs geom/ ops/ text/ tools/{about,flattener,scaler,homogenizer,text_ghoster,combine_by_color,favorite_markers}.rs
inx/ 7 files      dist/ package.sh README-dist.txt      docs/spec/ text-engine.md geometry-tools.md
tests/ golden.rs invariance.rs snapshot.rs edge.rs support/{xmldiff,fuzzy,render}.rs fonts/{DejaVuSans*,Roboto-*}.ttf data/{corpus,edge}/ upstream/ (gitignored)
tools/ fetch-upstream.sh gen-corpus.py gen-corpus.R bench.py      .github/workflows/ ci.yml release.yml
```
`.cargo/config.toml`: `[target.x86_64-pc-windows-msvc] rustflags = ["-C", "target-feature=+crt-static"]`.
`profile.release`: `opt-level=3, lto="fat", codegen-units=1, strip="symbols", panic="unwind"`. Deps:
quick-xml 0.42, svgtypes 0.16, kurbo 0.13, clap 4.6 (derive, env), fontdb 0.24, rustybuzz 0.20, ttf-parser
0.25; dev: resvg 0.48, roxmltree 0.21, insta 1, regex 1.

Artifacts per OS (unversioned names → stable `releases/latest/download/` URLs): `sciink-macos-universal.zip`,
`sciink-windows-x64.zip`, `sciink-linux-x64.zip`, each `sciink/*.inx`, `sciink/bin/sciink[.exe]`,
`sciink/README.txt`; unzipping into the extensions dir yields `extensions/sciink/`. `dist/package.sh <os>
<binary>`: copy inx (Windows: sed `bin/sciink.exe`), substitute version, `chmod 755`, zip on the building OS
(exec bits survive because each OS job zips its own output; `upload-artifact` strips perms from loose files).
- macOS: build arm64 + x86_64 on `macos-latest`, `lipo -create`, `codesign --force --sign -` (ad-hoc).
  Primary install = Terminal one-liner (curl/unzip never set quarantine):
  ```bash
  cd "$HOME/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions" && curl -fsSLO https://github.com/OWNER/sciink/releases/latest/download/sciink-macos-universal.zip && unzip -oq sciink-macos-universal.zip && rm sciink-macos-universal.zip
  ```
  Troubleshooting: `xattr -dr com.apple.quarantine "…/extensions/sciink"`. Developer-ID + notarization = v2.
- Windows: `x86_64-pc-windows-msvc`, static CRT, GUI subsystem. Install `%APPDATA%\inkscape\extensions\sciink\`.
- Linux: `x86_64-unknown-linux-musl`, fully static (no C deps in the dependency set). Install
  `~/.config/inkscape/extensions/sciink/` (Flatpak: `~/.var/app/org.inkscape.Inkscape/config/inkscape/extensions/`).
- CI (`ci.yml`): `check` on ubuntu (fmt, clippy `-D warnings`, test); `test` on macos/windows with
  `SCIINK_NO_SYSTEM_FONTS=1`; `build` matrix {mac universal, win msvc, linux musl} → smoke (`--tool=about`;
  on Windows both `cmd /c "sciink.exe --tool=about in.svg > out.svg"` and `--output out.svg`, asserting
  identity — the GUI-subsystem stdout de-risk) → package → upload zip. `release.yml` on `v*` tags: assert tag
  == Cargo version, `workflow_call` the builds, `softprops/action-gh-release@v2` with `dist/*.zip`. Actions:
  `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`, `actions/{upload,download}-artifact@v4`.
- Sizes ≈ 3–4 MB per arch (universal ≈ 7 MB), zips 1.5–3 MB; release build 3–5 min per target.

## C.5 Test strategy
(a) **Unit** per module: DOM byte-identity on 11 fixtures + `tests/data/edge/*` (DOCTYPE, CDATA sheet, PI,
`svg:` prefixes, comments in text, 50k-deep nesting, truncated, non-UTF-8, unknown entity, duplicate ids); id
index after detach/attach/clone; `num::fmt` table; cascade cases; CLI (bool spellings, repeated `--id`, order,
unknown arg error, `--output -`).

(b) **Golden vs upstream refs** (`tests/golden.rs`): data from `tests/upstream/` via `tools/fetch-upstream.sh`
(sparse clone of `tests/data` at `292ae77…`) or `SCIINK_UPSTREAM_TESTS=~/Downloads/Scientific-Inkscape-dev/
tests/data`; skip if absent. Runner calls `sciink::run(args, input)`, applies a port of
`CompareNumericFuzzy2` to both sides (`support/fuzzy.rs` ~120 LOC), then `support/xmldiff.rs` (~250 LOC:
prune comments, positional id normalization incl. `href`/`url(#)`/`class`/`inkscape:stockid`/ids in `<style>`,
tree deltas, `d` compared as parsed arrays, tolerances x/y ≤ 1, `Cambria Math`≈`Cambria`, height ≤ 1). Each
case has an `allowed_deltas` budget (default 0); `SCIINK_GOLDEN_REPORT=dir` dumps outputs + first 20 deltas.

| tool | args | fixture | reproducible here |
|---|---|---|---|
| combine_by_color | `--id=layer1` | Other_tests.svg | yes |
| favorite_markers | `--id=path6928 --id=path6952 --smarker=True --tab=markers` | Other_tests.svg | yes |
| scale_plots | `--id=g5224 --tab=correction` | Other_tests.svg | likely |
| scale_plots | `--id=g109153 --id=g109019 --tab=correction` | Other_tests_nonuniform.svg | likely |
| scale_plots | `--id=rect5248 --id=g4982 --tab=matching --hmatchopts=2 --vmatchopts=3` | Other_tests.svg | likely |
| scale_plots | `--id=g4982 --tab=scaling --hscale=120 --vscale=80` | Other_tests.svg | ref exists though mode dropped; budget |
| text_ghoster | `--id=text28136` | Other_tests.svg | probably (Arial) |
| homogenizer | `--id=layer1 --fontsize=7 --setfontsize=True --fixtextdistortion=True --fontmodes=2 --setfontfamily=True --fontfamily=Avenir --setstroke=True --setstrokew=0.75 --strokemodes=2 --fusetransforms=True` | Other_tests.svg → `homogenizer__5a83c7….out`; Other_tests_nonuniform.svg → `…b1a8ca….out` | probably (Avenir present) |
| flatten_plots | `--id=layer1 --testmode=True` | Text_tests.svg, Text_tests_dx.svg | mostly (Roboto+DejaVu vendored; Calibri/Franklin missing → budget) |
| flatten_plots | same | Acid_tests.svg | partial (Calibri/Franklin/URW/Nimbus/Pazo missing → budget) |
| flatten_plots | same | Flow_tests.svg | structure only → budget |
| skipped | `--debugparser` variants (used as extent oracle instead), Font_variants_all (Windows fonts), Text_tests_mod (fixture absent), autoexporter | | |

Fonts for determinism: `tests/fonts/` vendors DejaVu Sans (Bitstream Vera licence) and Roboto (Apache-2.0).

(c) **Visual invariance** (`tests/invariance.rs`, resvg with vendored-font `fontdb`, longest side 1500 px,
metric = fraction of pixels with max-channel diff > 32): Combine by Color ≤ 0.3 %; Homogenizer
`--fusetransforms` only ≤ 0.1 %; Flattener with `--fixtext=false --removerectw=false --removeduppaths=false`
≤ 0.5 %; Flattener full on the matplotlib corpus ≤ 1 % soft (fail > 3 %). Diff PNGs to `target/invariance/`.

(d) **Snapshots** (`insta`): full output for corpus files ≤ 100 KB, sha256 for larger; determinism test runs
each case twice in-process and once via the built binary and asserts byte equality; CI diffs digests across
the 3 OSes (informational).

(e) **Corpus generator** `tools/gen-corpus.py` (`uv run --with matplotlib`): line plot with legend + mathtext,
scatter (`<defs><path id="m…"/></defs><use>`), shared-axis subplots, bar chart with hatch `<pattern>`, log
axes, `imshow` + colorbar (`<image>`), each with `svg.fonttype` `none` and `path`; save SVG + PDF; import PDFs
via `inkscape --pdf-font-strategy=keep --export-type=svg …` and `--pdf-poppler …`. `tools/gen-corpus.R`
(svglite + ggplot2) only if `Rscript` exists (not here). Outputs committed (≤ 3 MB) so CI needs neither
Python nor Inkscape.

(f) **Benchmark** `tools/bench.py` (stdlib): N=10 runs per (fixture, tool, args), median/min + `SCIINK_LOG`
phases; optional upstream column via `inkscape --actions="select-by-id:layer1;burghoff.flattenplots1.noprefs;
export-type:svg;export-filename:<tmp>;export-do"` minus a no-extension baseline, or via the bundled Python
with cwd set to scratch (upstream appends a relative `Log.txt`). Blocked here until Inkscape's bundle is
repaired (user action). Targets (Apple M-series, release): Acid_tests Flattener < 1.0 s (parse ≤ 60 ms, write
≤ 40 ms), Other_tests any tool < 150 ms, typical 100–300 KB matplotlib figure < 50 ms incl. process start,
font DB load ≤ 80 ms and only when a tool touches text.

## C.6 Manual end-to-end on this Mac
```bash
cd /Users/yzheng/Projects/better-inkscape-scientific && cargo build --release
EXT="$HOME/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/sciink"
mkdir -p "$EXT/bin"; ln -sf "$PWD"/inx/*.inx "$EXT/"; ln -sf "$PWD/target/release/sciink" "$EXT/bin/sciink"
osascript -e 'quit app "Inkscape"'; SCIINK_LOG=/tmp/sciink.log open -a Inkscape tests/data/corpus/mpl_lines_text.svg
```
Inkscape re-reads `.inx` only at startup; the binary is re-executed per run → dev loop = `cargo build
--release` + Apply. Direct equivalent of what Inkscape does:
```bash
target/release/sciink --tool=flattener --tab=Options --deepungroup=true --fixtext=true --revertpaths=true --removeduppaths=true --removerectw=true --splitdistant=true --mergenearby=true --removemanualkerning=true --mergesubsuper=true --reversions=true --removetextclips=true --justification=1 --setreplacement=false --replacement=Arial --markexc=1 --id=layer1 in.svg > out.svg
rsvg-convert -o in.png in.svg; rsvg-convert -o out.png out.svg; compare -metric AE in.png out.png diff.png
```

## C.7 Risks, LOC, build order
Risks: (1) Gatekeeper kills browser-downloaded ad-hoc-signed binary — day-1 experiment with the About-only
binary; curl route + `xattr` fix documented; notarization later. (2) Windows GUI subsystem + stdout — CI smoke
test + one manual run before first release. (3) Windows `.inx` → explicit `.exe`. (4) Cascade divergence
from upstream (CSS by id only) → golden budgets. (5) Font resolution (`sans-serif`, `Sans`, `Helvetica-Bold`,
`NimbusSanL-Regu` PDF names) → `fonts.rs` name splitter + About prints resolutions. (6) musl allocator /
system font scan (~500 faces here) → measure via About in CI; `mimalloc` only if musl regresses > 20 %.
(7) Thousands of `--id` args hit Inkscape's own 32 KB Windows limit → document "group first". (8) Flatpak
font discovery (`/run/host/fonts`) untested. (9) Upstream test data pinned to `292ae77…`. (10) lxml
`text`/`tail` vs sibling Text nodes → `doc.tail(n)` helper.

LOC: dom 700, style 650, num 60, cli 200 (+40/tool), main 150, log 60, paths 60, fonts 200, about 80,
tests/support 450, tests 400, tools/*.py 300, workflows 200 YAML, inx ≈ 400 XML → ≈ 3.2k Rust + 1.3k other.

Build order: (1) Cargo skeleton, `num`, `dom` parse/write + roundtrip/edge tests, `main` echo/panic with
`--tool=about`, `ci.yml` check; (2) `cli` two-stage parse, About tool + `about.inx`, `SCIINK_LOG`, symlink
install, confirm dialog; (3) `style` cascade + tests, `fonts.rs` + vendored fonts (unblocks text engine);
(4) test infra: fetch-upstream, golden runner (validate ref-vs-itself = 0 deltas), invariance harness (About
passthrough = 0 diff), snapshots; (5) corpus generator + committed corpus, bench; (6) package.sh, build
matrix, release.yml, pre-release `v0.0.1` (About only) installed on 3 OSes to burn down risks 1–3; (7) tools
in order of infrastructure risk: Combine by Color → Favorite Markers → Text Ghoster → Homogenizer → Scaler →
Flattener (+ `--testmode`).

---

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

