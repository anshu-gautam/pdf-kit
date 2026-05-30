//! `pdfkit-wasm` — the `wasm-bindgen` surface (PRD §10 / M10).
//!
//! Mirrors the core read API for the browser / npm. Inputs are byte slices; from
//! JavaScript pass a `Uint8Array`. For a `Blob`/`ArrayBuffer`, convert first:
//! `new Uint8Array(await blob.arrayBuffer())`.

use pdfkit_core::{extract, Engine, ExtractOptions, Mode, OpenOptions, PdfError, TextOptions};
use wasm_bindgen::prelude::*;

/// The crate version.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Extract the text of a document from its bytes.
#[wasm_bindgen]
pub fn extract_text(data: &[u8], password: Option<String>) -> Result<String, JsValue> {
    let opts = ExtractOptions {
        mode: Mode::Text,
        password,
        ..ExtractOptions::default()
    };
    let result = extract(data.to_vec(), opts).map_err(to_js)?;
    Ok(result.text)
}

/// Extract an `Auto`-mode result as a JSON string (`text`, `pages_processed`,
/// image metadata, `truncated`).
#[wasm_bindgen]
pub fn extract_json(data: &[u8], password: Option<String>) -> Result<String, JsValue> {
    let opts = ExtractOptions {
        password,
        ..ExtractOptions::default()
    };
    let result = extract(data.to_vec(), opts).map_err(to_js)?;
    let json = serde_json::json!({
        "text": result.text,
        "pages_processed": result.pages_processed,
        "images": result
            .images
            .iter()
            .map(|i| serde_json::json!({
                "page": i.page,
                "width": i.width,
                "height": i.height,
                "png_bytes": i.png.len(),
            }))
            .collect::<Vec<_>>(),
        "truncated": { "text": result.truncated.text, "images": result.truncated.images },
    });
    serde_json::to_string(&json).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Number of pages in a document.
#[wasm_bindgen]
pub fn page_count(data: &[u8], password: Option<String>) -> Result<u32, JsValue> {
    let doc = Engine::new()
        .map_err(to_js)?
        .open(data.to_vec(), OpenOptions { password })
        .map_err(to_js)?;
    Ok(doc.page_count() as u32)
}

/// An opened document, mirroring the core `Document` read surface.
#[wasm_bindgen]
pub struct WasmDocument {
    inner: pdfkit_core::Document,
}

#[wasm_bindgen]
impl WasmDocument {
    /// Open a document from bytes.
    #[wasm_bindgen(constructor)]
    pub fn new(data: &[u8], password: Option<String>) -> Result<WasmDocument, JsValue> {
        let inner = Engine::new()
            .map_err(to_js)?
            .open(data.to_vec(), OpenOptions { password })
            .map_err(to_js)?;
        Ok(WasmDocument { inner })
    }

    /// Number of pages.
    #[wasm_bindgen(getter)]
    pub fn page_count(&self) -> u32 {
        self.inner.page_count() as u32
    }

    /// Whole-document text.
    pub fn text(&self) -> Result<String, JsValue> {
        self.inner.text(TextOptions::default()).map_err(to_js)
    }

    /// Text of a single one-based page.
    pub fn page_text(&self, one_based: u32) -> Result<String, JsValue> {
        self.inner
            .page(one_based as usize)
            .map_err(to_js)?
            .text()
            .map_err(to_js)
    }
}

fn to_js(error: PdfError) -> JsValue {
    JsValue::from_str(&error.to_string())
}

#[cfg(target_arch = "wasm32")]
#[cfg(test)]
mod wasm_tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn opens_bytes_and_extracts_text() {
        let bytes = pdfkit_fixtures::born_digital();
        let text = extract_text(&bytes, None).expect("extract");
        assert!(text.contains("Hello, pdfkit!"));

        let doc = WasmDocument::new(&bytes, None).expect("open");
        assert_eq!(doc.page_count(), 1);
        assert!(doc.text().expect("text").contains("born-digital"));
    }
}
