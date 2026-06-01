// DTO types mirroring the pdfkit-api responses (PRD §13.3). For a generated
// version, run `npm run gen:api` against a running API's /openapi.json.

export type ApiErrorBody = { code: string; message: string };

export type Truncated = { text: boolean; images: boolean };

export type PageImage = {
  page: number;
  width: number;
  height: number;
  png_base64: string;
};

export type Background = "white" | "transparent";

/** Page-render options shared by /v1/render and extract image modes. */
export type RenderParams = {
  page?: number;
  dpi?: number;
  scale?: number;
  width?: number;
  height?: number;
  background?: Background;
};

export type ExtractMode = "auto" | "text" | "images" | "both";

export type ExtractRequest = {
  mode?: ExtractMode;
  password?: string;
  pages?: number[];
  max_pages?: number;
  min_text_chars?: number;
  max_text_chars?: number;
  ocr?: boolean;
  /** How to render page images for mode = images | both. */
  render?: RenderParams;
};

export type ExtractResponse = {
  text: string;
  page_images: PageImage[];
  pages_processed: number[];
  truncated: Truncated;
};

export type OutlineNode = {
  title: string;
  page: number | null;
  children: OutlineNode[];
};

export type LinkTarget =
  | { kind: "uri"; uri: string }
  | { kind: "page"; page: number };

export type LinkDto = {
  rect: [number, number, number, number];
  target: LinkTarget;
};

export type PageLinks = { page: number; links: LinkDto[] };

export type MetadataResponse = {
  page_count: number;
  title: string | null;
  author: string | null;
  subject: string | null;
  keywords: string | null;
  creator: string | null;
  producer: string | null;
  creation_date: string | null;
  mod_date: string | null;
  pdf_version: string;
  encrypted: boolean;
  outline: OutlineNode[];
  links: PageLinks[];
};

export type FigureDto = {
  bbox: [number, number, number, number];
  caption: string | null;
};

export type PageFigures = { page: number; figures: FigureDto[] };

export type FiguresResponse = { pages: PageFigures[] };

export type ChunkFormat = "json" | "markdown";

export type ChunkRequest = {
  format?: ChunkFormat;
  password?: string;
  target_tokens?: number;
  overlap_tokens?: number;
  respect_boundaries?: boolean;
  contextual_prefix?: boolean;
};

export type RenderRequest = RenderParams & {
  password?: string;
};
