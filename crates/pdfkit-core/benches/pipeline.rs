//! Criterion benchmarks for the read pipeline (PRD §8): open + text + classify +
//! render across the synthetic fixtures.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pdfkit_core::{
    extract, Engine, ExtractOptions, Mode, NativeRenderer, OpenOptions, RenderOptions, Renderer,
};

fn bench_open_and_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("open_text");
    for (name, bytes) in [
        ("born_digital", pdfkit_fixtures::born_digital()),
        ("multi_heading", pdfkit_fixtures::multi_heading()),
    ] {
        group.bench_function(name, |b| {
            b.iter(|| {
                let opts = ExtractOptions {
                    mode: Mode::Text,
                    ..ExtractOptions::default()
                };
                let result = extract(black_box(bytes.clone()), opts).unwrap();
                black_box(result.text.len())
            })
        });
    }
    group.finish();
}

fn bench_classify(c: &mut Criterion) {
    let mut group = c.benchmark_group("classify");
    for (name, bytes) in [
        ("born_digital", pdfkit_fixtures::born_digital()),
        ("scanned", pdfkit_fixtures::scanned()),
        ("mixed", pdfkit_fixtures::mixed()),
    ] {
        let doc = Engine::new()
            .unwrap()
            .open(bytes, OpenOptions::default())
            .unwrap();
        group.bench_function(name, |b| {
            b.iter(|| black_box(doc.page(1).unwrap().classify()))
        });
    }
    group.finish();
}

fn bench_render(c: &mut Criterion) {
    let doc = Engine::new()
        .unwrap()
        .open(pdfkit_fixtures::scanned(), OpenOptions::default())
        .unwrap();
    let opts = RenderOptions {
        width: Some(425),
        ..RenderOptions::default()
    };
    c.bench_function("render_scanned", |b| {
        b.iter(|| {
            let page = doc.page(1).unwrap();
            black_box(NativeRenderer.render(&page, &opts).unwrap().rgba.len())
        })
    });
}

criterion_group!(benches, bench_open_and_text, bench_classify, bench_render);
criterion_main!(benches);
