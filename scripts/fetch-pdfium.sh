#!/usr/bin/env bash
#
# Fetch a prebuilt PDFIUM dynamic library into the pdfkit cache directory so the
# `render-pdfium` backend can bind to it at runtime. The library is NOT vendored
# in git (CLAUDE.md). Source: https://github.com/bblanchon/pdfium-binaries
#
# Destination: $PDFKIT_PDFIUM_DIR or ~/.cache/pdfkit/pdfium
# After running, the library lives at <dest>/lib/<platform name>, which the
# renderer finds automatically (or point $PDFKIT_PDFIUM_LIB at the file).
#
# Usage: scripts/fetch-pdfium.sh

set -euo pipefail

dest="${PDFKIT_PDFIUM_DIR:-$HOME/.cache/pdfkit/pdfium}"
mkdir -p "$dest"

os="$(uname -s)"
arch="$(uname -m)"
case "$os/$arch" in
  Darwin/arm64)  asset="pdfium-mac-arm64.tgz" ;;
  Darwin/x86_64) asset="pdfium-mac-x64.tgz" ;;
  Linux/x86_64)  asset="pdfium-linux-x64.tgz" ;;
  Linux/aarch64) asset="pdfium-linux-arm64.tgz" ;;
  *) echo "unsupported platform: $os/$arch (see github.com/bblanchon/pdfium-binaries/releases)" >&2; exit 1 ;;
esac

echo "resolving latest release asset: $asset"
url="$(curl -fsSL https://api.github.com/repos/bblanchon/pdfium-binaries/releases/latest \
  | grep -oE "https://[^\"']*${asset}" | head -1)"
[ -n "$url" ] || { echo "could not find $asset in the latest release" >&2; exit 1; }

echo "downloading $url"
# -C - resumes across flaky connections.
curl -fL -C - --retry 20 --retry-all-errors --retry-delay 3 -o "/tmp/$asset" "$url"
tar xzf "/tmp/$asset" -C "$dest"

lib="$(find "$dest/lib" -maxdepth 1 -name 'libpdfium.*' -o -name 'pdfium.dll' 2>/dev/null | head -1)"
echo "PDFIUM ready: ${lib:-$dest/lib}"
