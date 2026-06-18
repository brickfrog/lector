#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["pymupdf4llm>=0.0.17", "pymupdf>=1.24"]
# ///
"""Extract the Hirschman PDF to markdown, per-page, for inspection."""

import sys
import pymupdf4llm

PDF = ("/home/justin/Documents/Library/Albert O. Hirschman/"
       "Exit, Voice, and Loyalty_ Responses to Decline in Firms, "
       "Organizations, and States (402)/"
       "Exit, Voice, and Loyalty_ Responses to Dec - Albert O. Hirschman.pdf")

# page_chunks=True gives one dict per page so we can inspect structure / detect
# repeated running heads across pages.
pages = pymupdf4llm.to_markdown(PDF, page_chunks=True)

import json
with open("pages.json", "w") as f:
    json.dump([{"page": i, "text": p["text"]} for i, p in enumerate(pages)], f)

print(f"Extracted {len(pages)} pages -> pages.json")

# Print a few representative slices if requested
if len(sys.argv) > 1:
    for spec in sys.argv[1:]:
        i = int(spec)
        print(f"\n{'='*72}\nPAGE {i}\n{'='*72}")
        print(pages[i]["text"])
