//! Standard-14 Helvetica text metrics for line-breaking. The layout engine emits
//! Helvetica (regular / bold; oblique shares the upright widths), so wrapping
//! uses the matching Adobe AFM advance widths (units per 1000 em) to estimate
//! line width. Only the printable ASCII range (32..=126) is tabulated; other
//! code points fall back to a representative average, which is adequate for
//! wrapping decisions.

// Helvetica advance widths, code points 32..=126 (index = byte - 32).
#[rustfmt::skip]
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556,
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556,
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

// Helvetica-Bold advance widths, code points 32..=126.
#[rustfmt::skip]
const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611,
    975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556,
    333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611,
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

// Average advance for code points outside the tabulated ASCII range.
const DEFAULT_WIDTH: u16 = 556;

/// Adobe Helvetica / Helvetica-Bold advances `(regular, bold)` for the CP1252
/// punctuation the `pdfkit-edit` WinAnsi encoder can emit but that falls outside
/// ASCII 32..=126 — most importantly the list bullet, smart quotes, dashes, and
/// ellipsis. Without these, `text_width` would charge the 556-unit fallback and
/// over-indent bullet lists / mis-wrap typographic punctuation.
fn cp1252_punct_units(ch: char) -> Option<(u16, u16)> {
    Some(match ch {
        '\u{2022}' => (350, 350),                           // • bullet
        '\u{2013}' => (556, 556),                           // – en dash
        '\u{2014}' => (1000, 1000),                         // — em dash
        '\u{2018}' | '\u{2019}' | '\u{201a}' => (222, 278), // ‘ ’ ‚
        '\u{201c}' | '\u{201d}' | '\u{201e}' => (333, 500), // “ ” „
        '\u{2026}' => (1000, 1000),                         // … ellipsis
        '\u{2020}' => (556, 556),                           // † dagger
        '\u{2021}' => (556, 556),                           // ‡ daggerdbl
        '\u{2030}' => (1000, 1000),                         // ‰ perthousand
        '\u{2122}' => (1000, 1000),                         // ™ trademark
        '\u{20ac}' => (556, 556),                           // € euro
        '\u{0192}' => (556, 556),                           // ƒ florin
        '\u{02c6}' | '\u{02dc}' => (333, 333),              // ˆ ˜
        '\u{2039}' | '\u{203a}' => (333, 333),              // ‹ ›
        '\u{0152}' => (1000, 1000),                         // Œ
        '\u{0153}' => (500, 556),                           // œ
        '\u{0160}' => (667, 722),                           // Š
        '\u{0161}' => (500, 556),                           // š
        '\u{017d}' => (611, 667),                           // Ž
        '\u{017e}' => (500, 556),                           // ž
        '\u{0178}' => (667, 667),                           // Ÿ
        _ => return None,
    })
}

/// Advance width (per-1000-em units) of a single character in Helvetica
/// (regular or bold). Oblique/BoldOblique share the upright widths.
fn char_units(ch: char, bold: bool) -> u16 {
    let table = if bold { &HELVETICA_BOLD } else { &HELVETICA };
    let c = ch as u32;
    if (32..=126).contains(&c) {
        table[(c - 32) as usize]
    } else if let Some((reg, bold_w)) = cp1252_punct_units(ch) {
        if bold {
            bold_w
        } else {
            reg
        }
    } else {
        DEFAULT_WIDTH
    }
}

/// Width of `text` in points at `size_pt` for Helvetica (regular or bold).
///
/// Accumulates in `f32` (not an integer) so a pathologically long token can't
/// overflow — inputs are untrusted and the library must never panic.
pub fn text_width(text: &str, size_pt: f32, bold: bool) -> f32 {
    let units: f32 = text.chars().map(|c| f32::from(char_units(c, bold))).sum();
    (units / 1000.0) * size_pt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_and_letters_are_reasonable() {
        // A single space at 12pt ≈ 278/1000*12 = 3.336pt.
        let sp = text_width(" ", 12.0, false);
        assert!((sp - 3.336).abs() < 0.01, "space width {sp}");
        // Bold text is wider than regular for the same string.
        assert!(text_width("Width", 12.0, true) > text_width("Width", 12.0, false));
    }

    #[test]
    fn empty_string_is_zero() {
        assert_eq!(text_width("", 24.0, false), 0.0);
    }
}
