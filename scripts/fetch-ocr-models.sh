#!/usr/bin/env bash
#
# Fetch the ocrs ONNX (.rten) OCR models into the pdfkit cache directory.
# Models are NOT vendored in git (CLAUDE.md); run this once to enable the
# `ocr-ocrs` backend.
#
# Destination (first that applies):
#   $PDFKIT_OCR_MODELS
#   $XDG_CACHE_HOME/pdfkit/models
#   $HOME/.cache/pdfkit/models
#
# Usage: scripts/fetch-ocr-models.sh

set -euo pipefail

dest="${PDFKIT_OCR_MODELS:-${XDG_CACHE_HOME:-$HOME/.cache}/pdfkit/models}"
mkdir -p "$dest"

base="https://ocrs-models.s3-accelerate.amazonaws.com"
models=(
  "text-detection.rten"
  "text-recognition.rten"
)

for model in "${models[@]}"; do
  out="$dest/$model"
  if [ -f "$out" ]; then
    echo "already present: $out"
    continue
  fi
  echo "downloading $model -> $out"
  # -C - resumes across flaky connections; --retry handles transient errors.
  curl -fL -C - --retry 20 --retry-all-errors --retry-delay 3 \
    -o "$out" "$base/$model"
done

echo "OCR models ready in $dest"
