"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import {
  Code2,
  FileText,
  FileType2,
  Image as ImageIcon,
  Layers,
  PencilRuler,
  type LucideIcon,
} from "lucide-react";
import { cn } from "@/lib/cn";
import { ThemeToggle } from "@/components/theme-toggle";

type NavItem = { href: string; label: string; icon: LucideIcon };

const groups: ReadonlyArray<{ label: string; items: ReadonlyArray<NavItem> }> = [
  {
    label: "Tools",
    items: [
      { href: "/extract", label: "Extract", icon: FileText },
      { href: "/chunks", label: "Chunks", icon: Layers },
      { href: "/render", label: "Render", icon: ImageIcon },
      { href: "/edit", label: "Edit", icon: PencilRuler },
      { href: "/convert", label: "Word → PDF", icon: FileType2 },
    ],
  },
  {
    label: "Reference",
    items: [{ href: "/docs", label: "API", icon: Code2 }],
  },
];

const allItems = groups.flatMap((g) => g.items);

function isActive(pathname: string, href: string) {
  return pathname === href || pathname.startsWith(`${href}/`);
}

function Brand() {
  return (
    <Link href="/" className="flex items-center gap-2 text-[15px] font-semibold tracking-tight text-foreground">
      <span className="grid size-6 place-items-center rounded-md bg-primary text-[11px] font-semibold text-primary-foreground">
        pk
      </span>
      pdfkit
    </Link>
  );
}

export function Shell({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();

  return (
    <div className="flex min-h-screen">
      {/* Desktop sidebar */}
      <aside className="hidden w-64 shrink-0 flex-col border-r border-border bg-surface lg:flex">
        <div className="flex h-14 items-center border-b border-border px-5">
          <Brand />
        </div>
        <nav className="flex-1 space-y-6 px-3 py-5">
          {groups.map((group) => (
            <div key={group.label} className="space-y-1">
              <p className="px-2 pb-1 text-[11px] font-semibold uppercase tracking-[0.06em] text-subtle-foreground">
                {group.label}
              </p>
              {group.items.map((item) => {
                const active = isActive(pathname, item.href);
                const Icon = item.icon;
                return (
                  <Link
                    key={item.href}
                    href={item.href}
                    aria-current={active ? "page" : undefined}
                    className={cn(
                      "flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-[13px] font-medium transition-colors duration-150",
                      active
                        ? "bg-primary/10 font-semibold text-accent-text"
                        : "text-muted-foreground hover:bg-surface-subtle hover:text-foreground dark:hover:bg-white/[0.05]",
                    )}
                  >
                    <Icon className="size-4 shrink-0" />
                    {item.label}
                  </Link>
                );
              })}
            </div>
          ))}
        </nav>
        <div className="flex items-center justify-between border-t border-border px-4 py-3">
          <span className="text-xs text-subtle-foreground">v0.1.0</span>
          <ThemeToggle />
        </div>
      </aside>

      {/* Main column */}
      <div className="flex min-w-0 flex-1 flex-col">
        {/* Mobile header + nav */}
        <header className="sticky top-0 z-10 border-b border-border bg-surface/80 backdrop-blur lg:hidden">
          <div className="flex h-14 items-center justify-between px-4">
            <Brand />
            <ThemeToggle />
          </div>
          <nav className="flex gap-1 overflow-x-auto px-3 pb-2">
            {allItems.map((item) => {
              const active = isActive(pathname, item.href);
              return (
                <Link
                  key={item.href}
                  href={item.href}
                  aria-current={active ? "page" : undefined}
                  className={cn(
                    "shrink-0 rounded-lg px-3 py-1.5 text-[13px] font-medium transition-colors duration-150",
                    active
                      ? "bg-primary/10 font-semibold text-accent-text"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {item.label}
                </Link>
              );
            })}
          </nav>
        </header>

        <main className="flex-1">
          <div className="mx-auto w-full max-w-[1120px] px-4 py-8 sm:px-6 lg:px-8 lg:py-10">{children}</div>
        </main>

        <footer className="border-t border-border px-6 py-5 text-center text-xs text-muted-foreground">
          pdfkit — reference UI for the pdfkit-api service
        </footer>
      </div>
    </div>
  );
}
