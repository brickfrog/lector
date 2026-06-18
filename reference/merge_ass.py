#!/usr/bin/env -S uv run --script
# /// script
# dependencies = []
# ///
"""Merge per-chapter .ass karaoke into one book-level .ass aligned to the M4B.

Each chapter's Dialogue timestamps are relative to that chapter's audio; shift
them by the chapter's cumulative start (= sum of prior chapter mp3 durations,
matching how assemble_m4b concatenates) so they line up with the full audiobook.
Play:  mpv Exit_Voice_and_Loyalty.m4b --sub-file=Exit_Voice_and_Loyalty.ass --force-window=yes
"""

import re
import subprocess
import sys
from pathlib import Path

SEGMENTS = ["7-9", "13-33", "33-42", "42-56", "56-67", "67-74",
            "74-88", "88-118", "118-132", "132-141"]
OUT = "Exit_Voice_and_Loyalty.ass"


def dur_s(path):
    r = subprocess.run(["ffprobe", "-v", "error", "-show_entries",
                        "format=duration", "-of", "csv=p=0", path],
                       capture_output=True, text=True, check=True)
    return float(r.stdout.strip())


def parse_t(s):
    h, m, rest = s.split(":")
    return int(h) * 3600 + int(m) * 60 + float(rest)


def fmt_t(t):
    h = int(t // 3600); m = int(t % 3600 // 60); s = t % 60
    return f"{h}:{m:02d}:{s:05.2f}"


def shift_line(line, off):
    # Dialogue: Layer,Start,End,Style,...,Text  -> shift fields 1,2
    head, _, rest = line.partition(":")
    parts = rest.split(",", 9)
    parts[1] = fmt_t(parse_t(parts[1].strip()) + off)
    parts[2] = fmt_t(parse_t(parts[2].strip()) + off)
    return f"{head}:" + ",".join(parts)


def main():
    header, events, off = None, [], 0.0
    for seg in SEGMENTS:
        ass = Path(f"audio/ch_{seg}.ass")
        mp3 = Path(f"audio/ch_{seg}.mp3")
        if not ass.exists() or not mp3.exists():
            print(f"⚠ skip {seg} (missing)", file=sys.stderr)
            continue
        lines = ass.read_text("utf-8").splitlines()
        if header is None:
            header = [ln for ln in lines if not ln.startswith("Dialogue:")]
        for ln in lines:
            if ln.startswith("Dialogue:"):
                events.append(shift_line(ln, off))
        off += dur_s(str(mp3))

    Path(OUT).write_text("\n".join(header + events) + "\n", "utf-8")
    print(f"✓ {OUT}  ({len(events)} karaoke lines across {off/60:.1f} min)")


if __name__ == "__main__":
    main()
