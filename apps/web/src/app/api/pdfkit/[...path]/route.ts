// Same-origin proxy to the Rust pdfkit-api (PRD §16): the browser calls
// /api/pdfkit/v1/* and this forwards to $API_BASE_URL, avoiding CORS and hiding
// the internal API URL. Streams request and response bodies (no buffering).

import { NextRequest } from "next/server";

const API_BASE = process.env.API_BASE_URL ?? "http://127.0.0.1:8080";

type Ctx = { params: Promise<{ path: string[] }> };

async function forward(req: NextRequest, path: string[]): Promise<Response> {
  const target = `${API_BASE}/${path.join("/")}${req.nextUrl.search}`;

  const headers = new Headers(req.headers);
  // fetch/undici manages connection + framing; and don't relay browser
  // credentials across the trust boundary to the internal API.
  for (const h of [
    "host",
    "content-length",
    "content-encoding",
    "transfer-encoding",
    "cookie",
    "authorization",
  ]) {
    headers.delete(h);
  }

  const hasBody = req.method !== "GET" && req.method !== "HEAD";
  // Stream the request body through rather than buffering the whole upload.
  const init: RequestInit & { duplex?: "half" } = {
    method: req.method,
    headers,
    body: hasBody ? req.body : undefined,
  };
  if (hasBody) init.duplex = "half";

  let upstream: Response;
  try {
    upstream = await fetch(target, init);
  } catch (e) {
    return Response.json(
      { code: "upstream_unreachable", message: `cannot reach pdfkit-api: ${String(e)}` },
      { status: 502 },
    );
  }

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
