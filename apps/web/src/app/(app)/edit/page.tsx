"use client";

import { useState } from "react";
import { Combine, FileSignature, RotateCw, Scissors, Stamp } from "lucide-react";
import { toast } from "sonner";
import { Dropzone } from "@/components/pdf/Dropzone";
import { Uploader } from "@/components/pdf/Uploader";
import { Button, Card, ErrorBox, Field, Input, PageHeader, Textarea } from "@/components/ui";
import {
  editFill,
  editMerge,
  editRotate,
  editSplit,
  editWatermark,
  errorMessage,
} from "@/lib/api/client";
import { downloadBlob } from "@/lib/download";

function useEditAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function run(fn: () => Promise<Blob>, filename: string) {
    setBusy(true);
    setError(null);
    try {
      downloadBlob(await fn(), filename);
      toast.success(`Saved ${filename}`);
    } catch (e) {
      const msg = errorMessage(e);
      setError(msg);
      toast.error("Edit failed", { description: msg });
    } finally {
      setBusy(false);
    }
  }
  return { busy, error, run };
}

function SectionCard({
  icon,
  title,
  description,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <Card className="flex flex-col gap-4 p-6">
      <div className="flex items-start gap-3">
        <div className="grid size-9 shrink-0 place-items-center rounded-lg bg-surface-subtle text-muted-foreground">
          {icon}
        </div>
        <div>
          <h2 className="text-[15px] font-semibold text-foreground">{title}</h2>
          <p className="text-[13px] leading-[1.5] text-muted-foreground">{description}</p>
        </div>
      </div>
      {children}
    </Card>
  );
}

function MergeSection() {
  const [files, setFiles] = useState<File[]>([]);
  const { busy, error, run } = useEditAction();
  return (
    <SectionCard
      icon={<Combine className="size-5" />}
      title="Merge"
      description="Combine two or more PDFs into one, in order."
    >
      <Dropzone
        multiple
        onFiles={(dropped) =>
          setFiles((prev) => {
            // Accumulate across drops (dedupe by name+size) rather than replace.
            const byKey = new Map(prev.map((f) => [`${f.name}:${f.size}`, f]));
            for (const f of dropped) byKey.set(`${f.name}:${f.size}`, f);
            return [...byKey.values()];
          })
        }
      />
      {files.length > 0 && (
        <div className="flex items-center gap-2 text-[13px] text-muted-foreground">
          <span className="shrink-0 font-medium tabular-nums text-foreground">{files.length}</span>
          <span className="min-w-0 flex-1 truncate">{files.map((f) => f.name).join(", ")}</span>
          <Button variant="ghost" size="sm" onClick={() => setFiles([])}>
            Clear
          </Button>
        </div>
      )}
      <Button
        className="self-start"
        disabled={files.length < 2}
        loading={busy}
        onClick={() => run(() => editMerge(files), "merged.pdf")}
      >
        Merge → PDF
      </Button>
      <ErrorBox error={error} />
    </SectionCard>
  );
}

function parseRanges(input: string): Array<[number, number]> {
  return input
    .split(",")
    .map((p) => p.trim())
    .filter(Boolean)
    .map((p) => {
      const [a, b] = p.split("-").map((n) => Number(n.trim()));
      return [a, b ?? a] as [number, number];
    });
}

function SplitSection() {
  const [file, setFile] = useState<File | null>(null);
  const [ranges, setRanges] = useState("1-1");
  const { busy, error, run } = useEditAction();
  return (
    <SectionCard
      icon={<Scissors className="size-5" />}
      title="Split"
      description="One-based inclusive ranges; one PDF per range, zipped."
    >
      <Uploader file={file} onFile={setFile} />
      <Field label="Ranges" hint="e.g. 1-1, 2-3">
        <Input value={ranges} onChange={(e) => setRanges(e.target.value)} placeholder="1-1, 2-3" />
      </Field>
      <Button
        className="self-start"
        disabled={!file}
        loading={busy}
        onClick={() =>
          file &&
          run(() => {
            const parsed = parseRanges(ranges);
            if (
              parsed.length === 0 ||
              parsed.some(([a, b]) => !Number.isInteger(a) || !Number.isInteger(b) || a < 1 || b < a)
            ) {
              throw new Error("Enter valid one-based ranges, e.g. 1-1, 2-3.");
            }
            return editSplit(file, parsed);
          }, "split.zip")
        }
      >
        Split → ZIP
      </Button>
      <ErrorBox error={error} />
    </SectionCard>
  );
}

