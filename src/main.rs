#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if argv.iter().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("sciink {}", sciink::version());
    }
}
