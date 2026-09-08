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
