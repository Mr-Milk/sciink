//! sciink — Inkscape extensions for scientific figures, as one fast binary.
//! Inkscape launches `sciink --tool=<name> --param=value… --id=… input.svg`
//! and reads the modified SVG from stdout (see docs/spec/03-infrastructure.md §C.3).

pub mod num;

/// Version string shown to users: Cargo version plus the git SHA baked in by CI.
pub fn version() -> String {
    match option_env!("SCIINK_GIT_SHA") {
        Some(sha) => format!("{} ({sha})", env!("CARGO_PKG_VERSION")),
        None => env!("CARGO_PKG_VERSION").to_string(),
    }
}
