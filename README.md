# lector

Turn a PDF into a karaoke audiobook: a single audio file plus a word-synced
`.ass` subtitle track. Play it in mpv (it renders the karaoke) or any player.

One static binary. Synthesis runs natively via onnxruntime, around four times
faster than realtime, and long renders are resumable.

## How it works

```
PDF --pdftotext--> spans --clean--> paragraphs --pack--> sentence units
    --espeak-ng--> phonemes --Kokoro (onnxruntime)--> waveform + per-phoneme
    durations --align--> per-word timestamps --> mp3 + karaoke .ass
```

Word timing falls out of synthesis: the Kokoro duration model emits a frame
count per phoneme, so there is no separate forced aligner. The cleaner uses
per-word box height (a font-size proxy) to drop footnotes, headers, and page
numbers, dehyphenate line breaks, and reflow paragraphs.

## Requirements

- Rust (stable)
- `espeak-ng`, `pdftotext` (poppler), `ffmpeg` on PATH
- onnxruntime (the `ort` crate links the system lib or downloads one)
- `assets/`: `kokoro_dur.onnx`, `bm_george.bin`, `tokenizer.json`
  (`just vendor` fetches the voice + tokenizer; the model is built by
  `scripts/export_dur.py`)

## Use

```
just build
just render path/to/book.pdf out/        # -> out/book.mp3 + out/book.ass
mpv --sub-file=out/book.ass out/book.mp3
```

`lector <input.pdf> [outDir=.] [voice=bm_george] [lang=a|b]` (a = en-us,
b = en-gb).

## Resumable

Every unit's audio and word timings are committed to a `.<slug>.ckpt` directory
as it finishes. If a render is killed, re-run the same command: it replays the
checkpoint and continues from where it stopped, producing output identical to an
uninterrupted run. The checkpoint is removed on success.

## Layout

- `src/` Rust: `pdf`, `clean`, `chunk`, `synth`, `timing`, `ass`, `main`
- `scripts/` model export (`export_dur.py`) + asset vendor
- `assets/` model / voice / tokenizer (not committed)
