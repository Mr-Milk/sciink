# sciink Plan 1 — Foundation (M0 + M1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust binary `sciink` that Inkscape can launch as an extension, with a lossless SVG DOM, the upstream-compatible style cascade, and the geometry primitives every later tool needs.

**Architecture:** One crate (`lib` + thin `bin`). `main` reads the input file Inkscape passes, runs the tool named by `--tool=`, writes the result to stdout and messages to stderr; any failure echoes the input unchanged. `dom` is an arena over quick-xml events; `style` implements upstream's "specified style" cascade on top of it; `geom` wraps kurbo/svgtypes with the numeric policy from the spec. This plan ships only the `about` diagnostics tool; Plans 2+ add the real tools.

**Tech Stack:** Rust 1.93 (edition 2024), clap 4.6 (derive, env), quick-xml 0.42, svgtypes 0.16, kurbo 0.13; dev: roxmltree 0.21. No C dependencies.

**Spec:** `docs/spec/00-overview.md` (architecture, milestones), `docs/spec/03-infrastructure.md` (§C.1 DOM, §C.2 style, §C.3 CLI/.inx, §C.4 packaging, §C.5 tests), `docs/spec/02-geometry-tools.md` (§B.1 geometry primitives). The upstream Python being ported lives at `"$HOME/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/scientific_inkscape"`; upstream fixtures at `~/Downloads/Scientific-Inkscape-dev/tests/data/svg/`.

## Global Constraints

- Inkscape protocol (never break): argv = `[binary, --param=value…, --id=ID (repeated), --selected-nodes=…, /path/input.svg]`; output SVG on **stdout**; anything on **stderr** is shown to the user in a dialog; exit code ignored. Working directory is NOT the `.inx` directory — locate resources via `std::env::current_exe()`.
- On ANY failure (bad args, malformed XML, panic) the binary writes the ORIGINAL input bytes to stdout and one message to stderr ending with "The document was left unchanged."
- Zero subprocess calls, no Python, no network. Works unchanged on Inkscape 1.2–1.5.
- Namespace prefixes are literal strings (`inkscape:label`, `xlink:href`, `sodipodi:role`); never resolve namespaces. `tag()` compares local names only.
- Every number written into the document goes through `num::fmt` (8 significant digits, shortest repr, `-0` → `0`, non-finite → `0`).
- Attribute names used in code are exactly as written in the SVG (`"style"`, `"inkscape:label"`, `"xml:space"`).
- Style semantics = upstream inkex `cache.py`: specified style of a node = parent's specified style overridden by the node's cascaded style; cascaded = presentation attrs < `<style>` rules < inline `style=""`, where an `!important` declaration (sheet or inline) beats any normal declaration and specificity/order break ties. Every property propagates (no inherited/non-inherited distinction).
- Windows binary uses `#![cfg_attr(windows, windows_subsystem = "windows")]` (no console flash); stdout still works through Inkscape's pipe.
- `panic = "unwind"` in release (needed for `catch_unwind`). All tree walks are iterative (no recursion on document depth).
- Formatting/lints: `cargo fmt --all` and `cargo clippy --all-targets -- -D warnings` must be clean before every commit.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Tests that need upstream fixtures locate them via `SCIINK_UPSTREAM_TESTS` (a dir containing `svg/` and `refs/`) or `tests/upstream/data`; when absent they print `SKIP:` and pass.

---

## File structure

| File | Responsibility |
|---|---|
| `Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml`, `.gitignore`, `LICENSE` | crate metadata, toolchain pin, static CRT on Windows, ignores, GPL-2.0-or-later |
| `src/lib.rs` | module list, `Output`, `version()`, `run(argv, input)` dispatcher |
| `src/main.rs` | process boundary: read input, `catch_unwind`, echo-on-failure, stdout/stderr, `--output` |
| `src/num.rs` | `fmt(f64)`, `parse(&str)` |
| `src/dom.rs` | arena DOM: parse/write, navigation, mutation, id index, generation counters |
| `src/style.rs` | `Style` declarations, `Stylesheet` (selector subset), cascade + caches on `Doc` |
| `src/geom/mod.rs` | units, transforms, `Option<Rect>` algebra, `uniquetol` |
| `src/geom/path.rs` | `parse_d`/`fmt_d` (BezPath + command index), shape → path, end points, reverse, bbox |
| `src/cli.rs` | `ToolName`, `Common` args, `inx_bool`, `prescan`, `HELP` |
| `src/log.rs`, `src/paths.rs` | `SCIINK_LOG` file logging; exe/inx dir discovery |
| `src/tools/mod.rs`, `src/tools/about.rs` | tool registry; diagnostics tool |
| `inx/about.inx` | Inkscape menu entry for the diagnostics tool |
| `dist/dev-install.sh` | symlink install into Inkscape's user extension dir |
| `tests/support/mod.rs` | fixture discovery, semantic XML comparison |
| `tests/{num,dom,cli,style,geom}.rs` | integration tests per module |
| `tests/data/edge/simple.svg` | tiny fixture used by CLI tests and the Inkscape smoke test |
| `.github/workflows/ci.yml` | fmt + clippy + test on Linux, test on macOS/Windows |

---

### Task 1: Cargo skeleton and `sciink --version`

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.cargo/config.toml`, `.gitignore`, `LICENSE`, `src/lib.rs`, `src/main.rs`, `tests/cli.rs`
- Symlink (local only, gitignored): `tests/upstream` → `~/Downloads/Scientific-Inkscape-dev/tests`

**Interfaces:**
- Produces: `sciink::version() -> String`; binary prints `sciink <version>` for `--version`.

- [ ] **Step 1: Create the crate files**

Run:
```bash
cd /Users/yzheng/Projects/better-inkscape-scientific
mkdir -p src tests .cargo
curl -fsSL https://www.gnu.org/licenses/old-licenses/gpl-2.0.txt -o LICENSE
ln -sfn "$HOME/Downloads/Scientific-Inkscape-dev/tests" tests/upstream
```

`Cargo.toml`:
```toml
[package]
name = "sciink"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "GPL-2.0-or-later"
description = "Fast Inkscape extensions for scientific figures (a Rust rewrite of Scientific-Inkscape)"

[lib]
name = "sciink"
path = "src/lib.rs"

[[bin]]
name = "sciink"
path = "src/main.rs"

[dependencies]
clap = { version = "4.6", features = ["derive", "env"] }
kurbo = "0.13"
quick-xml = "0.42"
svgtypes = "0.16"

[dev-dependencies]
roxmltree = "0.21"

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "unwind"
```

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

`.cargo/config.toml`:
```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

`.gitignore`:
```
/target
/tests/upstream
```

`src/lib.rs`:
```rust
//! sciink — Inkscape extensions for scientific figures, as one fast binary.
//! Inkscape launches `sciink --tool=<name> --param=value… --id=… input.svg`
//! and reads the modified SVG from stdout (see docs/spec/03-infrastructure.md §C.3).

/// Version string shown to users: Cargo version plus the git SHA baked in by CI.
pub fn version() -> String {
    match option_env!("SCIINK_GIT_SHA") {
        Some(sha) => format!("{} ({sha})", env!("CARGO_PKG_VERSION")),
        None => env!("CARGO_PKG_VERSION").to_string(),
    }
}
```

`src/main.rs`:
```rust
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if argv.iter().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("sciink {}", sciink::version());
    }
}
```

- [ ] **Step 2: Write the failing test**

`tests/cli.rs`:
```rust
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sciink"))
}

#[test]
fn version_flag_prints_name_and_version() {
    let out = bin().arg("--version").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("sciink 0.1.0"), "got: {stdout}");
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test --test cli`
Expected: PASS (1 test). If `cargo` complains about edition 2024, the toolchain is older than 1.85 — run `rustup update stable`.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml .cargo .gitignore LICENSE src tests/cli.rs
git commit -m "build: cargo skeleton with --version

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: `num::fmt` — the single number formatter

**Files:**
- Create: `src/num.rs`, `tests/num.rs`
- Modify: `src/lib.rs` (add `pub mod num;`)

**Interfaces:**
- Produces: `sciink::num::fmt(v: f64) -> String`, `sciink::num::parse(s: &str) -> Option<f64>`.

- [ ] **Step 1: Write the failing test**

`tests/num.rs`:
```rust
use sciink::num::{fmt, parse};

#[test]
fn fmt_table() {
    let cases: &[(f64, &str)] = &[
        (0.30000000000000004, "0.3"),
        (0.0, "0"),
        (-0.0, "0"),
        (1e-9, "0.000000001"),
        (f64::NAN, "0"),
        (f64::INFINITY, "0"),
        (123456789.0, "123456790"),
        (1234.56789012, "1234.5679"),
        (2.0, "2"),
        (-1.5, "-1.5"),
        (1e15, "1000000000000000"),
        (100.0, "100"),
        (-0.00000000001, "-0.00000000001"),
    ];
    for (v, want) in cases {
        assert_eq!(fmt(*v), *want, "fmt({v})");
    }
}

#[test]
fn parse_trims_and_rejects_garbage() {
    assert_eq!(parse(" 1.5 "), Some(1.5));
    assert_eq!(parse("1e3"), Some(1000.0));
    assert_eq!(parse("abc"), None);
    assert_eq!(parse(""), None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test num`
Expected: FAIL — `unresolved import sciink::num`.

- [ ] **Step 3: Implement**

`src/num.rs`:
```rust
//! The one number formatter. Every number written into an SVG goes through `fmt`
//! so output is byte-stable across runs and platforms (spec §C.1).

/// 8 significant digits, shortest representation, no exponent, `-0` → `0`,
/// NaN/±inf → `0`.
pub fn fmt(v: f64) -> String {
    if !v.is_finite() || v == 0.0 {
        return "0".to_string();
    }
    // `{:.7e}` rounds to 8 significant digits; re-parsing gives the nearest f64,
    // whose `Display` is the shortest round-tripping decimal without exponent.
    let rounded: f64 = format!("{v:.7e}").parse().unwrap_or(0.0);
    if rounded == 0.0 {
        return "0".to_string();
    }
    format!("{rounded}")
}

/// Lenient float parse used for attribute values: trims whitespace.
pub fn parse(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}
```

Add to `src/lib.rs`: `pub mod num;`

- [ ] **Step 4: Run tests**

Run: `cargo test --test num`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/num.rs src/lib.rs tests/num.rs
git commit -m "feat(num): single number formatter with 8 significant digits

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: DOM parse and write (lossless round trip)

**Files:**
- Create: `src/dom.rs`, `tests/support/mod.rs`, `tests/dom.rs`
- Modify: `src/lib.rs` (add `pub mod dom;`)

**Interfaces:**
- Produces: `sciink::dom::{Doc, NodeId, Kind, Attr, DomError}`; `Doc::parse(&[u8]) -> Result<Doc, DomError>`, `Doc::write(&self, &mut Vec<u8>)`, `Doc::root()`, `Doc::svg()`. Navigation/mutation come in Task 4 but the struct fields (`nodes`, `root`, `svg`, `ids`, `next_auto_id`, `generation`, `sheet_generation`) are laid down here.
- Consumes: nothing.

- [ ] **Step 1: Write the fixture helper**

`tests/support/mod.rs`:
```rust
#![allow(dead_code)]
//! Shared helpers for integration tests.

use std::path::PathBuf;

/// Directory holding upstream's `svg/` and `refs/` test data, if available.
pub fn upstream_data_dir() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("SCIINK_UPSTREAM_TESTS").map(PathBuf::from),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/upstream/data")),
    ];
    candidates.into_iter().flatten().find(|p| p.join("svg").is_dir())
}

/// All upstream fixture SVGs, sorted; empty (with a SKIP note) when unavailable.
pub fn upstream_svgs() -> Vec<PathBuf> {
    let Some(dir) = upstream_data_dir() else {
        eprintln!("SKIP: upstream fixtures not found (set SCIINK_UPSTREAM_TESTS or symlink tests/upstream)");
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir.join("svg"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "svg"))
        .collect();
    v.sort();
    v
}

/// Asserts two XML documents are structurally identical (element names,
/// attributes as sets, text and comment content, node order).
pub fn assert_same_tree(a: &str, b: &str, context: &str) {
    let da = roxmltree::Document::parse(a).unwrap_or_else(|e| panic!("{context}: input does not parse: {e}"));
    let db = roxmltree::Document::parse(b).unwrap_or_else(|e| panic!("{context}: output does not parse: {e}"));
    let na: Vec<_> = da.descendants().collect();
    let nb: Vec<_> = db.descendants().collect();
    assert_eq!(na.len(), nb.len(), "{context}: node count differs");
    for (x, y) in na.iter().zip(nb.iter()) {
        assert_eq!(x.node_type(), y.node_type(), "{context}: node type differs at {:?}", x.range());
        if x.is_element() {
            assert_eq!(x.tag_name().name(), y.tag_name().name(), "{context}: tag differs");
            let mut ax: Vec<(String, String)> = x.attributes().map(|a| (a.name().to_string(), a.value().to_string())).collect();
            let mut ay: Vec<(String, String)> = y.attributes().map(|a| (a.name().to_string(), a.value().to_string())).collect();
            ax.sort();
            ay.sort();
            assert_eq!(ax, ay, "{context}: attributes differ on <{}>", x.tag_name().name());
        }
        if x.is_text() || x.is_comment() {
            assert_eq!(x.text(), y.text(), "{context}: text differs");
        }
    }
}
```

- [ ] **Step 2: Write the failing tests**

`tests/dom.rs`:
```rust
mod support;

use sciink::dom::{Doc, DomError};

const EDGE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n\
<!-- top comment -->\n\
<?xml-stylesheet href=\"a.css\"?>\n\
<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n\
<svg\n   xmlns=\"http://www.w3.org/2000/svg\"\n   xmlns:inkscape=\"http://www.inkscape.org/namespaces/inkscape\"\n   id=\"root\">\n\
  <style><![CDATA[.a { fill: red; }]]></style>\n\
  <g id=\"g1\" inkscape:label=\"L &amp; M&#10;N &quot;q&quot; &gt;\">\n\
    <path id=\"p1\" d=\"M 0,0 L 1,1\" />\n\
    <text id=\"t1\" xml:space=\"preserve\">a &lt; b &gt; c &amp; d</text>\n\
  </g>\n\
  <!-- inner --><g id=\"g2\"></g>\n\
</svg>\n";

fn roundtrip(s: &str) -> String {
    let doc = Doc::parse(s.as_bytes()).unwrap();
    let mut out = Vec::new();
    doc.write(&mut out);
    String::from_utf8(out).unwrap()
}

#[test]
fn edge_document_round_trips_byte_identically() {
    assert_eq!(roundtrip(EDGE), EDGE);
}

#[test]
fn entities_are_decoded_in_memory() {
    let doc = Doc::parse(EDGE.as_bytes()).unwrap();
    let g1 = doc.by_id("g1").unwrap();
    assert_eq!(doc.attr(g1, "inkscape:label"), Some("L & M\nN \"q\" >"));
    let t1 = doc.by_id("t1").unwrap();
    assert_eq!(doc.text_content(t1), "a < b > c & d");
}

#[test]
fn single_quoted_attributes_are_normalized() {
    let out = roundtrip("<svg xmlns=\"http://www.w3.org/2000/svg\" id='a' title='say \"hi\"'/>");
    assert_eq!(out, "<svg xmlns=\"http://www.w3.org/2000/svg\" id=\"a\" title=\"say &quot;hi&quot;\"/>");
}

#[test]
fn bom_is_stripped() {
    let out = roundtrip("\u{feff}<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
    assert_eq!(out, "<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
}

#[test]
fn unknown_entity_is_unsupported() {
    let err = Doc::parse(b"<svg><text>a&nbsp;b</text></svg>").err().unwrap();
    assert!(matches!(err, DomError::Unsupported(_)), "{err}");
}

#[test]
fn non_utf8_is_unsupported() {
    let err = Doc::parse(b"<svg>\xff\xfe</svg>").err().unwrap();
    assert!(matches!(err, DomError::Unsupported(_)), "{err}");
}

#[test]
fn malformed_xml_is_an_xml_error() {
    let err = Doc::parse(b"<svg><g></svg>").err().unwrap();
    assert!(matches!(err, DomError::Xml(_)), "{err}");
}

#[test]
fn root_must_be_svg() {
    let err = Doc::parse(b"<html/>").err().unwrap();
    assert!(err.to_string().contains("not <svg>"), "{err}");
}

#[test]
fn nonstandard_prefix_for_known_namespace_is_unsupported() {
    let err = Doc::parse(b"<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:ink=\"http://www.inkscape.org/namespaces/inkscape\"/>").err().unwrap();
    assert!(matches!(err, DomError::Unsupported(_)), "{err}");
}

#[test]
fn prefixed_root_is_accepted() {
    let doc = Doc::parse(b"<svg:svg xmlns:svg=\"http://www.w3.org/2000/svg\"><svg:g id=\"a\"/></svg:svg>").unwrap();
    assert_eq!(doc.tag(doc.svg()), "svg");
    assert_eq!(doc.tag(doc.by_id("a").unwrap()), "g");
}

#[test]
fn deep_nesting_does_not_overflow_the_stack() {
    let depth = 50_000;
    let mut s = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
    for _ in 0..depth {
        s.push_str("<g>");
    }
    for _ in 0..depth {
        s.push_str("</g>");
    }
    s.push_str("</svg>");
    assert_eq!(roundtrip(&s), s);
}

#[test]
fn duplicate_ids_first_wins() {
    let doc = Doc::parse(b"<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"x\" class=\"first\"/><g id=\"x\" class=\"second\"/></svg>").unwrap();
    let n = doc.by_id("x").unwrap();
    assert_eq!(doc.attr(n, "class"), Some("first"));
}

#[test]
fn upstream_fixtures_round_trip_semantically() {
    for p in support::upstream_svgs() {
        let input = std::fs::read_to_string(&p).unwrap();
        let out = roundtrip(&input);
        support::assert_same_tree(&input, &out, &p.display().to_string());
    }
}

#[test]
fn upstream_fixtures_round_trip_byte_identically() {
    // Fixtures that legitimately cannot be byte-identical go here with a reason.
    const KNOWN_DIFFERENT: &[&str] = &[];
    for p in support::upstream_svgs() {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if KNOWN_DIFFERENT.contains(&name.as_str()) {
            continue;
        }
        let input = std::fs::read(&p).unwrap();
        let doc = Doc::parse(&input).unwrap();
        let mut out = Vec::new();
        doc.write(&mut out);
        if out != input {
            let i = out.iter().zip(&input).position(|(a, b)| a != b).unwrap_or(out.len().min(input.len()));
            let lo = i.saturating_sub(60);
            panic!(
                "{name}: first difference at byte {i}\n input: {:?}\noutput: {:?}",
                String::from_utf8_lossy(&input[lo..(i + 60).min(input.len())]),
                String::from_utf8_lossy(&out[lo..(i + 60).min(out.len())])
            );
        }
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test dom`
Expected: FAIL — `unresolved import sciink::dom`.

