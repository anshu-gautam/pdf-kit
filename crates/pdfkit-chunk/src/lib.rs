//! `pdfkit-chunk` — structured / RAG chunking.
//!
//! Groups text runs into blocks, classifies them (heading / paragraph / list /
//! table / caption), tracks a heading stack, and packs blocks into token-sized
//! chunks. Exposes [`Chunk`], [`ChunkOptions`], and `chunk_document`.
//!
//! Implemented in M6 of `Prd.md`.
