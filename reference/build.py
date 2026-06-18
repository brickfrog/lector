#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["pymupdf>=1.24"]
# ///
"""
Audiobook text pipeline (everything up to TTS):

  extract (size>=15 filter) -> reflow/dehyphenate -> paragraph-pack <=MAX_CHARS
  -> LLM repair via `claude -p` (cached, length-checked)

Run on a page range to inspect output before spending TTS credits.
Reuses clean.py for extraction/reflow.
"""

import hashlib
import re
import subprocess
import sys
from pathlib import Path

import fitz
from clean import clean_pages

# Two independent chunk sizes (decoupled by design):
#   REPAIR: bounded by LLM *output* length + verbatim-drift risk, NOT input
#           context. ~35k chars ≈ ~9k output tokens — well under the ceiling,
#           small enough that the length guard can still catch drift.
#   TTS:    bounded by MiniMax's 10k-char/request sync API limit.
REPAIR_MAX_CHARS = 16000   # ~4k output tokens: reliably completes, ~21 calls/book
TTS_MAX_CHARS = 8000
REPAIR_TIMEOUT = 600       # seconds per claude call before recursive split
LEN_TOLERANCE = 0.15       # reject repair if length drifts > this fraction
REPAIR_CACHE = Path(".repair_cache")

REPAIR_PROMPT = (
    "The following text was extracted from a PDF with OCR/encoding corruption "
    "(stray punctuation inside words, wrong letters, spurious spaces in names). "
    "Fix ONLY the character-level corruption. Preserve wording exactly — do not "
    "paraphrase, summarize, add, or remove any content. Also remove superscript "
    "footnote-reference numbers attached to the end of words (e.g. 'anarchy.4' -> "
    "'anarchy.', 'competition6' -> 'competition'), but KEEP all legitimate numbers "
    "in the prose such as years, quantities, and dates. KEEP archaic/period "
    "spellings, foreign words, and proper nouns as-is (e.g. 'oeconomy' is NOT an "
    "error). Preserve paragraph breaks (blank lines between paragraphs) exactly. "
    "Output only the corrected text, no commentary."
)


def pack_paragraphs(paras, max_chars):
    """Greedy-pack whole paragraphs into chunks; never split a paragraph."""
    chunks, cur = [], ""
    for p in paras:
        if cur and len(cur) + len(p) + 1 > max_chars:
            chunks.append(cur)
            cur = p
        else:
            cur = f"{cur}\n\n{p}" if cur else p
    if cur:
        chunks.append(cur)
    return chunks


def _cache_path(text):
    h = hashlib.sha256((REPAIR_PROMPT + "|" + text).encode()).hexdigest()[:24]
    return REPAIR_CACHE / f"{h}.txt"


def _split_near_middle(text):
    """Split on the paragraph break closest to the midpoint (fallback: hard)."""
    mid = len(text) // 2
    cands = [k for k in (text.rfind("\n\n", 0, mid), text.find("\n\n", mid)) if k != -1]
    if not cands:
        return text[:mid], text[mid:]
    k = min(cands, key=lambda x: abs(x - mid))
    return text[:k].strip(), text[k:].strip()


def repair(text, depth=0):
    REPAIR_CACHE.mkdir(exist_ok=True)
    cp = _cache_path(text)
    if cp.exists():
        return cp.read_text("utf-8")

    try:
        proc = subprocess.run(
            ["claude", "-p", REPAIR_PROMPT],
            input=text, capture_output=True, text=True, timeout=REPAIR_TIMEOUT,
        )
        out = proc.stdout.strip()
    except subprocess.TimeoutExpired:
        # self-heal: halve on a paragraph boundary and retry; floor at keep-original
        if len(text) > 4000 and depth < 4:
            print(f"  ! repair timeout ({len(text)} chars); splitting", file=sys.stderr)
            a, b = _split_near_middle(text)
            out = repair(a, depth + 1) + "\n\n" + repair(b, depth + 1)
            cp.write_text(out, "utf-8")
            return out
        print(f"  ! repair timeout ({len(text)} chars); keeping original", file=sys.stderr)
        out = text
    except (FileNotFoundError, OSError) as e:
        # claude binary unavailable (e.g. offline jail, cold cache): keep original
        print(f"  ! claude unavailable ({e}); keeping original", file=sys.stderr)
        out = text

    # sanity: non-empty and length within tolerance, else keep original
    if not out:
        print("  ! empty repair output; keeping original", file=sys.stderr)
        out = text
    else:
        drift = abs(len(out) - len(text)) / max(len(text), 1)
        if drift > LEN_TOLERANCE:
            print(f"  ! repair length drift {drift:.0%} > {LEN_TOLERANCE:.0%}; "
                  f"keeping original", file=sys.stderr)
            out = text

    cp.write_text(out, "utf-8")
    return out


def repaired_paragraphs(doc, start, end):
    """Extract -> reflow -> repair in LARGE chunks -> return repaired paragraphs.

    Repair operates on big units (few LLM calls); the result is split back into
    paragraphs so the TTS stage can re-pack at its own (smaller) size.
    """
    paras = clean_pages(doc, start, end)
    rchunks = pack_paragraphs(paras, REPAIR_MAX_CHARS)
    out = []
    for i, c in enumerate(rchunks, 1):
        print(f"  repairing chunk {i}/{len(rchunks)} ({len(c)} chars)…",
              file=sys.stderr)
        fixed = repair(c)
        out.extend(p.strip() for p in re.split(r'\n\s*\n', fixed) if p.strip())
    return out


def tts_chunks(doc, start, end):
    """Repaired paragraphs re-packed into <=TTS_MAX_CHARS units for synthesis."""
    return pack_paragraphs(repaired_paragraphs(doc, start, end), TTS_MAX_CHARS)


if __name__ == "__main__":
    doc = fitz.open("book.pdf")
    a, b = (int(sys.argv[1]), int(sys.argv[2])) if len(sys.argv) > 2 else (13, 33)
    chunks = tts_chunks(doc, a, b)
    total = sum(len(c) for c in chunks)
    print(f"# pages {a}-{b}: {len(chunks)} TTS chunks, {total} chars\n")
    for i, c in enumerate(chunks):
        print(f"\n{'='*72}\nTTS CHUNK {i} ({len(c)} chars)\n{'='*72}")
        print(c)
