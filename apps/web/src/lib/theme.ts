// A tiny external store over the <html> `.dark` class. Lets every theme control
// (the app sidebar toggle and the landing-page nav toggle) stay in sync without
// setState-in-effect, and persists the choice. The pre-paint script in the root
// layout is the source of truth on first load; this just mirrors + flips it.

const listeners = new Set<() => void>();

export function subscribe(cb: () => void) {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function getSnapshot() {
  return document.documentElement.classList.contains("dark");
}

export function getServerSnapshot() {
  return false;
}

/** Flip (or force) the theme, persist it, and notify subscribers. */
export function toggleTheme(force?: boolean) {
  const next = force ?? !document.documentElement.classList.contains("dark");
  document.documentElement.classList.toggle("dark", next);
  try {
    localStorage.setItem("theme", next ? "dark" : "light");
  } catch {
    // ignore storage failures (private mode etc.)
  }
  listeners.forEach((l) => l());
}
