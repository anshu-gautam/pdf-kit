//! Always-on robustness gate (the deterministic complement to the cargo-fuzz
//! harness in `fuzz/`): feed fixtures, truncations, bit-flips, and random blobs
//! through every read path and assert the library never panics on any input
//! (the hard invariant). A panic anywhere aborts the test.

use pdfkit_chunk::{chunk_document, document_text, to_json, to_markdown, ChunkOptions};
use pdfkit_core::{extract, Engine, ExtractOptions, Mode, OpenOptions};

/// Run every read/extract/chunk path over `bytes`. Must not panic for any input.
fn read_all(bytes: &[u8]) {
    let engine = Engine::new().expect("engine");
    if let Ok(doc) = engine.open(bytes.to_vec(), OpenOptions::default()) {
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
    // The `extract` entry point opens internally; exercise it too (text mode, so
    // no rendering is required).
    let _ = extract(
        bytes.to_vec(),
        ExtractOptions {
            mode: Mode::Text,
            ..ExtractOptions::default()
        },
    );
}

/// Tiny deterministic xorshift64 PRNG (no dependency; reproducible).
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

#[test]
fn read_paths_never_panic_on_arbitrary_bytes() {
    let fixtures: Vec<Vec<u8>> = vec![
        pdfkit_fixtures::born_digital(),
        pdfkit_fixtures::scanned(),
        pdfkit_fixtures::mixed(),
        pdfkit_fixtures::multi_heading(),
        pdfkit_fixtures::table_doc(),
        pdfkit_fixtures::two_tables(),
        pdfkit_fixtures::forms(),
        pdfkit_fixtures::encrypted_default(),
        pdfkit_fixtures::outline_and_links(),
        pdfkit_fixtures::cyclic_outline(),
        pdfkit_fixtures::tagged_minimal(),
        pdfkit_fixtures::type0_identity(),
        pdfkit_fixtures::figure_with_caption(),
    ];
    let mut rng = Rng::new(0x9e37_79b9_7f4a_7c15);

    for seed in &fixtures {
        read_all(seed); // valid input

        // Truncations (header-only, mid-stream, near-complete).
        for frac in [1usize, 2, 4, 8, 16] {
            read_all(&seed[..seed.len() / frac]);
        }

        // Bit-flipped corruptions.
        for _ in 0..16 {
            let mut m = seed.clone();
            let flips = 1 + (rng.next() as usize % 8);
            for _ in 0..flips {
                if m.is_empty() {
                    break;
                }
                let i = rng.next() as usize % m.len();
                m[i] ^= (rng.next() & 0xff) as u8;
            }
            read_all(&m);
        }
    }

    // Pure random blobs.
    for _ in 0..64 {
        let len = (rng.next() as usize % 4096) + 1;
        let blob: Vec<u8> = (0..len).map(|_| (rng.next() & 0xff) as u8).collect();
        read_all(&blob);
    }
}
