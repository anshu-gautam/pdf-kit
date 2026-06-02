"use client";

import { useRef, useState } from "react";
import { Check, Copy } from "lucide-react";

/** Copies `text` to the clipboard and briefly flips to a success checkmark. */
export function CopyButton({ text, label = "Copy command" }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const onCopy = () => {
    void navigator.clipboard?.writeText(text);
    setCopied(true);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), 1400);
  };

  return (
    <button
      type="button"
      className="copy-btn"
      onClick={onCopy}
      aria-label={label}
      style={copied ? { color: "var(--success-text)" } : undefined}
    >
      {copied ? <Check /> : <Copy />}
    </button>
  );
}
