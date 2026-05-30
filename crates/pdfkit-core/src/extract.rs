//! The extraction entry point (PRD §4.7): open a document and produce text
//! and/or rendered page images, with an `Auto` mode that falls back from text
//! to images (and, with the `ocr` feature, to OCR — wired in M5).

use crate::document::{Document, Engine};
use crate::error::PdfError;
use crate::render::RenderOptions;
use crate::types::{OpenOptions, PdfInput};

#[cfg(feature = "render-native")]
use crate::classify::PageKind;
#[cfg(feature = "render-native")]
use crate::ocr::{ocr_page, OcrProvider};

/// What `extract` should produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Text first; fall back to OCR / rendered images for low-text pages.
    #[default]
    Auto,
    /// Text only.
    Text,
    /// Rendered page images only.
    Images,
    /// Both text and rendered page images.
    Both,
}

/// Options for [`extract`].
#[derive(Debug, Clone)]
pub struct ExtractOptions {
    /// Extraction mode.
    pub mode: Mode,
    /// Password for an encrypted document.
    pub password: Option<String>,
    /// Restrict to these one-based pages (in order).
    pub pages: Option<Vec<usize>>,
    /// Maximum number of pages visited.
    pub max_pages: usize,
    /// In `Auto`, return text-only when the recovered text reaches this many
    /// (non-whitespace) characters.
    pub min_text_chars: usize,
    /// Cap on returned text characters.
    pub max_text_chars: usize,
    /// Try OCR before rendering on scanned/image-only pages (needs an OCR
    /// feature; wired in M5).
    pub ocr: bool,
    /// How to render page images.
    pub image: RenderOptions,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        ExtractOptions {
            mode: Mode::Auto,
            password: None,
            pages: None,
            max_pages: 20,
            min_text_chars: 200,
            max_text_chars: 200_000,
            ocr: false,
            image: RenderOptions::default(),
        }
    }
}

/// A rendered page image in an [`ExtractResult`].
#[derive(Clone, PartialEq, Eq)]
pub struct PdfImage {
    /// One-based page number this image came from.
    pub page: usize,
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// PNG-encoded bytes.
    pub png: Vec<u8>,
}

impl std::fmt::Debug for PdfImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfImage")
            .field("page", &self.page)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("png_len", &self.png.len())
            .finish()
    }
}

/// Which outputs were cut short by a budget.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Truncated {
    /// Text hit `max_text_chars`.
    pub text: bool,
    /// Image rendering stopped on the pixel budget.
    pub images: bool,
}

/// The result of [`extract`].
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractResult {
    /// Recovered text.
    pub text: String,
    /// Rendered page images.
    pub images: Vec<PdfImage>,
    /// One-based page numbers that were processed.
    pub pages_processed: Vec<usize>,
    /// Budget-truncation flags.
    pub truncated: Truncated,
}

/// Open `input` and extract text and/or images per `opts`.
pub fn extract(
    input: impl Into<PdfInput>,
    opts: ExtractOptions,
) -> Result<ExtractResult, PdfError> {
    let engine = Engine::new()?;
    let doc = engine.open(
        input,
        OpenOptions {
            password: opts.password.clone(),
        },
    )?;
    let selected = select_pages(doc.page_count(), opts.pages.as_deref(), opts.max_pages)?;

    match opts.mode {
        Mode::Text => {
            let (text, truncated_text) = collect_text(&doc, &selected, opts.max_text_chars);
            Ok(ExtractResult {
                text,
                images: Vec::new(),
                pages_processed: selected,
                truncated: Truncated {
                    text: truncated_text,
                    images: false,
                },
            })
        }
        Mode::Images => {
            let (images, truncated_images) = render_pages(&doc, &selected, &opts.image);
            Ok(ExtractResult {
                text: String::new(),
                images,
                pages_processed: selected,
                truncated: Truncated {
                    text: false,
                    images: truncated_images,
                },
            })
        }
        Mode::Both => {
            let (text, truncated_text) = collect_text(&doc, &selected, opts.max_text_chars);
            let (images, truncated_images) = render_pages(&doc, &selected, &opts.image);
            Ok(ExtractResult {
                text,
                images,
                pages_processed: selected,
                truncated: Truncated {
                    text: truncated_text,
                    images: truncated_images,
                },
            })
        }
        Mode::Auto => extract_auto(&doc, &selected, &opts),
    }
}

