"use client";

import { useEffect, useState } from "react";
import { ExternalLink } from "lucide-react";
import { Badge, Card, PageHeader, Spinner } from "@/components/ui";

type Operation = { summary?: string };
type OpenApiDoc = {
  info?: { title?: string; version?: string };
  paths?: Record<string, Record<string, Operation>>;
};

const METHODS = ["get", "post", "put", "patch", "delete"] as const;
const METHOD_TONE: Record<string, "neutral" | "success" | "warning" | "danger" | "info"> = {
  GET: "success",
  POST: "info",
  PUT: "warning",
  PATCH: "warning",
  DELETE: "danger",
};

export default function DocsPage() {
  const [doc, setDoc] = useState<OpenApiDoc | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetch("/api/pdfkit/openapi.json")
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.json() as Promise<OpenApiDoc>;
      })
      .then(setDoc)
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  const rows: Array<{ method: string; path: string; summary: string }> = [];
  if (doc?.paths) {
    for (const [path, ops] of Object.entries(doc.paths)) {
      for (const method of METHODS) {
        const op = ops[method];
        if (op) rows.push({ method: method.toUpperCase(), path, summary: op.summary ?? "" });
      }
    }
    rows.sort((a, b) => a.path.localeCompare(b.path) || a.method.localeCompare(b.method));
  }

  return (
    <div>
      <PageHeader
        title="API"
        subtitle={doc?.info ? `${doc.info.title} v${doc.info.version}` : "The pdfkit-api HTTP surface."}
      />
      <Card className="space-y-5 p-6">
        <p className="text-sm text-muted-foreground">
          Live from the backend OpenAPI document.{" "}
          <a
            className="inline-flex items-center gap-1 font-medium text-accent-text hover:underline"
            href="/api/pdfkit/openapi.json"
            target="_blank"
            rel="noreferrer"
          >
            openapi.json <ExternalLink className="size-3.5" />
          </a>{" "}
          · the backend also serves Swagger UI at{" "}
          <code className="rounded bg-surface-subtle px-1 py-0.5 font-mono text-[12px]">/docs</code>.
        </p>

        {loading && (
          <div className="flex items-center gap-2 py-8 text-sm text-muted-foreground">
            <Spinner className="text-muted-foreground" /> Loading schema…
          </div>
        )}

        {error && (
          <p className="rounded-lg border border-warning-border bg-warning-tint px-3 py-2 text-[13px] text-warning-text">
            Could not reach the API ({error}). Make sure pdfkit-api is running and API_BASE_URL is set.
          </p>
        )}

        {doc && (
          <div className="overflow-x-auto">
            <table className="w-full border-collapse text-sm">
              <thead>
                <tr className="border-b border-border text-left">
                  <th className="py-2 pr-4 text-[11px] font-semibold uppercase tracking-[0.06em] text-subtle-foreground">
                    Method
                  </th>
                  <th className="py-2 pr-4 text-[11px] font-semibold uppercase tracking-[0.06em] text-subtle-foreground">
                    Path
                  </th>
                  <th className="py-2 text-[11px] font-semibold uppercase tracking-[0.06em] text-subtle-foreground">
                    Summary
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => (
                  <tr key={`${r.method} ${r.path}`} className="border-b border-border last:border-0">
                    <td className="py-2.5 pr-4">
                      <Badge tone={METHOD_TONE[r.method] ?? "neutral"}>{r.method}</Badge>
                    </td>
                    <td className="py-2.5 pr-4 font-mono text-[13px] text-foreground">{r.path}</td>
                    <td className="py-2.5 text-muted-foreground">{r.summary}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </div>
  );
}
