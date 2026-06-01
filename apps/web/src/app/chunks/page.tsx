"use client";

import { useState } from "react";
import { Copy, Download } from "lucide-react";
import { toast } from "sonner";
import { Uploader } from "@/components/pdf/Uploader";
import { Badge, Button, Card, ErrorBox, Field, Input, PageHeader, Segmented } from "@/components/ui";
import { ApiError, chunks } from "@/lib/api/client";
import { downloadBlob } from "@/lib/download";
import type { ChunkFormat } from "@/lib/api/types";

const FORMATS = [
  { value: "json", label: "JSON" },
  { value: "markdown", label: "Markdown" },
] as const;

export default function ChunksPage() {
  const [file, setFile] = useState<File | null>(null);
  const [format, setFormat] = useState<ChunkFormat>("json");
  const [targetTokens, setTargetTokens] = useState(512);
  const [contextual, setContextual] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [output, setOutput] = useState<string | null>(null);

  async function run() {
    if (!file) return;
    setBusy(true);
    setError(null);
    setOutput(null);
    try {
      const r = await chunks(file, {
        format,
        target_tokens: targetTokens,
        contextual_prefix: contextual,
      });
      setOutput(r.format === "json" ? JSON.stringify(r.json, null, 2) : r.markdown);
      toast.success("Chunks ready");
    } catch (e) {
      const msg = e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
      setError(msg);
      toast.error("Chunking failed", { description: msg });
    } finally {
      setBusy(false);
    }
  }

  async function copyOut() {
    if (!output) return;
    await navigator.clipboard.writeText(output);
    toast.success("Copied to clipboard");
  }

  function download() {
    if (!output) return;
    const ext = format === "json" ? "json" : "md";
    const type = format === "json" ? "application/json" : "text/markdown";
    downloadBlob(new Blob([output], { type }), `chunks.${ext}`);
  }

  return (
    <div>
      <PageHeader title="Chunks" subtitle="Produce RAG-ready chunks as JSON or Markdown, with provenance." />

      <Card className="space-y-6 p-6">
        <Uploader
          file={file}
          onFile={(f) => {
            setFile(f);
            setOutput(null);
          }}
        />
        <div className="flex flex-wrap items-end gap-4">
          <Field label="Format">
            <Segmented options={FORMATS} value={format} onChange={setFormat} />
          </Field>
          <Field label="Target tokens">
            <Input
              type="number"
              min={64}
              value={targetTokens}
              onChange={(e) => setTargetTokens(Number(e.target.value))}
              className="w-28"
            />
          </Field>
          <label className="flex h-9 items-center gap-2 text-sm text-foreground">
            <input
              type="checkbox"
              checked={contextual}
              onChange={(e) => setContextual(e.target.checked)}
              className="size-4 rounded border-border-strong accent-[var(--primary)]"
            />
            Contextual prefix
          </label>
          <Button className="ml-auto" onClick={run} disabled={!file} loading={busy}>
            {busy ? "Chunking…" : "Chunk"}
          </Button>
        </div>
        <ErrorBox error={error} />
      </Card>

      {output !== null && (
        <Card className="mt-6 animate-fade-in-up space-y-4 p-6">
          <div className="flex items-center justify-between gap-2">
            <Badge tone="info">{format.toUpperCase()}</Badge>
            <div className="flex gap-2">
              <Button variant="secondary" size="sm" onClick={copyOut}>
                <Copy className="size-4" />
                Copy
              </Button>
              <Button variant="secondary" size="sm" onClick={download}>
                <Download className="size-4" />
                Download
              </Button>
            </div>
          </div>
          <pre className="max-h-[32rem] overflow-auto rounded-xl border border-border bg-surface-subtle p-4 font-mono text-[13px] leading-[1.6] text-foreground">
            {output}
          </pre>
        </Card>
      )}
    </div>
  );
}
