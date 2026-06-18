#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["pymupdf>=1.24"]
# ///
"""
Clean text extraction for the audiobook pipeline.

Core insight (verified): body text is >=15pt; running heads (~14.4),
footnotes (~12.5-13.5), and superscript refs (~9) are all smaller. A single
font-size filter removes all three. Then: reflow wrapped lines, rejoin
hyphenated words, detect paragraphs by left-indent.
"""

import re
import sys
import fitz

BODY_MIN_SIZE = 15.0      # keep spans at/above this; drops heads/notes/refs
INDENT_EPS = 4.0          # x0 beyond left margin + this => new paragraph


def page_paragraphs(page):
    """Return list of paragraph strings for one page, body text only."""
    d = page.get_text("dict")
    lines = []  # (y0, x0, text)
    for blk in d["blocks"]:
        for line in blk.get("lines", []):
            spans = [s for s in line["spans"] if s["size"] >= BODY_MIN_SIZE]
            if not spans:
                continue
            text = "".join(s["text"] for s in spans).strip()
            if not text:
                continue
            x0 = min(s["bbox"][0] for s in spans)
            y0 = min(s["bbox"][1] for s in spans)
            lines.append((y0, x0, text))

    if not lines:
        return []

    lines.sort(key=lambda t: (round(t[0]), t[1]))   # reading order
    left_margin = min(x0 for _, x0, _ in lines)

    paras, cur = [], ""
    for _, x0, text in lines:
        # standalone page number / stray numeral -> skip
        if re.fullmatch(r'[\divxlcDIVXLC]{1,4}', text):
            continue
        indented = x0 > left_margin + INDENT_EPS
        if indented and cur:
            paras.append(cur)
            cur = text
        else:
            cur = f"{cur} {text}" if cur else text
    if cur:
        paras.append(cur)
    return paras


def dehyphenate(text):
    # word-<wrap>word  ->  wordword   (only lowercase-lowercase to spare em-dashes)
    return re.sub(r'([a-z])-\s+([a-z])', r'\1\2', text)


def squeeze(text):
    return re.sub(r'\s+', ' ', text).strip()


def clean_pages(doc, start, end):
    out = []
    for i in range(start, end):
        for p in page_paragraphs(doc[i]):
            out.append(squeeze(dehyphenate(p)))
    return out


if __name__ == "__main__":
    doc = fitz.open(sys.argv[1] if len(sys.argv) > 1 else "book.pdf")
    a, b = (int(sys.argv[2]), int(sys.argv[3])) if len(sys.argv) > 3 else (19, 40)
    paras = clean_pages(doc, a, b)
    print(f"# pages {a}-{b}: {len(paras)} paragraphs, "
          f"{sum(len(p) for p in paras)} chars\n")
    for j, p in enumerate(paras):
        print(f"[¶{j}] {p}\n")
