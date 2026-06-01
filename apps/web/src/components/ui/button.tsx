import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { Loader2 } from "lucide-react";
import { cn } from "@/lib/cn";

const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-2 rounded-lg text-[13px] font-medium whitespace-nowrap select-none outline-none transition-[color,background-color,border-color,box-shadow,transform] duration-150 ease-[var(--ease-out)] focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-50 active:scale-[0.98] [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary:
          "bg-primary text-primary-foreground hover:bg-primary-hover shadow-[0_1px_2px_0_rgb(16_24_40/0.06),inset_0_1px_0_rgb(255_255_255/0.15)]",
        secondary:
          "bg-surface border border-border text-foreground shadow-xs hover:bg-surface-subtle dark:bg-white/[0.03] dark:hover:bg-white/[0.06]",
        ghost:
          "text-muted-foreground hover:bg-surface-subtle hover:text-foreground dark:hover:bg-white/[0.06]",
        destructive: "bg-danger text-white hover:bg-danger-hover focus-visible:ring-danger/40",
        link: "text-accent-text underline-offset-4 hover:underline",
      },
      size: {
        sm: "h-8 px-3 gap-1.5",
        default: "h-9 px-3.5 py-2",
        lg: "h-10 px-6",
        icon: "size-9",
      },
    },
    defaultVariants: { variant: "primary", size: "default" },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  loading?: boolean;
}

/** Button with variants + a loading state that keeps the label and reserves width. */
export function Button({
  className,
  variant,
  size,
  loading = false,
  disabled,
  children,
  ...props
}: ButtonProps) {
  return (
    <button
      className={cn(buttonVariants({ variant, size }), className)}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      {...props}
    >
      {loading && <Loader2 className="size-4 animate-spin" />}
      {children}
    </button>
  );
}

export { buttonVariants };