- [ ] **Step 4: Implement `src/dom.rs` (parse + write + the minimal accessors the tests use)**

`src/dom.rs`:
```rust
//! Arena DOM for Inkscape SVG documents (spec §C.1).
//!
//! Lossless for everything Inkscape writes: XML declaration, DOCTYPE, comments,
//! processing instructions, CDATA, attribute order, the whitespace before each
//! attribute, the ` />` form, namespace prefixes as literal strings, all text.
//! Normalized on output: attribute quotes are always `"`, text is re-escaped
//! canonically (`& < >` in text, plus `"` and newline/tab/CR as char refs in
//! attributes). Namespace prefixes are never resolved: `svg:path` and `path`
//! compare equal by local name (ponytail: standard prefixes are enforced at parse).

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;

use quick_xml::events::{BytesStart, Event};

pub type NodeId = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub name: String,
    pub value: String,
    /// Whitespace that preceded this attribute in the source (`" "` for new attributes).
    pub ws: String,
}

#[derive(Debug, Clone)]
pub enum Kind {
    Document,
    Element {
        name: String,
        attrs: Vec<Attr>,
        self_closing: bool,
        /// Whitespace between the last attribute and `>` / `/>`.
        close_ws: String,
    },
    Text(String),
    CData(String),
    Comment(String),
    /// Everything between `<?` and `?>`.
    PI(String),
    /// Everything between `<!DOCTYPE ` and `>`.
    DocType(String),
    /// Everything between `<?` and `?>` of the XML declaration.
    Decl(String),
}

#[derive(Debug, Clone)]
struct Node {
    parent: Option<NodeId>,
    first: Option<NodeId>,
    last: Option<NodeId>,
    prev: Option<NodeId>,
    next: Option<NodeId>,
    kind: Kind,
}

impl Node {
    fn new(kind: Kind) -> Node {
        Node { parent: None, first: None, last: None, prev: None, next: None, kind }
    }
}

#[derive(Debug)]
pub enum DomError {
    /// Not well-formed XML.
    Xml(String),
    /// Well-formed but outside what sciink handles (encoding, entities, prefixes, root).
    Unsupported(String),
}

impl fmt::Display for DomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DomError::Xml(m) => write!(f, "invalid XML: {m}"),
            DomError::Unsupported(m) => write!(f, "unsupported document: {m}"),
        }
    }
}

impl std::error::Error for DomError {}

/// Namespaces whose prefix we rely on being the conventional one.
const KNOWN_NS: &[(&str, &str)] = &[
    ("http://www.w3.org/2000/svg", "svg"),
    ("http://www.inkscape.org/namespaces/inkscape", "inkscape"),
    ("http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd", "sodipodi"),
    ("http://www.w3.org/1999/xlink", "xlink"),
    ("http://www.w3.org/1999/02/22-rdf-syntax-ns#", "rdf"),
    ("http://creativecommons.org/ns#", "cc"),
    ("http://purl.org/dc/elements/1.1/", "dc"),
];

pub struct Doc {
    nodes: Vec<Node>,
    root: NodeId,
    svg: NodeId,
    ids: HashMap<String, NodeId>,
    next_auto_id: u32,
    /// Bumped on every mutation; consumers cache derived data keyed by it.
    pub(crate) generation: Cell<u64>,
    /// Bumped when a `<style>` element or its text changes.
    pub(crate) sheet_generation: Cell<u64>,
}

impl Doc {
    pub fn parse(bytes: &[u8]) -> Result<Doc, DomError> {
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let text = std::str::from_utf8(bytes)
            .map_err(|_| DomError::Unsupported("input is not valid UTF-8".to_string()))?;
        let mut reader = quick_xml::Reader::from_str(text);
        {
            let cfg = reader.config_mut();
            cfg.trim_text_start = false;
            cfg.trim_text_end = false;
            cfg.expand_empty_elements = false;
            cfg.check_end_names = true;
        }
        let mut doc = Doc {
            nodes: Vec::with_capacity(1024),
            root: 0,
            svg: 0,
            ids: HashMap::new(),
            next_auto_id: 1,
            generation: Cell::new(0),
            sheet_generation: Cell::new(0),
        };
        doc.nodes.push(Node::new(Kind::Document));
        let mut stack: Vec<NodeId> = vec![0];
        let mut text_buf = String::new();
        let mut text_pending = false;
        loop {
            let ev = reader.read_event().map_err(|e| DomError::Xml(e.to_string()))?;
            let continues_text = matches!(ev, Event::Text(_) | Event::GeneralRef(_));
            if text_pending && !continues_text {
                let id = doc.alloc(Kind::Text(std::mem::take(&mut text_buf)));
                doc.link_last(*stack.last().unwrap(), id);
                text_pending = false;
            }
            let parent = *stack.last().unwrap();
            match ev {
                Event::Text(t) => {
                    text_buf.push_str(&t);
                    text_pending = true;
                }
                Event::GeneralRef(r) => {
                    let name: &str = &r;
                    let ch = match name {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "quot" => '"',
                        "apos" => '\'',
                        _ => r
                            .resolve_char_ref()
                            .map_err(|e| DomError::Xml(e.to_string()))?
                            .ok_or_else(|| DomError::Unsupported(format!("undefined entity &{name};")))?,
                    };
                    text_buf.push(ch);
                    text_pending = true;
                }
                Event::Start(s) => {
                    let id = doc.element_from_start(&s, false)?;
                    doc.link_last(parent, id);
                    stack.push(id);
                }
                Event::Empty(s) => {
                    let id = doc.element_from_start(&s, true)?;
                    doc.link_last(parent, id);
                }
                Event::End(_) => {
                    if stack.len() > 1 {
                        stack.pop();
                    }
                }
                Event::CData(c) => {
                    let id = doc.alloc(Kind::CData(c.into_inner().into_owned()));
                    doc.link_last(parent, id);
                }
                Event::Comment(c) => {
                    let id = doc.alloc(Kind::Comment(c.into_inner().into_owned()));
                    doc.link_last(parent, id);
                }
                Event::PI(p) => {
                    let id = doc.alloc(Kind::PI(p.into_inner().into_owned()));
                    doc.link_last(parent, id);
                }
                Event::Decl(d) => {
                    let raw: &str = &d;
                    let id = doc.alloc(Kind::Decl(raw.to_string()));
                    doc.link_last(parent, id);
                }
                Event::DocType(t) => {
                    let id = doc.alloc(Kind::DocType(t.into_inner().into_owned()));
                    doc.link_last(parent, id);
                }
                Event::Eof => break,
            }
        }
        let svg = doc
            .children(doc.root)
            .find(|&c| doc.is_element(c))
            .ok_or_else(|| DomError::Unsupported("document has no root element".to_string()))?;
        if doc.tag(svg) != "svg" {
            return Err(DomError::Unsupported(format!("root element is <{}>, not <svg>", doc.qname(svg))));
        }
        doc.svg = svg;
        doc.check_namespaces()?;
        Ok(doc)
    }

    fn check_namespaces(&self) -> Result<(), DomError> {
        for a in self.attrs(self.svg) {
            let Some(prefix) = a.name.strip_prefix("xmlns:") else { continue };
            if let Some((uri, std_prefix)) = KNOWN_NS.iter().find(|(uri, _)| *uri == a.value) {
                if prefix != *std_prefix {
                    return Err(DomError::Unsupported(format!(
                        "namespace {uri} is bound to prefix '{prefix}' (expected '{std_prefix}')"
                    )));
                }
            }
        }
        Ok(())
    }

    fn element_from_start(&mut self, s: &BytesStart<'_>, self_closing: bool) -> Result<NodeId, DomError> {
        let name = s.name().0.to_string();
        let (attrs, close_ws) = parse_attributes(s.attributes_raw())?;
        let id = self.alloc(Kind::Element { name, attrs, self_closing, close_ws });
        if let Some(idv) = self.attr(id, "id").map(str::to_string) {
            self.ids.entry(idv).or_insert(id);
        }
        Ok(id)
    }

    fn alloc(&mut self, kind: Kind) -> NodeId {
        self.nodes.push(Node::new(kind));
        (self.nodes.len() - 1) as NodeId
    }

    fn link_last(&mut self, parent: NodeId, child: NodeId) {
        let last = self.nodes[parent as usize].last;
        {
            let c = &mut self.nodes[child as usize];
            c.parent = Some(parent);
            c.prev = last;
            c.next = None;
        }
        match last {
            Some(l) => self.nodes[l as usize].next = Some(child),
            None => self.nodes[parent as usize].first = Some(child),
        }
        self.nodes[parent as usize].last = Some(child);
    }

    /// Serializes the document; never pretty-prints, never reorders anything.
    pub fn write(&self, out: &mut Vec<u8>) {
        enum Step {
            Open(NodeId),
            Close(NodeId),
        }
        let mut stack: Vec<Step> = Vec::new();
        let mut c = self.nodes[self.root as usize].last;
        while let Some(n) = c {
            stack.push(Step::Open(n));
            c = self.nodes[n as usize].prev;
        }
        while let Some(step) = stack.pop() {
            match step {
                Step::Close(n) => {
                    if let Kind::Element { name, .. } = &self.nodes[n as usize].kind {
                        out.extend_from_slice(b"</");
                        out.extend_from_slice(name.as_bytes());
                        out.push(b'>');
                    }
                }
                Step::Open(n) => {
                    let node = &self.nodes[n as usize];
                    match &node.kind {
                        Kind::Document => {}
                        Kind::Element { name, attrs, self_closing, close_ws } => {
                            out.push(b'<');
                            out.extend_from_slice(name.as_bytes());
                            for a in attrs {
                                out.extend_from_slice(a.ws.as_bytes());
                                out.extend_from_slice(a.name.as_bytes());
                                out.extend_from_slice(b"=\"");
                                escape_attr(&a.value, out);
                                out.push(b'"');
                            }
                            out.extend_from_slice(close_ws.as_bytes());
                            if node.first.is_none() && *self_closing {
                                out.extend_from_slice(b"/>");
                            } else {
                                out.push(b'>');
                                stack.push(Step::Close(n));
                                let mut c = node.last;
                                while let Some(k) = c {
                                    stack.push(Step::Open(k));
                                    c = self.nodes[k as usize].prev;
                                }
                            }
                        }
                        Kind::Text(t) => escape_text(t, out),
                        Kind::CData(t) => {
                            out.extend_from_slice(b"<![CDATA[");
                            out.extend_from_slice(t.as_bytes());
                            out.extend_from_slice(b"]]>");
                        }
                        Kind::Comment(t) => {
                            out.extend_from_slice(b"<!--");
                            out.extend_from_slice(t.as_bytes());
                            out.extend_from_slice(b"-->");
                        }
                        Kind::PI(t) | Kind::Decl(t) => {
                            out.extend_from_slice(b"<?");
                            out.extend_from_slice(t.as_bytes());
                            out.extend_from_slice(b"?>");
                        }
                        Kind::DocType(t) => {
                            out.extend_from_slice(b"<!DOCTYPE ");
                            out.extend_from_slice(t.as_bytes());
                            out.push(b'>');
                        }
                    }
                }
            }
        }
    }

    // ---- minimal accessors used by parse/write; the full API is Task 4 ----

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn svg(&self) -> NodeId {
        self.svg
    }

    pub fn kind(&self, n: NodeId) -> &Kind {
        &self.nodes[n as usize].kind
    }

    pub fn is_element(&self, n: NodeId) -> bool {
        matches!(self.kind(n), Kind::Element { .. })
    }

    /// Qualified name as written (`inkscape:page`), or `""` for non-elements.
    pub fn qname(&self, n: NodeId) -> &str {
        match self.kind(n) {
            Kind::Element { name, .. } => name,
            _ => "",
        }
    }

    /// Local name (`page` for `inkscape:page`), or `""` for non-elements.
    pub fn tag(&self, n: NodeId) -> &str {
        let q = self.qname(n);
        q.rsplit(':').next().unwrap_or(q)
    }

    pub fn attrs(&self, n: NodeId) -> &[Attr] {
        match self.kind(n) {
            Kind::Element { attrs, .. } => attrs,
            _ => &[],
        }
    }

    pub fn attr(&self, n: NodeId, name: &str) -> Option<&str> {
        self.attrs(n).iter().find(|a| a.name == name).map(|a| a.value.as_str())
    }

    pub fn by_id(&self, id: &str) -> Option<NodeId> {
        self.ids.get(id).copied()
    }

    pub fn children(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut c = self.nodes[n as usize].first;
        std::iter::from_fn(move || {
            let cur = c?;
            c = self.nodes[cur as usize].next;
            Some(cur)
        })
    }

    /// Pre-order traversal including `n` itself. Iterative, so depth is unbounded.
    pub fn descendants(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut stack = vec![n];
        std::iter::from_fn(move || {
            let cur = stack.pop()?;
            let mut c = self.nodes[cur as usize].last;
            while let Some(k) = c {
                stack.push(k);
                c = self.nodes[k as usize].prev;
            }
            Some(cur)
        })
    }

    /// Content of a Text/CData node.
    pub fn text(&self, n: NodeId) -> Option<&str> {
        match self.kind(n) {
            Kind::Text(t) | Kind::CData(t) => Some(t),
            _ => None,
        }
    }

    /// Concatenated text of all Text/CData descendants (used for `<style>` sheets).
    pub fn text_content(&self, n: NodeId) -> String {
        let mut s = String::new();
        for d in self.descendants(n) {
            if let Some(t) = self.text(d) {
                s.push_str(t);
            }
        }
        s
    }
}

