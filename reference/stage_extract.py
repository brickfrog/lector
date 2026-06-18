#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["pymupdf>=1.24"]
# ///
"""Stage 1 (OFFLINE jail): parse untrusted PDF -> cleaned paragraph text artifact.

This is the only stage that touches the raw ebook. No network. Output is a plain
text file the downstream (networked) stages consume instead of the PDF.
"""

import sys
from pathlib import Path

import fitz
from clean import clean_pages

a, b = int(sys.argv[1]), int(sys.argv[2])
doc = fitz.open("book.pdf")
paras = clean_pages(doc, a, b)
Path("text").mkdir(exist_ok=True)
out = Path(f"text/raw_{a}-{b}.txt")
out.write_text("\n\n".join(paras), "utf-8")
print(f"✓ {out}  ({len(paras)} paragraphs, {sum(len(p) for p in paras)} chars)")
