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
/// `<cache dir>/fontcache-<format>-<key hash>.tsv` — the hash names one file per distinct scan
/// key, so two environments (e.g. with and without `SCIINK_FONT_DIRS`) each keep their own cache
/// instead of invalidating each other's on every alternating run.
pub fn cache_path(key: &ScanKey) -> Option<PathBuf> {
    if std::env::var_os("SCIINK_NO_FONT_CACHE").is_some_and(|v| v == "1") {
        return None;
    }
    if let Some(p) = std::env::var_os("SCIINK_FONT_CACHE") {
        return Some(PathBuf::from(p));
    }
    crate::paths::cache_dir().map(|d| {
        d.join(format!(
            "fontcache-{FONT_CACHE_FORMAT}-{:016x}.tsv",
            key_hash(key)
        ))
    })
}

/// FNV-1a over the parts of `key` that distinguish one cache file from another: whether system
/// fonts are scanned, each configured directory (each followed by a `0` separator, so
/// `["ab", "c"]` cannot hash the same as `["a", "bc"]`), and the bundled directory.
fn key_hash(key: &ScanKey) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a offset basis
    let mut mix = |bytes: &[u8]| {
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3); // FNV-1a prime
        }
    };
    mix(&[key.system as u8]);
    for d in &key.dirs {
        mix(d.to_string_lossy().as_bytes());
        mix(&[0]);
    }
    if let Some(b) = &key.bundled {
        mix(b.to_string_lossy().as_bytes());
    }
    h
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
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
    // A face without a path can't be cached at all; a path (a face's, a configured directory's,
    // or the bundled directory's) that is not valid UTF-8 would come back mangled through
    // `path_field`'s lossy conversion and could never validate again — bail on either up front,
    // the same way, instead of writing a cache file that can never be read back as valid.
    let not_utf8 = |p: &Path| p.to_str().is_none();
    if faces
        .iter()
        .any(|f| f.info.path.as_deref().is_none_or(not_utf8))
        || key.dirs.iter().any(|d| not_utf8(d))
        || key.bundled.as_deref().is_some_and(not_utf8)
    {
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
        key.bundled
            .as_deref()
            .map(path_field)
            .unwrap_or_else(|| "-".to_string())
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