function RotateSection() {
  const [file, setFile] = useState<File | null>(null);
  const [page, setPage] = useState(1);
  const [degrees, setDegrees] = useState(90);
  const { busy, error, run } = useEditAction();
  return (
    <SectionCard
      icon={<RotateCw className="size-5" />}
      title="Rotate"
      description="Rotate a page by a multiple of 90°."
    >
      <Uploader file={file} onFile={setFile} />
      <div className="flex flex-wrap items-end gap-3">
        <Field label="Page">
          <Input
            type="number"
            min={1}
            value={page}
            onChange={(e) => setPage(Number(e.target.value))}
            className="w-24"
          />
        </Field>
        <Field label="Degrees">
          <Input
            type="number"
            step={90}
            value={degrees}
            onChange={(e) => setDegrees(Number(e.target.value))}
            className="w-28"
          />
        </Field>
      </div>
      <Button
        className="self-start"
        disabled={!file}
        loading={busy}
        onClick={() => file && run(() => editRotate(file, [{ page, degrees }]), "rotated.pdf")}
      >
        Rotate → PDF
      </Button>
      <ErrorBox error={error} />
    </SectionCard>
  );
}

function WatermarkSection() {
  const [file, setFile] = useState<File | null>(null);
  const [text, setText] = useState("DRAFT");
  const { busy, error, run } = useEditAction();
  return (
    <SectionCard
      icon={<Stamp className="size-5" />}
      title="Watermark"
      description="Overlay diagonal text on every page."
    >
      <Uploader file={file} onFile={setFile} />
      <Field label="Text">
        <Input value={text} onChange={(e) => setText(e.target.value)} />
      </Field>
      <Button
        className="self-start"
        disabled={!file || !text}
        loading={busy}
        onClick={() => file && run(() => editWatermark(file, { text }), "watermarked.pdf")}
      >
        Watermark → PDF
      </Button>
      <ErrorBox error={error} />
    </SectionCard>
  );
}

function parseFields(input: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of input.split("\n")) {
    const idx = line.indexOf("=");
    if (idx > 0) out[line.slice(0, idx).trim()] = line.slice(idx + 1).trim();
  }
  return out;
}

function FillSection() {
  const [file, setFile] = useState<File | null>(null);
  const [fields, setFields] = useState("name=Ada");
  const { busy, error, run } = useEditAction();
  return (
    <SectionCard
      icon={<FileSignature className="size-5" />}
      title="Fill form"
      description="Set AcroForm text fields by name (one per line)."
    >
      <Uploader file={file} onFile={setFile} />
      <Field label="Fields" hint="name=value, one per line">
        <Textarea
          value={fields}
          onChange={(e) => setFields(e.target.value)}
          className="h-24"
          placeholder={"name=value\nother=value"}
        />
      </Field>
      <Button
        className="self-start"
        disabled={!file}
        loading={busy}
        onClick={() => file && run(() => editFill(file, parseFields(fields)), "filled.pdf")}
      >
        Fill → PDF
      </Button>
      <ErrorBox error={error} />
    </SectionCard>
  );
}

export default function EditPage() {
  return (
    <div>
      <PageHeader title="Edit" subtitle="Merge, split, rotate, watermark, and fill PDF forms." />
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <MergeSection />
        <SplitSection />
        <RotateSection />
        <WatermarkSection />
        <FillSection />
      </div>
    </div>
  );
}
