//! M8 acceptance: adapters produce the expected block shapes; the crate builds
//! with and without the `llm-adapter` feature.

use pdfkit_adapters::{to_data_urls, to_message_content, ContentBlock};
use pdfkit_core::{ExtractResult, PdfImage, Truncated};

fn sample() -> ExtractResult {
    ExtractResult {
        text: "hello world".to_string(),
        images: vec![PdfImage {
            page: 1,
            width: 2,
            height: 2,
            png: vec![1, 2, 3, 4, 5],
        }],
        pages_processed: vec![1],
        truncated: Truncated::default(),
    }
}

#[test]
fn message_content_is_text_then_images() {
    let blocks = to_message_content(&sample());
    assert_eq!(blocks.len(), 2);
    assert_eq!(
        blocks[0],
        ContentBlock::Text {
            text: "hello world".to_string()
        }
    );
    match &blocks[1] {
        ContentBlock::Image { media_type, data } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(data, &vec![1, 2, 3, 4, 5]);
        }
        other => panic!("expected image block, got {other:?}"),
    }
}

#[test]
fn empty_text_yields_no_text_block() {
    let mut r = sample();
    r.text.clear();
    let blocks = to_message_content(&r);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0], ContentBlock::Image { .. }));
}

#[test]
fn data_urls_are_well_formed() {
    let urls = to_data_urls(&sample());
    assert_eq!(urls.len(), 1);
    assert!(urls[0].starts_with("data:image/png;base64,"));
    // Base64 of [1,2,3,4,5] is "AQIDBAU=".
    assert!(urls[0].ends_with("AQIDBAU="), "got {}", urls[0]);
}

#[cfg(feature = "llm-adapter")]
#[test]
fn title_chunks_titles_only_untitled_non_headings() {
    use pdfkit_adapters::{title_chunks, LlmClient};
    use pdfkit_chunk::{Chunk, ElementKind};
    use pdfkit_core::PdfError;

    struct Mock;
    impl LlmClient for Mock {
        fn complete(&self, _prompt: &str) -> Result<String, PdfError> {
            Ok("  Synthesized Title  ".to_string())
        }
    }

    let mut chunks = vec![
        Chunk {
            text: "Body without a heading".into(),
            page: 1,
            bbox: None,
            kind: ElementKind::Paragraph,
            heading_path: vec![],
            token_estimate: 5,
        },
        Chunk {
            text: "Already in a section".into(),
            page: 1,
            bbox: None,
            kind: ElementKind::Paragraph,
            heading_path: vec!["Existing".into()],
            token_estimate: 4,
        },
        Chunk {
            text: "A Heading".into(),
            page: 1,
            bbox: None,
            kind: ElementKind::Heading,
            heading_path: vec![],
            token_estimate: 2,
        },
    ];

    title_chunks(&mut chunks, &Mock).expect("title_chunks");

    assert_eq!(
        chunks[0].heading_path,
        vec!["Synthesized Title".to_string()]
    );
    assert_eq!(chunks[1].heading_path, vec!["Existing".to_string()]);
    assert!(chunks[2].heading_path.is_empty());
}
