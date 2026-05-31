//! PDF text-string decoding (PDF spec §7.9.2.2): UTF-16BE when the string
//! carries a BOM, otherwise PDFDocEncoding. Shared by metadata, the outline,
//! and the tagged structure tree so they decode titles/labels identically.

/// Decode a PDF text string to UTF-8.
pub(crate) fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let pairs = bytes[2..].chunks_exact(2);
        let trailing = !pairs.remainder().is_empty();
        let mut units: Vec<u16> = pairs.map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        // A trailing odd byte is malformed; surface it as U+FFFD rather than
        // silently dropping it.
        if trailing {
            units.push(0xFFFD);
        }
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&b| pdfdoc_char(b)).collect()
    }
}

/// Map one PDFDocEncoding byte to its Unicode scalar (PDF spec Annex D.2).
///
/// `0x20..=0x7E` (ASCII) and `0xA1..=0xFF` (Latin-1) coincide with their byte
/// value; the differences are the accent block `0x18..=0x1F` and the punctuation
/// /ligature block `0x80..=0xA0`. Undefined codes map to U+FFFD.
fn pdfdoc_char(b: u8) -> char {
    match b {
        0x18 => '\u{02D8}', // breve
        0x19 => '\u{02C7}', // caron
        0x1A => '\u{02C6}', // circumflex
        0x1B => '\u{02D9}', // dotaccent
        0x1C => '\u{02DD}', // hungarumlaut
        0x1D => '\u{02DB}', // ogonek
        0x1E => '\u{02DA}', // ring
        0x1F => '\u{02DC}', // tilde
        0x80 => '\u{2022}', // bullet
        0x81 => '\u{2020}', // dagger
        0x82 => '\u{2021}', // daggerdbl
        0x83 => '\u{2026}', // ellipsis
        0x84 => '\u{2014}', // emdash
        0x85 => '\u{2013}', // endash
        0x86 => '\u{0192}', // florin
        0x87 => '\u{2044}', // fraction
        0x88 => '\u{2039}', // guilsinglleft
        0x89 => '\u{203A}', // guilsinglright
        0x8A => '\u{2212}', // minus
        0x8B => '\u{2030}', // perthousand
        0x8C => '\u{201E}', // quotedblbase
        0x8D => '\u{201C}', // quotedblleft
        0x8E => '\u{201D}', // quotedblright
        0x8F => '\u{2018}', // quoteleft
        0x90 => '\u{2019}', // quoteright
        0x91 => '\u{201A}', // quotesinglbase
        0x92 => '\u{2122}', // trademark
        0x93 => '\u{FB01}', // fi
        0x94 => '\u{FB02}', // fl
        0x95 => '\u{0141}', // Lslash
        0x96 => '\u{0152}', // OE
        0x97 => '\u{0160}', // Scaron
        0x98 => '\u{0178}', // Ydieresis
        0x99 => '\u{017D}', // Zcaron
        0x9A => '\u{0131}', // dotlessi
        0x9B => '\u{0142}', // lslash
        0x9C => '\u{0153}', // oe
        0x9D => '\u{0161}', // scaron
        0x9E => '\u{017E}', // zcaron
        0x9F => '\u{FFFD}', // undefined
        0xA0 => '\u{20AC}', // Euro
        other => other as char,
    }
}

#[cfg(test)]
mod tests {
    use super::decode_pdf_text;

    #[test]
    fn ascii_and_latin1_pass_through() {
        assert_eq!(decode_pdf_text(b"Hello"), "Hello");
        assert_eq!(decode_pdf_text(&[0xE9]), "\u{00E9}"); // é
    }

    #[test]
    fn pdfdocencoding_special_blocks() {
        assert_eq!(decode_pdf_text(&[0x92]), "\u{2122}"); // trademark
        assert_eq!(decode_pdf_text(&[0x80]), "\u{2022}"); // bullet
        assert_eq!(decode_pdf_text(&[0x8D]), "\u{201C}"); // left double quote
        assert_eq!(decode_pdf_text(&[0xA0]), "\u{20AC}"); // Euro
        assert_eq!(decode_pdf_text(&[0x18]), "\u{02D8}"); // breve
        assert_eq!(decode_pdf_text(&[0x9F]), "\u{FFFD}"); // undefined
                                                          // A whole word: "f<bullet>" should decode both bytes.
        assert_eq!(decode_pdf_text(&[b'f', 0x80]), "f\u{2022}");
    }

    #[test]
    fn utf16be_with_bom() {
        assert_eq!(decode_pdf_text(&[0xFE, 0xFF, 0x00, 0x41]), "A");
        // Odd trailing byte -> replacement char, not dropped.
        assert_eq!(
            decode_pdf_text(&[0xFE, 0xFF, 0x00, 0x41, 0x00]),
            "A\u{FFFD}"
        );
    }
}
