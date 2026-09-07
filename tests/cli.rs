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
