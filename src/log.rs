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
