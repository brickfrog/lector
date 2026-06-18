#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["pymupdf>=1.24"]
# ///
"""Stage 2 (ONLINE jail): glyph-repair the text artifact via `claude -p`.

Reads the raw text file, repairs in large chunks (cached), writes repaired text.
Sees only text + caches — never the PDF (masked to /dev/null in the jail).
On a warm cache this needs no network and no `claude` binary at all.
"""

import re
import sys
from pathlib import Path

from build import repair, pack_paragraphs, REPAIR_MAX_CHARS

a, b = int(sys.argv[1]), int(sys.argv[2])
raw = Path(f"text/raw_{a}-{b}.txt").read_text("utf-8")
paras = [p.strip() for p in re.split(r"\n\s*\n", raw) if p.strip()]

rchunks = pack_paragraphs(paras, REPAIR_MAX_CHARS)
out_paras = []
for i, c in enumerate(rchunks, 1):
    print(f"  repairing {i}/{len(rchunks)} ({len(c)} chars)…", file=sys.stderr)
    fixed = repair(c)
    out_paras.extend(p.strip() for p in re.split(r"\n\s*\n", fixed) if p.strip())

out = Path(f"text/rep_{a}-{b}.txt")
out.write_text("\n\n".join(out_paras), "utf-8")
print(f"✓ {out}  ({len(out_paras)} paragraphs, {sum(len(p) for p in out_paras)} chars)")
