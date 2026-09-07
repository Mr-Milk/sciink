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
    let out = bin()
        .args(["--tool=about", "--id=p"])
        .arg(&p)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, SIMPLE.as_bytes());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("sciink 0.1.0"), "{err}");
    assert!(
        err.contains("document: 3 elements (1 text, 1 path)"),
        "{err}"
    );
    assert!(err.contains("selection: 1 object(s)"), "{err}");
}

#[test]
fn unknown_argument_echoes_input_with_message() {
    let p = tmp("badarg.svg", SIMPLE);
    let out = bin()
        .args(["--tool=about", "--bogus=1"])
        .arg(&p)
        .output()
        .unwrap();
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
    let out = bin()
        .args(["--tool=scaler", "--tab=correction"])
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(out.stdout, SIMPLE.as_bytes());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not implemented"));
}

#[test]
fn output_flag_writes_file_and_keeps_stdout_empty() {
    let p = tmp("in.svg", SIMPLE);
    let o = std::env::temp_dir().join(format!("sciink-test-{}-out.svg", std::process::id()));
    let _ = std::fs::remove_file(&o);
    let out = bin()
        .arg("--tool=about")
        .arg("--output")
        .arg(&o)
        .arg(&p)
        .output()
        .unwrap();
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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(SIMPLE.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.stdout, SIMPLE.as_bytes());
}

#[test]
fn log_env_var_writes_phase_lines() {
    let p = tmp("log.svg", SIMPLE);
    let l = std::env::temp_dir().join(format!("sciink-test-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&l);
    let out = bin()
        .arg("--tool=about")
        .arg(&p)
        .env("SCIINK_LOG", &l)
        .output()
        .unwrap();
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
    let argv: Vec<OsString> = [
        "sciink",
        "--tool=about",
        "--deepungroup=true",
        "-o",
        "out.svg",
        "--id=a",
        "in.svg",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    let p = prescan(&argv);
    assert_eq!(p.tool.as_deref(), Some("about"));
    assert_eq!(p.input.as_deref().and_then(|x| x.to_str()), Some("in.svg"));
    assert_eq!(
        p.output.as_deref().and_then(|x| x.to_str()),
        Some("out.svg")
    );
    let argv: Vec<OsString> = ["sciink", "--tool", "scaler", "--output=o.svg", "in.svg"]
        .iter()
        .map(OsString::from)
        .collect();
    let p = prescan(&argv);
    assert_eq!(p.tool.as_deref(), Some("scaler"));
    assert_eq!(p.output.as_deref().and_then(|x| x.to_str()), Some("o.svg"));
    assert_eq!(p.input.as_deref().and_then(|x| x.to_str()), Some("in.svg"));
}
