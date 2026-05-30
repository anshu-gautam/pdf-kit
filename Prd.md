# pdfkit — Implementation Plan

A from-scratch, AI-oriented PDF toolkit in Rust. Read-first extraction with a text → OCR → render fallback, structured chunk output, and a separate edit/create path. Built as a Cargo workspace with feature flags so consumers compile in only what they need.

This document is written to be executed by Claude Code one milestone at a time. Each milestone has concrete deliverables and acceptance criteria. Do not skip ahead; finish and verify a milestone before starting the next.

---

## 1. Goals and non-goals

**Goals**
- A deterministic, LLM-free core that opens PDFs, extracts text, classifies pages, renders pages to PNG, OCRs scanned pages, and emits structured chunks.
- Pure-Rust by default (no native C++ engine, no network). PDFium and Tesseract are opt-in feature flags only.
- One source tree that targets native binary (CLI), library (server use), and WebAssembly (browser/npm).
- A separate write path for editing and creating PDFs.

**Non-goals**
- No hosted-LLM calls anywhere in the core. The only place a model is touched is an opt-in adapter where the caller supplies their own.
- No built-in vector database or retrieval pipeline. We emit chunks; retrieval is the consumer's architecture.
- Not aiming for full PDF/A or print-production fidelity in v1.

**Design rules that must hold throughout**
1. The core is deterministic and offline. OCR uses local ML models (ONNX), which is allowed; a network LLM call is not.
2. Reading and writing are separate subsystems. They share only the document model; the edit path never flows through the extraction engine.
3. Every heavy or optional dependency sits behind a Cargo feature. The default build must compile with zero native dependencies.
4. Public API uses typed errors (one `PdfError` enum). No `unwrap()`/`panic!` in library code paths.
5. One-based page numbers in all public API. Internal indexing can be zero-based but must convert at the boundary.

---

## 2. Tech stack

| Concern | Crate | Notes |
| --- | --- | --- |
| Low-level PDF parsing / object model | `lopdf` | Foundation for `pdfkit-core` and `pdfkit-edit`. Use the `nom`-based parser feature. |
| High-fidelity render (optional) | `pdfium-render` | Behind `render-pdfium`. Binds at runtime to a PDFium build (native or WASM). |
| Image buffers + PNG encode | `image`, `png` | RGBA bitmap handling and PNG output. |
| OCR (optional) | `ocrs` + `rten` | Behind `ocr-ocrs`. Pure-Rust, ONNX models, WASM-compatible. |
| OCR alt (optional) | `tesseract` bindings | Behind `ocr-tesseract`. Needs a system library; never in default build. |
| Tokenizer (chunk sizing) | `tiktoken-rs` or a simple heuristic | Token estimates for chunk splitting. Heuristic acceptable in v1. |
| CLI | `clap` (derive) | Argument parsing for `pdfkit-cli`. |
| WASM bindings | `wasm-bindgen`, `js-sys`, `web-sys` | For `pdfkit-wasm`. |
| Errors | `thiserror` | Derive the `PdfError` enum. |
| Async (only if needed) | `tokio` (optional) | Most of the core is sync; keep async out unless a backend forces it. |

Pin exact versions in each `Cargo.toml`. At time of writing: `pdfium-render` ~0.8/0.9, `lopdf` recent, `ocrs` ~0.2, `image` ~0.25. Resolve to the current stable on `cargo add` and record them in `Cargo.lock`.

---

## 3. Workspace layout

```
pdfkit/
├── Cargo.toml                 # workspace manifest
├── Cargo.lock
├── README.md
├── CLAUDE.md                  # agent working conventions (see §11)
├── rust-toolchain.toml        # pin stable channel
├── .github/workflows/ci.yml
├── crates/
│   ├── pdfkit-core/           # document model, text extraction, classification
│   ├── pdfkit-render/         # page -> RGBA -> PNG, feature-flagged backend
│   ├── pdfkit-ocr/            # rasterize + OCR scanned pages
│   ├── pdfkit-chunk/          # structured/RAG chunking
│   ├── pdfkit-edit/           # editing + creation (separate write path)
│   ├── pdfkit-adapters/       # message blocks, data urls, optional llm adapter
│   ├── pdfkit-cli/            # binary
│   └── pdfkit-wasm/           # wasm-bindgen wrapper
├── fixtures/                  # sample PDFs for tests (see §8)
└── benches/                   # criterion benchmarks
```

