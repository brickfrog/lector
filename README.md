# lector

Turn a book into a chaptered audiobook (`.m4b`) and a word-highlighted read-along
subtitle track (`.ass`). Narration runs locally using Kokoro and ONNX Runtime.
Play both in mpv, or open the M4B in an audiobook player.

## Install

Requirements:

- Rust **1.92 or newer** and a native C/C++ build toolchain. Linux is the tested platform.
- `espeak-ng` and `ffmpeg` on PATH for narration and encoding.
- Poppler (`pdftotext`, `pdfinfo`, `pdftoppm`) for PDF input and covers.
- Optional: Calibre's `ebook-convert` for MOBI, AZW/AZW3, FB2/FBZ, LIT, PDB, HTMLZ.
- Python 3 for asset downloads; [uv](https://docs.astral.sh/uv/) for the one-time
  model export. Export dependencies are locked separately from the Rust app.

On Debian/Ubuntu, install external tools with:

```sh
sudo apt-get install build-essential pkg-config libssl-dev espeak-ng ffmpeg poppler-utils
```

From this checkout:

```sh
python3 scripts/vendor.py --out assets
uv run --locked scripts/export_dur.py --out assets
cargo build --release --locked
./target/release/lector --assets assets doctor
```

The first two steps need network access. Downloads use a pinned Hugging Face
revision and verified SHA-256 hashes; each asset is replaced only after it passes
verification. The exporter pins its model revision, writes a checksum sidecar,
and validates the ONNX graph. Exporting requires several GB of available memory
and downloads a CPU PyTorch environment. The exported model is roughly 326 MB.
Python is **not** needed for subsequent narration.

This is a Rust executable plus model assets and external tools, not a standalone
static distribution. The `ort` build downloads its matching native ONNX Runtime
by default; building may therefore require network access even with `--locked`.

Asset directory discovery, in order:

1. `--assets DIR`, then `LECTOR_ASSETS`.
2. `./assets` in the current directory.
3. An `assets` directory beside the executable.
4. `$XDG_DATA_HOME/lector/assets`, or `$HOME/.local/share/lector/assets`.

Select one complete asset directory; files are never mixed across directories.
For an installed binary, set `LECTOR_ASSETS` to an absolute path. `doctor` checks
external tools, pinned companion asset hashes, and actual model inference.
Custom compatible voices are allowed; their names use letters, digits and `_`.

## Read, inspect, and preview

```sh
lector book.epub --dry-run                    # chapters and estimated duration
lector inspect book.epub                     # extracted narration text
lector inspect book.epub -o extracted.txt
lector preview book.epub -o out/              # first 85 words, roughly 30 seconds
lector preview book.epub --chapters 3 --words 120 -v af_heart -o out/
lector book.epub -o out/ --chapters 2-12 --exclude-chapters 6
mpv --sub-file=out/book.ass out/book.m4b
```

Chapter numbers refer to the **full book's** `--dry-run` listing, starting at 1.
Selections accept comma-separated numbers and inclusive ranges; exclusions are
applied after inclusions. Use them to omit front matter, appendices, or unwanted
sections. Preview uses a word budget, not an exact audio duration, and writes a
separate `<book>-preview` output/checkpoint.

EPUB extraction follows spine order, uses EPUB3 navigation or EPUB2 NCX chapter
boundaries, and falls back to headings when a resource has no navigation entries.
Lists, tables, blockquotes and plain containers are included. Navigation pages,
scripts and explicitly hidden/ARIA-hidden elements are skipped; CSS visibility
rules are not evaluated. Navigation labels mark chapters without being narrated
again. Inspect the extracted text before a long render.

PDF extraction uses Poppler word boxes and font-size/layout heuristics. It can
misread columns or remove small text; scanned PDFs need OCR elsewhere first.
EPUB is preferable when available. Calibre conversions use private temporary
directories, cleaned up on success or failure. Converted-book cover art is not
currently retained.

## Voices and pronunciation

```sh
lector voices
lector book.epub -o out/ -v af_heart
lector book.epub -o out/ -v bm_george --pronunciations names.json
```

A pronunciation file is a JSON object of **case-sensitive single words** to
spoken replacements:

```json
{"Ada": "Ay dah", "API": "A P I"}
```

Surrounding punctuation is preserved, and subtitles retain the original spelling.
Changing this dictionary requires a new render checkpoint. Voice prefixes infer
the eSpeak language (`a` = en-us, `b` = en-gb, etc.); override with `--lang`.
English is the tested path. Other languages depend on eSpeak's phonemes matching
the model vocabulary and may need additional language-specific processing.

## Output and reuse

```sh
lector book.epub -o out/ --speed 1.25 --bitrate 96 --export-timings
lector reencode out/.book.ckpt -o faster/ --speed 1.5 --export-timings
```

`--export-timings` adds word-level JSON and grouped SRT subtitles. ASS remains the
word-highlighted format. `--line-words`, `--sub-font`, and `--sub-size` control ASS
layout; line word count also controls SRT grouping. `--speed` supports 0.25–4.0
using FFmpeg's pitch-preserving tempo filter. Chapters and subtitles use the same
scaled timeline. Word alignment is estimated from model durations and phoneme
weights; it is not a separately verified forced alignment.

Checkpoints are **kept by default** so `reencode` can change speed, bitrate and
subtitle settings without loading a model, running eSpeak, or reopening the book.
It regenerates chapter timestamps from saved boundaries and reuses cached cover
art if available. An incomplete checkpoint must first be resumed with the original
render command. Use `--discard-checkpoint` to remove the cache after successful
encoding; the older `--keep-checkpoint` flag remains accepted.

The cache stores plaintext narration and 24 kHz mono 16-bit PCM (about **173 MB per
hour**), plus timing records. Plan disk space accordingly.

## Recovery and security

Re-run the same command to resume. The checkpoint identifies the book, voice,
model, tokenizer, eSpeak version, chapter selection, preview limit and pronunciation
file. Changed inputs are rejected instead of mixing old and new narration. Move the
checkpoint aside or choose another output directory to start over. Checkpoints
from the earlier implementation lack these fingerprints and cannot resume into
new synthesis; completed ones can still be passed to `reencode` after validation.

A per-output lock excludes concurrent writers. Audio is synced before a journal
record commits it. Recovery drops an incomplete final record and extra PCM tail;
missing audio or corruption in a committed record produces an error. Filesystem
and storage hardware must honor sync operations for power-loss durability.

FFmpeg failure is an error with diagnostics; no silent WAV fallback occurs, and
the expensive checkpoint stays available. Encoding happens in a temporary file.
Finished artifacts are replaced atomically **per file**, not as a multi-file
transaction; a publication error can be retried from the checkpoint. A successful
render/re-encode replaces existing output files with the same name. Different
filenames can normalize to the same ASCII output slug, so use separate output
directories when necessary.

Book text is sent to eSpeak through stdin, never as command-line options. EPUB
preflight limits archives to 10,000 entries, 64 MiB per decompressed entry,
512 MiB decompressed total and 1 GiB compressed. External parsers and model
runtimes are not sandboxed by Lector; keep Poppler, Calibre, FFmpeg and eSpeak
updated when handling untrusted inputs.

## Development

```sh
just check                # format, strict Clippy, Rust + downloader tests
just check-tools          # tests requiring eSpeak/FFmpeg/Poppler
just audit                # scan locked Rust + Python packages against OSV
```

CI runs these checks on stable Rust and Rust 1.92, plus `cargo audit` and a weekly
advisory scan covering both the Rust application and Python exporter.
The model and voice files are not committed. Tests that exercise the real model
can be run with `python3 scripts/smoke.py --assets assets` after `cargo build`.

- `src/`: source extraction, planning, synthesis, checkpointing, timing and output.
- `scripts/assets.json`: pinned companion asset manifest.
- `scripts/export_dur.py` and its lockfile: one-time duration-aware model export.
- `tests/`: CLI fixtures, extraction, encoding and failure-path regressions.
