#!/usr/bin/env -S uv run --script
# /// script
# dependencies = []
# ///
"""Stitch per-chapter mp3s into one chaptered M4B audiobook.

Reads each segment's duration via ffprobe, builds an ffmetadata chapter table
(titles from the TOC), concatenates, and encodes to AAC .m4b. Offline; operates
only on our own rendered audio.
"""

import subprocess
import sys
from pathlib import Path

TITLE = "Exit, Voice, and Loyalty"
AUTHOR = "Albert O. Hirschman"
NARRATOR = "Kokoro bm_george"

# (chapter title, segment mp3) in play order. Titles from the embedded TOC.
SEGMENTS = [
    ("Preface", "audio/ch_7-9.mp3"),
    ("1. Introduction and Doctrinal Background", "audio/ch_13-33.mp3"),
    ("2. Exit", "audio/ch_33-42.mp3"),
    ("3. Voice", "audio/ch_42-56.mp3"),
    ("4. A Special Difficulty in Combining Exit and Voice", "audio/ch_56-67.mp3"),
    ("5. How Monopoly Can be Comforted by Competition", "audio/ch_67-74.mp3"),
    ("6. On Spatial Duopoly and the Dynamics of Two-Party Systems", "audio/ch_74-88.mp3"),
    ("7. A Theory of Loyalty", "audio/ch_88-118.mp3"),
    ("8. Exit and Voice in American Ideology and Practice", "audio/ch_118-132.mp3"),
    ("9. The Elusive Optimal Mix of Exit and Voice", "audio/ch_132-141.mp3"),
]
OUT = "Exit_Voice_and_Loyalty.m4b"


def duration_ms(path):
    r = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration",
         "-of", "csv=p=0", path],
        capture_output=True, text=True, check=True)
    return int(float(r.stdout.strip()) * 1000)


def main():
    segs = [(t, p) for t, p in SEGMENTS if Path(p).exists()]
    missing = [p for _, p in SEGMENTS if not Path(p).exists()]
    if missing:
        print(f"⚠ missing (skipped): {missing}", file=sys.stderr)
    if not segs:
        sys.exit("no segments rendered yet")

    # ffmetadata with chapters
    meta = [";FFMETADATA1", f"title={TITLE}", f"artist={AUTHOR}",
            f"album={TITLE}", f"composer={NARRATOR}"]
    t = 0
    for title, path in segs:
        d = duration_ms(path)
        meta += ["[CHAPTER]", "TIMEBASE=1/1000", f"START={t}",
                 f"END={t + d}", f"title={title}"]
        t += d
    Path("chapters.txt").write_text("\n".join(meta) + "\n", "utf-8")

    # concat list
    Path("concat.txt").write_text(
        "".join(f"file '{Path(p).resolve()}'\n" for _, p in segs), "utf-8")

    print(f"stitching {len(segs)} chapters, {t/1000/60:.1f} min total -> {OUT}",
          file=sys.stderr)
    subprocess.run(
        ["ffmpeg", "-y", "-f", "concat", "-safe", "0", "-i", "concat.txt",
         "-i", "chapters.txt", "-map_metadata", "1", "-map", "0:a",
         "-c:a", "aac", "-b:a", "128k", OUT],
        check=True, capture_output=True)
    print(f"✓ {OUT}  ({t/1000/60:.1f} min, {len(segs)} chapters)")


if __name__ == "__main__":
    main()
