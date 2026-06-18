# lector

Turn a book (PDF / EPUB) into a **karaoke audiobook** — synthesized narration with
word-level highlighting — fully locally, no per-use cost.

This is a polyglot monorepo. The pieces are coupled by one **data contract**: a
list of `{text, phonemes, whitespace}` tokens plus the TTS model's per-phoneme
frame durations (`pred_dur`). From that, word-level timing falls out *for free* —
no forced aligner, no second model — because the TTS (Kokoro) already computes it
during synthesis.

## Layout

| dir | what | language | status |
|-----|------|----------|--------|
| **`core/`** | `brickfrog/lector` — the pure logic library: paragraph chunking, `pred_dur` → word timestamps, ASS karaoke rendering, book-timeline assembly. No I/O, no ML, no FFI; compiles to JS **and** native. | MoonBit | ✅ tested (9/9, incl. golden parity to the reference) |
| **`reference/`** | The working end-to-end Python pipeline this was ported from — and the source of ground-truth fixtures. Extract → glyph-repair (LLM) → Kokoro TTS → forced-align → ASS → chaptered M4B, sandboxed with bubblewrap + memory caps. | Python | ✅ produces real audiobooks |
| `web/` | Browser app: `core` (JS) + `kokoro-js` + JASSUB. | TS | ⬜ planned |
| `bridge/` | Kokoro → `(tokens, pred_dur)` extractor that feeds `core`. | TS / Py | ⬜ planned |

## Quickstart (core)

```sh
cd core
moon test              # unit + golden parity
moon run cmd/main      # decode a JSON fixture -> print ASS karaoke
moon build --target js # browser-host build
```

## Design notes

- **`core` is the brain, not the program.** It transforms data → strings. Ingest,
  TTS inference, and audio assembly live in the host (today: `reference/`; later:
  `web/` + `bridge/`).
- **Timing is bit-faithful** to the Python reference: `core`'s `join_timestamps`
  is verified against a Kokoro-generated fixture (`core/testdata/sentence.json`) to
  within 2 ms.
- **No source books or generated audio are committed** — they're copyrighted and/or
  huge. Bring your own input; `.gitignore` blocks the media.

Not published to mooncakes (yet).