Dependency direction (a crate may only depend on those above it):
```
core  <-  render  <-  ocr
  ^          ^         ^
  |          |         |
  +-------- chunk -----+
  ^
edit (depends on core only)
adapters (depends on core + chunk)
cli, wasm (depend on everything they expose)
```

---

## 4. Crate specifications

### 4.1 `pdfkit-core`

The foundation. No rendering, no OCR.

Public types:
```rust
pub enum PdfInput {
    Path(PathBuf),
    Bytes(Vec<u8>),
    // browser builds add Blob/ArrayBuffer via the wasm crate
}

pub struct Engine { /* holds parser state, reusable */ }
pub struct Document { /* owns parsed lopdf::Document */ }
pub struct Page<'d> { /* borrows from Document */ }

pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub page_count: usize,
    pub pdf_version: String,
    pub encrypted: bool,
}

pub enum PageKind { TextBased, Scanned, ImageOnly, Mixed }
```

Public API:
```rust
impl Engine {
    pub fn new() -> Result<Self, PdfError>;
    pub fn open(&self, input: impl Into<PdfInput>, opts: OpenOptions) -> Result<Document, PdfError>;
}

pub struct OpenOptions { pub password: Option<String> }

impl Document {
    pub fn metadata(&self) -> &Metadata;
    pub fn page_count(&self) -> usize;
    pub fn page(&self, one_based: usize) -> Result<Page<'_>, PdfError>;
    pub fn pages(&self) -> impl Iterator<Item = Page<'_>>;
    pub fn text(&self, opts: TextOptions) -> Result<String, PdfError>;
}

impl Page<'_> {
    pub fn number(&self) -> usize;          // one-based
    pub fn size_points(&self) -> (f32, f32);
    pub fn rotation(&self) -> i32;
    pub fn text(&self) -> Result<String, PdfError>;
    pub fn classify(&self) -> PageKind;     // see classification below
}

pub struct TextOptions {
    pub pages: Option<Vec<usize>>,
    pub max_pages: usize,                    // default 20
    pub max_chars: usize,                    // default 200_000
}
```

Page classification logic (`classify`):
1. Count extractable characters on the page from the text layer.
2. Count and measure embedded images (coverage as fraction of page area).
3. Heuristic: lots of text and little image coverage → `TextBased`; almost no text and a single full-page image → `Scanned`; no text and image content → `ImageOnly`; both substantial → `Mixed`.
4. Expose the raw signals (`text_char_count`, `image_coverage`) so callers can tune thresholds.

Acceptance criteria for the crate: open a path and bytes input, read page count and metadata, extract text from a born-digital PDF, and classify a known text PDF vs a known scanned PDF correctly.

### 4.2 `pdfkit-render`

Turns a page into pixels and PNG bytes. Backend chosen by feature flag.

```rust
pub struct RenderOptions {
    pub dpi: Option<f32>,        // mutually exclusive with scale/width/height
    pub scale: Option<f32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub background: Background,   // White | Transparent
    pub forms: bool,             // render AcroForm widgets
    pub max_pixels: u32,         // default 4_000_000, checked before allocation
    pub max_dimension: u32,      // default 10_000
}

pub struct Bitmap { pub width: u32, pub height: u32, pub rgba: Vec<u8> }

pub trait Renderer {
    fn render(&self, page: &Page, opts: &RenderOptions) -> Result<Bitmap, PdfError>;
}

pub fn encode_png(bmp: &Bitmap, compress: bool) -> Result<Vec<u8>, PdfError>;
```

