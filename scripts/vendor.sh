#!/usr/bin/env bash
# Fetch the voice + tokenizer into assets/. The model (kokoro_dur.onnx) is built
# by scripts/export_dur.py (needs torch); copy it to assets/kokoro_dur.onnx.
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p assets
base="https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/main"
curl -sL --retry 5 -o assets/tokenizer.json "$base/tokenizer.json"

# A spread of voices (a* = American, b* = British; f/m = female/male). Any voice
# from the repo works: drop its <name>.bin in assets/ and pass it as the 3rd arg.
voices="af_heart af_bella af_nicole am_michael am_adam am_onyx bf_emma bf_isabella bm_george bm_lewis"
for v in $voices; do
  echo "  voice $v"
  curl -sL --retry 5 -o "assets/$v.bin" "$base/voices/$v.bin"
done
echo "voices + tokenizer -> assets/"
echo "model: uv run scripts/export_dur.py, then copy kokoro_dur.onnx -> assets/"
