//! Phase timings on the synthetic document and, when `SCIINK_BIG_SVG` names a file, on that file.
//! Opt-in: `cargo test --release --test bench -- --ignored --nocapture`.

mod support;

use std::process::Command;

fn log_for(input: &std::path::Path, args: &[&str]) -> String {
    let log = std::env::temp_dir().join(format!("sciink-bench-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let status = Command::new(env!("CARGO_BIN_EXE_sciink"))
        .args(args)
        .arg("--log")
        .arg(&log)
        .arg(input)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::read_to_string(&log).unwrap_or_default()
}

fn total_ms(log: &str) -> f64 {
    log.lines()
        .filter(|l| l.contains("phase=total"))
        .filter_map(|l| l.split_whitespace().find_map(|f| f.strip_prefix("dt=")))
        .filter_map(|v| v.parse::<f64>().ok())
        .next_back()
        .unwrap_or(f64::NAN)
}

#[test]
#[ignore = "timing report; run with --release --ignored --nocapture"]
fn phase_timings() {
    let synthetic = std::env::temp_dir().join(format!("sciink-bench-{}.svg", std::process::id()));
    std::fs::write(&synthetic, support::BigDoc::default().svg()).unwrap();
    let mut inputs: Vec<(String, std::path::PathBuf, &str)> =
        vec![("synthetic".into(), synthetic.clone(), "fig0")];
    if let Some(p) = std::env::var_os("SCIINK_BIG_SVG") {
        inputs.push(("SCIINK_BIG_SVG".into(), p.into(), "figure_1-3"));
    }
    for (name, path, figure) in inputs {
        for args in [
            vec!["--tool=flattener".to_string(), "--id=layer1".to_string()],
            vec!["--tool=flattener".to_string(), format!("--id={figure}")],
            vec![
                "--tool=homogenizer".to_string(),
                "--setfontsize=true".to_string(),
                "--fontsize=7".to_string(),
                "--fontmodes=2".to_string(),
                "--id=layer1".to_string(),
            ],
        ] {
            let args: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();
            let log = log_for(&path, &args);
            println!("== {name} {}\n{log}", args.join(" "));
            let t = total_ms(&log);
            assert!(t < 60_000.0, "{name} {}: total {t} ms", args.join(" "));
        }
    }
}