- `render-pdfium` feature: a `PdfiumRenderer` that wires `pdfium-render`. This is the high-fidelity path and gets clawpdf parity.
- `render-native` feature: a best-effort pure-Rust path. May lag on complex vector content; document the limitation. For scanned pages the work is mostly extracting/scaling the embedded image, which is tractable in pure Rust.
- Pixel budget must be enforced before allocating the buffer (raise `PdfError::Budget`).

Acceptance criteria: render page 1 of a fixture to a `Bitmap` and `encode_png` it; output is a valid PNG of expected dimensions; budget errors trigger correctly on an oversized request.

### 4.3 `pdfkit-ocr`

```rust
pub trait OcrProvider {
    fn recognize(&self, bmp: &Bitmap) -> Result<OcrResult, PdfError>;
}

pub struct OcrResult {
    pub text: String,
    pub confidence: f32,                 // 0.0..=1.0
    pub words: Vec<OcrWord>,             // text + bbox + confidence
}
```

- `ocr-ocrs` feature: `OcrsProvider` loading `.rten` models, runs locally. Models are downloaded by a build/setup script into a cache dir, not vendored in git.
- `ocr-tesseract` feature: `TesseractProvider`; requires system Tesseract.
- The crate also offers `ocr_page(page, renderer, provider) -> OcrResult`: rasterize the page, then recognize.

Acceptance criteria: given a scanned fixture, `ocr_page` returns non-empty text with a confidence score; works behind `ocr-ocrs` with no system dependency.

### 4.4 `pdfkit-chunk`

```rust
pub struct Chunk {
    pub text: String,
    pub page: usize,                     // one-based
    pub bbox: Option<[f32; 4]>,          // x0,y0,x1,y1 in points
    pub kind: ElementKind,               // Heading | Paragraph | List | Table | Caption
    pub heading_path: Vec<String>,       // breadcrumb of enclosing headings
    pub token_estimate: usize,
}

pub struct ChunkOptions {
    pub target_tokens: usize,            // default ~512
    pub overlap_tokens: usize,           // default 0
    pub respect_boundaries: bool,        // never split mid-paragraph (default true)
}

pub fn chunk_document(doc: &Document, opts: &ChunkOptions) -> Result<Vec<Chunk>, PdfError>;
```

Algorithm:
1. Pull text runs with positions per page.
2. Group runs into blocks by proximity and line/paragraph detection.
3. Classify each block (heading via relative font size/weight, list via leading glyphs, table via column alignment — basic heuristics in v1).
4. Maintain a heading stack to populate `heading_path`.
5. Pack blocks into chunks near `target_tokens` without crossing block boundaries; carry metadata.

Acceptance criteria: chunking a multi-heading fixture produces chunks with correct page numbers, populated `heading_path`, and sizes near the target.

### 4.5 `pdfkit-edit` (separate write path)

Depends only on `pdfkit-core` (shares the document model). Built on `lopdf`'s object model.

```rust
pub struct PdfBuilder { /* author a new document */ }
impl PdfBuilder {
    pub fn new() -> Self;
    pub fn add_page(&mut self, size: PageSize) -> PageRef;
    pub fn draw_text(&mut self, page: PageRef, text: &str, at: (f32,f32), font: FontSpec);
    pub fn place_image(&mut self, page: PageRef, png: &[u8], rect: [f32;4]);
    pub fn save(&self, out: impl Write) -> Result<(), PdfError>;
}

pub struct PdfEditor { /* mutate an existing document */ }
impl PdfEditor {
    pub fn open(input: impl Into<PdfInput>) -> Result<Self, PdfError>;
    pub fn merge(&mut self, other: &PdfEditor) -> Result<(), PdfError>;
    pub fn split(&self, ranges: &[(usize,usize)]) -> Result<Vec<Vec<u8>>, PdfError>;
    pub fn remove_pages(&mut self, pages: &[usize]) -> Result<(), PdfError>;
    pub fn rotate_page(&mut self, page: usize, degrees: i32) -> Result<(), PdfError>;
    pub fn watermark(&mut self, text: &str, opts: WatermarkOptions) -> Result<(), PdfError>;
    pub fn fill_form(&mut self, fields: &HashMap<String,String>) -> Result<(), PdfError>;
    pub fn save(&self, out: impl Write) -> Result<(), PdfError>;   // use lopdf save_modern for object streams
}
```

