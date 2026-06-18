#!/usr/bin/env bash
# Fetch the local-served assets for the browser app (gitignored, regenerable).
#   bash vendor.sh
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p public/models public/voices public/tokenizer
base="https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/main"

echo "audio model (stock fp16, ~163MB)…"
curl -sL --retry 5 --retry-all-errors -C - -o public/models/kokoro_stock_fp16.onnx \
  "$base/onnx/model_fp16.onnx"

echo "duration model (ours)…"
if [ -f ../reference/models/kokoro_duronly.onnx ]; then
  cp ../reference/models/kokoro_duronly.onnx public/models/
else
  echo "  MISSING: build it first -> uv run ../reference/export_duronly.py"
  exit 1
fi

echo "voice + tokenizer…"
curl -sL --retry 5 -o public/voices/bm_george.bin "$base/voices/bm_george.bin"
curl -sL --retry 5 -o public/tokenizer/tokenizer.json "$base/tokenizer.json"
echo "done."
