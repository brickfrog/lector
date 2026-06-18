#!/usr/bin/env bash
# Render the remaining segments end to end. Each: repair-warm (unjailed, claude)
# then the full jailed Kokoro pipeline. Free + local; resumable via caches.
set -euo pipefail
cd "$(dirname "$0")"

# label start end   (ch2=33-42 and ch3=42-56 already rendered -> omitted)
SEGS=(
  "preface 7 9"
  "ch1 13 33"
  "ch4 56 67"
  "ch5 67 74"
  "ch6 74 88"
  "ch7 88 118"
  "ch8 118 132"
  "ch9 132 141"
)

for s in "${SEGS[@]}"; do
  read -r name a b <<< "$s"
  echo ""; echo "############ $name (pp.$a-$b) ############"
  uv run stage_extract.py "$a" "$b"   # PDF -> raw text
  uv run stage_repair.py "$a" "$b"    # raw -> repaired (claude, warms cache)
  ./run.sh "$a" "$b"                  # full jailed render -> audio/ch_a-b.{mp3,ass,mp4}
done
echo ""; echo "==== batch complete ===="
