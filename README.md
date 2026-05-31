# pdfkit

A from-scratch, AI-oriented PDF toolkit in Rust. **Read-first extraction**
(text → OCR → render fallback), **structured output for RAG** (chunks +
JSON/Markdown/HTML/CSV with provenance), and a **separate edit/create path** —
in one Cargo workspace with feature flags, so you compile in only what you need.

> Deterministic and offline by default. No hosted-LLM calls anywhere except an
> opt-in adapter where *you* supply the model client; OCR runs locally (ONNX).

See [`CLAUDE.md`](./CLAUDE.md) for
working conventions.

## What you get

- **Text extraction that reads correctly** — layout-aware reflow with
  multi-column reading order, accurate word spacing (real `/Widths` and Type0
  CID metrics), and encoding-aware decoding.
- **Structured chunks for RAG** — headings, paragraphs, lists, tables, captions,
  and figures, with a heading breadcrumb, token-sized packing, and optional
  overlap. Serialize to **JSON, Markdown, HTML, or CSV**.
- **Provenance on every chunk** — one-based page, bounding box, a stable content
  `id`, and an exact `char_start`/`char_len` span back into the reconstructed
  document text. The grounding hosted/VLM parsers don't give you.
- **Real tables** — a normalized cell grid (column inference, colspan, per-cell
  bbox) rendered to HTML/CSV/Markdown, not a tab-joined blob.
- **Authoritative structure when present** — reads the tagged-PDF
  `/StructTreeRoot` (heading levels, table cells, list nesting, figure alt-text,
  reading order) and prefers it over geometry heuristics.
- **Document structure** — outline/bookmarks, link annotations (URI + internal
  page), figures with their captions, and full info-dict metadata.
- **Scanned pages** — local, offline OCR (ONNX via `ocrs`) or system Tesseract,
  behind feature flags.
- **A separate write path** — create / merge / split / rotate / watermark / fill
  forms, which never flows through the extraction engine.
- **Three surfaces from one tree** — native CLI, library (server), and WebAssembly.

## Design principles

- **Deterministic, offline core.** No network, no hosted model in the default
  build. A network LLM call exists only in `pdfkit-adapters` behind
  `llm-adapter`, where the caller supplies the client.
- **Pure-Rust by default.** The default build compiles and tests with **zero
  native dependencies and zero network access**. PDFIUM and Tesseract are opt-in
  feature flags only.
- **Reading and writing are separate subsystems.** The edit path and the
  extraction engine share only the document model.
