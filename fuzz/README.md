# pdfkit fuzzing

A [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) (libFuzzer) harness for
pdfkit's read paths. PDFs are untrusted, hostile input and the library's hard
invariant is "never panic on any input"; this harness hunts for any input that
violates it (panic, abort, OOM, hang).

This is a **detached workspace** (`[workspace]` in `Cargo.toml`), so it never
affects `cargo build`/`cargo test --workspace` on stable. The always-on
deterministic regression test lives in
`crates/pdfkit-chunk/tests/no_panic.rs`; this harness is for deep, on-demand or
scheduled campaigns.

## Targets

- `parse` — open arbitrary bytes, then run extraction, classification, the
  readers (outline / structure tree / links / figures), and chunking +
  JSON/Markdown serialization.

## Run

Requires a nightly toolchain and `cargo-fuzz` (`cargo install cargo-fuzz`):

```sh
# Seed the corpus from the committed fixtures (valid PDFs), then fuzz:
mkdir -p fuzz/corpus/parse && cp fixtures/*.pdf fuzz/corpus/parse/
cargo +nightly fuzz run parse

# Bounded run (what CI does):
cargo +nightly fuzz run parse -- -max_total_time=60 -timeout=10
```

A crash is written to `fuzz/artifacts/parse/`; reproduce with
`cargo +nightly fuzz run parse fuzz/artifacts/parse/crash-<hash>`.
