//! M9 acceptance: each CLI command runs against fixtures with correct output and
//! exit codes. Drives the built `pdfkit` binary.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pdfkit"))
}

fn temp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("pdfkit-m9-{name}"));
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn extract_text_to_stdout() {
    let file = temp("born.pdf", &pdfkit_fixtures::born_digital());
    let out = bin().arg(&file).output().unwrap();
    assert!(out.status.success(), "exit {:?}", out.status.code());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Hello, pdfkit!"), "stdout: {text}");
}

#[test]
fn extract_json() {
    let file = temp("born-json.pdf", &pdfkit_fixtures::born_digital());
    let out = bin().arg(&file).arg("--json").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("\"pages_processed\""));
    assert!(text.contains("\"text\""));
    // Valid JSON.
    let _: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
}

#[test]
fn render_page_to_png_file() {
    let file = temp("scan.pdf", &pdfkit_fixtures::scanned());
    let png = std::env::temp_dir().join("pdfkit-m9-out.png");
    let out = bin()
        .arg("render")
        .arg(&file)
        .arg("--page")
        .arg("1")
        .arg("-o")
        .arg(&png)
        .output()
        .unwrap();
    assert!(out.status.success(), "exit {:?}", out.status.code());
    let bytes = std::fs::read(&png).unwrap();
    assert!(bytes.starts_with(&[137, 80, 78, 71]), "not a PNG");
}

#[test]
fn wrong_password_exits_three() {
    let file = temp("enc.pdf", &pdfkit_fixtures::encrypted_default());
    let out = bin()
        .arg(&file)
        .arg("--password")
        .arg("nope")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "expected password exit code");
}

#[test]
fn stdin_dash_is_read() {
    let mut child = bin()
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&pdfkit_fixtures::born_digital())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Hello, pdfkit!"));
}

#[test]
fn missing_args_exits_two() {
    let out = bin().output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn render_out_of_range_page_errors() {
    let file = temp("born-range.pdf", &pdfkit_fixtures::born_digital());
    let out = bin()
        .arg("render")
        .arg(&file)
        .arg("--page")
        .arg("99")
        .arg("-o")
        .arg(std::env::temp_dir().join("pdfkit-m9-never.png"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}
