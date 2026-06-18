#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["torch>=2.2", "torchaudio>=2.2", "pymupdf>=1.24",
#                 "soundfile>=0.12", "requests>=2.31.0", "pydub>=0.25.1",
#                 "audioop-lts>=0.2.1; python_version>='3.13'"]
# ///
"""
Forced alignment -> ASS karaoke subtitle.

Aligns the EXACT repaired text against the synthesized audio (alignment, not
transcription) using torchaudio's MMS_FA aligner, then writes a .ass file with
progressive word-level karaoke. Play with:  mpv <audio> --sub-file=<ass>

Footgun (known): the TTS speaks normalized forms ("1767" -> "seventeen sixty
seven"), but our text token is "1767". Such tokens won't grapheme-align; we give
them zero-width highlight so they display but don't break neighbors.
"""

import re
import sys
import unicodedata
from pathlib import Path

import numpy as np
import torch
import torchaudio
from pydub import AudioSegment

from build import pack_paragraphs, TTS_MAX_CHARS


def normalize(word):
    """Map a display word to MMS_FA dictionary symbols ([a-z'] roughly)."""
    w = unicodedata.normalize("NFKD", word).encode("ascii", "ignore").decode()
    return re.sub(r"[^a-z']", "", w.lower())


def split_lines(words, max_words=12):
    """Group display words into screen lines on sentence enders or width."""
    lines, cur = [], []
    for w in words:
        cur.append(w)
        if (re.search(r'[.!?:;]["\')\]]?$', w) and len(cur) >= 4) or len(cur) >= max_words:
            lines.append(cur)
            cur = []
    if cur:
        lines.append(cur)
    return lines


def load_aligner():
    bundle = torchaudio.pipelines.MMS_FA
    return bundle.get_model(), bundle.get_dict(), bundle.sample_rate


def align(audio_path, words, model, DICT, sample_rate):
    """Align one chunk's audio against its words. Returns (times, duration_s).

    MUST be called per-chunk: forced_align's trellis is O(frames x tokens), so a
    whole 39-min chapter (117k frames x 29k tokens) OOMs the box. Per ~8-min
    chunk it stays bounded — the size the prototype proved safe.
    """
    seg = AudioSegment.from_file(audio_path)              # mp3 via ffmpeg
    duration_s = len(seg) / 1000.0
    samples = np.array(seg.get_array_of_samples(), dtype=np.float32)
    if seg.channels == 2:
        samples = samples.reshape(-1, 2).mean(1)          # mono
    samples /= float(1 << (8 * seg.sample_width - 1))     # int -> [-1,1]
    wav = torch.from_numpy(samples).unsqueeze(0)
    if seg.frame_rate != sample_rate:
        wav = torchaudio.functional.resample(wav, seg.frame_rate, sample_rate)

    # wav2vec2 self-attention is O(frames^2); a whole 8-min clip in one forward
    # peaks >12GB. Window the FORWARD into ~20s segments (attention bounded to
    # 20s^2) and stitch emissions; forced_align over the stitched result is cheap.
    WIN = int(20 * sample_rate)
    embs = []
    with torch.inference_mode():
        for s in range(0, wav.size(1), WIN):
            e, _ = model(wav[:, s:s + WIN])
            embs.append(e)
    emission = torch.cat(embs, dim=1)

    # build flat token target from alignable words; remember word->token counts
    norms = [normalize(w) for w in words]
    tok_words, counts, idx_map = [], [], []
    for i, n in enumerate(norms):
        toks = [DICT[c] for c in n if c in DICT]
        if toks:
            tok_words.extend(toks)
            counts.append(len(toks))
            idx_map.append(i)

    targets = torch.tensor([tok_words], dtype=torch.int32)
    aligned, scores = torchaudio.functional.forced_align(emission, targets, blank=0)
    spans = torchaudio.functional.merge_tokens(aligned[0], scores[0].exp())

    # seconds per emission frame
    spf = (wav.size(1) / sample_rate) / emission.size(1)

    # group token-spans back into words
    times = [None] * len(words)   # (start_s, end_s) per display word
    pos = 0
    for wi, c in zip(idx_map, counts):
        grp = spans[pos:pos + c]
        pos += c
        times[wi] = (grp[0].start * spf, grp[-1].end * spf)

    # fill unaligned (numbers/punct) words with zero-width at neighbor time
    last_end = 0.0
    for i in range(len(words)):
        if times[i] is None:
            times[i] = (last_end, last_end)
        last_end = times[i][1]
    return times, duration_s


def to_ass(words, times, out_path):
    def cs(t):  # seconds -> ASS h:mm:ss.cc
        h = int(t // 3600); m = int(t % 3600 // 60); s = t % 60
        return f"{h}:{m:02d}:{s:05.2f}"

    header = (
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 1280\nPlayResY: 720\n"
        "WrapStyle: 0\n\n[V4+ Styles]\n"
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, "
        "OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, "
        "ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, "
        "MarginL, MarginR, MarginV, Encoding\n"
        # Primary = bright (sung), Secondary = dim gray (upcoming).  &HAABBGGRR
        "Style: Read,Georgia,40,&H0000FFFF,&H80AAAAAA,&H00000000,&H64000000,"
        "0,0,0,0,100,100,0,0,1,2,1,2,80,80,60,1\n\n"
        "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, "
        "MarginV, Effect, Text\n"
    )

    events = []
    # iterate display lines, mapping each word index
    wi = 0
    for line in split_lines(words):
        n = len(line)
        idxs = list(range(wi, wi + n))
        wi += n
        start = times[idxs[0]][0]
        # per-word karaoke durations: fold inter-word gaps into preceding word
        durs = []
        for k, gi in enumerate(idxs):
            s, e = times[gi]
            nxt = times[idxs[k + 1]][0] if k + 1 < n else e
            durs.append(max(1, round((nxt - s) * 100)))   # centiseconds, >=1
        end = start + sum(durs) / 100.0
        text = "".join(f"{{\\kf{d}}}{w} " for d, w in zip(durs, line)).strip()
        events.append(f"Dialogue: 0,{cs(start)},{cs(end)},Read,,0,0,0,,{text}")

    Path(out_path).write_text(header + "\n".join(events) + "\n", "utf-8")


if __name__ == "__main__":
    from kokoro_tts import chunk_cache_path, GAP_MS

    a, b = int(sys.argv[1]), int(sys.argv[2])
    out = Path(f"audio/ch_{a}-{b}.ass")

    # reconstruct the exact chunks that were synthesized
    rep = Path(f"text/rep_{a}-{b}.txt").read_text("utf-8")
    paras = [p.strip() for p in re.split(r"\n\s*\n", rep) if p.strip()]
    chunks = pack_paragraphs(paras, TTS_MAX_CHARS)

    model, DICT, sr = load_aligner()
    all_words, all_times, offset = [], [], 0.0
    for i, chunk in enumerate(chunks, 1):
        mp3 = chunk_cache_path(chunk)
        words = chunk.split()
        print(f"aligning chunk {i}/{len(chunks)} ({len(words)} words) @ +{offset:.1f}s…",
              file=sys.stderr)
        times, dur = align(str(mp3), words, model, DICT, sr)
        all_words.extend(words)
        all_times.extend((s + offset, e + offset) for s, e in times)
        offset += dur + GAP_MS / 1000.0          # next chunk follows a gap (assemble())

    to_ass(all_words, all_times, out)
    print(f"✓ {out}  ({len(all_words)} words, {len(chunks)} chunks)   "
          f"play: mpv audio/ch_{a}-{b}.mp4")
