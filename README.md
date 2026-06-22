# lector

Turn a book into a karaoke audiobook: a chaptered `.m4b` plus a word-synced
`.ass` subtitle track. Play it in mpv (it renders the karaoke and exposes chapter
nav) or any audiobook player.

One static binary. Synthesis runs natively via onnxruntime, around four times
faster than realtime, and long renders are resumable.

## Formats

- **PDF** and **EPUB** are handled natively.
- **MOBI / AZW3 / FB2 / …** go through calibre's `ebook-convert` (to a temp EPUB)
  when it's on PATH; otherwise convert to EPUB first.

EPUB gives cleaner results than PDF (explicit paragraphs and headings, no
font-size guessing, no dehyphenation), so prefer it when you have the choice.

## How it works

```
source -> Blocks (paragraphs + headings)
  PDF:  pdftotext spans -> clean (drop footnotes/headers by size, dehyphenate, reflow)
  EPUB: spine XHTML -> <p>/<h*> straight from the markup
Blocks -> chapters at headings -> sentence units
  -> espeak-ng phonemes -> Kokoro (onnxruntime) -> waveform + per-phoneme durations
  -> per-word timestamps -> stream PCM -> ffmpeg .m4b (AAC + chapters) + karaoke .ass
```

Word timing falls out of synthesis: the Kokoro duration model emits a frame
count per phoneme, so there is no separate forced aligner. Headings (EPUB `<h1>`/
`<h2>`, or PDF lines notably larger than body text) become chapter markers and
are still narrated; title and author become file metadata.

## Requirements

- Rust (stable)
- `espeak-ng`, `pdftotext` (poppler), `ffmpeg` on PATH (`ebook-convert` for MOBI etc.)
- onnxruntime (the `ort` crate links the system lib or downloads one)
- `assets/`: `kokoro_dur.onnx`, `bm_george.bin`, `tokenizer.json`
  (`just vendor` fetches the voice + tokenizer; the model is built by
  `scripts/export_dur.py`)

## Use

```
just build
just render path/to/book.epub out/        # -> out/book.m4b + out/book.ass
mpv --sub-file=out/book.ass out/book.m4b
```

`lector <book.pdf|.epub|.mobi|...> [outDir=.] [voice=bm_george] [lang=a|b]`
(a = en-us, b = en-gb).

## Resumable

Every unit's audio and word timings commit to a `.<slug>.ckpt` directory as it
finishes. If a render is killed, re-run the same command: it replays the
checkpoint and continues from where it stopped, producing output identical to an
uninterrupted run. The checkpoint is removed on success.

## Layout

- `src/` Rust: `source`, `pdf`, `epub`, `clean`, `chunk`, `synth`, `timing`, `ass`, `main`
- `scripts/` model export (`export_dur.py`) + asset vendor
- `assets/` model / voice / tokenizer (not committed)
