"use client";

import { useState } from "react";
import { FileType2 } from "lucide-react";
import { toast } from "sonner";
import { Uploader } from "@/components/pdf/Uploader";
import { Button, Card, ErrorBox, PageHeader } from "@/components/ui";
import { convertDocxToPdf, errorMessage } from "@/lib/api/client";
import { downloadBlob } from "@/lib/download";

// Accept the .docx extension and its MIME type.
const DOCX_ACCEPT =
  ".docx,application/vnd.openxmlformats-officedocument.wordprocessingml.document";

function outputName(file: File): string {
  const base = file.name.replace(/\.docx$/i, "");
  return `${base || "converted"}.pdf`;
}

export default function ConvertPage() {
  const [file, setFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function run() {
    if (!file) return;
    setBusy(true);
    setError(null);
    try {
      const blob = await convertDocxToPdf(file);
      const name = outputName(file);
      downloadBlob(blob, name);
      toast.success("Converted to PDF", { description: name });
    } catch (e) {
      const msg = errorMessage(e);
      setError(msg);
      toast.error("Conversion failed", { description: msg });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <PageHeader
        title="Word → PDF"
        subtitle="Convert a Word .docx document to a PDF — pure-Rust and offline. Renders paragraphs, headings, bold/italic, bullet & numbered lists, tables, and images."
      />

      <Card className="space-y-6 p-6">
        <Uploader
          file={file}
          onFile={(f) => {
            setFile(f);
            setError(null);
          }}
          accept={DOCX_ACCEPT}
          noun="Word document"
        />
        <div className="flex flex-wrap items-center justify-between gap-3">
          <p className="flex items-center gap-2 text-[13px] text-muted-foreground">
            <FileType2 className="size-4 text-accent-text" />
            Accepts <span className="font-mono text-foreground">.docx</span> (Office Open XML).
          </p>
          <Button onClick={run} disabled={!file} loading={busy}>
            {busy ? "Converting…" : "Convert → PDF"}
          </Button>
        </div>
        <ErrorBox error={error} />
      </Card>
    </div>
  );
}
