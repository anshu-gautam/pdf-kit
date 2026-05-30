//! `pdfkit-adapters` — turn extraction results into model-ready shapes.
//!
//! `to_message_content` and `to_data_urls` are deterministic and offline. The
//! `llm-adapter` feature adds an opt-in [`LlmClient`] trait and `title_chunks`;
//! it is the only place in the workspace a model is invoked, and it never ships
//! a default client.
//!
//! Implemented in M8 of `Prd.md`.
