"use client";

import { useState } from "react";
import { Copy } from "lucide-react";
import { toast } from "sonner";
import { Uploader } from "@/components/pdf/Uploader";
import {
  Badge,
  Button,
  Card,
  ErrorBox,
  Field,
  PageHeader,
  Segmented,
  Textarea,
} from "@/components/ui";
import { ApiError, extract } from "@/lib/api/client";
import type { ExtractMode, ExtractResponse } from "@/lib/api/types";

const MODES = [
  { value: "auto", label: "Auto" },
  { value: "text", label: "Text" },
  { value: "images", label: "Images" },
  { value: "both", label: "Both" },
] as const;

export default function ExtractPage() {
  const [file, setFile] = useState<File | null>(null);
  const [mode, setMode] = useState<ExtractMode>("auto");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<ExtractResponse | null>(null);

  async function run() {
    if (!file) return;
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const r = await extract(file, { mode });
      setResult(r);
      toast.success("Extraction complete", {
        description: `${r.pages_processed.length} page(s) processed`,
      });
    } catch (e) {
      const msg = e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
      setError(msg);
      toast.error("Extraction failed", { description: msg });
    } finally {
      setBusy(false);
    }
  }

  async function copyText() {
    if (!result) return;
    await navigator.clipboard.writeText(result.text);
    toast.success("Copied to clipboard");
  }

  return (
    <div>
      <PageHeader title="Extract" subtitle="Pull text and rendered page images from a PDF." />

      <Card className="space-y-6 p-6">
        <Uploader
          file={file}
          onFile={(f) => {
            setFile(f);
            setResult(null);
          }}
        />
        <div className="flex flex-wrap items-end justify-between gap-4">
          <Field label="Mode">
            <Segmented options={MODES} value={mode} onChange={setMode} />
          </Field>
          <Button onClick={run} disabled={!file} loading={busy}>
            {busy ? "Extracting…" : "Extract"}
          </Button>
        </div>
        <ErrorBox error={error} />
      </Card>

      {result && (
        <Card className="mt-6 animate-fade-in-up space-y-4 p-6">
          <div className="flex flex-wrap items-center gap-2">
            <Badge tone="info">{result.pages_processed.length} pages</Badge>
            <Badge>{result.text.length.toLocaleString()} chars</Badge>
            {result.page_images.length > 0 && (
              <Badge tone="info">{result.page_images.length} images</Badge>
            )}
            {result.truncated.text && <Badge tone="warning">text truncated</Badge>}
            {result.truncated.images && <Badge tone="warning">images truncated</Badge>}
            <div className="ml-auto">
              <Button variant="secondary" size="sm" onClick={copyText} disabled={!result.text}>
                <Copy className="size-4" />
                Copy
              </Button>
            </div>
          </div>
          {result.text && <Textarea readOnly value={result.text} className="h-80" />}
          {result.page_images.length > 0 && (
            <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
              {result.page_images.map((img) => (
                // eslint-disable-next-line @next/next/no-img-element
                <img
                  key={img.page}
                  alt={`page ${img.page}`}
                  src={`data:image/png;base64,${img.png_base64}`}
                  className="rounded-lg border border-border"
                />
              ))}
            </div>
          )}
        </Card>
      )}
    </div>
  );
}
