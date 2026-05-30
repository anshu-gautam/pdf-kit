# pdfkit

A from-scratch, AI-oriented PDF toolkit in Rust. Read-first extraction with a
**text → OCR → render** fallback, structured chunk output for RAG, and a
separate edit/create path. Built as a Cargo workspace with feature flags so
consumers compile in only what they need.

> Status: under active construction. See [`Prd.md`](./Prd.md) for the full
> implementation plan and milestone roadmap (M0–M11), and
> [`CLAUDE.md`](./CLAUDE.md) for working conventions.

## Why

- **Deterministic, offline core.** No hosted-LLM calls anywhere except an opt-in
  adapter where *you* supply the model client. OCR runs locally (ONNX).
- **Pure-Rust by default.** The default build compiles and tests with zero
  native dependencies and zero network access. PDFium and Tesseract are opt-in
  feature flags only.
- **One source tree, three surfaces.** Native binary (CLI), library (server),
  and WebAssembly (browser / npm).
- **Reading and writing are separate subsystems.** The edit path never flows
  through the extraction engine; they share only the document model.

## Workspace layout

| Crate | Responsibility |
| --- | --- |
| `pdfkit-core` | Document model, text extraction, page classification, the `extract` entry point. Everything depends on this. |
| `pdfkit-render` | Page → RGBA → PNG. Backend behind `render-pdfium` / `render-native`. Enforces a pixel budget. |
| `pdfkit-ocr` | Rasterize + OCR. `ocr-ocrs` (local ONNX) / `ocr-tesseract` (system dep). |
| `pdfkit-chunk` | Structured / RAG chunks with page, bbox, kind, heading path. |
| `pdfkit-edit` | Create / merge / split / rotate / watermark / fill forms. Write path. |
| `pdfkit-adapters` | Message blocks, data URLs, opt-in `llm-adapter`. |
| `pdfkit-cli` | Command-line surface. |
| `pdfkit-wasm` | `wasm-bindgen` surface. |

## Feature flags

| Flag | Default | Effect |
| --- | --- | --- |
| `render-native` | ✅ on | Pure-Rust render path. |
| `render-pdfium` | off | High-fidelity render via PDFium (native or WASM). |
| `ocr-ocrs` | off | Local ONNX OCR via ocrs. |
| `ocr-tesseract` | off | Tesseract OCR (system dependency). |
| `edit` | ✅ on | Editing + creation. |
| `chunk` | ✅ on | Structured chunking. |
| `llm-adapter` | off | Opt-in LLM titling/cleanup adapter. |
| `wasm` | off | wasm-bindgen surface. |

The default build (`render-native`, `edit`, `chunk`) must compile and pass tests
with **no** native dependencies and **no** network.

## Quick start

```bash
# Build everything
cargo build --workspace

# Run the default test suite
cargo test --workspace

# Prove the zero-native-dependency path still works
cargo test --workspace --no-default-features --features render-native

# Lint + format (run before every commit)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

## License

Licensed under either of [MIT](./LICENSE-MIT) or
[Apache-2.0](./LICENSE-APACHE) at your option. Every dependency in the default
build is MIT/Apache-2.0 compatible; GPL-licensed dependencies are not pulled in
without an explicit license decision (PRD §12.5).
