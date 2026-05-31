//! Reading-order line grouping: single-column equivalence, multi-column
//! ordering (issue #4), and composite-font advances (issue #7).

use pdfkit_core::{group_runs_into_lines, Engine, OpenOptions, TextRun};

/// A run spanning `[x0, x1]` on baseline `y` at `size` points.
fn run(text: &str, x0: f32, y: f32, x1: f32, size: f32) -> TextRun {
    TextRun {
        text: text.to_string(),
        bbox: [x0, y, x1, y + size],
        font_size: size,
        mcid: None,
    }
}

fn texts(lines: &[pdfkit_core::Line]) -> Vec<String> {
    lines.iter().map(|l| l.text.clone()).collect()
}

#[test]
fn single_column_is_one_band_top_to_bottom() {
    let runs: Vec<TextRun> = (0..6)
        .map(|i| {
            run(
                &format!("line{i}"),
                72.0,
                700.0 - i as f32 * 18.0,
                300.0,
                12.0,
            )
        })
        .collect();
    let lines = group_runs_into_lines(runs);

    assert_eq!(lines.len(), 6);
    // Never split: every line is column 0, so consumers' column logic is inert.
    assert!(lines.iter().all(|l| l.column == 0));
    // Ordered top (highest y) to bottom.
    assert_eq!(
        texts(&lines),
        vec!["line0", "line1", "line2", "line3", "line4", "line5"]
    );
    assert!(lines.windows(2).all(|w| w[0].y >= w[1].y));
}

#[test]
fn two_columns_read_in_column_order_with_header() {
    let mut runs = Vec::new();
    // Full-width header spanning the gutter.
    runs.push(run("Header", 72.0, 760.0, 500.0, 14.0));
    // Left and right columns, 8 baselines each, interleaved in content order so
    // a baseline-only grouping would scramble them.
    for i in 0..8 {
        let y = 740.0 - i as f32 * 18.0;
        runs.push(run(&format!("L{i}"), 72.0, y, 260.0, 12.0));
        runs.push(run(&format!("R{i}"), 300.0, y, 520.0, 12.0));
    }
    let lines = group_runs_into_lines(runs);

    let got = texts(&lines);
    let mut expected = vec!["Header".to_string()];
    expected.extend((0..8).map(|i| format!("L{i}")));
    expected.extend((0..8).map(|i| format!("R{i}")));
    assert_eq!(
        got, expected,
        "expected header, full left column, full right column"
    );

    // Header, left, right occupy three distinct column bands.
    assert_eq!(lines[0].column, 0);
    let left_cols: Vec<usize> = lines[1..9].iter().map(|l| l.column).collect();
    let right_cols: Vec<usize> = lines[9..17].iter().map(|l| l.column).collect();
    assert!(left_cols.iter().all(|&c| c == left_cols[0]));
    assert!(right_cols.iter().all(|&c| c == right_cols[0]));
    assert_ne!(left_cols[0], right_cols[0]);
    assert_ne!(left_cols[0], 0);
}

#[test]
fn short_columns_are_not_split() {
    // Two visual columns but only 4 rows each: below MIN_LINES_PER_COLUMN, so
    // the split is rejected and we keep the conservative single-band behavior.
    let mut runs = Vec::new();
    for i in 0..4 {
        let y = 700.0 - i as f32 * 18.0;
        runs.push(run(&format!("L{i}"), 72.0, y, 260.0, 12.0));
        runs.push(run(&format!("R{i}"), 300.0, y, 520.0, 12.0));
    }
    let lines = group_runs_into_lines(runs);
    assert!(
        lines.iter().all(|l| l.column == 0),
        "short table must not split"
    );
    // Same-baseline left/right runs merged into one line (the pre-existing
    // behavior we deliberately preserve for short/table layouts).
    assert_eq!(lines.len(), 4);
}

#[test]
fn full_width_run_between_columns_bails_to_single_band() {
    let mut runs = Vec::new();
    for i in 0..6 {
        let y = 740.0 - i as f32 * 18.0;
        runs.push(run(&format!("L{i}"), 72.0, y, 260.0, 12.0));
        runs.push(run(&format!("R{i}"), 300.0, y, 520.0, 12.0));
    }
    // A full-width rule/figure caption strictly between the column rows: the
    // page is not a clean header/columns/footer stack, so bail (don't scramble).
    runs.push(run("spanning middle", 72.0, 680.0, 500.0, 12.0));
    let lines = group_runs_into_lines(runs);
    assert!(lines.iter().all(|l| l.column == 0));
}

#[test]
fn empty_and_degenerate_inputs_do_not_panic() {
    assert!(group_runs_into_lines(Vec::new()).is_empty());

    // All runs at the same zero-width x: no gutter, single band, no panic.
    let runs: Vec<TextRun> = (0..6)
        .map(|i| run("x", 100.0, 700.0 - i as f32 * 18.0, 100.0, 12.0))
        .collect();
    let lines = group_runs_into_lines(runs);
    assert!(lines.iter().all(|l| l.column == 0));

    // A NaN coordinate must fall back to a single band rather than panic.
    let mut bad = vec![run("a", 72.0, 700.0, 200.0, 12.0)];
    bad.push(run("b", f32::NAN, 680.0, 200.0, 12.0));
    let _ = group_runs_into_lines(bad); // must not panic

    // A pathological x-span must not allocate an unbounded histogram (OOM):
    // detect_columns caps the bin count and falls back to a single band.
    let huge = vec![
        run("near", 0.0, 700.0, 10.0, 12.0),
        run("far", 1.0e10, 700.0, 1.0e10 + 10.0, 12.0),
    ];
    let lines = group_runs_into_lines(huge);
    assert!(lines.iter().all(|l| l.column == 0));
}

#[test]
fn type0_identity_font_uses_cid_widths_for_advance() {
    let doc = Engine::new()
        .unwrap()
        .open(pdfkit_fixtures::type0_identity(), OpenOptions::default())
        .expect("open type0 fixture");
    let runs = doc.page(1).expect("page 1").text_runs();
    assert_eq!(runs.len(), 1, "one show-text operator => one run");

    let width = runs[0].bbox[2] - runs[0].bbox[0];
    assert!(
        (width - pdfkit_fixtures::TYPE0_ADVANCE_PTS).abs() < 0.6,
        "CID-aware advance should be ~{}pt, got {width}",
        pdfkit_fixtures::TYPE0_ADVANCE_PTS
    );
}

#[test]
fn rotated_run_bbox_is_tall_not_wide() {
    let doc = Engine::new()
        .unwrap()
        .open(pdfkit_fixtures::rotated_text(), OpenOptions::default())
        .expect("open rotated fixture");
    let runs = doc.page(1).expect("page 1").text_runs();
    assert_eq!(runs.len(), 1);
    let b = runs[0].bbox;
    let (w, h) = (b[2] - b[0], b[3] - b[1]);
    // 90°-rotated text advances upward, so its box is taller than it is wide
    // (the old horizontal-only formula would have made it wide and ~12pt tall).
    assert!(h > w, "rotated run should be tall, got w={w} h={h}");
    assert!(w > 0.0 && h > 0.0);
}
