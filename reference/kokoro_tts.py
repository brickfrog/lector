#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["kokoro>=0.9.4", "soundfile>=0.12", "pydub>=0.25.1",
#                 "numpy>=1.26", "audioop-lts>=0.2.1; python_version>='3.13'"]
# ///
"""Local TTS stage (OFFLINE jail): Kokoro 82M. No network, credits, or limits.

Reads the repaired text artifact, synthesizes per TTS-chunk (cached), assembles
the chapter mp3 with inter-chunk gaps + one loudnorm pass. Drop-in replacement
for the MiniMax stage; align.py reads the same per-chunk cache via
chunk_cache_path(). Self-contained (no fitz import) so the env stays light.
"""

import hashlib
import io
import os
import re
import sys
from pathlib import Path

import numpy as np
from pydub import AudioSegment

VOICE = os.environ.get("KOKORO_VOICE", "bm_george")
LANG = VOICE[0]                 # 'b' British / 'a' American -> Kokoro lang_code
SR = 24000
GAP_MS = 400
TTS_MAX_CHARS = 8000            # must match build.TTS_MAX_CHARS (align re-packs)
CACHE_DIR = Path(".tts_cache")


def pack_paragraphs(paras, max_chars=TTS_MAX_CHARS):
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


def chunk_cache_path(text):
    key = hashlib.sha256(f"kokoro|{VOICE}|{text}".encode()).hexdigest()[:24]
    return CACHE_DIR / f"{key}.mp3"


_pipe = None
def _pipeline():
    global _pipe
    if _pipe is None:
        from kokoro import KPipeline
        _pipe = KPipeline(lang_code=LANG)
    return _pipe


def synth(text):
    CACHE_DIR.mkdir(exist_ok=True)
    cp = chunk_cache_path(text)
    if cp.exists():
        return cp.read_bytes()
    segs = [audio for _, _, audio in _pipeline()(text, voice=VOICE)]
    samples = np.concatenate(segs) if segs else np.zeros(1, dtype=np.float32)
    pcm = (np.clip(samples, -1, 1) * 32767).astype(np.int16)
    seg = AudioSegment(pcm.tobytes(), frame_rate=SR, sample_width=2, channels=1)
    buf = io.BytesIO()
    seg.export(buf, format="mp3", bitrate="128k")
    raw = buf.getvalue()
    cp.write_bytes(raw)
    return raw


def assemble(chunk_mp3s, out_path, gap_ms=GAP_MS):
    import tempfile
    combined = AudioSegment.empty()
    gap = AudioSegment.silent(duration=gap_ms)
    for i, raw in enumerate(chunk_mp3s):
        fd, name = tempfile.mkstemp(suffix=".mp3")
        os.close(fd)
        tmp = Path(name)
        tmp.write_bytes(raw)
        try:
            s = AudioSegment.from_mp3(tmp)
        finally:
            tmp.unlink(missing_ok=True)
        if i:
            combined += gap
        combined += s
    combined.export(out_path, format="mp3", bitrate="128k",
                    parameters=["-ar", "44100", "-ac", "2",
                                "-af", "loudnorm=I=-16:TP=-1.5:LRA=11"])
    return len(combined)


if __name__ == "__main__":
    a, b = int(sys.argv[1]), int(sys.argv[2])
    out = sys.argv[3] if len(sys.argv) > 3 else f"audio/ch_{a}-{b}.mp3"
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    rep = Path(f"text/rep_{a}-{b}.txt").read_text("utf-8")
    paras = [p.strip() for p in re.split(r"\n\s*\n", rep) if p.strip()]
    chunks = pack_paragraphs(paras, TTS_MAX_CHARS)
    mp3s = []
    for i, c in enumerate(chunks, 1):
        print(f"  synth chunk {i}/{len(chunks)} ({len(c)} chars)…", file=sys.stderr)
        mp3s.append(synth(c))
    ms = assemble(mp3s, out)
    print(f"✓ {out}  ({ms/1000:.1f}s, {len(chunks)} chunks, voice={VOICE})")
