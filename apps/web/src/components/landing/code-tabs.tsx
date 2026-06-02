"use client";

import { useRef, useState } from "react";

type Pane = "cli" | "rust" | "http";

const TABS: { id: Pane; label: string }[] = [
  { id: "cli", label: "CLI" },
  { id: "rust", label: "Rust" },
  { id: "http", label: "HTTP" },
];

export function CodeTabs() {
  const [active, setActive] = useState<Pane>("cli");
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  // WAI-ARIA tabs pattern: arrow/Home/End move selection and focus together.
  function onTabKeyDown(e: React.KeyboardEvent<HTMLDivElement>) {
    const idx = TABS.findIndex((t) => t.id === active);
    let next = idx;
    if (e.key === "ArrowRight") next = (idx + 1) % TABS.length;
    else if (e.key === "ArrowLeft") next = (idx - 1 + TABS.length) % TABS.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = TABS.length - 1;
    else return;
    e.preventDefault();
    setActive(TABS[next].id);
    tabRefs.current[next]?.focus();
  }

  return (
    <>
      <div className="tabs" role="tablist" aria-label="Code examples by surface" onKeyDown={onTabKeyDown}>
        {TABS.map((t, i) => (
          <button
            key={t.id}
            ref={(el) => {
              tabRefs.current[i] = el;
            }}
            className="tab"
            role="tab"
            id={`tab-${t.id}`}
            aria-selected={active === t.id}
            aria-controls={`pane-${t.id}`}
            tabIndex={active === t.id ? 0 : -1}
            onClick={() => setActive(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div className="code-window">
        <div
          className={`code-pane${active === "cli" ? " active" : ""}`}
          role="tabpanel"
          id="pane-cli"
          aria-labelledby="tab-cli"
          tabIndex={0}
        >
          <div className="ln"><span className="cm"># structured RAG chunks — JSON, Markdown, or text</span></div>
          <div className="ln"><span className="pr">$</span> pdfkit chunk document.pdf --format json</div>
          <div className="ln"><span className="pr">$</span> pdfkit chunk document.pdf --format md --context</div>
          <div className="ln">&nbsp;</div>
          <div className="ln"><span className="cm"># inspect structure · render a page</span></div>
          <div className="ln"><span className="pr">$</span> pdfkit outline document.pdf</div>
          <div className="ln"><span className="pr">$</span> pdfkit render document.pdf --page 1 -o p1.png</div>
        </div>
        <div
          className={`code-pane${active === "rust" ? " active" : ""}`}
          role="tabpanel"
          id="pane-rust"
          aria-labelledby="tab-rust"
          tabIndex={0}
        >
          <div className="ln"><span className="kw">use</span> pdfkit_core::{"{Engine, OpenOptions}"};</div>
          <div className="ln"><span className="kw">use</span> pdfkit_chunk::{"{chunk_document, to_json, ChunkOptions}"};</div>
          <div className="ln">&nbsp;</div>
          <div className="ln"><span className="kw">let</span> doc = Engine::<span className="fn">new</span>()?.<span className="fn">open</span>(<span className="str">&quot;document.pdf&quot;</span>, OpenOptions::<span className="fn">default</span>())?;</div>
          <div className="ln"><span className="kw">let</span> chunks = <span className="fn">chunk_document</span>(&amp;doc, &amp;ChunkOptions::<span className="fn">default</span>())?;</div>
          <div className="ln"><span className="kw">let</span> json = <span className="fn">to_json</span>(&amp;chunks)?; <span className="cm">{"// lossless, with provenance"}</span></div>
        </div>
        <div
          className={`code-pane${active === "http" ? " active" : ""}`}
          role="tabpanel"
          id="pane-http"
          aria-labelledby="tab-http"
          tabIndex={0}
        >
          <div className="ln"><span className="cm"># self-host the API, then:</span></div>
          <div className="ln"><span className="pr">$</span> curl -F file=@report.pdf \</div>
          <div className="ln">    -F <span className="str">&apos;options=&#123;&quot;format&quot;:&quot;json&quot;&#125;&apos;</span> \</div>
          <div className="ln">    http://127.0.0.1:8080/v1/chunks</div>
          <div className="ln">&nbsp;</div>
          <div className="ln"><span className="cm"># typed routes: /v1/extract · /metadata · /render · /edit/*</span></div>
          <div className="ln"><span className="cm"># live schema at /openapi.json · Swagger at /docs</span></div>
        </div>
      </div>
    </>
  );
}