- **Never panics on untrusted input.** Library paths return `Result`; a
  `cargo-fuzz` harness plus an always-on no-panic test guard the invariant
  (PDFs are hostile input). See [Robustness](#robustness).

## Workspace layout

| Crate | Responsibility |
| --- | --- |
| `pdfkit-core` | Document model, text extraction, reading-order/line grouping, page classification, outline/links/metadata, tagged-structure reader, figure detection, the render types + native renderer, the OCR abstraction, and the `extract` entry point. Everything depends on this. |
| `pdfkit-render` | Public render surface: re-exports core's render API and adds the PDFIUM backend (`render-pdfium`). |
| `pdfkit-ocr` | OCR providers: local ONNX (`ocr-ocrs`) and system Tesseract (`ocr-tesseract`); re-exports the core OCR abstraction. |
| `pdfkit-chunk` | Structured / RAG chunks (page, bbox, kind, heading path, char spans, stable ids), the normalized table grid, tagged-structure-aware chunking, and JSON / Markdown / HTML / CSV serialization. |
| `pdfkit-edit` | Create / merge / split / rotate / watermark / fill forms. Write path. |
| `pdfkit-adapters` | Message content blocks, data URLs, figure image blocks, and the opt-in `llm-adapter`. |
| `pdfkit-cli` | The `pdfkit` command-line tool. |
| `pdfkit-wasm` | `wasm-bindgen` surface for the browser / npm. |
| `pdfkit-fixtures` | Internal: deterministic synthetic test PDFs (not published). |
| `fuzz/` | `cargo-fuzz` harness (a detached workspace; see [Robustness](#robustness)). |

> Architecture note: the render types/trait + the pure-Rust `NativeRenderer` and
> the OCR trait live in `pdfkit-core` so the in-core `extract` entry point can
> use them without a dependency cycle. `pdfkit-render` / `pdfkit-ocr` re-export
> those and add the heavy/optional backends.

## Quick start (CLI)

```bash
# Extract text  (use --json for the structured extraction result)
pdfkit document.pdf
pdfkit document.pdf --json

# Structured RAG chunks — JSON (lossless), Markdown, or plain reading-order text
pdfkit chunk document.pdf --format json
pdfkit chunk document.pdf --format md
pdfkit chunk document.pdf --format text --target-tokens 512 --overlap-tokens 64 --context

# Inspect document structure (all emit JSON)
pdfkit outline   document.pdf   # bookmarks / table of contents (with page numbers)
pdfkit structure document.pdf   # tagged-PDF logical structure tree, or {"tagged": false}
pdfkit figures   document.pdf   # image/figure regions: page, bbox, caption

# Encrypted documents / stdin
pdfkit secret.pdf --password hunter2
pdfkit secret.pdf --password-file ./pass.txt
cat document.pdf | pdfkit -

# Render a page to PNG (pure-Rust native backend: rasterizes embedded images,
# not vector/text — see backends below)
pdfkit render document.pdf --page 1 -o page1.png --dpi 200
```

Exit codes: `0` success, `2` usage error, `3` wrong/missing password, `1` other.

Every subcommand accepts `--password` / `--password-file` and `-` for stdin.

### Rendering backends

| `--backend` | Needs | Renders |
| --- | --- | --- |
| `native` (default build) | nothing (pure Rust) | page background + embedded raster images (great for scans; **blank for vector/text pages**) |
| `pdfium` | `--features render-pdfium` + `scripts/fetch-pdfium.sh` | full fidelity — text, vector, and images |
| `auto` | — | PDFIUM when compiled in and available, else native |

```bash
scripts/fetch-pdfium.sh   # one-time: download libpdfium into ~/.cache/pdfkit
cargo build --release -p pdfkit-cli --features render-pdfium
pdfkit render document.pdf --page 1 --backend pdfium -o page1.png --dpi 200
```

PDFIUM is located at runtime via `$PDFKIT_PDFIUM_LIB`, then
`~/.cache/pdfkit/pdfium/lib/`, then the system library path.

## Quick start (library)

**Extract** (text, with an automatic render/OCR fallback for scans):

```rust
use pdfkit_core::{extract, ExtractOptions, Mode};

let result = extract("document.pdf", ExtractOptions { mode: Mode::Auto, ..Default::default() })?;
println!("{}", result.text);
for image in &result.images {
    // image.page, image.width, image.height, image.png (PNG bytes)
}
# Ok::<(), pdfkit_core::PdfError>(())
```

**Chunk for RAG, then serialize** — chunks carry full provenance, and on a
*tagged* PDF the structure tree drives the chunking automatically:

```rust
use pdfkit_core::{Engine, OpenOptions};
use pdfkit_chunk::{chunk_document, to_json, to_markdown, document_text, ChunkOptions};

let doc = Engine::new()?.open("document.pdf", OpenOptions::default())?;
let chunks = chunk_document(&doc, &ChunkOptions::default())?;

// Each chunk: id, text, page (1-based), bbox, kind (Heading/Paragraph/List/
// Table/Caption/Figure), heading_path, char_start/char_len, token_estimate,
// and `table` (a cell grid) for Table chunks.
let json = to_json(&chunks)?;        // lossless, with provenance (needs the `serde` feature)
let md   = to_markdown(&chunks);     // headings, GFM tables, lists, captions
let text = document_text(&chunks);   // reading-order text the char spans index into
# Ok::<(), pdfkit_core::PdfError>(())
```

**Read document structure** (all read-only, deterministic):

```rust
use pdfkit_core::{Engine, OpenOptions};

let doc = Engine::new()?.open("document.pdf", OpenOptions::default())?;

let outline = doc.outline();                  // Vec<OutlineItem> bookmark tree (resolved page numbers)
let links   = doc.page(1)?.links();           // Vec<Link>: rect + LinkTarget (Uri | Page)
let figures = doc.page(1)?.image_regions();   // figures: bbox + nearest caption
let meta    = doc.metadata();                 // title/author/subject/keywords/creator/producer/dates

if let Some(tree) = doc.structure_tree() {    // tagged-PDF logical structure, when present
    // tree.tag, tree.text, tree.alt, tree.page, tree.children (reading order)
}
# Ok::<(), pdfkit_core::PdfError>(())
```

**Extract a figure as an image** for a multimodal model:

```rust
use pdfkit_core::{Engine, OpenOptions, NativeRenderer, Renderer, RenderOptions, encode_png};
use pdfkit_adapters::image_block;

let doc  = Engine::new()?.open("document.pdf", OpenOptions::default())?;
let page = doc.page(1)?;
let (w, h) = page.size_points();
let bitmap = NativeRenderer.render(&page, &RenderOptions::default())?;
for region in page.image_regions() {
    let crop = bitmap.crop_region(w, h, region.bbox);   // pixels for this figure
    let png  = encode_png(&crop, true)?;
    let _block = image_block(png);                      // -> a model message ContentBlock
}
# Ok::<(), pdfkit_core::PdfError>(())
```

**Create / edit** (write path):

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
| `serde` | chunk | ✅ on | `Serialize`/`Deserialize` + `to_json` for chunks. |
| `render-pdfium` | render | off | High-fidelity render via PDFIUM (runtime native lib). |
| `ocr-ocrs` | ocr | off | Local ONNX OCR via `ocrs` (pure Rust). |
| `ocr-tesseract` | ocr | off | Tesseract OCR (system dependency). |
| `wasm` | core | off | Route RNG through JS for the wasm32 build. |
| `llm-adapter` | adapters | off | Opt-in LLM titling adapter (caller supplies the client). |

The default build compiles and passes tests with **no** native dependencies and
**no** network. The minimal build is verified in CI:

```bash
cargo test --workspace --no-default-features --features render-native
```

## OCR (scanned pages)

`ocr-ocrs` performs **local, offline** OCR via `ocrs` + `rten` (pure Rust, no
native deps). It loads `.rten` models that are not vendored in git — fetch them
once:

```bash
scripts/fetch-ocr-models.sh   # downloads into $PDFKIT_OCR_MODELS or ~/.cache/pdfkit/models
```

Then recover text from scanned/image-only pages:

```rust
use pdfkit_core::{extract_with_ocr, ExtractOptions};
use pdfkit_ocr::OcrsProvider;

let provider = OcrsProvider::new()?;                 // loads the cached models
let opts = ExtractOptions { ocr: true, ..Default::default() };
let result = extract_with_ocr("scan.pdf", opts, &provider)?;
# Ok::<(), pdfkit_core::PdfError>(())
```

`ocrs` is an early-stage engine (Latin script, preview-grade accuracy); for the
highest accuracy, plug in `ocr-tesseract` or your own `OcrProvider`.

## Robustness

PDFs are untrusted input, so the read paths must never panic, hang, or read out
of bounds. Two layers guard this:

- **`crates/pdfkit-chunk/tests/no_panic.rs`** — an always-on, deterministic test
  (runs in normal CI) that feeds every fixture plus truncations, bit-flips, and
  seeded random blobs through open → extract → chunk → all the readers, asserting
  no panic.
- **`fuzz/`** — a `cargo-fuzz` (libFuzzer) harness for deep, on-demand campaigns,
  seeded from the committed `fixtures/`. It's a detached workspace, so it never
  affects the stable build; CI builds it and runs a short smoke campaign.

```bash
cargo +nightly fuzz run parse           # deep fuzzing (needs cargo-fuzz)
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
cargo +nightly fuzz run parse                                          # fuzz (needs cargo-fuzz)
```

## Provenance

- The default and `render-native` builds use only MIT/Apache-2.0 pure-Rust
  dependencies (lopdf, image, png, thiserror, clap, serde).
- `render-pdfium` binds a PDFIUM library at runtime via `pdfium-render` 0.9; the
  library is **not** vendored. `scripts/fetch-pdfium.sh` downloads it from
  [`bblanchon/pdfium-binaries`](https://github.com/bblanchon/pdfium-binaries)
  (verified against tag `chromium/7857`, mac-arm64) into `~/.cache/pdfkit`.
- `ocr-ocrs` runs local OCR via `ocrs` + `rten` and downloads ONNX `.rten`
  models via `scripts/fetch-ocr-models.sh` (from the ocrs model bucket); the
  models are not vendored.

## Known limitations

Tagged-PDF chunks currently omit per-cell bbox (page + char offsets still
locate them); tagged tables are emitted as text (the cell grid is reconstructed
for untagged/geometry tables); figure placement uses a single y-axis order; and
true row spans need ruled-line parsing. These are tracked as `TODO(design)`
notes in the source.

## License

Licensed under either of [MIT](./LICENSE-MIT) or
[Apache-2.0](./LICENSE-APACHE) at your option.
