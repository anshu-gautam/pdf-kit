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
        ("two-tables.pdf", pdfkit_fixtures::two_tables()),
        ("forms.pdf", pdfkit_fixtures::forms()),
        ("encrypted.pdf", pdfkit_fixtures::encrypted_default()),
        (
            "outline-and-links.pdf",
            pdfkit_fixtures::outline_and_links(),
        ),
        ("cyclic-outline.pdf", pdfkit_fixtures::cyclic_outline()),
        ("tagged-minimal.pdf", pdfkit_fixtures::tagged_minimal()),
        ("type0-identity.pdf", pdfkit_fixtures::type0_identity()),
        (
            "figure-with-caption.pdf",
            pdfkit_fixtures::figure_with_caption(),
        ),
    ];

    for (name, bytes) in outputs {
        let path = dir.join(name);
        std::fs::write(&path, bytes)?;
        println!("wrote {} ({} bytes)", path.display(), bytes.len());
    }
    Ok(())
}
