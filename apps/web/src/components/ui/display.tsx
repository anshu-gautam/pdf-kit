import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/cn";

export function Card({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn("rounded-xl border border-border bg-surface text-foreground shadow-sm", className)}
      {...props}
    />
  );
}

const badgeVariants = cva(
  "inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs font-medium tabular-nums",
  {
    variants: {
      tone: {
        neutral: "border-border bg-surface-subtle text-muted-foreground",
        success: "border-success-border bg-success-tint text-success-text",
        warning: "border-warning-border bg-warning-tint text-warning-text",
        info: "border-primary/20 bg-primary/10 text-accent-text",
        danger: "border-danger-border bg-danger-tint text-danger-text",
      },
    },
    defaultVariants: { tone: "neutral" },
  },
);

export function Badge({
  className,
  tone,
  ...props
}: React.HTMLAttributes<HTMLSpanElement> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ tone }), className)} {...props} />;
}

export function Spinner({ className }: { className?: string }) {
  return (
    <span
      aria-hidden
      className={cn(
        "inline-block size-4 animate-spin rounded-full border-2 border-current border-t-transparent",
        className,
      )}
    />
  );
}

export function ErrorBox({ error }: { error: string | null }) {
  if (!error) return null;
  return (
    <p
      role="alert"
      className="rounded-lg border border-danger-border bg-danger-tint px-3 py-2 text-[13px] text-danger-text"
    >
      {error}
    </p>
  );
}

export function EmptyState({
  icon,
  title,
  description,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
}) {
  return (
    <div className="flex flex-col items-center gap-3 py-10 text-center">
      <div className="rounded-full bg-surface-subtle p-3 text-muted-foreground [&_svg]:size-6">{icon}</div>
      <h3 className="text-sm font-medium text-foreground">{title}</h3>
      <p className="max-w-sm text-sm text-muted-foreground">{description}</p>
    </div>
  );
}

export function PageHeader({ title, subtitle }: { title: string; subtitle?: string }) {
  return (
    <div className="mb-8 space-y-1.5">
      <h1 className="text-[2rem] font-semibold leading-[1.15] tracking-[-0.025em] text-foreground">
        {title}
      </h1>
      {subtitle && <p className="text-[15px] leading-[1.55] text-muted-foreground">{subtitle}</p>}
    </div>
  );
}
