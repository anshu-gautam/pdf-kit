"use client";

import { useSyncExternalStore } from "react";
import { Moon, Sun } from "lucide-react";
import { Button } from "@/components/ui";
import { getServerSnapshot, getSnapshot, subscribe, toggleTheme } from "@/lib/theme";

/** Toggles the `.dark` class on <html> and persists the choice. */
export function ThemeToggle() {
  const dark = useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);

  return (
    <Button
      variant="ghost"
      size="icon"
      onClick={() => toggleTheme()}
      aria-label="Toggle theme"
      title="Toggle theme"
    >
      {dark ? <Sun /> : <Moon />}
    </Button>
  );
}