Acceptance criteria: create a one-page PDF with text and reopen it to read that text back; merge two fixtures and confirm combined page count; split and confirm ranges.

### 4.6 `pdfkit-adapters`

```rust
pub enum ContentBlock {
    Text { text: String },
    Image { media_type: String, data: Vec<u8> }, // raw bytes; caller base64s if needed
}

pub fn to_message_content(result: &ExtractResult) -> Vec<ContentBlock>;
pub fn to_data_urls(result: &ExtractResult) -> Vec<String>;

// Optional, behind `llm-adapter`. Caller supplies the model client.
#[cfg(feature = "llm-adapter")]
pub trait LlmClient { fn complete(&self, prompt: &str) -> Result<String, PdfError>; }
#[cfg(feature = "llm-adapter")]
pub fn title_chunks<C: LlmClient>(chunks: &mut [Chunk], client: &C) -> Result<(), PdfError>;
```

The LLM adapter is the *only* place a model is invoked, it is opt-in, and it never ships a default client.

### 4.7 The extraction entry point (lives in `pdfkit-core`, uses render/ocr when their features are on)

```rust
pub enum Mode { Auto, Text, Images, Both }

pub struct ExtractOptions {
    pub mode: Mode,                  // default Auto
    pub password: Option<String>,
    pub pages: Option<Vec<usize>>,
    pub max_pages: usize,            // default 20
    pub min_text_chars: usize,       // default 200
    pub max_text_chars: usize,       // default 200_000
    pub ocr: bool,                   // try OCR before rendering on scanned pages
    pub image: RenderOptions,
}

pub struct ExtractResult {
    pub text: String,
    pub images: Vec<PdfImage>,       // page, width, height, png bytes
    pub pages_processed: Vec<usize>,
    pub truncated: Truncated,        // { text: bool, images: bool }
}

pub fn extract(input: impl Into<PdfInput>, opts: ExtractOptions) -> Result<ExtractResult, PdfError>;
```

`Auto` flow (this is the heart of the tool — matches the architecture diagram):
1. Open, select pages (respecting `pages`/`max_pages`).
2. For each page: extract text.
3. If total text length ≥ `min_text_chars`, return text only.
4. Otherwise, for each low-text page: if `ocr` and the page is `Scanned`/`ImageOnly`, rasterize → OCR → append recovered text; else render → PNG into `images`.
5. Stop rendering when the pixel budget is exhausted; set `truncated.images`.

---

## 5. Feature flags

| Flag | Default | Effect |
| --- | --- | --- |
| `render-pdfium` | off | High-fidelity render via PDFium (native or WASM). |
| `render-native` | on | Pure-Rust render path. |
| `ocr-ocrs` | off | Local ONNX OCR via ocrs. |
| `ocr-tesseract` | off | Tesseract OCR (system dependency). |
| `edit` | on | Editing + creation. |
| `chunk` | on | Structured chunking. |
| `llm-adapter` | off | Opt-in LLM titling/cleanup adapter. |
| `wasm` | off | wasm-bindgen surface. |

The default build (`render-native`, `edit`, `chunk`) must compile and pass tests with **no** native dependencies and **no** network.

---

## 6. Error model

```rust
#[derive(thiserror::Error, Debug)]
pub enum PdfError {
    #[error("invalid input or malformed PDF")] Format(#[source] Option<Box<dyn Error + Send + Sync>>),
    #[error("missing or incorrect password")] Password,
    #[error("unsupported security handler")] Security,
    #[error("page {0} out of range")] PageRange(usize),
    #[error("render or extraction budget exceeded")] Budget,
    #[error("use after destroy")] Destroyed,
    #[error("backend error: {0}")] Backend(String),
}
```
Every public function returns `Result<_, PdfError>`. No panics in library paths.

