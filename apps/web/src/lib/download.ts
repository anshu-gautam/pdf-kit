/** Trigger a browser download of a Blob. */
export function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  // Defer revocation: revoking in the same tick as click() can cancel the
  // not-yet-started download in Safari/Firefox.
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
