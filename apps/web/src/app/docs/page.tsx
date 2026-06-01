"use client";

import { useEffect, useState } from "react";

type Operation = { summary?: string; tags?: string[] };
type OpenApiDoc = {
  info?: { title?: string; version?: string };
  paths?: Record<string, Record<string, Operation>>;
};

const METHODS = ["get", "post", "put", "patch", "delete"] as const;

export default function DocsPage() {
  const [doc, setDoc] = useState<OpenApiDoc | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetch("/api/pdfkit/openapi.json")
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.json() as Promise<OpenApiDoc>;
      })
      .then(setDoc)
      .catch((e: unknown) => setError(String(e)));
  }, []);

  const rows: Array<{ method: string; path: string; summary: string }> = [];
  if (doc?.paths) {
    for (const [path, ops] of Object.entries(doc.paths)) {
      for (const method of METHODS) {
        const op = ops[method];
        if (op) rows.push({ method: method.toUpperCase(), path, summary: op.summary ?? "" });
      }
    }
    rows.sort((a, b) => a.path.localeCompare(b.path));
  }

  return (
    <div className="space-y-4">
      <h1 className="text-xl font-bold">API</h1>
      <p className="text-sm text-gray-600">
        Live from the backend&apos;s OpenAPI document via the proxy. Raw spec:{" "}
        <a className="text-blue-600 underline" href="/api/pdfkit/openapi.json">
          /api/pdfkit/openapi.json
        </a>
        . The backend also serves Swagger UI at <code className="rounded bg-gray-100 px-1">/docs</code>.
      </p>
      {error && (
        <p className="rounded-md bg-amber-50 px-3 py-2 text-sm text-amber-700">
          Could not reach the API ({error}). Is pdfkit-api running and{" "}
          <code className="rounded bg-amber-100 px-1">API_BASE_URL</code> set?
        </p>
      )}
      {doc && (
        <>
          <p className="text-sm text-gray-600">
            {doc.info?.title} v{doc.info?.version}
          </p>
          <table className="w-full border-collapse text-sm">
            <thead>
              <tr className="border-b text-left text-gray-500">
                <th className="py-2 pr-4">Method</th>
                <th className="py-2 pr-4">Path</th>
                <th className="py-2">Summary</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={`${r.method} ${r.path}`} className="border-b">
                  <td className="py-2 pr-4 font-mono text-xs">{r.method}</td>
                  <td className="py-2 pr-4 font-mono text-xs">{r.path}</td>
                  <td className="py-2 text-gray-600">{r.summary}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </div>
  );
}
