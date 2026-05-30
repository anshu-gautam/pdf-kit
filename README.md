# pdfkit

A from-scratch, AI-oriented PDF toolkit in Rust. Read-first extraction with a
**text → OCR → render** fallback, structured chunk output for RAG, and a
separate edit/create path. Built as a Cargo workspace with feature flags so
consumers compile in only what they need.

> Built milestone by milestone per [`Prd.md`](./Prd.md) (M0–M11). See
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
| `pdfkit-core` | Document model, text extraction, page classification, positioned text runs, the render types + native renderer, the OCR abstraction, and the `extract` entry point. Everything depends on this. |
| `pdfkit-render` | Public render surface: re-exports core's render API and adds the PDFIUM backend (`render-pdfium`). |
| `pdfkit-ocr` | OCR providers: local ONNX (`ocr-ocrs`) and system Tesseract (`ocr-tesseract`); re-exports the core OCR abstraction. |
| `pdfkit-chunk` | Structured / RAG chunks with page, bbox, kind, and heading path. |
| `pdfkit-edit` | Create / merge / split / rotate / watermark / fill forms. Write path. |
| `pdfkit-adapters` | Message content blocks, data URLs, and the opt-in `llm-adapter`. |
| `pdfkit-cli` | The `pdfkit` command-line tool. |
| `pdfkit-wasm` | `wasm-bindgen` surface for the browser / npm. |
| `pdfkit-fixtures` | Internal: deterministic synthetic test PDFs (not published). |

> Architecture note: the render types/trait + the pure-Rust `NativeRenderer` and
> the OCR trait live in `pdfkit-core` so the in-core `extract` entry point can
> use them without a dependency cycle (the PRD's `render → core` arrow plus
> "`extract` lives in core" would otherwise be circular). `pdfkit-render` /
> `pdfkit-ocr` re-export those and add the heavy/optional backends.

## Quick start (CLI)

```bash
# Extract text
pdfkit document.pdf

# Extraction result as JSON
pdfkit document.pdf --json

# Encrypted documents
pdfkit secret.pdf --password hunter2
pdfkit secret.pdf --password-file ./pass.txt

# Read from stdin
cat document.pdf | pdfkit -

# Render a page to PNG (pure-Rust native backend; rasterizes embedded images,
# not vector/text)
pdfkit render document.pdf --page 1 -o page1.png --dpi 200

# Faithful rendering of text/vector pages needs the PDFIUM backend:
scripts/fetch-pdfium.sh   # one-time: download libpdfium into ~/.cache/pdfkit
cargo build --release -p pdfkit-cli --features render-pdfium
pdfkit render document.pdf --page 1 --backend pdfium -o page1.png --dpi 200
```

Exit codes: `0` success, `2` usage error, `3` wrong/missing password, `1` other.

### Rendering backends

| `--backend` | Needs | Renders |
| --- | --- | --- |
| `native` (default build) | nothing (pure Rust) | page background + embedded raster images (great for scans; **blank for vector/text pages**) |
| `pdfium` | `--features render-pdfium` + `scripts/fetch-pdfium.sh` | full fidelity — text, vector, and images |
| `auto` | — | PDFIUM when compiled in and available, else native |

PDFIUM is located at runtime via `$PDFKIT_PDFIUM_LIB`, then
`~/.cache/pdfkit/pdfium/lib/`, then the system library path.

## Quick start (library)

```rust
use pdfkit_core::{extract, ExtractOptions, Mode};

let result = extract("document.pdf", ExtractOptions { mode: Mode::Auto, ..Default::default() })?;
println!("{}", result.text);
for image in &result.images {
    // image.page, image.width, image.height, image.png (PNG bytes)
}
# Ok::<(), pdfkit_core::PdfError>(())
```

Chunking for RAG:

```rust
use pdfkit_core::{Engine, OpenOptions};
use pdfkit_chunk::{chunk_document, ChunkOptions};

let doc = Engine::new()?.open("document.pdf", OpenOptions::default())?;
let chunks = chunk_document(&doc, &ChunkOptions::default())?;
// each chunk: text, page, bbox, kind, heading_path, token_estimate
# Ok::<(), pdfkit_core::PdfError>(())
```

Create / edit (write path):

```rust
use pdfkit_edit::{PdfBuilder, PageSize, FontSpec, PdfEditor};

let mut b = PdfBuilder::new();
let page = b.add_page(PageSize::Letter);
b.draw_text(page, "Hello", (72.0, 720.0), FontSpec::default());
let mut bytes = Vec::new();
b.save(&mut bytes)?;

let mut editor = PdfEditor::open("a.pdf")?;
editor.merge(&PdfEditor::open("b.pdf")?)?;
# Ok::<(), pdfkit_core::PdfError>(())
```

## Feature flags

| Flag | Crate | Default | Effect |
| --- | --- | --- | --- |
| `render-native` | core / render | ✅ on | Pure-Rust render path (`image` crate). |
| `render-pdfium` | render | off | High-fidelity render via PDFIUM. |
| `ocr-ocrs` | ocr | off | Local ONNX OCR via ocrs. |
| `ocr-tesseract` | ocr | off | Tesseract OCR (system dependency). |
| `wasm` | core | off | Route RNG through JS for the wasm32 build. |
| `llm-adapter` | adapters | off | Opt-in LLM titling adapter. |

The default build (`render-native`) compiles and passes tests with **no** native
dependencies and **no** network.

## OCR models

`ocr-ocrs` loads local `.rten` models that are not vendored in git. Fetch them
once into a cache directory:

```bash
scripts/fetch-ocr-models.sh   # downloads into $PDFKIT_OCR_MODELS or ~/.cache/pdfkit/models
```

## Performance

Criterion benchmarks over the synthetic fixtures (`cargo bench -p pdfkit-core`),
Apple Silicon, release build — indicative, not a guarantee:

| Benchmark | Time |
| --- | --- |
| open + text (born-digital) | ~49 µs |
| open + text (multi-heading) | ~58 µs |
| classify (born-digital) | ~11 µs |
| classify (scanned) | ~5 µs |
| classify (mixed) | ~11 µs |
| render (scanned → 425px-wide PNG) | ~176 µs |

## Commands

```bash
cargo build --workspace
cargo test --workspace
cargo test --workspace --no-default-features --features render-native  # zero-native-dep path
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo bench -p pdfkit-core
wasm-pack build crates/pdfkit-wasm
cargo run -p pdfkit-fixtures --bin write-fixtures                       # regenerate fixtures/
```

## Provenance

- The default and `render-native` builds use only MIT/Apache-2.0 pure-Rust
  dependencies (lopdf, image, png, thiserror, clap).
- `render-pdfium` binds a PDFIUM library at runtime via `pdfium-render` 0.9; the
  library is **not** vendored. `scripts/fetch-pdfium.sh` downloads it from
  [`bblanchon/pdfium-binaries`](https://github.com/bblanchon/pdfium-binaries)
  (verified against tag `chromium/7857`, mac-arm64) into `~/.cache/pdfkit`.
- `ocr-ocrs` downloads ONNX `.rten` models via the setup script; they are not
  vendored.

## License

Licensed under either of [MIT](./LICENSE-MIT) or
[Apache-2.0](./LICENSE-APACHE) at your option.
