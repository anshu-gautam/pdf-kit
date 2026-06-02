"use client";

import { FileText } from "lucide-react";
import { Dropzone } from "./Dropzone";
import { Button } from "@/components/ui";

/** Dropzone when empty; a compact file chip once a file is chosen. */
export function Uploader({
  file,
  onFile,
  accept,
  noun,
}: {
  file: File | null;
  onFile: (file: File | null) => void;
  /** `accept` attribute forwarded to the Dropzone (defaults to PDFs). */
  accept?: string;
  /** Singular noun forwarded to the Dropzone (e.g. "Word document"). */
  noun?: string;
}) {
  if (!file) {
    return <Dropzone onFiles={(files) => onFile(files[0] ?? null)} accept={accept} noun={noun} />;
  }
  return (
    <div className="flex items-center justify-between gap-3 rounded-lg border border-border bg-surface-subtle px-3 py-2">
      <span className="flex min-w-0 items-center gap-2 text-sm">
        <FileText className="size-4 shrink-0 text-muted-foreground" />
        <span className="truncate font-medium text-foreground">{file.name}</span>
        <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
          {(file.size / 1024).toFixed(0)} KB
        </span>
      </span>
      <Button variant="ghost" size="sm" onClick={() => onFile(null)}>
        Change
      </Button>
    </div>
  );
}
