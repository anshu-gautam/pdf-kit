//! Write the generated fixtures into the workspace `fixtures/` directory so they
//! can be committed and consumed by the CLI and cross-crate tests.
//!
//! Run with: `cargo run -p pdfkit-fixtures --bin write-fixtures`

use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    // crates/pdfkit-fixtures -> workspace root -> fixtures/
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures");
    std::fs::create_dir_all(&dir)?;

    let outputs: &[(&str, Vec<u8>)] = &[
        ("born-digital.pdf", pdfkit_fixtures::born_digital()),
        ("scanned.pdf", pdfkit_fixtures::scanned()),
        ("mixed.pdf", pdfkit_fixtures::mixed()),
        ("multi-heading.pdf", pdfkit_fixtures::multi_heading()),
        ("table.pdf", pdfkit_fixtures::table_doc()),
        ("forms.pdf", pdfkit_fixtures::forms()),
        ("encrypted.pdf", pdfkit_fixtures::encrypted_default()),
    ];

    for (name, bytes) in outputs {
        let path = dir.join(name);
        std::fs::write(&path, bytes)?;
        println!("wrote {} ({} bytes)", path.display(), bytes.len());
    }
    Ok(())
}