/// The `Auto` flow (PRD §4.7): text first, with an image/OCR fallback for the
/// low-text case.
fn extract_auto(
    doc: &Document,
    selected: &[usize],
    opts: &ExtractOptions,
) -> Result<ExtractResult, PdfError> {
    let per_page = page_texts(doc, selected);
    let (text, truncated_text) = join_capped(&per_page, opts.max_text_chars);

    let total_nonws: usize = per_page
        .iter()
        .map(|(_, t)| t.chars().filter(|c| !c.is_whitespace()).count())
        .sum();

    // Enough text overall: return text only.
    if total_nonws >= opts.min_text_chars {
        return Ok(ExtractResult {
            text,
            images: Vec::new(),
            pages_processed: selected.to_vec(),
            truncated: Truncated {
                text: truncated_text,
                images: false,
            },
        });
    }

    // Low-text document: render (or, in M5, OCR) the low-text pages.
    let low_text_pages: Vec<usize> = per_page
        .iter()
        .filter(|(_, t)| t.chars().filter(|c| !c.is_whitespace()).count() < opts.min_text_chars)
        .map(|(p, _)| *p)
        .collect();

    // OCR fallback (M5) will recover text from Scanned/ImageOnly pages here when
    // `opts.ocr` is set; until then those pages are rendered as images.
    let (images, truncated_images) = render_pages(doc, &low_text_pages, &opts.image);

    Ok(ExtractResult {
        text,
        images,
        pages_processed: selected.to_vec(),
        truncated: Truncated {
            text: truncated_text,
            images: truncated_images,
        },
    })
}

/// Like [`extract`] in `Auto` mode, but recovers text from scanned/image-only
/// low-text pages with the supplied OCR `provider` instead of (or before)
/// rendering them. Pages whose OCR fails, or which aren't scanned/image-only,
/// fall back to rendered images.
///
/// The provider is supplied by the caller (e.g. `pdfkit_ocr::OcrsProvider`),
/// which keeps the heavy OCR backends out of `pdfkit-core` and avoids a
/// dependency cycle.
#[cfg(feature = "render-native")]
pub fn extract_with_ocr<P>(
    input: impl Into<PdfInput>,
    opts: ExtractOptions,
    provider: &P,
) -> Result<ExtractResult, PdfError>
where
    P: OcrProvider,
{
    use crate::render::NativeRenderer;

    let engine = Engine::new()?;
    let doc = engine.open(
        input,
        OpenOptions {
            password: opts.password.clone(),
        },
    )?;
    let selected = select_pages(doc.page_count(), opts.pages.as_deref(), opts.max_pages)?;
    let per_page = page_texts(&doc, &selected);

    let total_nonws: usize = per_page
        .iter()
        .map(|(_, t)| t.chars().filter(|c| !c.is_whitespace()).count())
        .sum();

    // Enough text overall: text only.
    if total_nonws >= opts.min_text_chars {
        let (text, truncated_text) = join_capped(&per_page, opts.max_text_chars);
        return Ok(ExtractResult {
            text,
            images: Vec::new(),
            pages_processed: selected,
            truncated: Truncated {
                text: truncated_text,
                images: false,
            },
        });
    }

    let renderer = NativeRenderer;
    let mut effective: Vec<(usize, String)> = Vec::with_capacity(per_page.len());
    let mut to_render: Vec<usize> = Vec::new();

    for (p, layer_text) in &per_page {
        let has_text =
            layer_text.chars().filter(|c| !c.is_whitespace()).count() >= opts.min_text_chars;
        if has_text {
            effective.push((*p, layer_text.clone()));
            continue;
        }

        // Try OCR on scanned / image-only pages.
        let recovered = if opts.ocr {
            match doc.page(*p) {
                Ok(page) if matches!(page.classify(), PageKind::Scanned | PageKind::ImageOnly) => {
                    ocr_page(&page, &renderer, provider).ok().map(|r| r.text)
                }
                _ => None,
            }
        } else {
            None
        };

        match recovered {
            Some(text) => effective.push((*p, text)),
            None => {
                effective.push((*p, layer_text.clone()));
                to_render.push(*p);
            }
        }
    }

    let (text, truncated_text) = join_capped(&effective, opts.max_text_chars);
    let (images, truncated_images) = render_pages(&doc, &to_render, &opts.image);

    Ok(ExtractResult {
        text,
        images,
        pages_processed: selected,
        truncated: Truncated {
            text: truncated_text,
            images: truncated_images,
        },
    })
}

