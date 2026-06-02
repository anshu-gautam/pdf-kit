import Link from "next/link";
import { CodeTabs } from "@/components/landing/code-tabs";
import { CopyButton } from "@/components/landing/copy-button";
import { LandingThemeToggle } from "@/components/landing/theme-toggle";
import "./landing.css";

const REPO_URL = "https://github.com/anshu-gautam/pdf-kit";

export default function Home() {
  return (
    <div className="lp">
      <a className="skip-link" href="#top">
        Skip to content
      </a>
      {/* ===================== NAV ===================== */}
      <header className="nav">
        <div className="wrap nav-inner">
          <a className="brand" href="#top">
            <span className="glyph">pk</span>
            pdfkit
            <span className="ver">v1</span>
          </a>
          <nav className="nav-links">
            <a href="#provenance">Provenance</a>
            <a href="#how">How it works</a>
            <a href="#features">Features</a>
            <a href="#surfaces">Surfaces</a>
            <Link href="/docs">Docs</Link>
          </nav>
          <div className="nav-right">
            <LandingThemeToggle />
            <a className="gh-pill" href={REPO_URL} target="_blank" rel="noopener noreferrer">
              <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor" aria-hidden="true">
                <path d="M12 .5C5.7.5.5 5.7.5 12c0 5.1 3.3 9.4 7.9 10.9.6.1.8-.2.8-.5v-2c-3.2.7-3.9-1.4-3.9-1.4-.5-1.3-1.3-1.7-1.3-1.7-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1 1.8 2.7 1.3 3.4 1 .1-.8.4-1.3.7-1.6-2.6-.3-5.3-1.3-5.3-5.7 0-1.3.5-2.3 1.2-3.1-.1-.3-.5-1.5.1-3.1 0 0 1-.3 3.3 1.2a11.5 11.5 0 0 1 6 0C17.3 4.7 18.3 5 18.3 5c.6 1.6.2 2.8.1 3.1.8.8 1.2 1.8 1.2 3.1 0 4.4-2.7 5.4-5.3 5.7.4.4.8 1.1.8 2.2v3.3c0 .3.2.6.8.5 4.6-1.5 7.9-5.8 7.9-10.9C23.5 5.7 18.3.5 12 .5z" />
              </svg>
              Star
            </a>
            <Link className="btn btn-primary" href="/extract">
              Get started
            </Link>
          </div>
        </div>
      </header>

      <main id="top">
        {/* ===================== HERO ===================== */}
        <section className="hero">
          <div className="wrap hero-grid">
            <div className="hero-copy">
              <div className="hero-badges">
                <span className="tagpill">
                  <span className="dot" />
                  Rust · one Cargo workspace
                </span>
                <span className="tagpill">MIT / Apache-2.0</span>
              </div>
              <h1>
                Read PDFs like
                <br />a machine should.
                <br />
                <span className="accent">With receipts.</span>
              </h1>
              <p className="lead">
                A read-first, AI-oriented PDF toolkit: layout-aware text extraction, RAG-ready chunks
                with provenance on every one, and a separate edit path. Deterministic and offline by
                default.
              </p>
              <div className="hero-cta">
                <div className="install">
                  <span>
                    <span className="sigil">$</span> cargo install pdfkit-cli
                  </span>
                  <CopyButton text="cargo install pdfkit-cli" label="Copy install command" />
                </div>
                <div className="actions">
                  <a className="btn btn-secondary btn-lg" href="#how">
                    How it works
                  </a>
                </div>
              </div>
              <div className="trust">
                <span>
                  <Check />
                  No hosted-LLM calls in the core
                </span>
                <span>
                  <Check />
                  Zero native deps by default
                </span>
                <span>
                  <Check />
                  Never panics on hostile input
                </span>
              </div>
            </div>

            <div className="hero-visual">
              <div className="window" aria-hidden="true">
                <div className="win-bar">
                  <span className="dots">
                    <i />
                    <i />
                    <i />
                  </span>
                  <span className="title mono">zsh — pdfkit</span>
                  <span className="badge">deterministic · offline</span>
                </div>
                <div className="term-body" id="term">
                  <div className="ln">
                    <span className="pr">$</span> pdfkit report.pdf <span className="fl">--json</span>
                  </div>
                  <div className="ln dim">› opened · 24 pages · born-digital · text layer ✓</div>
                  <div className="ln">
                    <span className="p">{"{"}</span> <span className="key">{'"text"'}</span>:{" "}
                    <span className="str">{'"Quarterly results…"'}</span>,
                  </div>
                  <div className="ln">
                    {"  "}
                    <span className="key">{'"pages_processed"'}</span>: <span className="num">[1..24]</span>,
                  </div>
                  <div className="ln">
                    {"  "}
                    <span className="key">{'"truncated"'}</span>: <span className="p">{"{"}</span>{" "}
                    <span className="key">{'"text"'}</span>: <span className="num">false</span>{" "}
                    <span className="p">{"} }"}</span>
                  </div>
                  <div className="ln">&nbsp;</div>
                  <div className="ln">
                    <span className="pr">$</span> pdfkit chunk report.pdf{" "}
                    <span className="fl">--format md --target-tokens 512</span>
                  </div>
                  <div className="ln dim">› 38 chunks · heading paths · provenance ✓</div>
                  <div className="ln">
                    <span className="str"># Results ▸ Revenue</span>
                  </div>
                  <div className="ln dim">
                    {"  "}
                    <span className="hl">p.4 · bbox 72,560,520,610 · char 8421+612 · #c2a1</span>
                  </div>
                  <div className="ln">
                    <span className="pr">$</span> <span className="cursor" />
                  </div>
                </div>
              </div>
            </div>
          </div>
        </section>

        {/* ===================== STATS ===================== */}
        <section className="stats">
          <div className="wrap stats-grid">
            <div className="stat">
              <div className="v tnum">
                ~49<span className="u">µs</span>
              </div>
              <div className="k">open + extract text</div>
            </div>
            <div className="stat">
              <div className="v tnum">0</div>
              <div className="k">native deps (default build)</div>
            </div>
            <div className="stat">
              <div className="v">never</div>
              <div className="k">panics · cargo-fuzz guarded</div>
            </div>
            <div className="stat">
              <div className="v tnum">5</div>
              <div className="k">surfaces, one source tree</div>
            </div>
          </div>
        </section>

        {/* ===================== PROVENANCE ===================== */}
        <section className="band" id="provenance">
          <div className="wrap">
            <div className="section-head">
              <p className="eyebrow">Structured output for RAG</p>
              <h2>Grounded chunks. Every one traceable.</h2>
              <p>
                Hosted and VLM parsers hand you text. pdfkit hands you text <em>plus</em> the exact
                place it came from — so your retrieval can cite the source, and you can highlight it
                back in the document.
              </p>
            </div>

            <div className="prov-grid">
              <div className="panel">
                <div className="panel-bar">
                  <span className="title">report.pdf — page 4</span>
                  <span className="tag">raw</span>
                </div>
                <div className="doc-skeleton" aria-hidden="true">
                  <div className="row" style={{ width: "62%" }} />
                  <div className="row" style={{ width: "100%" }} />
                  <div className="row" style={{ width: "96%" }} />
                  <div className="row" style={{ width: "88%" }} />
                  <div className="blk">
                    <div className="ph">table</div>
                    <div className="ph">figure</div>
                  </div>
                  <div className="row" style={{ width: "92%" }} />
                  <div className="row" style={{ width: "74%" }} />
                  <div className="row" style={{ width: "84%" }} />
                </div>
              </div>

              <div className="arrow-mid">
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <path d="M5 12h14M13 6l6 6-6 6" />
                </svg>
              </div>

              <div className="panel window-like">
                <div className="panel-bar">
                  <span className="title mono">chunk.json</span>
                  <span className="tag">lossless · with provenance</span>
                </div>
                <div className="json-body">
                  <div className="ln">
                    <span className="p">{"{"}</span>
                  </div>
                  <div className="ln">
                    {"  "}
                    <span className="k">{'"id"'}</span>: <span className="s">{'"c2a1f0"'}</span>,
                  </div>
                  <div className="ln">
                    {"  "}
                    <span className="k">{'"kind"'}</span>: <span className="s">{'"Paragraph"'}</span>,
                  </div>
                  <div className="ln">
                    {"  "}
                    <span className="k">{'"text"'}</span>:{" "}
                    <span className="s">{'"Revenue rose 18% to…"'}</span>,
                  </div>
                  <div className="ln">
                    <span className="hl">
                      {"  "}
                      <span className="k">{'"page"'}</span>: <span className="n">4</span>,
                    </span>
                  </div>
                  <div className="ln">
                    <span className="hl">
                      {"  "}
                      <span className="k">{'"bbox"'}</span>:{" "}
                      <span className="n">[72, 560, 520, 610]</span>,
                    </span>
                  </div>
                  <div className="ln">
                    <span className="hl">
                      {"  "}
                      <span className="k">{'"char_start"'}</span>: <span className="n">8421</span>,{" "}
                      <span className="k">{'"char_len"'}</span>: <span className="n">612</span>,
                    </span>
                  </div>
                  <div className="ln">
                    {"  "}
                    <span className="k">{'"heading_path"'}</span>: <span className="p">[</span>
                    <span className="s">{'"Results"'}</span>, <span className="s">{'"Revenue"'}</span>
                    <span className="p">]</span>,
                  </div>
                  <div className="ln">
                    {"  "}
                    <span className="k">{'"token_estimate"'}</span>: <span className="n">154</span>
                  </div>
                  <div className="ln">
                    <span className="p">{"}"}</span>
                  </div>
                </div>
              </div>
            </div>

            <div className="prov-legend">
              <span className="sw">
                <i /> Highlighted fields are provenance
              </span>
              <span>
                One-based page · bounding box in points · exact char span into the reconstructed
                document text · stable id.
              </span>
            </div>
          </div>
        </section>

        {/* ===================== HOW IT WORKS ===================== */}
        <section className="band tight" id="how">
          <div className="wrap">
            <div className="section-head">
              <p className="eyebrow">extract() · Mode::Auto</p>
              <h2>One engine. Three fallbacks. Every page takes the cheapest path that works.</h2>
              <p>
                Read-first by design: try the text layer, fall back to local OCR for scans, render to
                PNG only when there&apos;s nothing else to read — then emit chunks with provenance.
              </p>
            </div>

            <div className="pipe">
              <div className="pipe-row">
                <div className="node start">
                  <div className="nk">input</div>
                  <div className="nt">PDF</div>
                </div>
                <div className="pipe-arrow">→</div>
                <div className="node decision">
                  <div className="nk">classify</div>
                  <div className="nt">Text layer?</div>
                  <div className="nd">char count · image coverage</div>
                </div>
              </div>

              <div className="branches">
                <div className="branch">
                  <div className="blabel">enough text ↓</div>
                  <div className="node">
                    <div className="nk">path a</div>
                    <div className="nt">Extract text</div>
                    <div className="nd">layout-aware reflow</div>
                  </div>
                </div>
                <div className="branch">
                  <div className="blabel muted">scanned ↓</div>
                  <div className="node">
                    <div className="nk">path b</div>
                    <div className="nt">OCR</div>
                    <div className="nd">local ONNX · offline</div>
                  </div>
                </div>
                <div className="branch">
                  <div className="blabel muted">image-only ↓</div>
                  <div className="node">
                    <div className="nk">path c</div>
                    <div className="nt">Render → PNG</div>
                    <div className="nd">for a multimodal model</div>
                  </div>
                </div>
              </div>

              <div className="pipe-down">↓</div>

              <div className="pipe-out pipe-row">
                <div className="node out" style={{ minWidth: 280 }}>
                  <div className="nk">output</div>
                  <div className="nt">Chunks + provenance</div>
                  <div className="nd mono">{"{ page · bbox · char_span · id · heading_path }"}</div>
                </div>
              </div>
            </div>
          </div>
        </section>

        {/* ===================== FEATURES ===================== */}
        <section className="band tight" id="features">
          <div className="wrap">
            <div className="section-head">
              <p className="eyebrow">Capabilities</p>
              <h2>Everything you need to read, structure, and write PDFs.</h2>
            </div>

            <div className="feat-grid">
              <div className="card span2">
                <div className="ic">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="M4 6h16M4 10h10M4 14h16M4 18h7" />
                  </svg>
                </div>
                <h3>Text extraction that reads correctly</h3>
                <p>
                  Multi-column reading order, accurate word spacing from real{" "}
                  <span className="mono">/Widths</span> and Type0 CID metrics, and encoding-aware
                  decoding. When a PDF is tagged, the <span className="mono">/StructTreeRoot</span>{" "}
                  drives heading levels, table cells, list nesting, and figure alt-text — preferred
                  over geometry heuristics.
                </p>
              </div>
              <div className="card">
                <div className="ic">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <rect x="3" y="3" width="18" height="18" rx="2" />
                    <path d="M3 9h18M9 21V9" />
                  </svg>
                </div>
                <h3>Real tables</h3>
                <p>
                  A normalized cell grid with column inference, colspan, and per-cell bbox — to HTML,
                  CSV, or Markdown. Not a tab-joined blob.
                </p>
              </div>
              <div className="card">
                <div className="ic">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <circle cx="12" cy="12" r="9" />
                    <path d="M12 3a9 9 0 0 1 0 18" />
                  </svg>
                </div>
                <h3>Deterministic &amp; offline</h3>
                <p>
                  No network, no hosted model in the default build. Same input, same output, every
                  run. OCR runs locally via ONNX.
                </p>
              </div>
              <div className="card">
                <div className="ic">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="M4 7V4h16v3M9 20h6M12 4v16" />
                  </svg>
                </div>
                <h3>Four serializations</h3>
                <p>
                  Chunks to JSON, Markdown, HTML, or CSV — lossless, with provenance intact and
                  token-sized packing with optional overlap.
                </p>
              </div>
              <div className="card">
                <div className="ic">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" />
                  </svg>
                </div>
                <h3>A separate write path</h3>
                <p>
                  Create, merge, split, rotate, watermark, and fill forms — a subsystem that never
                  flows through the extraction engine.
                </p>
              </div>
              <div className="card span2">
                <div className="ic">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                    <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" />
                  </svg>
                </div>
                <h3>Pure-Rust by default — opt into the heavy stuff</h3>
                <p>
                  The default build compiles and tests with zero native dependencies and no network.
                  PDFIUM (high-fidelity render) and Tesseract OCR are feature flags only. Compile in
                  exactly what you need:{" "}
                  <span className="mono">render-native · serde · render-pdfium · ocr-ocrs · llm-adapter</span>.
                </p>
              </div>
            </div>
          </div>
        </section>

        {/* ===================== SURFACES ===================== */}
        <section className="band tight" id="surfaces">
          <div className="wrap">
            <div className="surf-grid">
              <div>
                <p className="eyebrow">One source tree</p>
                <h2
                  style={{
                    fontSize: "clamp(26px,3.2vw,38px)",
                    lineHeight: 1.1,
                    letterSpacing: "-0.025em",
                    fontWeight: 600,
                    margin: "14px 0 0",
                  }}
                >
                  Reach for it from anywhere.
                </h2>
                <div className="surf-list">
                  <div className="surf-item">
                    <span className="si">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <path d="M4 17l6-6-6-6M12 19h8" />
                      </svg>
                    </span>
                    <span className="txt">
                      <b>CLI</b>
                      <span>
                        — the <span className="mono">pdfkit</span> command
                      </span>
                    </span>
                  </div>
                  <div className="surf-item">
                    <span className="si">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <path d="m16 18 6-6-6-6M8 6l-6 6 6 6" />
                      </svg>
                    </span>
                    <span className="txt">
                      <b>Library</b>
                      <span>— embeddable Rust crates</span>
                    </span>
                  </div>
                  <div className="surf-item">
                    <span className="si">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <rect x="2" y="4" width="20" height="16" rx="2" />
                        <path d="M6 8h.01M10 8h.01" />
                      </svg>
                    </span>
                    <span className="txt">
                      <b>WebAssembly</b>
                      <span>— in the browser / npm</span>
                    </span>
                  </div>
                  <div className="surf-item">
                    <span className="si">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <path d="M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0zM3 12h18M12 3a15 15 0 0 1 0 18 15 15 0 0 1 0-18z" />
                      </svg>
                    </span>
                    <span className="txt">
                      <b>HTTP API</b>
                      <span>— self-hostable, OpenAPI 3.1</span>
                    </span>
                  </div>
                  <div className="surf-item">
                    <span className="si">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                        <rect x="3" y="3" width="18" height="18" rx="2" />
                        <path d="M3 9h18" />
                      </svg>
                    </span>
                    <span className="txt">
                      <b>Web UI</b>
                      <span>— a Next.js reference app</span>
                    </span>
                  </div>
                </div>
              </div>

              <div>
                <CodeTabs />
              </div>
            </div>
          </div>
        </section>

        {/* ===================== CTA ===================== */}
        <section className="band tight" id="start">
          <div className="wrap">
            <div className="cta">
              <h2>Compile in only what you need.</h2>
              <p>
                Start with the pure-Rust core and add render, OCR, or the HTTP API when you want them.
                Deterministic, offline, and built from scratch.
              </p>
              <div className="features-line">
                render-native · serde · render-pdfium · ocr-ocrs · ocr-tesseract · llm-adapter · wasm
              </div>
              <div className="cta-actions">
                <Link className="btn btn-primary btn-lg" href="/docs">
                  Read the docs
                </Link>
                <Link className="btn btn-secondary btn-lg" href="/extract">
                  Quick start
                </Link>
              </div>
            </div>
          </div>
        </section>
      </main>

      {/* ===================== FOOTER ===================== */}
      <footer className="foot">
        <div className="wrap">
          <div className="foot-grid">
            <div>
              <a className="brand" href="#top">
                <span className="glyph">pk</span> pdfkit
              </a>
              <p className="blurb">
                A from-scratch, AI-oriented PDF toolkit in Rust. Read-first extraction, structured
                output for RAG, and a separate edit path.
              </p>
            </div>
            <div className="foot-col">
              <h3>Read</h3>
              <Link href="/extract">Extract text</Link>
              <Link href="/chunks">Chunk for RAG</Link>
              <Link href="/render">Render a page</Link>
              <Link href="/docs">Tagged structure</Link>
            </div>
            <div className="foot-col">
              <h3>Write</h3>
              <Link href="/edit">Create &amp; merge</Link>
              <Link href="/edit">Split &amp; rotate</Link>
              <Link href="/edit">Watermark</Link>
              <Link href="/edit">Fill forms</Link>
            </div>
            <div className="foot-col">
              <h3>Project</h3>
              <a href={REPO_URL} target="_blank" rel="noopener noreferrer">
                GitHub
              </a>
              <Link href="/docs">Documentation</Link>
              <Link href="/docs">HTTP API</Link>
              <a href={REPO_URL} target="_blank" rel="noopener noreferrer">
                Benchmarks
              </a>
            </div>
          </div>
          <div className="foot-bottom">
            <span className="mono">core ← render ← ocr · chunk · edit · cli · wasm · api</span>
            <span>Licensed MIT / Apache-2.0 · PDFs are hostile input — we treat them that way.</span>
          </div>
        </div>
      </footer>
    </div>
  );
}

/** Small inline check used in the hero trust row. */
function Check() {
  return (
    <svg
      className="ck"
      width="15"
      height="15"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M20 6 9 17l-5-5" />
    </svg>
  );
}
