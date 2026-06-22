#!/usr/bin/env bash
# Fetch the voice + tokenizer into assets/. The model (kokoro_dur.onnx) is built
# by scripts/export_dur.py (needs torch); copy it to assets/kokoro_dur.onnx.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p assets
base="https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/main"
curl -sL --retry 5 -o assets/bm_george.bin "$base/voices/bm_george.bin"
curl -sL --retry 5 -o assets/tokenizer.json "$base/tokenizer.json"
echo "voice + tokenizer -> assets/"
echo "model: uv run scripts/export_dur.py, then copy kokoro_dur.onnx -> assets/"