/// Splits quick-xml's raw attribute string (everything after the tag name) into
/// attributes, remembering the whitespace before each one and after the last.
fn parse_attributes(raw: &str) -> Result<(Vec<Attr>, String), DomError> {
    let mut attrs = Vec::new();
    let mut rest = raw;
    loop {
        let trimmed = rest.trim_start();
        let ws = &rest[..rest.len() - trimmed.len()];
        if trimmed.is_empty() {
            return Ok((attrs, ws.to_string()));
        }
        let name_end = trimmed
            .find(|c: char| c == '=' || c.is_whitespace())
            .ok_or_else(|| DomError::Xml(format!("attribute without value: {trimmed}")))?;
        let name = &trimmed[..name_end];
        let after_eq = trimmed[name_end..]
            .trim_start()
            .strip_prefix('=')
            .ok_or_else(|| DomError::Xml(format!("attribute '{name}' has no '='")))?
            .trim_start();
        let quote = after_eq
            .chars()
            .next()
            .filter(|c| *c == '"' || *c == '\'')
            .ok_or_else(|| DomError::Xml(format!("attribute '{name}' value is not quoted")))?;
        let body = &after_eq[1..];
        let end = body
            .find(quote)
            .ok_or_else(|| DomError::Xml(format!("unterminated value for attribute '{name}'")))?;
        let value = quick_xml::escape::unescape(&body[..end])
            .map_err(|e| DomError::Unsupported(format!("attribute '{name}': {e}")))?
            .into_owned();
        attrs.push(Attr { name: name.to_string(), value, ws: ws.to_string() });
        rest = &body[end + 1..];
    }
}

fn escape_text(s: &str, out: &mut Vec<u8>) {
    for b in s.bytes() {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            _ => out.push(b),
        }
    }
}

fn escape_attr(s: &str, out: &mut Vec<u8>) {
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
```

Add to `src/lib.rs`: `pub mod dom;`

- [ ] **Step 5: Run tests**

Run: `cargo test --test dom`
Expected: PASS (14 tests). If `upstream_fixtures_round_trip_byte_identically` fails on a fixture for a benign reason (e.g. a `&#x2212;` char ref written literally, or DOCTYPE spacing), inspect the printed byte window; if the difference is benign add the file name to `KNOWN_DIFFERENT` with a `//` reason comment, otherwise fix the writer. `edge_document_round_trips_byte_identically` must pass as is.

- [ ] **Step 6: Commit**

```bash
git add src/dom.rs src/lib.rs tests/support/mod.rs tests/dom.rs
git commit -m "feat(dom): lossless arena DOM parse/write over quick-xml

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: DOM navigation, mutation, id index

**Files:**
- Modify: `src/dom.rs` (add the API below inside `impl Doc`)
- Modify: `tests/dom.rs` (append tests)

**Interfaces:**
- Produces (all on `Doc`): `parent, first_child, last_child, next_sibling, prev_sibling, ancestors, is_text, is_comment, set_attr, remove_attr, href, set_text, tail, ensure_id, new_element, new_text, new_comment, deep_clone, append_child, prepend_child, insert_before, insert_after, detach, replace, defs, selection, element_count` plus `pub(crate) fn bump(&self)`.
- Contract: every mutation bumps `generation`; mutations touching a `<style>` element (attach/detach of a subtree containing one, or `set_text` on its text) also bump `sheet_generation`. Attach operations move a node that is already attached (they detach it first). Ids follow nodes: detach removes the subtree's ids from the index, attach re-adds them (first-wins). `deep_clone` drops `id` attributes in the copy.

- [ ] **Step 1: Write the failing tests** (append to `tests/dom.rs`)

```rust
const DOC: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\">\
<defs id=\"d\"/><g id=\"g\"><path id=\"p\" d=\"M 0,0\"/>tail<use id=\"u\" xlink:href=\"#p\"/></g><text id=\"t\">hi</text><style id=\"s\">g{fill:red}</style></svg>";

fn parse(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}

fn out(doc: &Doc) -> String {
    let mut v = Vec::new();
    doc.write(&mut v);
    String::from_utf8(v).unwrap()
}

#[test]
fn navigation_basics() {
    let d = parse(DOC);
    let g = d.by_id("g").unwrap();
    let p = d.by_id("p").unwrap();
    let u = d.by_id("u").unwrap();
    assert_eq!(d.parent(p), Some(g));
    assert_eq!(d.first_child(g), Some(p));
    assert_eq!(d.last_child(g), Some(u));
    let tail = d.tail(p).unwrap();
    assert_eq!(d.text(tail), Some("tail"));
    assert_eq!(d.next_sibling(tail), Some(u));
    assert_eq!(d.prev_sibling(u), Some(tail));
    assert_eq!(d.tail(u), None);
    assert_eq!(d.href(u), Some("#p"));
    assert_eq!(d.ancestors(p).collect::<Vec<_>>(), vec![g, d.svg(), d.root()]);
    let tags: Vec<&str> = d.descendants(d.svg()).filter(|&n| d.is_element(n)).map(|n| d.tag(n)).collect();
    assert_eq!(tags, vec!["svg", "defs", "g", "path", "use", "text", "style"]);
    assert_eq!(d.element_count(), 6);
}

#[test]
fn set_and_remove_attr_bump_generation_and_keep_id_index() {
    let mut d = parse(DOC);
    let p = d.by_id("p").unwrap();
    let before = out(&d);
    d.set_attr(p, "d", "M 1,1");
    d.set_attr(p, "stroke", "red");
    assert_eq!(d.attr(p, "d"), Some("M 1,1"));
    assert!(out(&d).contains("<path id=\"p\" d=\"M 1,1\" stroke=\"red\"/>"), "{}", out(&d));
    assert_ne!(out(&d), before);
    d.set_attr(p, "id", "p2");
    assert_eq!(d.by_id("p2"), Some(p));
    assert_eq!(d.by_id("p"), None);
    assert_eq!(d.remove_attr(p, "stroke"), Some("red".to_string()));
    assert_eq!(d.remove_attr(p, "stroke"), None);
    d.remove_attr(p, "id");
    assert_eq!(d.by_id("p2"), None);
}

#[test]
fn detach_and_reattach_moves_ids_with_the_subtree() {
    let mut d = parse(DOC);
    let g = d.by_id("g").unwrap();
    let t = d.by_id("t").unwrap();
    d.detach(g);
    assert_eq!(d.by_id("p"), None);
    assert_eq!(d.parent(g), None);
    assert!(!out(&d).contains("<g"));
    d.insert_after(g, t);
    assert!(d.by_id("p").is_some());
    let order: Vec<&str> = d.children(d.svg()).map(|n| d.tag(n)).collect();
    assert_eq!(order, vec!["defs", "text", "g", "style"]);
    d.prepend_child(d.svg(), g);
    let order: Vec<&str> = d.children(d.svg()).map(|n| d.tag(n)).collect();
    assert_eq!(order, vec!["g", "defs", "text", "style"]);
    d.insert_before(t, g);
    let order: Vec<&str> = d.children(d.svg()).map(|n| d.tag(n)).collect();
    assert_eq!(order, vec!["text", "g", "defs", "style"]);
}

#[test]
fn append_child_moves_an_attached_node() {
    let mut d = parse(DOC);
    let g = d.by_id("g").unwrap();
    let t = d.by_id("t").unwrap();
    d.append_child(g, t);
    assert_eq!(d.parent(t), Some(g));
    assert_eq!(d.last_child(g), Some(t));
    assert_eq!(d.children(d.svg()).count(), 3);
}

#[test]
fn replace_swaps_nodes_in_place() {
    let mut d = parse(DOC);
    let p = d.by_id("p").unwrap();
    let r = d.new_element("rect");
    d.set_attr(r, "id", "r");
    d.replace(p, r);
    assert_eq!(d.parent(p), None);
    assert_eq!(d.first_child(d.by_id("g").unwrap()), Some(r));
    assert_eq!(d.by_id("p"), None);
    assert!(out(&d).contains("<g id=\"g\"><rect id=\"r\"/>tail<use"), "{}", out(&d));
}

#[test]
fn deep_clone_copies_subtree_without_ids() {
    let mut d = parse(DOC);
    let g = d.by_id("g").unwrap();
    let c = d.deep_clone(g);
    assert_eq!(d.parent(c), None);
    assert_eq!(d.attr(c, "id"), None);
    let kinds: Vec<String> = d.descendants(c).map(|n| if d.is_element(n) { d.tag(n).to_string() } else { format!("#{}", d.text(n).unwrap_or("")) }).collect();
    assert_eq!(kinds, vec!["g", "path", "#tail", "use"]);
    assert_eq!(d.attr(d.first_child(c).unwrap(), "d"), Some("M 0,0"));
    assert!(d.by_id("g").is_some(), "original ids untouched");
    d.append_child(d.svg(), c);
    assert_eq!(d.children(d.svg()).count(), 5);
}

#[test]
fn ensure_id_generates_unique_deterministic_ids() {
    let mut d = parse("<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"sciink-1\"/><g/><g/></svg>");
    let kids: Vec<_> = d.children(d.svg()).collect();
    assert_eq!(d.ensure_id(kids[0]), "sciink-1");
    assert_eq!(d.ensure_id(kids[1]), "sciink-2");
    assert_eq!(d.ensure_id(kids[2]), "sciink-3");
    assert_eq!(d.by_id("sciink-3"), Some(kids[2]));
}

#[test]
fn set_text_and_new_nodes() {
    let mut d = parse(DOC);
    let t = d.by_id("t").unwrap();
    let txt = d.first_child(t).unwrap();
    assert!(d.is_text(txt));
    d.set_text(txt, "a < b");
    assert!(out(&d).contains("<text id=\"t\">a &lt; b</text>"));
    let c = d.new_comment(" note ");
    assert!(d.is_comment(c));
    d.append_child(t, c);
    let n = d.new_text("x");
    d.append_child(t, n);
    assert!(out(&d).contains("<text id=\"t\">a &lt; b<!-- note -->x</text>"));
}

#[test]
fn defs_is_found_or_created_first() {
    let mut d = parse(DOC);
    assert_eq!(d.defs(), d.by_id("d").unwrap());
    let mut e = parse("<svg xmlns=\"http://www.w3.org/2000/svg\"><g/></svg>");
    let defs = e.defs();
    assert_eq!(e.tag(defs), "defs");
    assert_eq!(e.first_child(e.svg()), Some(defs));
    assert_eq!(e.defs(), defs);
}

#[test]
fn selection_is_in_document_order_and_ignores_unknown_ids() {
    let d = parse(DOC);
    let sel = d.selection(&["t".to_string(), "nope".to_string(), "p".to_string(), "g".to_string()]);
    let tags: Vec<&str> = sel.iter().map(|&n| d.tag(n)).collect();
    assert_eq!(tags, vec!["g", "path", "text"]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test dom`
Expected: FAIL to compile — missing methods (`parent`, `first_child`, …).

- [ ] **Step 3: Implement** (add inside `impl Doc` in `src/dom.rs`)

```rust
    pub fn parent(&self, n: NodeId) -> Option<NodeId> {
        self.nodes[n as usize].parent
    }

    pub fn first_child(&self, n: NodeId) -> Option<NodeId> {
        self.nodes[n as usize].first
    }

    pub fn last_child(&self, n: NodeId) -> Option<NodeId> {
        self.nodes[n as usize].last
    }

    pub fn next_sibling(&self, n: NodeId) -> Option<NodeId> {
        self.nodes[n as usize].next
    }

    pub fn prev_sibling(&self, n: NodeId) -> Option<NodeId> {
        self.nodes[n as usize].prev
    }

    /// Parent chain, nearest first, ending with the Document node.
    pub fn ancestors(&self, n: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut c = self.nodes[n as usize].parent;
        std::iter::from_fn(move || {
            let cur = c?;
            c = self.nodes[cur as usize].parent;
            Some(cur)
        })
    }

    pub fn is_text(&self, n: NodeId) -> bool {
        matches!(self.kind(n), Kind::Text(_) | Kind::CData(_))
    }

    pub fn is_comment(&self, n: NodeId) -> bool {
        matches!(self.kind(n), Kind::Comment(_))
    }

    /// Number of elements below the root `<svg>` (excluding it).
    pub fn element_count(&self) -> usize {
        self.descendants(self.svg).skip(1).filter(|&n| self.is_element(n)).count()
    }

    pub fn set_attr(&mut self, n: NodeId, name: &str, value: impl Into<String>) {
        let value = value.into();
        if name == "id" {
            if let Some(old) = self.attr(n, "id").map(str::to_string) {
                if self.ids.get(&old) == Some(&n) {
                    self.ids.remove(&old);
                }
            }
            self.ids.insert(value.clone(), n);
        }
        if let Kind::Element { attrs, .. } = &mut self.nodes[n as usize].kind {
            match attrs.iter_mut().find(|a| a.name == name) {
                Some(a) => a.value = value,
                None => attrs.push(Attr { name: name.to_string(), value, ws: " ".to_string() }),
            }
        }
        self.bump();
    }

    pub fn remove_attr(&mut self, n: NodeId, name: &str) -> Option<String> {
        if name == "id" {
            if let Some(old) = self.attr(n, "id").map(str::to_string) {
                if self.ids.get(&old) == Some(&n) {
                    self.ids.remove(&old);
                }
            }
        }
        let Kind::Element { attrs, .. } = &mut self.nodes[n as usize].kind else {
            return None;
        };
        let i = attrs.iter().position(|a| a.name == name)?;
        let removed = attrs.remove(i).value;
        self.bump();
        Some(removed)
    }

    /// `xlink:href` or SVG 2 `href`.
    pub fn href(&self, n: NodeId) -> Option<&str> {
        self.attr(n, "xlink:href").or_else(|| self.attr(n, "href"))
    }

    pub fn set_text(&mut self, n: NodeId, s: &str) {
        match &mut self.nodes[n as usize].kind {
            Kind::Text(t) | Kind::CData(t) => *t = s.to_string(),
            _ => return,
        }
        if self.parent(n).is_some_and(|p| self.tag(p) == "style") {
            self.bump_sheet();
        }
        self.bump();
    }

    /// The Text node directly following `n` (lxml's `.tail`), if any.
    pub fn tail(&self, n: NodeId) -> Option<NodeId> {
        let nx = self.next_sibling(n)?;
        matches!(self.kind(nx), Kind::Text(_)).then_some(nx)
    }

    /// Returns the element's id, assigning `sciink-N` if it has none.
    pub fn ensure_id(&mut self, n: NodeId) -> String {
        if let Some(id) = self.attr(n, "id") {
            return id.to_string();
        }
        loop {
            let cand = format!("sciink-{}", self.next_auto_id);
            self.next_auto_id += 1;
            if !self.ids.contains_key(&cand) {
                self.set_attr(n, "id", cand.clone());
                return cand;
            }
        }
    }

    /// New detached element written as `<name/>` until children are added.
    pub fn new_element(&mut self, name: &str) -> NodeId {
        self.alloc(Kind::Element { name: name.to_string(), attrs: Vec::new(), self_closing: true, close_ws: String::new() })
    }

    pub fn new_text(&mut self, s: &str) -> NodeId {
        self.alloc(Kind::Text(s.to_string()))
    }

    pub fn new_comment(&mut self, s: &str) -> NodeId {
        self.alloc(Kind::Comment(s.to_string()))
    }

    /// Detached copy of `n` and its subtree; `id` attributes are dropped.
    pub fn deep_clone(&mut self, n: NodeId) -> NodeId {
        fn copy_kind(k: &Kind) -> Kind {
            match k {
                Kind::Element { name, attrs, self_closing, close_ws } => Kind::Element {
                    name: name.clone(),
                    attrs: attrs.iter().filter(|a| a.name != "id").cloned().collect(),
                    self_closing: *self_closing,
                    close_ws: close_ws.clone(),
                },
                other => other.clone(),
            }
        }
        let root_copy = self.alloc(copy_kind(&self.nodes[n as usize].kind));
        let mut stack: Vec<(NodeId, NodeId)> = vec![(n, root_copy)];
        while let Some((src, dst)) = stack.pop() {
            let kids: Vec<NodeId> = self.children(src).collect();
            for k in kids {
                let kc = self.alloc(copy_kind(&self.nodes[k as usize].kind));
                self.link_last(dst, kc);
                stack.push((k, kc));
            }
        }
        root_copy
    }

    /// Unlinks `n` from its parent (keeping its subtree). No-op if detached.
    pub fn detach(&mut self, n: NodeId) {
        let Some(p) = self.nodes[n as usize].parent else { return };
        let (prev, next) = (self.nodes[n as usize].prev, self.nodes[n as usize].next);
        match prev {
            Some(x) => self.nodes[x as usize].next = next,
            None => self.nodes[p as usize].first = next,
        }
        match next {
            Some(x) => self.nodes[x as usize].prev = prev,
            None => self.nodes[p as usize].last = prev,
        }
        {
            let node = &mut self.nodes[n as usize];
            node.parent = None;
            node.prev = None;
            node.next = None;
        }
        self.unindex_subtree(n);
        if self.subtree_has_style(n) {
            self.bump_sheet();
        }
        self.bump();
    }

    pub fn append_child(&mut self, parent: NodeId, n: NodeId) {
        debug_assert!(parent != n && !self.ancestors(parent).any(|a| a == n), "cannot append an ancestor");
        self.detach(n);
        self.link_last(parent, n);
        self.after_attach(n);
    }

    pub fn prepend_child(&mut self, parent: NodeId, n: NodeId) {
        match self.nodes[parent as usize].first {
            Some(f) if f != n => self.insert_before(n, f),
            Some(_) => {}
            None => self.append_child(parent, n),
        }
    }

    pub fn insert_before(&mut self, n: NodeId, anchor: NodeId) {
        debug_assert!(n != anchor, "cannot insert a node before itself");
        self.detach(n);
        let p = self.nodes[anchor as usize].parent.expect("anchor must be attached");
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
        self.after_attach(n);
    }

    pub fn insert_after(&mut self, n: NodeId, anchor: NodeId) {
        debug_assert!(n != anchor, "cannot insert a node after itself");
        match self.nodes[anchor as usize].next {
            Some(nx) if nx != n => self.insert_before(n, nx),
            Some(_) => {}
            None => {
                let p = self.nodes[anchor as usize].parent.expect("anchor must be attached");
                self.append_child(p, n);
            }
        }
    }

    /// Puts `new` where `old` is and detaches `old`.
    pub fn replace(&mut self, old: NodeId, new: NodeId) {
        self.insert_before(new, old);
        self.detach(old);
    }

    /// First direct `<defs>` child of the root, created (and prepended) if absent.
    pub fn defs(&mut self) -> NodeId {
        if let Some(d) = self.children(self.svg).find(|&c| self.is_element(c) && self.tag(c) == "defs") {
            return d;
        }
        let d = self.new_element("defs");
        self.prepend_child(self.svg, d);
        d
    }

    /// Nodes for the given ids in document order; unknown ids are dropped.
    pub fn selection(&self, ids: &[String]) -> Vec<NodeId> {
        let wanted: std::collections::HashSet<NodeId> = ids.iter().filter_map(|i| self.by_id(i)).collect();
        self.descendants(self.svg).filter(|n| wanted.contains(n)).collect()
    }

    pub(crate) fn bump(&self) {
        self.generation.set(self.generation.get() + 1);
    }

    fn bump_sheet(&self) {
        self.sheet_generation.set(self.sheet_generation.get() + 1);
    }

    fn after_attach(&mut self, n: NodeId) {
        self.index_subtree(n);
        if self.subtree_has_style(n) {
            self.bump_sheet();
        }
        self.bump();
    }

    fn index_subtree(&mut self, n: NodeId) {
        let ids: Vec<(String, NodeId)> =
            self.descendants(n).filter_map(|d| self.attr(d, "id").map(|id| (id.to_string(), d))).collect();
        for (id, d) in ids {
            self.ids.entry(id).or_insert(d);
        }
    }

    fn unindex_subtree(&mut self, n: NodeId) {
        let ids: Vec<(String, NodeId)> =
            self.descendants(n).filter_map(|d| self.attr(d, "id").map(|id| (id.to_string(), d))).collect();
        for (id, d) in ids {
            if self.ids.get(&id) == Some(&d) {
                self.ids.remove(&id);
            }
        }
    }

    fn subtree_has_style(&self, n: NodeId) -> bool {
        self.descendants(n).any(|d| self.is_element(d) && self.tag(d) == "style")
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test dom`
Expected: PASS (24 tests).

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings`
Expected: no warnings (fix any; typical ones are `needless_return` or `collapsible_if`).

```bash
git add src/dom.rs tests/dom.rs
git commit -m "feat(dom): navigation, mutation and id index

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: CLI, process boundary, and the `about` tool

**Files:**
- Create: `src/cli.rs`, `src/log.rs`, `src/paths.rs`, `src/tools/mod.rs`, `src/tools/about.rs`, `tests/data/edge/simple.svg`
- Modify: `src/lib.rs` (modules, `Output`, `run`), `src/main.rs` (full process boundary), `tests/cli.rs` (append tests)

**Interfaces:**
- Produces: `sciink::Output { svg: Vec<u8>, messages: Vec<String> }`; `sciink::run(argv: &[OsString], input: &[u8]) -> Result<Output, String>`; `cli::{ToolName, Common, inx_bool, prescan, Prescan, HELP}`; `log::{init, line}`; `paths::{exe_path, inx_dir, target_triple}`; `tools::about::run`.
- Contract for later tool tasks: a tool is `pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String>` in `src/tools/<name>.rs`, parsing its own `#[derive(clap::Parser)]` struct that `#[command(flatten)]`s `Common`; it is registered in `sciink::run`'s match. Messages for the user go into `Output::messages` (shown as a dialog); never print to stdout/stderr from a tool.

- [ ] **Step 1: Create the fixture**

`tests/data/edge/simple.svg`:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="50" viewBox="0 0 100 50" id="layer0">
  <g id="layer1">
    <path id="p" d="M 10,10 L 90,10" style="stroke:#000000;stroke-width:1;fill:none"/>
    <text id="t" x="10" y="30" style="font-size:10px;font-family:sans-serif">hello</text>
  </g>
</svg>
```

- [ ] **Step 2: Write the failing tests** (append to `tests/cli.rs`)

```rust
use std::io::Write;
use std::path::PathBuf;

const SIMPLE: &str = include_str!("data/edge/simple.svg");

fn tmp(name: &str, content: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("sciink-test-{}-{name}", std::process::id()));
    std::fs::write(&p, content).unwrap();
    p
}

#[test]
fn about_echoes_document_and_reports_on_stderr() {
    let p = tmp("about.svg", SIMPLE);
    let out = bin().args(["--tool=about", "--id=p"]).arg(&p).output().unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, SIMPLE.as_bytes());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("sciink 0.1.0"), "{err}");
    assert!(err.contains("document: 3 elements (1 text, 1 path)"), "{err}");
    assert!(err.contains("selection: 1 object(s)"), "{err}");
}

#[test]
fn unknown_argument_echoes_input_with_message() {
    let p = tmp("badarg.svg", SIMPLE);
    let out = bin().args(["--tool=about", "--bogus=1"]).arg(&p).output().unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, SIMPLE.as_bytes());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("sciink about:"), "{err}");
    assert!(err.contains("The document was left unchanged."), "{err}");
}

