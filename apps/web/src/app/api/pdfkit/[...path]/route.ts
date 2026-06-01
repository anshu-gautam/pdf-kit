// Same-origin proxy to the Rust pdfkit-api (PRD §16): the browser calls
// /api/pdfkit/v1/* and this forwards to $API_BASE_URL, avoiding CORS and hiding
// the internal API URL. Forwards multipart bodies and streams responses back.

import { NextRequest } from "next/server";

const API_BASE = process.env.API_BASE_URL ?? "http://127.0.0.1:8080";

type Ctx = { params: Promise<{ path: string[] }> };

async function forward(req: NextRequest, path: string[]): Promise<Response> {
  const target = `${API_BASE}/${path.join("/")}${req.nextUrl.search}`;

  const headers = new Headers(req.headers);
  // Let fetch/undici set these for the upstream connection.
  headers.delete("host");
  headers.delete("content-length");

  const hasBody = req.method !== "GET" && req.method !== "HEAD";
  const upstream = await fetch(target, {
    method: req.method,
    headers,
    body: hasBody ? await req.arrayBuffer() : undefined,
  });

  // undici already decoded any content-encoding, so strip headers that would
  // misdescribe the body we stream back.
  const respHeaders = new Headers(upstream.headers);
  respHeaders.delete("content-encoding");
  respHeaders.delete("content-length");
  respHeaders.delete("transfer-encoding");

  return new Response(upstream.body, {
    status: upstream.status,
    statusText: upstream.statusText,
    headers: respHeaders,
  });
}

export async function GET(req: NextRequest, ctx: Ctx): Promise<Response> {
  const { path } = await ctx.params;
  return forward(req, path);
}

export async function POST(req: NextRequest, ctx: Ctx): Promise<Response> {
  const { path } = await ctx.params;
  return forward(req, path);
}
