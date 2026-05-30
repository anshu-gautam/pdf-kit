//! Chunk a PDF and print a summary of the structured chunks.
//!
//! Usage: cargo run --example chunk_pdf -p pdfkit-chunk -- <file.pdf>

use pdfkit_chunk::{chunk_document, ChunkOptions};
use pdfkit_core::{Engine, OpenOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: chunk_pdf <file.pdf>")?;

    let doc = Engine::new()?.open(path, OpenOptions::default())?;
    let chunks = chunk_document(&doc, &ChunkOptions::default())?;

    println!(
        "{} chunks across {} pages\n",
        chunks.len(),
        doc.page_count()
    );
    for (i, c) in chunks.iter().take(15).enumerate() {
        let preview: String = c
            .text
            .chars()
            .take(64)
            .collect::<String>()
            .replace('\n', " ");
        println!(
            "[{i:>2}] p{:<2} {:<9?} ~{:>3}tok  path={:?}\n      {preview}",
            c.page, c.kind, c.token_estimate, c.heading_path
        );
    }
    Ok(())
}
