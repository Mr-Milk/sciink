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
    sciink::log::init(
        pre.log.as_deref().or(std::env::var_os("SCIINK_LOG")
            .map(std::path::PathBuf::from)
            .as_deref()),
    );
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
        Ok(Err(e)) => (
            input.clone(),
            vec![format!(
                "sciink {tool}: {e}\nThe document was left unchanged."
            )],
        ),
        Err(_) => {
            let msg = LAST_PANIC
                .lock()
                .ok()
                .and_then(|g| g.clone())
                .unwrap_or_else(|| "unknown panic".to_string());
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