#[test]
fn malformed_xml_echoes_input() {
    let bad = "<svg><g></svg>";
    let p = tmp("bad.svg", bad);
    let out = bin().arg("--tool=about").arg(&p).output().unwrap();
    assert_eq!(out.stdout, bad.as_bytes());
    assert!(String::from_utf8_lossy(&out.stderr).contains("The document was left unchanged."));
}

#[test]
fn missing_tool_echoes_input() {
    let p = tmp("notool.svg", SIMPLE);
    let out = bin().arg(&p).output().unwrap();
    assert_eq!(out.stdout, SIMPLE.as_bytes());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--tool"));
}

#[test]
fn unimplemented_tool_echoes_input() {
    let p = tmp("scaler.svg", SIMPLE);
    let out = bin().args(["--tool=scaler", "--tab=correction"]).arg(&p).output().unwrap();
    assert_eq!(out.stdout, SIMPLE.as_bytes());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not implemented"));
}

#[test]
fn output_flag_writes_file_and_keeps_stdout_empty() {
    let p = tmp("in.svg", SIMPLE);
    let o = std::env::temp_dir().join(format!("sciink-test-{}-out.svg", std::process::id()));
    let _ = std::fs::remove_file(&o);
    let out = bin().arg("--tool=about").arg("--output").arg(&o).arg(&p).output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(std::fs::read_to_string(&o).unwrap(), SIMPLE);
}

#[test]
fn reads_stdin_when_no_path_is_given() {
    let mut child = bin()
        .arg("--tool=about")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(SIMPLE.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.stdout, SIMPLE.as_bytes());
}

#[test]
fn log_env_var_writes_phase_lines() {
    let p = tmp("log.svg", SIMPLE);
    let l = std::env::temp_dir().join(format!("sciink-test-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&l);
    let out = bin().arg("--tool=about").arg(&p).env("SCIINK_LOG", &l).output().unwrap();
    assert!(out.status.success());
    let log = std::fs::read_to_string(&l).unwrap();
    assert!(log.contains("tool=about phase=parse"), "{log}");
}

#[test]
fn inx_bool_accepts_inkscape_and_python_spellings() {
    use sciink::cli::inx_bool;
    assert_eq!(inx_bool("true"), Ok(true));
    assert_eq!(inx_bool("True"), Ok(true));
    assert_eq!(inx_bool("1"), Ok(true));
    assert_eq!(inx_bool("false"), Ok(false));
    assert_eq!(inx_bool("0"), Ok(false));
    assert!(inx_bool("maybe").is_err());
}

#[test]
fn prescan_finds_tool_input_and_output_in_both_forms() {
    use sciink::cli::prescan;
    use std::ffi::OsString;
    let argv: Vec<OsString> = ["sciink", "--tool=about", "--deepungroup=true", "-o", "out.svg", "--id=a", "in.svg"].iter().map(OsString::from).collect();
    let p = prescan(&argv);
    assert_eq!(p.tool.as_deref(), Some("about"));
    assert_eq!(p.input.as_deref().and_then(|x| x.to_str()), Some("in.svg"));
    assert_eq!(p.output.as_deref().and_then(|x| x.to_str()), Some("out.svg"));
    let argv: Vec<OsString> = ["sciink", "--tool", "scaler", "--output=o.svg", "in.svg"].iter().map(OsString::from).collect();
    let p = prescan(&argv);
    assert_eq!(p.tool.as_deref(), Some("scaler"));
    assert_eq!(p.output.as_deref().and_then(|x| x.to_str()), Some("o.svg"));
    assert_eq!(p.input.as_deref().and_then(|x| x.to_str()), Some("in.svg"));
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test cli`
Expected: FAIL to compile — `sciink::cli` missing.

- [ ] **Step 4: Implement**

`src/cli.rs`:
```rust
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
    let args: Vec<String> = argv.iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
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
Set SCIINK_LOG=<file> to append timing information.";
```

`src/log.rs`:
```rust
//! Optional append-only log file (`SCIINK_LOG`). Never writes to stderr:
//! stderr is the dialog Inkscape shows the user.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static LOG: OnceLock<Mutex<Option<File>>> = OnceLock::new();

pub fn init(path: Option<&Path>) {
    let file = path.and_then(|p| OpenOptions::new().create(true).append(true).open(p).ok());
    let _ = LOG.set(Mutex::new(file));
}

pub fn line(msg: &str) {
    if let Some(m) = LOG.get() {
        if let Ok(mut guard) = m.lock() {
            if let Some(f) = guard.as_mut() {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let _ = writeln!(f, "t={secs} {msg}");
            }
        }
    }
}
```

`src/paths.rs`:
```rust
//! Where things are. Inkscape does not chdir for directly executed binaries,
//! so everything is located relative to the executable: `<inx dir>/bin/sciink`.

use std::path::PathBuf;

pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("sciink"))
}

/// The extension folder holding the `.inx` files (parent of `bin/`).
pub fn inx_dir() -> PathBuf {
    exe_path()
        .parent()
        .and_then(|bin| bin.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn target_triple() -> String {
    option_env!("SCIINK_TARGET")
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS))
}
```

`src/tools/mod.rs`:
```rust
//! One module per Inkscape menu entry. Each exposes
//! `pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String>`.

pub mod about;
```

`src/tools/about.rs`:
```rust
//! Diagnostics: proves the Inkscape ↔ binary protocol works and reports what
//! the binary sees. Echoes the document unchanged.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::time::Instant;

use clap::Parser;

use crate::cli::Common;
use crate::dom::Doc;
use crate::Output;

#[derive(Parser, Debug)]
#[command(name = "sciink", disable_help_flag = true, disable_version_flag = true)]
pub struct AboutCli {
    #[command(flatten)]
    pub common: Common,
}

pub fn run(argv: &[OsString], input: &[u8]) -> Result<Output, String> {
    let cli = AboutCli::try_parse_from(argv).map_err(|e| e.to_string())?;
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
    let mut r = String::new();
    let _ = writeln!(r, "sciink {} — {}", crate::version(), crate::paths::target_triple());
    let _ = writeln!(r, "executable: {}", crate::paths::exe_path().display());
    let _ = writeln!(r, "extension dir: {}", crate::paths::inx_dir().display());
    let _ = writeln!(r, "document: {elements} elements ({texts} text, {paths} path), parsed in {parse_ms:.1} ms");
    let _ = writeln!(r, "selection: {} object(s)", cli.common.ids.len());
    crate::log::line(&format!("tool=about phase=parse ms={parse_ms:.1} elements={elements}"));
    Ok(Output { svg: input.to_vec(), messages: vec![r.trim_end().to_string()] })
}
```

`src/lib.rs` (replace whole file):
```rust
//! sciink — Inkscape extensions for scientific figures, as one fast binary.
//! Inkscape launches `sciink --tool=<name> --param=value… --id=… input.svg`
//! and reads the modified SVG from stdout (see docs/spec/03-infrastructure.md §C.3).

pub mod cli;
pub mod dom;
pub mod log;
pub mod num;
pub mod paths;
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
        "flattener" | "scaler" | "homogenizer" | "text-ghoster" | "combine-by-color" | "favorite-markers" => {
            Err(format!("the {tool} tool is not implemented yet"))
        }
        other => Err(format!("unknown tool '{other}'")),
    }
}
```

`src/main.rs` (replace whole file):
```rust
//! Process boundary. Everything that can fail is caught here; on failure the
//! original document is echoed so Inkscape never loses the user's work.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;

static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

fn main() {
    let argv: Vec<OsString> = std::env::args_os().collect();
    if argv.iter().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("sciink {}", sciink::version());
        return;
    }
    if argv.iter().skip(1).any(|a| a == "--help" || a == "-h") {
        println!("{}", sciink::cli::HELP);
        return;
    }
    let pre = sciink::cli::prescan(&argv);
    sciink::log::init(pre.log.as_deref().or(std::env::var_os("SCIINK_LOG").map(std::path::PathBuf::from).as_deref()));
    let input = match read_input(pre.input.as_deref()) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("sciink: cannot read input: {e}");
            std::process::exit(1);
        }
    };
    let tool = pre.tool.clone().unwrap_or_else(|| "?".to_string());
    std::panic::set_hook(Box::new(|info| {
        if let Ok(mut g) = LAST_PANIC.lock() {
            *g = Some(info.to_string());
        }
    }));
    let result = std::panic::catch_unwind(|| sciink::run(&argv, &input));
    let (bytes, messages) = match result {
        Ok(Ok(out)) => (out.svg, out.messages),
        Ok(Err(e)) => (input.clone(), vec![format!("sciink {tool}: {e}\nThe document was left unchanged.")]),
        Err(_) => {
            let msg = LAST_PANIC.lock().ok().and_then(|g| g.clone()).unwrap_or_else(|| "unknown panic".to_string());
            sciink::log::line(&format!("tool={tool} panic={msg}"));
            (
                input.clone(),
                vec![format!(
                    "sciink {tool}: internal error (a bug): {msg}\nThe document was left unchanged. Set SCIINK_LOG=<file> and report the log."
                )],
            )
        }
    };
    for m in &messages {
        eprintln!("{m}");
    }
    let output = pre.output.as_deref().filter(|p| p.as_os_str() != "-");
    if let Err(e) = write_output(&bytes, output) {
        eprintln!("sciink: cannot write output: {e}");
        std::process::exit(1);
    }
}

fn read_input(path: Option<&Path>) -> std::io::Result<Vec<u8>> {
    match path {
        Some(p) => std::fs::read(p),
        None => {
            let mut buf = Vec::new();
            std::io::stdin().lock().read_to_end(&mut buf)?;
            Ok(buf)
        }
    }
}

fn write_output(bytes: &[u8], path: Option<&Path>) -> std::io::Result<()> {
    match path {
        Some(p) => {
            let tmp = p.with_extension("sciink-tmp");
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, p)
        }
        None => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            match lock.write_all(bytes).and_then(|_| lock.flush()) {
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
                other => other,
            }
        }
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --test cli`
Expected: PASS (11 tests). Common pitfalls: `Common` must derive `clap::Args` (not `Parser`); the `about` message must contain exactly `document: 3 elements (1 text, 1 path)` for `simple.svg` (`g`, `path`, `text` below the root).

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: clean, all tests pass.

```bash
git add src tests/cli.rs tests/data/edge/simple.svg
git commit -m "feat(cli): Inkscape process contract with echo-on-failure and about tool

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: `about.inx`, dev install, and the real Inkscape smoke test

