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