---

## 7. Public surface / re-exports

`pdfkit` umbrella crate (optional, for ergonomics) re-exports: `Engine`, `Document`, `Page`, `extract`, `ExtractOptions`, `ExtractResult`, `Mode`, `RenderOptions`, `encode_png`, `chunk_document`, `Chunk`, `PdfBuilder`, `PdfEditor`, the adapters, and `PdfError`. Also export build constants for any vendored PDFium provenance (release tag + wasm sha256) when `render-pdfium` is on.

---

## 8. Testing strategy

- **Fixtures** in `fixtures/`: `born-digital.pdf` (text layer), `scanned.pdf` (single image page, no text), `forms.pdf` (AcroForm), `mixed.pdf`, `encrypted.pdf` (known password), `multi-heading.pdf` (for chunk tests). Keep them small; generate synthetic ones where possible so they can be committed.
- **Unit tests** per crate for pure logic (classification thresholds, chunk packing, PNG header validity).
- **Integration tests** in each crate's `tests/` covering the acceptance criteria listed per crate.
- **Snapshot tests** for extracted text and chunk metadata (use `insta`).
- **Benchmarks** with `criterion` in `benches/`: open+text+render of each fixture; track total time and peak memory. Mirror clawpdf's sample categories (form, hello, scientific, magazine, checkmark).
- **WASM tests**: `wasm-bindgen-test` smoke test that opens bytes and extracts text in a headless browser target.
- Run `cargo test --workspace` and `cargo test --workspace --no-default-features --features render-native` to guarantee the zero-native-dep path works.

---

## 9. CI (`.github/workflows/ci.yml`)

Jobs:
1. **fmt**: `cargo fmt --all --check`.
2. **clippy**: `cargo clippy --workspace --all-targets -- -D warnings`.
3. **test-default**: `cargo test --workspace` on Linux/macOS/Windows.
4. **test-minimal**: `cargo test --workspace --no-default-features --features render-native` (proves zero-native-dep build).
5. **wasm-build**: `wasm-pack build crates/pdfkit-wasm` and run `wasm-bindgen-test` headless.
6. **msrv**: build on the pinned minimum supported Rust version.

A green CI requires all jobs to pass. Do not merge a milestone with failing clippy or fmt.

---

## 10. Milestones (build in this order)

Each milestone ends with: code compiles, its tests pass, clippy clean, committed with a clear message.

**M0 — Scaffold**
- Create the workspace, all eight crate skeletons, `rust-toolchain.toml`, `CLAUDE.md`, `README.md`, and the CI file.
- Each crate compiles empty (`lib.rs` with a placeholder).
- Acceptance: `cargo build --workspace` and `cargo clippy --workspace` pass.

**M1 — Core open + text**
- Implement `Engine`, `Document`, `Page`, `Metadata`, `OpenOptions`, `TextOptions`, `PdfError`.
- Open path + bytes; read metadata and page count; extract text.
- Acceptance: text extraction from `born-digital.pdf` matches snapshot; encrypted PDF with wrong password returns `PdfError::Password`.

**M2 — Classification**
- Implement `Page::classify` and expose raw signals.
- Acceptance: `born-digital.pdf` → `TextBased`, `scanned.pdf` → `Scanned`, `mixed.pdf` → `Mixed`.

**M3 — Render + PNG (PDFium backend first)**
- Implement `pdfkit-render` with `render-pdfium`, `RenderOptions`, `Bitmap`, `encode_png`, budget checks.
- Wire the `Renderer` trait into core.
- Acceptance: render page 1 of `scientific.pdf` to a valid PNG; oversized request returns `PdfError::Budget`. (This is clawpdf parity.)

