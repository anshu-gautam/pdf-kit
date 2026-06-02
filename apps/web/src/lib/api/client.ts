// Thin typed client for pdfkit-api.
//
// Defaults to the same-origin Next proxy (`/api/pdfkit/*` → the Rust API), which
// avoids CORS (PRD §16). Set NEXT_PUBLIC_API_BASE_URL to call the Rust API
// directly instead (the API must then allow the browser origin via CORS).

import type {
  ApiErrorBody,
  ChunkRequest,
  ExtractRequest,
  ExtractResponse,
  FiguresResponse,
  MetadataResponse,
  RenderRequest,
} from "./types";

const BASE = process.env.NEXT_PUBLIC_API_BASE_URL ?? "/api/pdfkit";

export class ApiError extends Error {
  readonly code: string;
  readonly status: number;
  constructor(status: number, body: ApiErrorBody | string) {
    const message = typeof body === "string" ? body : body.message;
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = typeof body === "string" ? "error" : body.code;
  }
}

/** Format any thrown error for a user-facing message. */
export function errorMessage(e: unknown): string {
  if (e instanceof ApiError) return `${e.code}: ${e.message}`;
  if (e instanceof Error) return e.message;
  return String(e);
}

async function ensureOk(res: Response): Promise<Response> {
  if (res.ok) return res;
  // Read the body ONCE, then try to parse the JSON ApiError envelope. Calling
  // res.json() then res.text() would throw "body stream already read".
  const raw = await res.text().catch(() => "");
  let body: ApiErrorBody | string = raw || `${res.status} ${res.statusText}`.trim();
  try {
    const parsed = JSON.parse(raw) as Partial<ApiErrorBody>;
    if (parsed && typeof parsed === "object" && typeof parsed.message === "string") {
      body = { code: parsed.code ?? "error", message: parsed.message };
    }
  } catch {
    // Not JSON (e.g. a 413/408/proxy-500 text/HTML body) — keep the raw text.
  }
  throw new ApiError(res.status, body);
}

function form(file: File, options?: unknown): FormData {
  const fd = new FormData();
  fd.append("file", file);
  if (options !== undefined) fd.append("options", JSON.stringify(options));
  return fd;
}

async function postJson<T>(path: string, body: FormData): Promise<T> {
  const res = await ensureOk(await fetch(`${BASE}${path}`, { method: "POST", body }));
  return (await res.json()) as T;
}

async function postBlob(path: string, body: FormData): Promise<Blob> {
  const res = await ensureOk(await fetch(`${BASE}${path}`, { method: "POST", body }));
  return res.blob();
}

export function extract(file: File, options?: ExtractRequest): Promise<ExtractResponse> {
  return postJson("/v1/extract", form(file, options));
}

export function metadata(file: File, password?: string): Promise<MetadataResponse> {
  return postJson("/v1/metadata", form(file, password ? { password } : undefined));
}

export function figures(file: File): Promise<FiguresResponse> {
  return postJson("/v1/figures", form(file));
}

export type ChunkResult =
  | { format: "json"; json: unknown }
  | { format: "markdown"; markdown: string };

export async function chunks(file: File, options?: ChunkRequest): Promise<ChunkResult> {
  const res = await ensureOk(
    await fetch(`${BASE}/v1/chunks`, { method: "POST", body: form(file, options) }),
  );
  const contentType = res.headers.get("content-type") ?? "";
  if (contentType.includes("application/json")) {
    return { format: "json", json: await res.json() };
  }
  return { format: "markdown", markdown: await res.text() };
}

export function renderPage(file: File, options: RenderRequest): Promise<Blob> {
  return postBlob("/v1/render", form(file, options));
}

export async function editMerge(files: File[]): Promise<Blob> {
  const fd = new FormData();
  for (const f of files) fd.append("files", f);
  return postBlob("/v1/edit/merge", fd);
}

export function editSplit(file: File, ranges: Array<[number, number]>): Promise<Blob> {
  return postBlob("/v1/edit/split", form(file, { ranges }));
}

export function editRotate(
  file: File,
  rotations: Array<{ page: number; degrees: number }>,
): Promise<Blob> {
  return postBlob("/v1/edit/rotate", form(file, { rotations }));
}

export function editWatermark(
  file: File,
  opts: { text: string; font_size?: number; gray?: number; rotation_degrees?: number },
): Promise<Blob> {
  return postBlob("/v1/edit/watermark", form(file, opts));
}

export function editFill(file: File, fields: Record<string, string>): Promise<Blob> {
  return postBlob("/v1/edit/fill", form(file, { fields }));
}

/** Convert a Word .docx document to a PDF (returned as a Blob). */
export function convertDocxToPdf(file: File): Promise<Blob> {
  return postBlob("/v1/convert/docx-to-pdf", form(file));
}
