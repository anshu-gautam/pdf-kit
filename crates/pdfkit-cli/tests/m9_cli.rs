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
fn chunk_json_is_valid_array_with_provenance() {
    let file = temp("chunk-json.pdf", &pdfkit_fixtures::multi_heading());
    let out = bin()
        .arg("chunk")
        .arg(&file)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();
    assert!(out.status.success(), "exit {:?}", out.status.code());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let arr = value.as_array().expect("json array of chunks");
    assert!(!arr.is_empty());
    let first = &arr[0];
    for key in [
        "id",
        "text",
        "page",
        "kind",
        "heading_path",
        "char_start",
        "char_len",
    ] {
        assert!(first.get(key).is_some(), "chunk missing {key}: {first}");
    }
}

#[test]
fn chunk_markdown_renders_headings() {
    let file = temp("chunk-md.pdf", &pdfkit_fixtures::multi_heading());
    let out = bin()
        .arg("chunk")
        .arg(&file)
        .arg("--format")
        .arg("md")
        .output()
        .unwrap();
    assert!(out.status.success());
    let md = String::from_utf8_lossy(&out.stdout);
    assert!(md.contains("# Chapter One"), "markdown: {md}");
    assert!(md.contains("Alpha body"));
}

#[test]
fn chunk_text_is_reading_order_plain_text() {
    let file = temp("chunk-text.pdf", &pdfkit_fixtures::multi_heading());
    let out = bin()
        .arg("chunk")
        .arg(&file)
        .arg("--format")
        .arg("text")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Chapter One"));
    assert!(
        !text.contains('#'),
        "plain text must not have markdown markers: {text}"
    );
}

#[test]
fn outline_json_lists_bookmarks() {
    let file = temp("outline.pdf", &pdfkit_fixtures::outline_and_links());
    let out = bin().arg("outline").arg(&file).output().unwrap();
    assert!(out.status.success(), "exit {:?}", out.status.code());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let arr = v.as_array().expect("array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["title"], "Chapter 1");
    assert_eq!(arr[0]["page"], 1);
    assert_eq!(arr[0]["children"][0]["title"], "Section 1.1");
}

#[test]
fn structure_json_for_tagged_document() {
    let file = temp("tagged.pdf", &pdfkit_fixtures::tagged_minimal());
    let out = bin().arg("structure").arg(&file).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(v["tag"], "Root");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("\"Title\"") && text.contains("A pie chart"),
        "{text}"
    );
}

#[test]
fn structure_json_untagged_is_false() {
    let file = temp("born-struct.pdf", &pdfkit_fixtures::born_digital());
    let out = bin().arg("structure").arg(&file).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert_eq!(v["tagged"], false);
}

#[test]
fn figures_json_lists_regions_with_captions() {
    let file = temp("figs.pdf", &pdfkit_fixtures::figure_with_caption());
    let out = bin().arg("figures").arg(&file).output().unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let arr = v.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["page"], 1);
    assert_eq!(arr[0]["caption"], "Figure 1: A sample chart.");
    assert!(arr[0]["bbox"].is_array());
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
