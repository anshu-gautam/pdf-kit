import Link from "next/link";
import { ArrowRight, Code2, FileText, Image as ImageIcon, Layers, PencilRuler } from "lucide-react";
import { buttonVariants } from "@/components/ui";
import { cn } from "@/lib/cn";

const features = [
  { href: "/extract", title: "Extract", body: "Pull clean text and rendered page images from any PDF.", icon: FileText },
  { href: "/chunks", title: "Chunks", body: "RAG-ready chunks as JSON or Markdown, with provenance.", icon: Layers },
  { href: "/render", title: "Render", body: "Rasterize a page to a crisp PNG at any DPI.", icon: ImageIcon },
  { href: "/edit", title: "Edit", body: "Merge, split, rotate, watermark, and fill forms.", icon: PencilRuler },
  { href: "/docs", title: "API", body: "A typed HTTP API with a live OpenAPI schema.", icon: Code2 },
];

export default function Home() {
  return (
    <div>
      <section className="py-6 sm:py-10">
        <p className="text-[12px] font-semibold uppercase tracking-[0.06em] text-accent-text">PDF toolkit</p>
        <h1 className="mt-3 max-w-3xl text-[clamp(2.25rem,6vw,3.25rem)] font-semibold leading-[1.05] tracking-[-0.035em] text-foreground">
          Read, chunk, render, and edit PDFs — fast.
        </h1>
        <p className="mt-4 max-w-2xl text-[18px] leading-[1.6] text-muted-foreground">
          A read-first, AI-oriented PDF toolkit. Upload a document and pick a tool — every action runs
          against the local pdfkit-api service.
        </p>
        <div className="mt-6 flex flex-wrap gap-3">
          <Link href="/extract" className={cn(buttonVariants({ variant: "primary", size: "lg" }))}>
            Start extracting <ArrowRight className="size-4" />
          </Link>
          <Link href="/docs" className={cn(buttonVariants({ variant: "secondary", size: "lg" }))}>
            View the API
          </Link>
        </div>
      </section>

      <section className="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {features.map((f) => {
          const Icon = f.icon;
          return (
            <Link
              key={f.href}
              href={f.href}
              className="group rounded-xl border border-border bg-surface p-6 shadow-sm transition-[transform,box-shadow,border-color] duration-150 ease-[var(--ease-out)] hover:-translate-y-0.5 hover:border-border-strong hover:shadow-md"
            >
              <div className="mb-4 grid size-10 place-items-center rounded-lg bg-surface-subtle text-muted-foreground transition-colors group-hover:text-accent-text">
                <Icon className="size-5" />
              </div>
              <h2 className="flex items-center gap-1.5 text-[18px] font-semibold tracking-[-0.01em] text-foreground">
                {f.title}
                <ArrowRight className="size-4 -translate-x-1 text-accent-text opacity-0 transition-all duration-150 group-hover:translate-x-0 group-hover:opacity-100" />
              </h2>
              <p className="mt-1 text-sm leading-[1.55] text-muted-foreground">{f.body}</p>
            </Link>
          );
        })}
      </section>
    </div>
  );
}
