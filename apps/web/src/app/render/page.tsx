"use client";

import { useState } from "react";
import { Download } from "lucide-react";
import { toast } from "sonner";
import { Uploader } from "@/components/pdf/Uploader";
import { Badge, Button, Card, ErrorBox, Field, Input, PageHeader } from "@/components/ui";
import { ApiError, renderPage } from "@/lib/api/client";
import { downloadBlob } from "@/lib/download";

export default function RenderPage() {
  const [file, setFile] = useState<File | null>(null);
  const [page, setPage] = useState(1);
  const [dpi, setDpi] = useState(150);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pngUrl, setPngUrl] = useState<string | null>(null);
  const [blob, setBlob] = useState<Blob | null>(null);

  async function run() {
    if (!file) return;
    setBusy(true);
    setError(null);
    if (pngUrl) URL.revokeObjectURL(pngUrl);
    setPngUrl(null);
    setBlob(null);
    try {
      const b = await renderPage(file, { page, dpi });
      setBlob(b);
      setPngUrl(URL.createObjectURL(b));
      toast.success(`Rendered page ${page}`);
    } catch (e) {
      const msg = e instanceof ApiError ? `${e.code}: ${e.message}` : String(e);
      setError(msg);
      toast.error("Render failed", { description: msg });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <PageHeader
        title="Render"
        subtitle="Rasterize a page to PNG. Requires the API built with the render-pdfium feature."
      />

      <Card className="space-y-6 p-6">
        <Uploader
          file={file}
          onFile={(f) => {
            setFile(f);
            setError(null);
          }}
        />
        <div className="flex flex-wrap items-end gap-4">
          <Field label="Page">
            <Input
              type="number"
              min={1}
              value={page}
              onChange={(e) => setPage(Number(e.target.value))}
              className="w-24"
            />
          </Field>
          <Field label="DPI">
            <Input
              type="number"
              min={1}
              value={dpi}
              onChange={(e) => setDpi(Number(e.target.value))}
              className="w-28"
            />
          </Field>
          <Button className="ml-auto" onClick={run} disabled={!file} loading={busy}>
            {busy ? "Rendering…" : "Render"}
          </Button>
        </div>
        <ErrorBox error={error} />
      </Card>

      {pngUrl && blob && (
        <Card className="mt-6 animate-fade-in-up space-y-4 p-6">
          <div className="flex items-center justify-between gap-2">
            <Badge tone="info">
              page {page} · {dpi} DPI
            </Badge>
            <Button variant="secondary" size="sm" onClick={() => downloadBlob(blob, `page-${page}.png`)}>
              <Download className="size-4" />
              Download PNG
            </Button>
          </div>
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img src={pngUrl} alt={`rendered page ${page}`} className="mx-auto rounded-lg border border-border" />
        </Card>
      )}
    </div>
  );
}
