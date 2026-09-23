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
