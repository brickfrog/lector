#!/usr/bin/env bash
# Final sandboxed pipeline: one chapter (page range A..B) end to end.
#
#   run.sh <A> <B>
#
# Zones:
#   extract  OFFLINE  PDF -> text/raw      (untrusted parse, no net)
#   repair   OFFLINE* raw -> text/rep      (*offline OK on warm cache; cold cache
#                                            needs the online jail for claude)
#   tts      ONLINE   rep -> audio/*.mp3   (MiniMax; PDF masked; secret via env)
#   align    OFFLINE  rep+mp3 -> *.ass     (torchaudio, no net)
#   video    OFFLINE  mp3+ass -> *.mp4     (ffmpeg burn-in, no net)
set -euo pipefail
cd "$(dirname "$0")"
export WORK="$PWD"
mkdir -p text audio .repair_cache .tts_cache

# Every stage runs in a memory-capped transient scope. If a stage misbehaves
# (see: align's O(frames^2) blowup), the kernel OOMs *that scope* — never the
# ghostty/session cgroup. Swap off so it fails fast instead of thrashing 25G.
MEMMAX="${MEMMAX:-12G}"
cap() {
  systemd-run --user --scope -p MemoryMax="$MEMMAX" -p MemorySwapMax=0 --quiet \
    env WORK="$WORK" KOKORO_VOICE="${KOKORO_VOICE:-bm_george}" \
    ./jail.sh "$@"
}

A="${1:?usage: run.sh <startpage> <endpage> [voice]}"
B="${2:?usage: run.sh <startpage> <endpage> [voice]}"
export KOKORO_VOICE="${3:-${KOKORO_VOICE:-bm_george}}"
echo "voice: $KOKORO_VOICE"
MP3="audio/ch_${A}-${B}.mp3"
ASS="audio/ch_${A}-${B}.ass"
MP4="audio/ch_${A}-${B}.mp4"

echo "### [1/5] extract (offline)"
cap offline_jail uv run --offline stage_extract.py "$A" "$B"

echo "### [2/5] repair (offline; warm cache)"
cap offline_jail uv run --offline stage_repair.py "$A" "$B"

echo "### [3/5] tts (offline; local Kokoro, no network/credits)"
cap offline_jail uv run --offline kokoro_tts.py "$A" "$B"

echo "### [4/5] align (offline)"
cap offline_jail uv run --offline align.py "$A" "$B"

echo "### [5/5] video burn-in (offline)"
cap offline_jail bash -c \
  "ffmpeg -y -f lavfi -i color=c=black:s=1280x720:r=30 -i '$MP3' \
     -vf \"ass=$ASS\" -shortest -c:v libx264 -preset medium -crf 18 \
     -c:a aac -b:a 192k '$MP4' >/dev/null 2>&1"

echo ""
echo "✓ chapter $A-$B done:"
echo "   audio:  $MP3"
echo "   karaoke: mpv $MP4"
