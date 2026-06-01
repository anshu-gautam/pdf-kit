"use client";

import { useCallback, useRef, useState } from "react";
import { UploadCloud } from "lucide-react";
import { cn } from "@/lib/cn";

const DEFAULT_MAX_BYTES = 50 * 1024 * 1024;

export function Dropzone({
  onFiles,
  multiple = false,
  maxBytes = DEFAULT_MAX_BYTES,
  className,
}: {
  onFiles: (files: File[]) => void;
  multiple?: boolean;
  maxBytes?: number;
  className?: string;
}) {
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const handle = useCallback(
    (list: FileList | null) => {
      if (!list || list.length === 0) return;
      const files = Array.from(list);
      const tooBig = files.find((f) => f.size > maxBytes);
      if (tooBig) {
        setError(`${tooBig.name} exceeds ${(maxBytes / 1024 / 1024).toFixed(0)} MB`);
        return;
      }
      setError(null);
      onFiles(files);
    },
    [maxBytes, onFiles],
  );

  const maxMb = (maxBytes / 1024 / 1024).toFixed(0);

  return (
    <div className={className}>
      <div
        role="button"
        tabIndex={0}
        aria-label="Upload a PDF"
        onClick={() => inputRef.current?.click()}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            inputRef.current?.click();
          }
        }}
        onDragOver={(e) => {
          e.preventDefault();
          setDragging(true);
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={(e) => {
          e.preventDefault();
          setDragging(false);
          handle(e.dataTransfer.files);
        }}
        className={cn(
          "group flex cursor-pointer flex-col items-center justify-center gap-3 rounded-xl border-2 border-dashed px-6 py-10 text-center outline-none transition-[border-color,background-color,box-shadow,transform] duration-150 ease-[var(--ease-out)] focus-visible:ring-[3px] focus-visible:ring-ring/50",
          dragging
            ? "border-primary bg-primary/5 ring-4 ring-primary/10"
            : "border-border bg-surface-subtle hover:border-border-strong dark:bg-white/[0.02]",
        )}
      >
        <div className={cn("transition-transform duration-150 ease-[var(--ease-out)]", dragging && "scale-[1.03]")}>
          <UploadCloud className={cn("size-8 transition-colors", dragging ? "text-primary" : "text-muted-foreground")} />
        </div>
        <div className="space-y-1">
          <p className="text-sm font-medium text-foreground">
            {dragging ? (
              "Drop to upload"
            ) : (
              <>
                Drop {multiple ? "PDFs" : "a PDF"} here, or <span className="text-accent-text">browse</span>
              </>
            )}
          </p>
          <p className="text-xs text-muted-foreground">
            PDF up to {maxMb} MB{multiple ? " · multiple allowed" : ""}
          </p>
        </div>
        <input
          ref={inputRef}
          type="file"
          accept="application/pdf"
          multiple={multiple}
          className="hidden"
          onChange={(e) => handle(e.target.files)}
        />
      </div>
      {error && <p className="mt-2 text-xs text-danger-text">{error}</p>}
    </div>
  );
}