**M4 — Extraction entry point (Auto fallback)**
- Implement `extract`, `ExtractOptions`, `ExtractResult`, all four modes.
- Acceptance: a text PDF returns text only with no images; a scanned PDF (OCR off) returns rendered PNG(s); `truncated` flags behave under tight budgets.

**M5 — OCR**
- Implement `pdfkit-ocr` with `ocr-ocrs`, the `OcrProvider` trait, and `ocr_page`; wire `ocr: true` into `extract`.
- Add the model-download setup script (cache dir, not git).
- Acceptance: scanned PDF with `ocr: true` returns recovered text + confidence; default build still compiles without OCR features.

**M6 — Chunking**
- Implement `pdfkit-chunk`: grouping, classification, heading stack, token-aware packing.
- Acceptance: `multi-heading.pdf` yields chunks with correct pages, `heading_path`, and sizes near target.

**M7 — Edit + create**
- Implement `pdfkit-edit`: `PdfBuilder` (create) and `PdfEditor` (merge/split/remove/rotate/watermark/fill_form), `save_modern` with object streams.
- Acceptance: round-trip create→reopen→read text; merge page-count check; split ranges check.

**M8 — Adapters**
- Implement `to_message_content`, `to_data_urls`, and the opt-in `llm-adapter` trait + `title_chunks`.
- Acceptance: adapters produce expected block shapes; build with and without `llm-adapter`.

**M9 — CLI**
- Implement `pdfkit-cli` with `clap`: `pdfkit <file>`, `--json`, `render <file> --page N`, `--password/--password-file`, stdin via `-`, sensible exit codes.
- Acceptance: each command runs against fixtures and produces correct output/exit codes.

**M10 — WASM**
- Implement `pdfkit-wasm`: `wasm-bindgen` surface mirroring the core API, Blob/ArrayBuffer input, packaged or configurable PDFium WASM URL when `render-pdfium` is used.
- Acceptance: `wasm-pack build` succeeds; headless `wasm-bindgen-test` opens bytes and extracts text.

**M11 — Benchmarks + docs polish**
- Add criterion benches and a performance table in the README; finalize per-feature docs.
- Acceptance: benches run; README documents install, quick start, features, and provenance.

---

## 11. `CLAUDE.md` working conventions (create this file in M0)

- Work one milestone at a time; do not start the next until the current one's acceptance criteria pass.
- After every change: `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test`.
- Practice test-first where practical: write the acceptance test, then implement until green.
- Never add a hosted-LLM call to any crate except `pdfkit-adapters` behind `llm-adapter`.
- Never add a native or network dependency to the default feature set. New heavy deps go behind a feature flag.
- Keep public API one-based for page numbers; convert at the boundary.
- No `unwrap()`/`expect()`/`panic!` in library code; return `PdfError`.
- Commit per milestone (or per sub-feature) with a message describing the deliverable and that tests pass.
- If a design decision is ambiguous, prefer the simplest implementation that satisfies the acceptance criteria and leave a `// TODO(design):` note rather than blocking.

---

## 12. Open decisions to confirm before or during the build

1. **Umbrella crate vs import each crate?** Plan assumes an optional `pdfkit` re-export crate. Confirm you want it.
2. **Native render scope.** How much vector fidelity does `render-native` need, or is it acceptable that high-fidelity render requires `render-pdfium`? (Recommendation: ship PDFium as the rendering path in v1, treat pure-Rust render as best-effort.)
3. **Tokenizer.** Real `tiktoken` vs a char/word heuristic for `token_estimate` in v1. (Recommendation: heuristic in v1, swap later.)
4. **PDFium provenance.** If `render-pdfium` ships a vendored WASM build, decide the pinned release tag and record its sha256 (mirror clawpdf's provenance approach).
5. **License.** MIT/Apache-2.0 dual (Rust default) vs the GPL that some pure-Rust PDF libs carry — check the license of every dependency you pull in, especially OCR and any oxidize-pdf-derived code, before committing to a project license.

Decision (5) matters: confirm dependency licenses early so the project license is compatible.