/// Resolve the ordered list of one-based pages to process.
fn select_pages(
    count: usize,
    pages: Option<&[usize]>,
    max_pages: usize,
) -> Result<Vec<usize>, PdfError> {
    let selected: Vec<usize> = match pages {
        Some(list) => {
            for &p in list {
                if p == 0 || p > count {
                    return Err(PdfError::PageRange(p));
                }
            }
            list.to_vec()
        }
        None => (1..=count).collect(),
    };
    Ok(selected.into_iter().take(max_pages).collect())
}

/// Per-page extracted text, in `pages` order.
fn page_texts(doc: &Document, pages: &[usize]) -> Vec<(usize, String)> {
    pages
        .iter()
        .map(|&p| {
            let text = doc.page(p).and_then(|pg| pg.text()).unwrap_or_default();
            (p, text)
        })
        .collect()
}

/// Join per-page text with newlines, capped at `max_chars` characters.
fn join_capped(per_page: &[(usize, String)], max_chars: usize) -> (String, bool) {
    let mut out = String::new();
    let mut count = 0usize;
    let mut truncated = false;
    for (i, (_, piece)) in per_page.iter().enumerate() {
        if i > 0 && count < max_chars && !out.is_empty() {
            out.push('\n');
            count += 1;
        }
        for ch in piece.chars() {
            if count >= max_chars {
                truncated = true;
                break;
            }
            out.push(ch);
            count += 1;
        }
        if truncated {
            break;
        }
    }
    (out, truncated)
}

/// Collect document text across `pages`, capped at `max_chars`.
fn collect_text(doc: &Document, pages: &[usize], max_chars: usize) -> (String, bool) {
    join_capped(&page_texts(doc, pages), max_chars)
}

/// Render `pages` to PNG images, honoring a total pixel budget
/// (`image.max_pixels`). Returns the images and whether rendering was truncated.
#[cfg(feature = "render-native")]
fn render_pages(doc: &Document, pages: &[usize], image: &RenderOptions) -> (Vec<PdfImage>, bool) {
    use crate::render::{encode_png, NativeRenderer, Renderer};

    let renderer = NativeRenderer;
    let budget = u64::from(image.max_pixels);
    let mut used: u64 = 0;
    let mut images = Vec::new();
    let mut truncated = false;

    for &p in pages {
        let Ok(page) = doc.page(p) else { continue };
        let (pw, ph) = page.size_points();
        let (w, h) = match image.output_dimensions(pw, ph) {
            Ok(dims) => dims,
            Err(_) => {
                truncated = true;
                break;
            }
        };
        let px = u64::from(w) * u64::from(h);
        if used + px > budget {
            truncated = true;
            break;
        }
        match renderer.render(&page, image) {
            Ok(bmp) => {
                used += px;
                if let Ok(png) = encode_png(&bmp, true) {
                    images.push(PdfImage {
                        page: p,
                        width: bmp.width,
                        height: bmp.height,
                        png,
                    });
                }
            }
            Err(PdfError::Budget) => {
                truncated = true;
                break;
            }
            Err(_) => continue,
        }
    }
    (images, truncated)
}

/// Without a render backend there is no way to produce images; report
/// truncation if any were requested.
#[cfg(not(feature = "render-native"))]
fn render_pages(_doc: &Document, pages: &[usize], _image: &RenderOptions) -> (Vec<PdfImage>, bool) {
    (Vec::new(), !pages.is_empty())
}
