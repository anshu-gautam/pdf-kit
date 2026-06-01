# pdfkit web UI (`apps/web`)

Reference Next.js 16 UI for the `pdfkit-api` service (PRD §14). App Router,
TypeScript, Tailwind v4, Turbopack.

## Run

1. Start the API (from the repo root):

   ```bash
   cargo run -p pdfkit-api            # http://127.0.0.1:8080
   # rendering needs: cargo run -p pdfkit-api --features render-pdfium
   ```

2. Start the web app:

   ```bash
   cd apps/web
   cp .env.example .env.local         # set API_BASE_URL if not the default
   npm install
   npm run dev                        # http://localhost:3000
   ```

## How it talks to the API

The browser calls the same-origin proxy at `/api/pdfkit/*`
(`src/app/api/pdfkit/[...path]/route.ts`), which forwards to `API_BASE_URL`.
This avoids CORS and hides the internal API URL. To call the Rust API directly
instead, set `NEXT_PUBLIC_API_BASE_URL` (the API must then allow the origin via
`PDFKIT_ALLOWED_ORIGINS`).

## Pages

`/extract`, `/chunks`, `/render`, `/edit`, `/docs` — each drives the matching
API endpoint. The typed client lives in `src/lib/api/`; `npm run gen:api`
regenerates types from a running API's `/openapi.json`.

## Checks

```bash
npm run lint
npm run build
```