**Files:**
- Create: `inx/about.inx`, `dist/dev-install.sh`, `README.md`

**Interfaces:**
- Produces: the `.inx` skeleton every later tool copies (`<param name="tool" gui-hidden="true">`, `<command location="inx">bin/sciink</command>`); `dist/dev-install.sh` for the local loop.

- [ ] **Step 1: Write the files**

`inx/about.inx`:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<inkscape-extension xmlns="http://www.inkscape.org/namespace/inkscape/extension">
    <name>Diagnostics (sciink)</name>
    <id>org.sciink.about</id>
    <param name="tool" type="string" gui-hidden="true">about</param>
    <effect needs-live-preview="false">
        <object-type>all</object-type>
        <effects-menu>
            <submenu name="Scientific"/>
        </effects-menu>
    </effect>
    <script>
        <command location="inx">bin/sciink</command>
    </script>
</inkscape-extension>
```

`dist/dev-install.sh`:
```bash
#!/usr/bin/env bash
# Build the release binary and symlink it plus the .inx files into Inkscape's
# user extension directory (macOS/Linux). Restart Inkscape afterwards; the
# binary is re-executed on every run, so later `cargo build --release` calls
# take effect immediately.
set -euo pipefail
cd "$(dirname "$0")/.."
case "$(uname -s)" in
  Darwin) EXT="$HOME/Library/Application Support/org.inkscape.Inkscape/config/inkscape/extensions/sciink" ;;
  *)      EXT="${XDG_CONFIG_HOME:-$HOME/.config}/inkscape/extensions/sciink" ;;
