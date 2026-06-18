#!/usr/bin/env bash
# Fetch the local-served assets for the browser app (gitignored, regenerable).
#   bash vendor.sh
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p public/models public/voices public/tokenizer
base="https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/main"

# Our duration-exposing fp32 model (built by reference/export_dur.py). fp32 is
# the only format that makes audio under ort-web; q8/fp16 come back silent.
if [ -f ../reference/models/kokoro_dur.onnx ]; then
  cp ../reference/models/kokoro_dur.onnx public/models/
else
  echo "MISSING model: build it first -> uv run ../reference/export_dur.py"
  exit 1
fi

echo "voice + tokenizer…"
curl -sL --retry 5 -o public/voices/bm_george.bin "$base/voices/bm_george.bin"
curl -sL --retry 5 -o public/tokenizer/tokenizer.json "$base/tokenizer.json"
echo "done."
