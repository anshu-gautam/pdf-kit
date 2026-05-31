//! `pdfkit-adapters` — turn extraction results into model-ready shapes
//! (PRD §4.6).
//!
//! `to_message_content` and `to_data_urls` are deterministic and offline. The
//! `llm-adapter` feature adds an opt-in [`LlmClient`] trait and [`title_chunks`];
//! it is the only place in the workspace a model is invoked, and it never ships a
//! default client.

use pdfkit_core::ExtractResult;

/// A model message content block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentBlock {
    /// A text block.
    Text {
        /// The text.
        text: String,
    },
    /// An image block. `data` is raw bytes; the caller base64-encodes if needed.
    Image {
        /// MIME type, e.g. `image/png`.
        media_type: String,
        /// Raw image bytes.
        data: Vec<u8>,
    },
}

/// Convert an [`ExtractResult`] into ordered content blocks: the text (if any)
/// followed by one image block per rendered page image.
pub fn to_message_content(result: &ExtractResult) -> Vec<ContentBlock> {
    let mut blocks = Vec::with_capacity(1 + result.images.len());
    if !result.text.is_empty() {
        blocks.push(ContentBlock::Text {
            text: result.text.clone(),
        });
    }
    for image in &result.images {
        blocks.push(ContentBlock::Image {
            media_type: "image/png".to_string(),
            data: image.png.clone(),
        });
    }
    blocks
}

/// Convert each rendered image in an [`ExtractResult`] into a `data:` URL.
pub fn to_data_urls(result: &ExtractResult) -> Vec<String> {
    result
        .images
        .iter()
        .map(|image| format!("data:image/png;base64,{}", base64_encode(&image.png)))
        .collect()
}

/// Wrap raw PNG bytes — e.g. a figure cropped via
/// `pdfkit_core::Bitmap::crop_region` — as an image [`ContentBlock`] for a
/// multimodal model message.
pub fn image_block(png: Vec<u8>) -> ContentBlock {
    ContentBlock::Image {
        media_type: "image/png".to_string(),
        data: png,
    }
}

/// A `data:` URL for raw PNG bytes (e.g. a cropped figure).
pub fn image_data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64_encode(png))
}

/// Standard Base64 encoding (RFC 4648, with `=` padding). Implemented locally to
/// keep the default build dependency-free.
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18 & 63) as usize] as char);
        out.push(ALPHABET[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(feature = "llm-adapter")]
pub use llm::{title_chunks, LlmClient};

#[cfg(feature = "llm-adapter")]
mod llm {
    use pdfkit_chunk::{Chunk, ElementKind};
    use pdfkit_core::PdfError;

    /// A model client supplied by the caller. This trait is the only model
    /// touchpoint in the workspace; pdfkit never ships an implementation.
    pub trait LlmClient {
        /// Complete `prompt`, returning the model's text.
        fn complete(&self, prompt: &str) -> Result<String, PdfError>;
    }

    /// Synthesize a short heading for chunks that lack one, using `client`.
    /// Only untitled, non-heading chunks are touched; their `heading_path` is
    /// set to the generated title.
    pub fn title_chunks<C: LlmClient>(chunks: &mut [Chunk], client: &C) -> Result<(), PdfError> {
        for chunk in chunks.iter_mut() {
            if chunk.kind == ElementKind::Heading || !chunk.heading_path.is_empty() {
                continue;
            }
            let prompt = format!(
                "Give a concise 3-5 word title for the following passage. \
                 Reply with only the title.\n\n{}",
                chunk.text
            );
            let title = client.complete(&prompt)?.trim().to_string();
            if !title.is_empty() {
                chunk.heading_path = vec![title];
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod image_block_tests {
    use super::{image_block, image_data_url, ContentBlock};

    #[test]
    fn png_becomes_an_image_block_and_data_url() {
        let png = vec![137u8, 80, 78, 71, 0, 1, 2];
        assert_eq!(
            image_block(png.clone()),
            ContentBlock::Image {
                media_type: "image/png".to_string(),
                data: png.clone(),
            }
        );
        let url = image_data_url(&png);
        assert!(url.starts_with("data:image/png;base64,"));
        assert!(url.len() > "data:image/png;base64,".len());
    }
}