esac
cargo build --release
mkdir -p "$EXT/bin"
ln -sfn "$PWD/target/release/sciink" "$EXT/bin/sciink"
for f in inx/*.inx; do ln -sfn "$PWD/$f" "$EXT/$(basename "$f")"; done
echo "installed into: $EXT"
echo "restart Inkscape, then use Extensions > Scientific"
```

`README.md`:
```markdown
# sciink

Fast, dependency-free Inkscape extensions for scientific figures — a Rust rewrite of
[Scientific-Inkscape](https://github.com/burghoff/Scientific-Inkscape) (Flattener, Scaler,
Homogenizer, Text Ghoster, Combine by Color, Favorite Markers).

Status: early development. Design specs live in `docs/spec/`.

## Developing

    cargo test
    dist/dev-install.sh      # symlink into Inkscape's user extensions dir, then restart Inkscape

Set `SCIINK_LOG=/tmp/sciink.log` in Inkscape's environment to get timing lines.
```

Run: `chmod +x dist/dev-install.sh`

- [ ] **Step 2: Install and run the headless Inkscape smoke test**

Run:
```bash
dist/dev-install.sh
/Applications/Inkscape.app/Contents/MacOS/inkscape --actions="org.sciink.about.noprefs;export-type:svg;export-filename:/tmp/sciink-about-out.svg;export-do" tests/data/edge/simple.svg 2>&1 | tee /tmp/sciink-inkscape-smoke.log
test -s /tmp/sciink-about-out.svg && grep -q "sciink 0.1.0" /tmp/sciink-inkscape-smoke.log && echo SMOKE-OK
```
Expected: the log contains Inkscape's "Script Error" framing around our diagnostics text (that is how headless Inkscape prints an extension's stderr), including `sciink 0.1.0` and `document: 3 elements`, and `/tmp/sciink-about-out.svg` exists → `SMOKE-OK`. This proves Inkscape found the `.inx`, executed the binary directly (no interpreter), passed the document, and accepted stdout. If the log shows `Extension "org.sciink.about" not found`, Inkscape is not reading the user extensions dir — check the symlink target and that `bin/sciink` has the exec bit.

- [ ] **Step 3: Commit**

```bash
git add inx/about.inx dist/dev-install.sh README.md
git commit -m "feat(inx): diagnostics menu entry and dev install script

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Style declarations and the specified-style cascade

**Files:**
- Create: `src/style.rs`, `tests/style.rs`
- Modify: `src/dom.rs` (add the `caches` field), `src/lib.rs` (add `pub mod style;`)

**Interfaces:**
- Produces: `sciink::style::{Style, Stylesheet, Caches, default_value, parse_stylesheet, PRESENTATION_ATTRS}`; on `Doc`: `cascaded_style(n) -> Style`, `specified_style(n) -> Rc<Style>`, `specified(n, prop) -> Option<String>`, `computed(n, prop) -> String`, `set_style(n, prop, value)`, `set_style_map(n, &Style)`, `remove_style(n, prop)`.
- Consumes: `Doc` navigation from Task 4; `generation`/`sheet_generation` counters from Task 3.

- [ ] **Step 1: Write the failing tests**

`tests/style.rs`:
```rust
use sciink::dom::{Doc, NodeId};
use sciink::style::Style;

fn doc(s: &str) -> Doc {
    Doc::parse(s.as_bytes()).unwrap()
}

fn id(d: &Doc, i: &str) -> NodeId {
    d.by_id(i).unwrap_or_else(|| panic!("no element with id {i}"))
}

const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

#[test]
fn parse_and_serialize_declarations() {
    let s = Style::parse("fill:#fff; stroke : none ;;bad; Opacity:0.5");
    assert_eq!(
        s.0,
        vec![
            ("fill".to_string(), "#fff".to_string()),
            ("stroke".to_string(), "none".to_string()),
            ("opacity".to_string(), "0.5".to_string())
        ]
    );
    assert_eq!(s.to_css(), "fill:#fff;stroke:none;opacity:0.5");
    let mut t = s.clone();
    t.set("fill", "red");
    t.set("stroke-width", "2");
    assert_eq!(t.to_css(), "fill:red;stroke:none;opacity:0.5;stroke-width:2");
    assert_eq!(t.remove("stroke"), Some("none".to_string()));
    assert_eq!(t.get("stroke"), None);
}

#[test]
fn precedence_presentation_sheet_inline() {
    let d = doc(&format!(
        "<svg {NS}><style>rect{{fill:red}} .c{{fill:green}} #r{{fill:blue}}</style>\
         <rect id=\"r\" class=\"c\" fill=\"black\" style=\"stroke:red\"/>\
         <rect id=\"s\" class=\"c\" fill=\"black\" style=\"fill:yellow\"/>\
         <rect id=\"t\" fill=\"black\"/>\
         <rect id=\"u\" class=\"c\" fill=\"black\"/></svg>"
    ));
    assert_eq!(d.computed(id(&d, "r"), "fill"), "blue", "id rule beats class and tag");
    assert_eq!(d.computed(id(&d, "s"), "fill"), "yellow", "inline beats every rule");
    assert_eq!(d.computed(id(&d, "t"), "fill"), "red", "tag rule beats presentation attribute");
    assert_eq!(d.computed(id(&d, "u"), "fill"), "green", "class beats tag");
    assert_eq!(d.computed(id(&d, "r"), "stroke"), "red");
}

#[test]
fn important_in_sheet_beats_inline() {
    let d = doc(&format!("<svg {NS}><style>rect{{fill:red !important}}</style><rect id=\"r\" style=\"fill:blue\"/></svg>"));
    assert_eq!(d.computed(id(&d, "r"), "fill"), "red");
}

#[test]
fn specified_style_propagates_every_property_from_ancestors() {
    let d = doc(&format!("<svg {NS}><g style=\"fill:red;opacity:0.5\"><g stroke=\"blue\"><path id=\"p\" style=\"fill:green\"/></g></g></svg>"));
    let p = id(&d, "p");
    assert_eq!(d.specified(p, "fill"), Some("green".to_string()));
    assert_eq!(d.specified(p, "opacity"), Some("0.5".to_string()), "non-inherited props propagate too (upstream semantics)");
    assert_eq!(d.specified(p, "stroke"), Some("blue".to_string()));
    assert_eq!(d.specified(p, "stroke-width"), None);
    assert_eq!(d.computed(p, "stroke-width"), "1", "defaults fill the gaps");
    assert_eq!(d.computed(p, "font-size"), "medium");
    let own = d.cascaded_style(p);
    assert_eq!(own.to_css(), "fill:green", "cascaded style is the element's own declarations only");
}

#[test]
fn svglite_cdata_sheet_with_descendant_selectors() {
    let d = doc(&format!(
        "<svg {NS}><style><![CDATA[.svglite line, .svglite polyline {{ fill: none; stroke: #000000; }}\n.svglite text {{ white-space: pre; }}]]></style>\
         <g class=\"svglite\"><g><line id=\"l\"/></g><text id=\"t\"/></g><line id=\"outside\"/></svg>"
    ));
    assert_eq!(d.computed(id(&d, "l"), "stroke"), "#000000");
    assert_eq!(d.computed(id(&d, "l"), "fill"), "none");
    assert_eq!(d.computed(id(&d, "t"), "white-space"), "pre");
    assert_eq!(d.computed(id(&d, "outside"), "stroke"), "none");
}

#[test]
fn matplotlib_universal_rule_and_font_shorthand() {
    let d = doc(&format!(
        "<svg {NS}><style>*{{stroke-linecap:butt;stroke-linejoin:round;}}</style>\
         <g id=\"g\"><text id=\"t\" style=\"font: italic bold 12px/30px 'DejaVu Sans', sans-serif\">x</text></g></svg>"
    ));
    assert_eq!(d.computed(id(&d, "g"), "stroke-linecap"), "butt");
    let t = id(&d, "t");
    assert_eq!(d.specified(t, "font-style"), Some("italic".to_string()));
    assert_eq!(d.specified(t, "font-weight"), Some("bold".to_string()));
    assert_eq!(d.specified(t, "font-size"), Some("12px".to_string()));
    assert_eq!(d.specified(t, "line-height"), Some("30px".to_string()));
    assert_eq!(d.specified(t, "font-family"), Some("'DejaVu Sans', sans-serif".to_string()));
    assert_eq!(Style::parse("font: 10px 'DejaVu Sans'").get("font-family"), Some("'DejaVu Sans'"));
}

#[test]
fn child_combinator_and_unsupported_selectors() {
    let d = doc(&format!(
        "<svg {NS}><style>svg > rect{{fill:red}} a:hover{{fill:pink}} rect[x]{{fill:pink}} g rect{{stroke:blue}}</style>\
         <rect id=\"a\"/><g><rect id=\"b\"/></g></svg>"
    ));
    assert_eq!(d.computed(id(&d, "a"), "fill"), "red");
    assert_eq!(d.computed(id(&d, "b"), "fill"), "black", "child combinator does not match grandchildren");
    assert_eq!(d.computed(id(&d, "b"), "stroke"), "blue");
    assert_eq!(d.computed(id(&d, "a"), "stroke"), "none");
}

#[test]
fn set_style_moves_presentation_attribute_into_style() {
    let mut d = doc(&format!("<svg {NS}><rect id=\"r\" fill=\"black\" stroke=\"red\"/></svg>"));
    let r = id(&d, "r");
    assert_eq!(d.computed(r, "fill"), "black");
    d.set_style(r, "fill", "red");
    assert_eq!(d.attr(r, "fill"), None);
    assert_eq!(d.attr(r, "style"), Some("fill:red"));
    assert_eq!(d.computed(r, "fill"), "red", "cache invalidated by the mutation");
    d.remove_style(r, "stroke");
    assert_eq!(d.attr(r, "stroke"), None);
    assert_eq!(d.computed(r, "stroke"), "none");
    d.remove_style(r, "fill");
    assert_eq!(d.attr(r, "style"), None, "empty style attribute is removed");
    d.set_style_map(r, &Style::parse("fill:blue;opacity:0.5"));
    assert_eq!(d.attr(r, "style"), Some("fill:blue;opacity:0.5"));
}

#[test]
fn sheet_changes_invalidate_the_cache() {
    let mut d = doc(&format!("<svg {NS}><style id=\"s\">rect{{fill:red}}</style><rect id=\"r\"/></svg>"));
    let r = id(&d, "r");
    assert_eq!(d.computed(r, "fill"), "red");
    let txt = d.first_child(id(&d, "s")).unwrap();
    d.set_text(txt, "rect{fill:green}");
    assert_eq!(d.computed(r, "fill"), "green");
    let s = id(&d, "s");
    d.detach(s);
    assert_eq!(d.computed(r, "fill"), "black");
}

#[test]
fn upstream_fixture_styles_resolve() {
    // Text_tests.svg uses class sheets (`class="st38 st39"`); every text must resolve a font-family.
    let Some(dir) = std::env::var_os("SCIINK_UPSTREAM_TESTS").map(std::path::PathBuf::from).or_else(|| {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/upstream/data");
        p.join("svg").is_dir().then_some(p)
    }) else {
        eprintln!("SKIP: upstream fixtures not found");
        return;
    };
    let d = Doc::parse(&std::fs::read(dir.join("svg/Text_tests.svg")).unwrap()).unwrap();
    let mut texts = 0;
    for n in d.descendants(d.svg()) {
        if d.tag(n) == "text" {
            texts += 1;
            assert!(!d.computed(n, "font-family").is_empty());
        }
    }
    assert!(texts > 100, "expected many text elements, found {texts}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test style`
Expected: FAIL to compile — `sciink::style` missing.

- [ ] **Step 3: Add the cache slot to `Doc`**

In `src/dom.rs`: add `use std::cell::RefCell;` (extend the existing `use std::cell::Cell;` to `use std::cell::{Cell, RefCell};`), add the field

```rust
    /// Style-cascade caches (owned here so `style.rs` can keep them on the document).
    pub(crate) caches: RefCell<crate::style::Caches>,
```

to `pub struct Doc`, and initialize it in `Doc::parse`'s struct literal:

```rust
            caches: RefCell::new(crate::style::Caches::default()),
```

- [ ] **Step 4: Implement `src/style.rs`**

```rust
//! Style declarations and the "specified style" cascade (spec §C.2; upstream
//! `inkex/text/cache.py`).
//!
//! `specified_style(n)` = `specified_style(parent)` overridden by `cascaded_style(n)`,
//! where cascaded = presentation attributes < matching `<style>` rules (ordered by
//! `!important`, specificity, source order) < inline `style=""`. Every property
//! propagates down — upstream does not distinguish inherited properties and the
//! ported algorithms rely on that. Missing values come from `default_value`.

use std::collections::HashMap;
use std::rc::Rc;

use crate::dom::{Doc, NodeId};

/// Ordered `(name, value)` declarations, serialized as `k:v;k:v` (Inkscape's own format).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style(pub Vec<(String, String)>);

impl Style {
    /// Parses `k:v;k:v`. Names are lower-cased and trimmed, empty or malformed
    /// declarations are skipped, the `font` shorthand is expanded.
    pub fn parse(css: &str) -> Style {
        let mut st = Style::default();
        for decl in css.split(';') {
            let Some((k, v)) = decl.split_once(':') else { continue };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            if k.is_empty() || v.is_empty() {
                continue;
            }
            st.set_decl(&k, v);
        }
        st
    }

    fn set_decl(&mut self, k: &str, v: &str) {
        if k == "font" {
            expand_font_shorthand(self, v);
        } else {
            self.set(k, v);
        }
    }

    pub fn get(&self, k: &str) -> Option<&str> {
        self.0.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    /// Replaces in place (keeping position) or appends.
    pub fn set(&mut self, k: &str, v: &str) {
        match self.0.iter_mut().find(|(n, _)| n == k) {
            Some(e) => e.1 = v.to_string(),
            None => self.0.push((k.to_string(), v.to_string())),
        }
    }

    pub fn remove(&mut self, k: &str) -> Option<String> {
        let i = self.0.iter().position(|(n, _)| n == k)?;
        Some(self.0.remove(i).1)
    }

    /// `other`'s declarations override ours.
    pub fn merge_over(&mut self, other: &Style) {
        for (k, v) in &other.0 {
            self.set(k, v);
        }
    }

    pub fn to_css(&self) -> String {
        self.0.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(";")
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `font: [style] [variant] [weight] size[/line-height] family` (matplotlib emits
/// `font: 10px 'DejaVu Sans'` for every text when `svg.fonttype=none`).
fn expand_font_shorthand(st: &mut Style, v: &str) {
    let tokens = split_font_tokens(v);
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        match t {
            "italic" | "oblique" => st.set("font-style", t),
            "small-caps" => st.set("font-variant", t),
            "bold" | "bolder" | "lighter" | "100" | "200" | "300" | "400" | "500" | "600" | "700" | "800" | "900" => {
                st.set("font-weight", t)
            }
            "normal" => {}
            _ => break,
        }
        i += 1;
    }
    let Some(size) = tokens.get(i) else { return };
    match size.split_once('/') {
        Some((s, lh)) => {
            st.set("font-size", s);
            st.set("line-height", lh);
        }
        None => st.set("font-size", size),
    }
    let family = tokens[i + 1..].join(" ");
    if !family.is_empty() {
        st.set("font-family", &family);
    }
}

fn split_font_tokens(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in v.chars() {
        match quote {
            Some(q) => {
                cur.push(ch);
                if ch == q {
                    quote = None;
                }
            }
            None if ch == '\'' || ch == '"' => {
                quote = Some(ch);
                cur.push(ch);
            }
            None if ch.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            None => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Attributes that Inkscape treats as presentation attributes (upstream
/// `cache.py:267-332`, minus `clip`, `clip-path`, `mask`, `transform`).
pub const PRESENTATION_ATTRS: &[&str] = &[
    "alignment-baseline", "baseline-shift", "clip-rule", "color", "color-interpolation",
    "color-interpolation-filters", "color-rendering", "cursor", "direction", "display",
    "dominant-baseline", "enable-background", "fill", "fill-opacity", "fill-rule", "filter",
    "flood-color", "flood-opacity", "font", "font-family", "font-size", "font-size-adjust",
    "font-stretch", "font-style", "font-variant", "font-weight", "glyph-orientation-horizontal",
    "glyph-orientation-vertical", "image-rendering", "letter-spacing", "lighting-color", "marker",
    "marker-end", "marker-mid", "marker-start", "opacity", "overflow", "paint-order",
    "pointer-events", "shape-rendering", "stop-color", "stop-opacity", "stroke", "stroke-dasharray",
    "stroke-dashoffset", "stroke-linecap", "stroke-linejoin", "stroke-miterlimit", "stroke-opacity",
    "stroke-width", "text-anchor", "text-decoration", "text-rendering", "unicode-bidi",
    "vector-effect", "visibility", "word-spacing", "writing-mode", "line-height", "text-align",
    "white-space", "inline-size", "shape-inside", "shape-padding", "shape-subtract",
    "mix-blend-mode", "isolation", "vertical-align", "font-feature-settings",
    "font-variant-ligatures", "font-variant-caps", "font-variant-numeric", "font-variant-east-asian",
    "font-variant-position", "font-variant-alternates", "font-kerning",
];

/// SVG initial values (spec §C.2; cross-checked with Inkscape's `inkex/properties.py`).
const DEFAULTS: &[(&str, &str)] = &[
    ("fill", "black"), ("stroke", "none"), ("stroke-width", "1"), ("font-size", "medium"),
    ("font-family", "sans-serif"), ("font-weight", "normal"), ("font-style", "normal"),
    ("font-stretch", "normal"), ("font-variant", "normal"), ("text-anchor", "start"),
    ("letter-spacing", "normal"), ("word-spacing", "normal"), ("line-height", "normal"),
    ("opacity", "1"), ("fill-opacity", "1"), ("stroke-opacity", "1"), ("stroke-linecap", "butt"),
    ("stroke-linejoin", "miter"), ("stroke-miterlimit", "4"), ("stroke-dasharray", "none"),
    ("stroke-dashoffset", "0"), ("display", "inline"), ("visibility", "visible"),
    ("clip-path", "none"), ("mask", "none"), ("filter", "none"), ("marker-start", "none"),
    ("marker-mid", "none"), ("marker-end", "none"), ("direction", "ltr"), ("writing-mode", "lr-tb"),
    ("white-space", "normal"), ("stop-color", "black"), ("stop-opacity", "1"), ("color", "black"),
    ("fill-rule", "nonzero"), ("clip-rule", "nonzero"), ("text-decoration", "none"),
    ("baseline-shift", "baseline"), ("text-align", "start"), ("font-variant-ligatures", "normal"),
    ("paint-order", "normal"), ("vector-effect", "none"), ("shape-rendering", "auto"),
    ("text-rendering", "auto"),
];

pub fn default_value(prop: &str) -> Option<&'static str> {
    DEFAULTS.iter().find(|(k, _)| *k == prop).map(|(_, v)| *v)
}

// ---------------------------------------------------------------------------
// Stylesheets: the selector subset exporters actually use (`*`, tag, .class,
// #id, compounds, comma lists, descendant and child combinators).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
}

impl Compound {
    fn matches(&self, doc: &Doc, n: NodeId) -> bool {
        if let Some(t) = &self.tag {
            if doc.tag(n) != t {
                return false;
            }
        }
        if let Some(id) = &self.id {
            if doc.attr(n, "id") != Some(id.as_str()) {
                return false;
            }
        }
        if !self.classes.is_empty() {
            let cls = doc.attr(n, "class").unwrap_or("");
            for c in &self.classes {
                if !cls.split_ascii_whitespace().any(|x| x == c) {
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
}

#[derive(Debug, Clone)]
struct Selector {
    /// `parts[k].1` relates `parts[k]` to `parts[k-1]`; the first is `None`.
    parts: Vec<(Compound, Option<Combinator>)>,
    specificity: (u32, u32, u32),
}

impl Selector {
    fn matches(&self, doc: &Doc, n: NodeId) -> bool {
        self.match_from(doc, n, self.parts.len() - 1)
    }

    fn match_from(&self, doc: &Doc, n: NodeId, idx: usize) -> bool {
        let (comp, comb) = &self.parts[idx];
        if !comp.matches(doc, n) {
            return false;
        }
        if idx == 0 {
            return true;
        }
        match comb {
            Some(Combinator::Descendant) => {
                let mut a = doc.parent(n);
                while let Some(p) = a {
                    if doc.is_element(p) && self.match_from(doc, p, idx - 1) {
                        return true;
                    }
                    a = doc.parent(p);
                }
                false
            }
            _ => {
                let Some(p) = doc.parent(n) else { return false };
                doc.is_element(p) && self.match_from(doc, p, idx - 1)
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Rule {
    selector: Selector,
    /// `(name, value, important)`
    decls: Vec<(String, String, bool)>,
    order: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    rules: Vec<Rule>,
}

impl Stylesheet {
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

pub fn parse_stylesheet(css: &str) -> Stylesheet {
    let css = strip_comments(css);
    let bytes = css.as_bytes();
    let mut sheet = Stylesheet::default();
    let mut i = 0;
    let mut order = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'@' {
            let mut j = i;
            while j < bytes.len() && bytes[j] != b';' && bytes[j] != b'{' {
                j += 1;
            }
            i = if j < bytes.len() && bytes[j] == b'{' { skip_block(bytes, j) } else { j + 1 };
            continue;
        }
        let Some(open) = css[i..].find('{') else { break };
        let selector_text = &css[i..i + open];
        let close = skip_block(bytes, i + open);
        let body_end = if close > i + open + 1 && bytes[close - 1] == b'}' { close - 1 } else { close };
        let decls = parse_declarations(&css[i + open + 1..body_end]);
        for sel_text in selector_text.split(',') {
            if let Some(selector) = parse_selector(sel_text.trim()) {
                sheet.rules.push(Rule { selector, decls: decls.clone(), order });
                order += 1;
            }
        }
        i = close;
    }
    sheet
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Index just past the `}` matching the `{` at `open` (or `bytes.len()`).
fn skip_block(bytes: &[u8], open: usize) -> usize {
    let mut depth = 0i32;
    let mut j = open;
    while j < bytes.len() {
        match bytes[j] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    bytes.len()
}

fn parse_declarations(body: &str) -> Vec<(String, String, bool)> {
    let mut out = Vec::new();
    for decl in body.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let k = k.trim().to_ascii_lowercase();
        let (v, important) = strip_important(v.trim());
        if k.is_empty() || v.is_empty() {
            continue;
        }
        out.push((k, v.to_string(), important));
    }
    out
}

fn strip_important(v: &str) -> (&str, bool) {
    let lower = v.to_ascii_lowercase();
    if lower.ends_with("important") {
        let rest = v[..v.len() - "important".len()].trim_end();
        if let Some(rest) = rest.strip_suffix('!') {
            return (rest.trim_end(), true);
        }
    }
    (v, false)
}

fn parse_selector(text: &str) -> Option<Selector> {
    if text.is_empty() {
        return None;
    }
    let mut parts: Vec<(Compound, Option<Combinator>)> = Vec::new();
    let mut cur = Compound::default();
    let mut have_cur = false;
    let mut pending: Option<Combinator> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' | '\r' => {
                if have_cur {
                    parts.push((std::mem::take(&mut cur), pending.take()));
                    have_cur = false;
                    pending = Some(Combinator::Descendant);
                }
            }
            '>' => {
                if have_cur {
                    parts.push((std::mem::take(&mut cur), pending.take()));
                    have_cur = false;
                }
                pending = Some(Combinator::Child);
            }
            '*' => have_cur = true,
            '.' | '#' => {
                let ident = read_ident(&mut chars);
                if ident.is_empty() {
                    return None;
                }
                if c == '.' {
                    cur.classes.push(ident);
                } else {
                    cur.id = Some(ident);
                }
                have_cur = true;
            }
            c if c.is_alphanumeric() || c == '_' || c == '-' => {
                let mut ident = String::from(c);
                ident.push_str(&read_ident(&mut chars));
                cur.tag = Some(ident);
                have_cur = true;
            }
            // attribute selectors, pseudo-classes, sibling combinators: not supported → drop the rule
            _ => return None,
        }
    }
    if have_cur {
        parts.push((cur, pending.take()));
    }
    if parts.is_empty() {
        return None;
    }
    let mut specificity = (0, 0, 0);
    for (comp, _) in &parts {
        specificity.0 += u32::from(comp.id.is_some());
        specificity.1 += comp.classes.len() as u32;
        specificity.2 += u32::from(comp.tag.is_some());
    }
    Some(Selector { parts, specificity })
}

fn read_ident(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_alphanumeric() || c == '_' || c == '-' {
            s.push(c);
            chars.next();
        } else {
            break;
        }
    }
    s
}

// ---------------------------------------------------------------------------
// Cascade on the document
// ---------------------------------------------------------------------------

/// Per-document caches, invalidated through `Doc`'s generation counters.
#[derive(Default)]
pub struct Caches {
    sheet: Option<(u64, Rc<Stylesheet>)>,
    specified: HashMap<NodeId, (u64, Rc<Style>)>,
}

impl Doc {
    fn stylesheet(&self) -> Rc<Stylesheet> {
        let g = self.sheet_generation.get();
        if let Some((sg, s)) = &self.caches.borrow().sheet {
            if *sg == g {
                return s.clone();
            }
        }
        let mut css = String::new();
        for n in self.descendants(self.svg()) {
            if self.is_element(n) && self.tag(n) == "style" {
                css.push_str(&self.text_content(n));
                css.push('\n');
            }
        }
        let sheet = Rc::new(parse_stylesheet(&css));
        self.caches.borrow_mut().sheet = Some((g, sheet.clone()));
        sheet
    }

    /// The element's own declarations from all three sources, no inheritance.
    /// This is what `ungroup` pushes down onto children.
    pub fn cascaded_style(&self, n: NodeId) -> Style {
        // sort key: (important, is_inline, specificity, order) — later wins
        let mut decls: Vec<((bool, bool, (u32, u32, u32), usize), String, String)> = Vec::new();
        for a in self.attrs(n) {
            if PRESENTATION_ATTRS.contains(&a.name.as_str()) {
                decls.push(((false, false, (0, 0, 0), 0), a.name.clone(), a.value.clone()));
            }
        }
        let sheet = self.stylesheet();
        for rule in &sheet.rules {
            if rule.selector.matches(self, n) {
                for (k, v, imp) in &rule.decls {
                    decls.push(((*imp, false, rule.selector.specificity, rule.order + 1), k.clone(), v.clone()));
                }
            }
        }
        if let Some(inline) = self.attr(n, "style") {
            for (idx, decl) in inline.split(';').enumerate() {
                let Some((k, v)) = decl.split_once(':') else { continue };
                let k = k.trim().to_ascii_lowercase();
                let (v, imp) = strip_important(v.trim());
                if k.is_empty() || v.is_empty() {
                    continue;
                }
                decls.push(((imp, true, (0, 0, 0), idx), k, v.to_string()));
            }
        }
        decls.sort_by(|a, b| a.0.cmp(&b.0));
        let mut st = Style::default();
        for (_, k, v) in decls {
            st.set_decl(&k, &v);
        }
        st
    }

    /// Parent's specified style overridden by this element's cascaded style (cached).
    pub fn specified_style(&self, n: NodeId) -> Rc<Style> {
        let g = self.generation.get();
        if let Some(s) = self.cached_specified(n, g) {
            return s;
        }
        let mut chain = vec![n];
        let mut base: Option<Rc<Style>> = None;
        let mut cur = self.parent(n);
        while let Some(p) = cur {
            if !self.is_element(p) {
                break;
            }
            if let Some(s) = self.cached_specified(p, g) {
                base = Some(s);
                break;
            }
            chain.push(p);
            cur = self.parent(p);
        }
        let mut acc: Style = base.map(|s| (*s).clone()).unwrap_or_default();
        let mut result = None;
        for &node in chain.iter().rev() {
            acc.merge_over(&self.cascaded_style(node));
            let rc = Rc::new(acc.clone());
            self.caches.borrow_mut().specified.insert(node, (g, rc.clone()));
            result = Some(rc);
        }
        result.expect("chain is never empty")
    }

    fn cached_specified(&self, n: NodeId, g: u64) -> Option<Rc<Style>> {
        self.caches.borrow().specified.get(&n).filter(|(cg, _)| *cg == g).map(|(_, s)| s.clone())
    }

    pub fn specified(&self, n: NodeId, prop: &str) -> Option<String> {
        self.specified_style(n).get(prop).map(str::to_string)
    }

    /// Specified value or the SVG initial value (`""` for unknown properties).
    pub fn computed(&self, n: NodeId, prop: &str) -> String {
        self.specified(n, prop).unwrap_or_else(|| default_value(prop).unwrap_or("").to_string())
    }

    /// Writes into the inline `style` attribute and drops a same-named presentation attribute.
    pub fn set_style(&mut self, n: NodeId, prop: &str, value: &str) {
        let mut st = self.attr(n, "style").map(Style::parse).unwrap_or_default();
        st.set(prop, value);
        self.set_attr(n, "style", st.to_css());
        if PRESENTATION_ATTRS.contains(&prop) {
            self.remove_attr(n, prop);
        }
    }

    /// Replaces the whole inline `style` attribute.
    pub fn set_style_map(&mut self, n: NodeId, st: &Style) {
        if st.is_empty() {
            self.remove_attr(n, "style");
        } else {
            self.set_attr(n, "style", st.to_css());
        }
    }

    /// Removes the property from the inline style and as a presentation attribute.
    pub fn remove_style(&mut self, n: NodeId, prop: &str) {
        if let Some(inline) = self.attr(n, "style") {
            let mut st = Style::parse(inline);
            if st.remove(prop).is_some() {
                self.set_style_map(n, &st);
            }
        }
        if PRESENTATION_ATTRS.contains(&prop) {
            self.remove_attr(n, prop);
        }
    }
}
```

Add to `src/lib.rs`: `pub mod style;`

- [ ] **Step 5: Run tests**

Run: `cargo test --test style && cargo test`
Expected: PASS (10 style tests; everything else still green). If clippy later complains about the `PRESENTATION_ATTRS` formatting, let `cargo fmt` reflow it.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings`

```bash
git add src/style.rs src/dom.rs src/lib.rs tests/style.rs
git commit -m "feat(style): declarations, selector subset and specified-style cascade

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Geometry primitives (units, transforms, rect algebra)

**Files:**
- Create: `src/geom/mod.rs`, `tests/geom.rs`
- Modify: `src/lib.rs` (add `pub mod geom;`)

**Interfaces:**
- Produces: `sciink::geom::{TOL, ipx, parse_transform, fmt_transform, is_identity, affine_eq, scale_factor, inverse, transform_rect, union, intersection, intersects, uniquetol}` and re-exports `kurbo::{Affine, BezPath, Point, Rect, Vec2}`.

- [ ] **Step 1: Write the failing tests**

`tests/geom.rs`:
```rust
mod support;

use sciink::geom::*;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn ipx_converts_units() {
    assert!(close(ipx("10pt").unwrap(), 13.333333333));
    assert_eq!(ipx("2in"), Some(192.0));
    assert!(close(ipx("1mm").unwrap(), 3.7795275591));
    assert!(close(ipx("1cm").unwrap(), 37.795275591));
    assert_eq!(ipx("5"), Some(5.0));
    assert_eq!(ipx(" 5px "), Some(5.0));
    assert_eq!(ipx("2pc"), Some(32.0));
    assert_eq!(ipx("50%"), None);
    assert_eq!(ipx("1em"), None);
    assert_eq!(ipx("abc"), None);
}

#[test]
fn transform_parse_and_format() {
    let t = parse_transform("translate(10,20) scale(2)").unwrap();
    let c = t.as_coeffs();
    assert!(close(c[0], 2.0) && close(c[3], 2.0) && close(c[4], 10.0) && close(c[5], 20.0));
    assert_eq!(parse_transform(""), Some(Affine::IDENTITY));
    assert_eq!(parse_transform("garbage("), None);
    assert_eq!(fmt_transform(Affine::IDENTITY), None);
    assert_eq!(fmt_transform(Affine::new([1.0, 0.0, 0.0, 1.0, 3.0, -4.5])), Some("translate(3,-4.5)".to_string()));
    assert_eq!(fmt_transform(Affine::new([2.0, 0.0, 0.0, 0.5, 0.0, 0.0])), Some("scale(2,0.5)".to_string()));
    assert_eq!(fmt_transform(Affine::new([1.0, 0.5, 0.0, 1.0, 0.0, 0.0])), Some("matrix(1,0.5,0,1,0,0)".to_string()));
    assert!(is_identity(Affine::new([1.000001, 0.0, 0.0, 1.0, 0.0, 0.0])));
    assert!(!affine_eq(Affine::IDENTITY, Affine::new([1.0001, 0.0, 0.0, 1.0, 0.0, 0.0])));
}

#[test]
fn scale_factor_and_inverse() {
    let t = Affine::new([2.0, 0.0, 0.0, 3.0, 5.0, 6.0]);
    assert!(close(scale_factor(t), 6f64.sqrt()));
    let inv = inverse(t).unwrap();
    assert!(affine_eq(t * inv, Affine::IDENTITY));
    assert_eq!(inverse(Affine::new([1.0, 2.0, 2.0, 4.0, 0.0, 0.0])), None);
}

#[test]
fn rect_algebra() {
    let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    let b = Rect::new(5.0, 5.0, 20.0, 20.0);
    assert_eq!(union(Some(a), Some(b)), Some(Rect::new(0.0, 0.0, 20.0, 20.0)));
    assert_eq!(union(None, Some(b)), Some(b));
    assert_eq!(union(None, None), None);
    assert_eq!(intersection(Some(a), Some(b)), Some(Rect::new(5.0, 5.0, 10.0, 10.0)));
    assert_eq!(intersection(None, Some(b)), Some(b), "upstream quirk: null first operand returns the second");
    assert_eq!(intersection(Some(a), None), None);
    assert_eq!(intersection(Some(a), Some(Rect::new(11.0, 0.0, 12.0, 1.0))), None);
    assert_eq!(intersection(Some(a), Some(Rect::new(10.0, 0.0, 12.0, 1.0))), Some(Rect::new(10.0, 0.0, 10.0, 1.0)), "touching edges give a zero-width box");
    assert!(intersects(a, b));
    assert!(!intersects(a, Rect::new(10.0, 0.0, 12.0, 1.0)), "touching is not intersecting");
    let r = transform_rect(Affine::new([0.0, 1.0, -1.0, 0.0, 0.0, 0.0]), Rect::new(0.0, 0.0, 2.0, 1.0));
    assert!(close(r.x0, -1.0) && close(r.y0, 0.0) && close(r.x1, 0.0) && close(r.y1, 2.0));
}

#[test]
fn uniquetol_counts_clusters() {
    assert_eq!(uniquetol(&[1.0, 1.0005, 2.0, 2.0004, 5.0], 0.001), 3);
    assert_eq!(uniquetol(&[3.0, 1.0, 2.0], 0.5), 3);
    assert_eq!(uniquetol(&[], 0.5), 0);
    assert_eq!(uniquetol(&[1.0, 1.4, 1.8], 0.5), 2, "tolerance is measured from the last kept value");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test geom`
Expected: FAIL to compile — `sciink::geom` missing.

- [ ] **Step 3: Implement `src/geom/mod.rs`**

```rust
//! Geometry primitives shared by every tool (spec §B.1). kurbo's
//! `Affine::new([a,b,c,d,e,f])` has exactly the SVG `matrix(a b c d e f)` meaning;
//! upstream's `A @ B` is kurbo's `A * B` (apply `B` first) and `-A` is `inverse(A)`.

pub mod path;

use std::str::FromStr;

pub use kurbo::{Affine, BezPath, Point, Rect, Vec2};

use crate::num;

/// Transform components closer than this are equal (`inkex/transforms.py`).
pub const TOL: f64 = 1e-5;

/// Length with unit → px. `%`, `em`, `ex` and unknown units → `None`.
pub fn ipx(s: &str) -> Option<f64> {
    use svgtypes::LengthUnit as U;
    let l = svgtypes::Length::from_str(s.trim()).ok()?;
    let factor = match l.unit {
        U::None | U::Px => 1.0,
        U::In => 96.0,
        U::Cm => 96.0 / 2.54,
        U::Mm => 96.0 / 25.4,
        U::Pt => 96.0 / 72.0,
        U::Pc => 16.0,
        U::Em | U::Ex | U::Percent => return None,
    };
    Some(l.number * factor)
}

/// Parses a `transform` attribute (any list of transform functions). Empty → identity.
pub fn parse_transform(s: &str) -> Option<Affine> {
    if s.trim().is_empty() {
        return Some(Affine::IDENTITY);
    }
    let t = svgtypes::Transform::from_str(s).ok()?;
    Some(Affine::new([t.a, t.b, t.c, t.d, t.e, t.f]))
}

/// `translate(e,f)` / `scale(a,d)` / `matrix(…)`; `None` for identity (write no attribute).
pub fn fmt_transform(t: Affine) -> Option<String> {
    if is_identity(t) {
        return None;
    }
    let [a, b, c, d, e, f] = t.as_coeffs();
    let z = |v: f64| v.abs() <= TOL;
    if z(a - 1.0) && z(d - 1.0) && z(b) && z(c) {
        return Some(format!("translate({},{})", num::fmt(e), num::fmt(f)));
    }
    if z(e) && z(f) && z(b) && z(c) {
        return Some(format!("scale({},{})", num::fmt(a), num::fmt(d)));
    }
    Some(format!(
        "matrix({},{},{},{},{},{})",
        num::fmt(a),
        num::fmt(b),
        num::fmt(c),
        num::fmt(d),
        num::fmt(e),
        num::fmt(f)
    ))
}

pub fn affine_eq(a: Affine, b: Affine) -> bool {
    a.as_coeffs().iter().zip(b.as_coeffs().iter()).all(|(x, y)| (x - y).abs() <= TOL)
}

pub fn is_identity(t: Affine) -> bool {
    affine_eq(t, Affine::IDENTITY)
}

/// `sqrt(|det|)`: the uniform scale a transform applies to lengths.
pub fn scale_factor(t: Affine) -> f64 {
    t.determinant().abs().sqrt()
}

/// `None` for singular transforms (upstream raised `ZeroDivisionError`).
pub fn inverse(t: Affine) -> Option<Affine> {
    if t.determinant().abs() < 1e-12 {
        None
    } else {
        Some(t.inverse())
    }
}

/// Bounding box of the four transformed corners.
pub fn transform_rect(t: Affine, r: Rect) -> Rect {
    let pts = [
        t * Point::new(r.x0, r.y0),
        t * Point::new(r.x1, r.y0),
        t * Point::new(r.x0, r.y1),
        t * Point::new(r.x1, r.y1),
    ];
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in pts {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    Rect::new(x0, y0, x1, y1)
}

/// Null-absorbing union (`utils.py:655-667`).
pub fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) => Some(a.union(b)),
    }
}

/// Upstream quirk preserved (`utils.py:669-681`): a null first operand returns the
/// second; a null second operand gives null; touching edges give a zero-size box.
pub fn intersection(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    let Some(a) = a else { return b };
    let b = b?;
    let r = Rect::new(a.x0.max(b.x0), a.y0.max(b.y0), a.x1.min(b.x1), a.y1.min(b.y1));
    if r.x1 < r.x0 || r.y1 < r.y0 { None } else { Some(r) }
}

/// Strict overlap test (`utils.py:649-653`).
pub fn intersects(a: Rect, b: Rect) -> bool {
    let (ac, bc) = (a.center(), b.center());
    (ac.x - bc.x).abs() * 2.0 < a.width() + b.width() && (ac.y - bc.y).abs() * 2.0 < a.height() + b.height()
}

/// Number of distinct values, where a value counts as new when it is more than
/// `tol` above the last kept value (`utils.py:130-153`).
pub fn uniquetol(xs: &[f64], tol: f64) -> usize {
    let mut v: Vec<f64> = xs.iter().copied().filter(|x| x.is_finite()).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut count = 0;
    let mut last = f64::NAN;
    for x in v {
        if count == 0 || (x - last) > tol {
            count += 1;
            last = x;
        }
    }
    count
}
```

Create an empty placeholder so the module compiles before Task 9: `src/geom/path.rs` containing only `//! Path data (Task 9).`

Add to `src/lib.rs`: `pub mod geom;`

- [ ] **Step 4: Run tests**

Run: `cargo test --test geom`
Expected: PASS (5 tests).

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings`

```bash
git add src/geom src/lib.rs tests/geom.rs
git commit -m "feat(geom): units, transforms and rect algebra

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: Path data (parse, format, shapes, end points, bbox)

**Files:**
- Modify: `src/geom/path.rs` (replace placeholder), `tests/geom.rs` (append tests)

**Interfaces:**
- Produces: `sciink::geom::path::{ParsedPath, parse_d, fmt_d, end_points, reverse, path_eq, bbox_exact, bbox_rough, shape_path}`.
- `ParsedPath { path: BezPath, cmd_start: Vec<usize> }` — `cmd_start[i]` is the index of the first `PathEl` produced by source command `i` (length = commands + 1); needed to read `inkscape-scientific-combined-by-color` ranges written by the Python version.
- Consumes: `Doc::{tag, attr}` (Task 3), `geom::ipx` (Task 8), `num::fmt` (Task 2).

- [ ] **Step 1: Write the failing tests** (append to `tests/geom.rs`)

```rust
use kurbo::PathEl;
use sciink::dom::Doc;
use sciink::geom::path::*;

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

#[test]
fn parse_d_handles_relative_and_shorthand_commands() {
    let p = parse_d("M 0 0 h 10 v 10 z").unwrap();
    assert_eq!(p.path.elements(), &[PathEl::MoveTo(pt(0.0, 0.0)), PathEl::LineTo(pt(10.0, 0.0)), PathEl::LineTo(pt(10.0, 10.0)), PathEl::ClosePath]);
    assert_eq!(p.cmd_start, vec![0, 1, 2, 3, 4]);
    assert_eq!(end_points(&p.path), vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 0.0)]);
    let q = parse_d("m 1,2 3,4 l -1,-1").unwrap();
    assert_eq!(q.path.elements(), &[PathEl::MoveTo(pt(1.0, 2.0)), PathEl::LineTo(pt(4.0, 6.0)), PathEl::LineTo(pt(3.0, 5.0))]);
    let s = parse_d("M0 0 C 0 10 10 10 10 0 S 20 -10 20 0").unwrap();
    assert_eq!(s.path.elements()[2], PathEl::CurveTo(pt(10.0, -10.0), pt(20.0, -10.0), pt(20.0, 0.0)), "S reflects the previous control point");
    let t = parse_d("M0 0 Q 5 10 10 0 T 20 0").unwrap();
    assert_eq!(t.path.elements()[2], PathEl::QuadTo(pt(15.0, -10.0), pt(20.0, 0.0)));
    let after_close = parse_d("M 0 0 L 10 0 Z l 5 5").unwrap();
    assert_eq!(after_close.path.elements()[3], PathEl::LineTo(pt(5.0, 5.0)), "relative after Z starts from the subpath start");
    assert!(parse_d("").is_none());
    assert!(parse_d("L 1 1").is_none(), "must start with a moveto");
}

#[test]
fn arcs_become_cubics_and_degenerate_arcs_become_lines() {
    let a = parse_d("M 0 0 A 10 10 0 0 1 20 0").unwrap();
    assert!(a.path.elements().iter().skip(1).all(|e| matches!(e, PathEl::CurveTo(..))));
    let bb = bbox_exact(&a.path).unwrap();
    assert!((bb.width() - 20.0).abs() < 1e-3 && (bb.height() - 10.0).abs() < 1e-3, "{bb:?}");
    assert_eq!(a.cmd_start.len(), 3);
    let d = parse_d("M 0 0 A 0 0 0 0 1 20 0").unwrap();
    assert_eq!(d.path.elements()[1], PathEl::LineTo(pt(20.0, 0.0)));
}

#[test]
fn fmt_d_is_absolute_and_round_trips() {
    let p = parse_d("M 0 0 h 10 v 10 z").unwrap();
    assert_eq!(fmt_d(&p.path), "M 0,0 L 10,0 L 10,10 Z");
    let q = parse_d("M0 0 C 0 10 10 10 10 0 Q 15 5 20 0").unwrap();
    assert_eq!(fmt_d(&q.path), "M 0,0 C 0,10 10,10 10,0 Q 15,5 20,0");
    let again = parse_d(&fmt_d(&q.path)).unwrap();
    assert!(path_eq(&q.path, &again.path, 1e-9));
}

#[test]
fn reverse_and_equality() {
    let p = parse_d("M 0 0 L 10 0 L 10 10").unwrap();
    let r = reverse(&p.path);
    assert_eq!(end_points(&r).first(), Some(&pt(10.0, 10.0)));
    assert_eq!(end_points(&r).last(), Some(&pt(0.0, 0.0)));
    assert!(path_eq(&p.path, &reverse(&r), 1e-9));
    assert!(!path_eq(&p.path, &r, 1e-9));
    let shifted = parse_d("M 0 0.0000001 L 10 0 L 10 10").unwrap();
    assert!(path_eq(&p.path, &shifted.path, 1e-6));
    assert!(!path_eq(&p.path, &shifted.path, 1e-9));
}

#[test]
fn rough_and_exact_bboxes() {
    let p = parse_d("M 0 0 C 0 10 10 10 10 0").unwrap();
    let exact = bbox_exact(&p.path).unwrap();
    let rough = bbox_rough(&p.path).unwrap();
    assert!((exact.y1 - 7.5).abs() < 1e-9, "cubic extremum, got {exact:?}");
    assert!((rough.y1 - 10.0).abs() < 1e-9, "control box, got {rough:?}");
    assert_eq!(bbox_exact(&BezPath::new()), None);
}

#[test]
fn shapes_convert_to_paths() {
    let d = Doc::parse(
        "<svg xmlns=\"http://www.w3.org/2000/svg\">\
         <rect id=\"r\" x=\"1\" y=\"2\" width=\"10\" height=\"5\"/>\
         <rect id=\"rr\" x=\"0\" y=\"0\" width=\"10\" height=\"10\" rx=\"2\"/>\
         <circle id=\"c\" cx=\"5\" cy=\"5\" r=\"5\"/>\
         <ellipse id=\"e\" cx=\"0\" cy=\"0\" rx=\"4\" ry=\"2\"/>\
         <line id=\"l\" x1=\"1\" y1=\"1\" x2=\"3\" y2=\"4\"/>\
         <polyline id=\"pl\" points=\"0,0 10,0 10,10\"/>\
         <polygon id=\"pg\" points=\"0,0 10,0 10,10\"/>\
         <path id=\"p\" d=\"M 0 0 L 1 1\"/>\
         <text id=\"t\">x</text>\
         <rect id=\"bad\" width=\"50%\" height=\"1\"/></svg>"
            .as_bytes(),
    )
    .unwrap();
    let bb = |id: &str| bbox_exact(&shape_path(&d, d.by_id(id).unwrap()).unwrap().path).unwrap();
    assert_eq!(bb("r"), Rect::new(1.0, 2.0, 11.0, 7.0));
    let rr = bb("rr");
    assert!((rr.x0).abs() < 1e-3 && (rr.x1 - 10.0).abs() < 1e-3 && (rr.y1 - 10.0).abs() < 1e-3, "{rr:?}");
    let c = bb("c");
    assert!((c.x0).abs() < 1e-3 && (c.x1 - 10.0).abs() < 1e-3 && (c.y0).abs() < 1e-3 && (c.y1 - 10.0).abs() < 1e-3, "{c:?}");
    let e = bb("e");
    assert!((e.x0 + 4.0).abs() < 1e-3 && (e.y1 - 2.0).abs() < 1e-3, "{e:?}");
    assert_eq!(bb("l"), Rect::new(1.0, 1.0, 3.0, 4.0));
    assert_eq!(bb("pl"), Rect::new(0.0, 0.0, 10.0, 10.0));
    let pg = shape_path(&d, d.by_id("pg").unwrap()).unwrap();
    assert_eq!(pg.path.elements().last(), Some(&PathEl::ClosePath));
    assert_eq!(shape_path(&d, d.by_id("p").unwrap()).unwrap().path.elements().len(), 2);
    assert!(shape_path(&d, d.by_id("t").unwrap()).is_none());
    assert!(shape_path(&d, d.by_id("bad").unwrap()).is_none(), "percent lengths are unsupported");
}

#[test]
fn upstream_paths_survive_parse_format_parse() {
    for file in support::upstream_svgs() {
        let doc = Doc::parse(&std::fs::read(&file).unwrap()).unwrap();
        let mut failures = Vec::new();
        let mut count = 0;
        for n in doc.descendants(doc.svg()) {
            if doc.tag(n) != "path" {
                continue;
            }
            let Some(d) = doc.attr(n, "d") else { continue };
            if d.trim().is_empty() {
                continue;
            }
            let Some(pp) = parse_d(d) else {
                failures.push(format!("unparsable: {:?}", doc.attr(n, "id")));
                continue;
            };
            let again = parse_d(&fmt_d(&pp.path)).unwrap();
            if !path_eq(&pp.path, &again.path, 1e-3) {
                failures.push(format!("changed after fmt/parse: {:?}", doc.attr(n, "id")));
            }
            count += 1;
        }
        assert!(failures.is_empty(), "{}: {} of {} paths failed:\n{}", file.display(), failures.len(), count, failures.join("\n"));
        eprintln!("{}: {count} paths ok", file.display());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test geom`
Expected: FAIL to compile — `parse_d` etc. missing.

- [ ] **Step 3: Implement `src/geom/path.rs`**

```rust
//! Path data: SVG `d` ↔ kurbo `BezPath` with the source-command index kept (spec §B.1).
//! Arcs become cubics (`kurbo::Arc::from_svg_arc`, tolerance 1e-4); degenerate arcs
//! become lines; H/V → L and S/T → C/Q. Output is always absolute `M L Q C Z`.
//! Never re-serialize a `d` you did not modify.

use kurbo::{Arc, BezPath, PathEl, Point, Rect, Shape, SvgArc, Vec2};
use svgtypes::{PathParser, PathSegment};

use super::ipx;
use crate::dom::{Doc, NodeId};
use crate::num;

#[derive(Debug, Clone)]
pub struct ParsedPath {
    pub path: BezPath,
    /// `cmd_start[i]` = index of the first `PathEl` produced by source command `i`;
    /// the last entry is the element count.
    pub cmd_start: Vec<usize>,
}

pub fn parse_d(d: &str) -> Option<ParsedPath> {
    let mut path = BezPath::new();
    let mut cmd_start = Vec::new();
    let mut cur = Point::ZERO;
    let mut start = Point::ZERO;
    let mut last_cubic_ctrl: Option<Point> = None;
    let mut last_quad_ctrl: Option<Point> = None;
    let mut seen_any = false;
    for seg in PathParser::from(d) {
        let seg = seg.ok()?;
        if !seen_any && !matches!(seg, PathSegment::MoveTo { .. }) {
            return None;
        }
        cmd_start.push(path.elements().len());
        let mut next_cubic = None;
        let mut next_quad = None;
        match seg {
            PathSegment::MoveTo { abs, x, y } => {
                let p = pt(abs, cur, x, y);
                path.move_to(p);
                cur = p;
                start = p;
            }
            PathSegment::LineTo { abs, x, y } => {
                let p = pt(abs, cur, x, y);
                path.line_to(p);
                cur = p;
            }
            PathSegment::HorizontalLineTo { abs, x } => {
                let p = Point::new(if abs { x } else { cur.x + x }, cur.y);
                path.line_to(p);
                cur = p;
            }
            PathSegment::VerticalLineTo { abs, y } => {
                let p = Point::new(cur.x, if abs { y } else { cur.y + y });
                path.line_to(p);
                cur = p;
            }
            PathSegment::CurveTo { abs, x1, y1, x2, y2, x, y } => {
                let (c1, c2, p) = (pt(abs, cur, x1, y1), pt(abs, cur, x2, y2), pt(abs, cur, x, y));
                path.curve_to(c1, c2, p);
                next_cubic = Some(c2);
                cur = p;
            }
            PathSegment::SmoothCurveTo { abs, x2, y2, x, y } => {
                let c1 = reflect(cur, last_cubic_ctrl);
                let (c2, p) = (pt(abs, cur, x2, y2), pt(abs, cur, x, y));
                path.curve_to(c1, c2, p);
                next_cubic = Some(c2);
                cur = p;
            }
            PathSegment::Quadratic { abs, x1, y1, x, y } => {
                let (c, p) = (pt(abs, cur, x1, y1), pt(abs, cur, x, y));
                path.quad_to(c, p);
                next_quad = Some(c);
                cur = p;
            }
            PathSegment::SmoothQuadratic { abs, x, y } => {
                let c = reflect(cur, last_quad_ctrl);
                let p = pt(abs, cur, x, y);
                path.quad_to(c, p);
                next_quad = Some(c);
                cur = p;
            }
            PathSegment::EllipticalArc { abs, rx, ry, x_axis_rotation, large_arc, sweep, x, y } => {
                let p = pt(abs, cur, x, y);
                let svg_arc = SvgArc {
                    from: cur,
                    to: p,
                    radii: Vec2::new(rx.abs(), ry.abs()),
                    x_rotation: x_axis_rotation.to_radians(),
                    large_arc,
                    sweep,
                };
                match Arc::from_svg_arc(&svg_arc) {
                    Some(arc) => {
                        for el in arc.append_iter(1e-4) {
                            path.push(el);
                        }
                    }
                    None => path.line_to(p),
                }
                cur = p;
            }
            PathSegment::ClosePath { .. } => {
                path.close_path();
                cur = start;
            }
        }
        last_cubic_ctrl = next_cubic;
        last_quad_ctrl = next_quad;
        seen_any = true;
    }
    if !seen_any {
        return None;
    }
    cmd_start.push(path.elements().len());
    Some(ParsedPath { path, cmd_start })
}

fn pt(abs: bool, cur: Point, x: f64, y: f64) -> Point {
    if abs { Point::new(x, y) } else { Point::new(cur.x + x, cur.y + y) }
}

fn reflect(cur: Point, ctrl: Option<Point>) -> Point {
    match ctrl {
        Some(c) => Point::new(2.0 * cur.x - c.x, 2.0 * cur.y - c.y),
        None => cur,
    }
}

/// Absolute `M x,y L x,y Q … C … Z`, numbers through `num::fmt`.
pub fn fmt_d(path: &BezPath) -> String {
    let mut s = String::new();
    for el in path.elements() {
        if !s.is_empty() {
            s.push(' ');
        }
        match el {
            PathEl::MoveTo(p) => {
                s.push_str("M ");
                push_pt(&mut s, *p);
            }
            PathEl::LineTo(p) => {
                s.push_str("L ");
                push_pt(&mut s, *p);
            }
            PathEl::QuadTo(c, p) => {
                s.push_str("Q ");
                push_pt(&mut s, *c);
                s.push(' ');
                push_pt(&mut s, *p);
            }
            PathEl::CurveTo(c1, c2, p) => {
                s.push_str("C ");
                push_pt(&mut s, *c1);
                s.push(' ');
                push_pt(&mut s, *c2);
                s.push(' ');
                push_pt(&mut s, *p);
            }
            PathEl::ClosePath => s.push('Z'),
        }
    }
    s
}

fn push_pt(s: &mut String, p: Point) {
    s.push_str(&num::fmt(p.x));
    s.push(',');
    s.push_str(&num::fmt(p.y));
}

/// One point per element; `Z` yields the subpath start (`inkex/paths.py:1446-1457`).
pub fn end_points(path: &BezPath) -> Vec<Point> {
    let mut out = Vec::with_capacity(path.elements().len());
    let mut start = Point::ZERO;
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                start = *p;
                out.push(*p);
            }
            PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => out.push(*p),
            PathEl::ClosePath => out.push(start),
        }
    }
    out
}

pub fn reverse(path: &BezPath) -> BezPath {
    path.reverse_subpaths()
}

/// Same element kinds, every point within `tol` (upstream compared floats exactly).
pub fn path_eq(a: &BezPath, b: &BezPath, tol: f64) -> bool {
    let (ea, eb) = (a.elements(), b.elements());
    if ea.len() != eb.len() {
        return false;
    }
    let close = |p: Point, q: Point| (p.x - q.x).abs() <= tol && (p.y - q.y).abs() <= tol;
    ea.iter().zip(eb).all(|(x, y)| match (x, y) {
        (PathEl::MoveTo(p), PathEl::MoveTo(q)) | (PathEl::LineTo(p), PathEl::LineTo(q)) => close(*p, *q),
        (PathEl::QuadTo(a1, a2), PathEl::QuadTo(b1, b2)) => close(*a1, *b1) && close(*a2, *b2),
        (PathEl::CurveTo(a1, a2, a3), PathEl::CurveTo(b1, b2, b3)) => close(*a1, *b1) && close(*a2, *b2) && close(*a3, *b3),
        (PathEl::ClosePath, PathEl::ClosePath) => true,
        _ => false,
    })
}

/// Tight box from Bézier extrema (`paths.py:679-687`).
pub fn bbox_exact(path: &BezPath) -> Option<Rect> {
    if path.elements().is_empty() { None } else { Some(path.bounding_box()) }
}

/// Control-point box (upstream `roughpath=True`, `dhelpers.py:1482-1494`).
pub fn bbox_rough(path: &BezPath) -> Option<Rect> {
    if path.elements().is_empty() { None } else { Some(path.control_box()) }
}

/// Geometry of a shape element as a path in its own coordinates (`cache.py:426-467`,
/// `inkex/elements/_polygons.py:350-362`). `None` for non-shapes or unusable attributes.
pub fn shape_path(doc: &Doc, n: NodeId) -> Option<ParsedPath> {
    let num_attr = |name: &str| doc.attr(n, name).and_then(ipx);
    let f = num::fmt;
    match doc.tag(n) {
        "path" => parse_d(doc.attr(n, "d")?),
        "rect" => {
            let (x, y) = (num_attr("x").unwrap_or(0.0), num_attr("y").unwrap_or(0.0));
            let w = doc.attr(n, "width").map(ipx)?;
            let h = doc.attr(n, "height").map(ipx)?;
            let (w, h) = (w?, h?);
            let (rx0, ry0) = (num_attr("rx"), num_attr("ry"));
            let d = match (rx0, ry0) {
                (None, None) => format!("M {},{} h {} v {} h {} z", f(x), f(y), f(w), f(h), f(-w)),
                _ => {
                    let rx = rx0.filter(|v| *v > 0.0).or(ry0).unwrap_or(0.0).min(w / 2.0);
                    let ry = ry0.filter(|v| *v > 0.0).or(rx0).unwrap_or(0.0).min(h / 2.0);
                    format!(
                        "M {},{} h {} a {},{} 0 0 1 {},{} v {} a {},{} 0 0 1 {},{} h {} a {},{} 0 0 1 {},{} v {} a {},{} 0 0 1 {},{} z",
                        f(x + rx), f(y), f(w - 2.0 * rx),
                        f(rx), f(ry), f(rx), f(ry),
                        f(h - 2.0 * ry),
                        f(rx), f(ry), f(-rx), f(ry),
                        f(-(w - 2.0 * rx)),
                        f(rx), f(ry), f(-rx), f(-ry),
                        f(-(h - 2.0 * ry)),
                        f(rx), f(ry), f(rx), f(-ry)
                    )
                }
            };
            parse_d(&d)
        }
        "circle" | "ellipse" => {
            let (cx, cy) = (num_attr("cx").unwrap_or(0.0), num_attr("cy").unwrap_or(0.0));
            let (rx, ry) = if doc.tag(n) == "circle" {
                let r = num_attr("r")?;
                (r, r)
            } else {
                (num_attr("rx")?, num_attr("ry")?)
            };
            parse_d(&format!(
                "M {},{} a {},{} 0 1 0 {},{} a {},{} 0 0 0 {},{} z",
                f(cx), f(cy - ry), f(rx), f(ry), f(rx), f(ry), f(rx), f(ry), f(-rx), f(-ry)
            ))
        }
        "line" => parse_d(&format!(
            "M {},{} L {},{}",
            f(num_attr("x1").unwrap_or(0.0)),
            f(num_attr("y1").unwrap_or(0.0)),
            f(num_attr("x2").unwrap_or(0.0)),
            f(num_attr("y2").unwrap_or(0.0))
        )),
        "polyline" | "polygon" => {
            let pts: Vec<f64> = doc
                .attr(n, "points")?
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<f64>().ok())
                .collect::<Option<Vec<_>>>()?;
            if pts.len() < 4 {
                return None;
            }
            let mut d = String::new();
            for (i, xy) in pts.chunks_exact(2).enumerate() {
                d.push_str(if i == 0 { "M " } else { " L " });
                d.push_str(&format!("{},{}", f(xy[0]), f(xy[1])));
            }
            if doc.tag(n) == "polygon" {
                d.push_str(" Z");
            }
            parse_d(&d)
        }
        _ => None,
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test geom`
Expected: PASS (12 tests). The upstream test prints a per-fixture path count (Acid_tests: 8460). If `Arc::from_svg_arc` in your kurbo version returns `Some` for zero radii, the degenerate-arc assertion tells you; then add an explicit `if rx == 0.0 || ry == 0.0 || cur == p { path.line_to(p) } else { … }` guard.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`

```bash
git add src/geom/path.rs tests/geom.rs
git commit -m "feat(geom): path data parsing, formatting, shapes and bboxes

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: Continuous integration

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: CI that runs the exact commands developers run locally; the release workflow is Plan 6's job.

- [ ] **Step 1: Write the workflow**

`.github/workflows/ci.yml`:
```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:
env:
  CARGO_TERM_COLOR: always
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test
      - run: cargo build --release
      - name: smoke (about tool through a pipe)
        shell: bash
        run: |
          ./target/release/sciink --tool=about tests/data/edge/simple.svg > out.svg
          cmp out.svg tests/data/edge/simple.svg
```

- [ ] **Step 2: Run the same commands locally**

Run: `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build --release && ./target/release/sciink --tool=about tests/data/edge/simple.svg > /tmp/sciink-smoke.svg && cmp /tmp/sciink-smoke.svg tests/data/edge/simple.svg && echo LOCAL-CI-OK`
Expected: `LOCAL-CI-OK` (the about report appears on stderr).

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: fmt, clippy and tests on linux, macos and windows

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

## Plan self-review notes

- **Spec coverage (M0/M1):** §C.1 DOM → Tasks 3–4 (lossless set, `sciink-N` ids, first-wins duplicates, iterative walks, byte-identity test); §C.1 numbers → Task 2; §C.2 style → Task 7 (sources, precedence, `!important`, selector subset incl. `@`-rule skipping, `font` shorthand, defaults table, generation-keyed caches, sheet cache keyed by `sheet_generation`); §C.3 CLI → Task 5 (two-stage parse, `inx_bool`, echo-on-failure, panic hook, `SCIINK_LOG`, `--output`, stdin, `windows_subsystem`) and `.inx` skeleton → Task 6; §B.1 primitives → Tasks 8–9. Deferred to later plans on purpose: `fonts.rs` (Plan 2, text engine), packaging/release workflow (Plan 6), golden/invariance harnesses (Plan 2+ when there is output to compare).
- **Deviation from §C.2 recorded:** the cascade follows upstream (every property propagates via `specified_style`) instead of CSS inheritance rules, because Appendix A/B algorithms were derived against upstream semantics; `default_value` supplies initial values.
- **Type consistency:** `Doc::descendants` includes the start node (callers `skip(1)`); `tail(n)` returns the following Text node; `Output { svg, messages }` is the tool contract; `Common` is a `clap::Args`, tools are `clap::Parser`; `num::fmt` is the only formatter; `geom::path::ParsedPath.cmd_start` has `commands + 1` entries.

## Execution

Run task-by-task with `superpowers:subagent-driven-development`: one fresh subagent per task, each ending with green `cargo test`, clean fmt/clippy, and a commit. Task 6 needs this Mac (Inkscape 1.4.4) and the tests/upstream symlink; all other tasks are machine-independent.
