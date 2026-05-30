# CLAUDE.md — pdfkit

pdfkit is a from-scratch, AI-oriented PDF toolkit in Rust: read-first extraction
(text → OCR → render fallback), structured chunk output, and a separate
edit/create path. Built as a Cargo workspace with feature flags.

The full build is specified in `IMPLEMENTATION_PLAN.md`. Read it before starting,
and work one milestone (M0–M11) at a time. Do not start the next milestone until
the current one's acceptance criteria pass.

## Architecture invariants (do not violate)

- The core is deterministic and offline. No hosted-LLM calls anywhere except in
  `pdfkit-adapters` behind the `llm-adapter` feature, where the caller supplies
  their own model client. Local ML for OCR (ONNX via ocrs) is fine; a network
  LLM call is not.
- The default feature set must compile and pass tests with **zero native
  dependencies and zero network access**. Any heavy, native, or networked
  dependency goes behind a Cargo feature, never in the default set.
- Reading and writing are separate subsystems. `pdfkit-edit` depends only on
  `pdfkit-core` and never flows through the extraction engine.
- Public API uses **one-based** page numbers. Convert at the boundary; internal
  indexing may be zero-based.
- Library code returns `Result<_, PdfError>`. No `unwrap()`, `expect()`, or
  `panic!` on any library path.

## Workflow

- Test-first where practical: write the milestone's acceptance test, then
  implement until it passes.
- After every change, in order:
  1. `cargo fmt --all`
  2. `cargo clippy --workspace --all-targets -- -D warnings`
  3. `cargo test --workspace`
- Before declaring a milestone done, also run the minimal-build check:
  `cargo test --workspace --no-default-features --features render-native`
  (this proves the zero-native-dependency path still works).
- Commit per milestone (or per self-contained sub-feature) with a message that
  names the deliverable and states that tests pass.
- If a design choice is ambiguous, pick the simplest implementation that meets
  the acceptance criteria and leave a `// TODO(design): ...` note rather than
  blocking. Surface anything in `IMPLEMENTATION_PLAN.md` §12 you cannot resolve.

## Commands

- Build everything: `cargo build --workspace`
- Default tests: `cargo test --workspace`
- Minimal (zero-native-dep) tests: `cargo test --workspace --no-default-features --features render-native`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt --all`
- WASM build: `wasm-pack build crates/pdfkit-wasm`
- Benchmarks: `cargo bench`

## Crate boundaries

- `pdfkit-core` — document model, text extraction, page classification, the
  `extract` entry point. Everything depends on this.
- `pdfkit-render` — page → RGBA → PNG. Backend behind `render-pdfium` /
  `render-native`. Enforce the pixel budget before allocating.
- `pdfkit-ocr` — rasterize + OCR. `ocr-ocrs` (default-off, local) /
  `ocr-tesseract` (system dep).
- `pdfkit-chunk` — structured/RAG chunks with page, bbox, kind, heading path.
- `pdfkit-edit` — create/merge/split/rotate/watermark/fill forms. Write path.
- `pdfkit-adapters` — message blocks, data urls, opt-in `llm-adapter`.
- `pdfkit-cli`, `pdfkit-wasm` — surfaces; add last.

## Do not

- Do not add a network or native dependency to the default features.
- Do not add an LLM call outside `pdfkit-adapters` + `llm-adapter`.
- Do not vendor OCR models or large binaries into git; download into a cache dir
  via a setup script.
- Do not pull in a GPL-licensed dependency without flagging it first — it would
  force the whole project's license. Check `IMPLEMENTATION_PLAN.md` §12.5.
- Do not skip clippy or fmt before a commit.

## Notes

- This file is guidance, not an enforced gate. The hard gate is CI
  (`.github/workflows/ci.yml`): fmt, clippy, default tests, the minimal-build
  test, and the WASM build must all be green before a milestone is considered
  complete.