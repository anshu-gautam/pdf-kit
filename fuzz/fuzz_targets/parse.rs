//! Fuzz the read paths: open arbitrary bytes and run extraction, classification,
//! the readers (outline / structure tree / links / figures), and chunking +
//! serialization. The library must never panic / abort on any input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfkit_chunk::{chunk_document, document_text, to_json, to_markdown, ChunkOptions};
use pdfkit_core::{extract, Engine, ExtractOptions, Mode, OpenOptions};

fuzz_target!(|data: &[u8]| {
    let Ok(engine) = Engine::new() else {
        return;
    };
    if let Ok(doc) = engine.open(data.to_vec(), OpenOptions::default()) {
        let _ = doc.metadata();
        for p in 1..=doc.page_count().min(8) {
            if let Ok(page) = doc.page(p) {
                let _ = page.text();
                let _ = page.text_runs();
                let _ = page.classify();
                let _ = page.signals();
                let _ = page.links();
                let _ = page.image_regions();
            }
        }
        let _ = doc.outline();
        let _ = doc.structure_tree();
        if let Ok(chunks) = chunk_document(&doc, &ChunkOptions::default()) {
            let _ = to_markdown(&chunks);
            let _ = document_text(&chunks);
            let _ = to_json(&chunks);
        }
    }
    // The `extract` entry opens internally; exercise it too (text mode = no render).
    let _ = extract(
        data.to_vec(),
        ExtractOptions {
            mode: Mode::Text,
            ..ExtractOptions::default()
        },
    );
});
