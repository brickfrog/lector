#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["requests>=2.31.0", "pydub>=0.25.1", "pymupdf>=1.24",
#                 "audioop-lts>=0.2.1; python_version>='3.13'"]
# ///
"""
TTS stage: repaired text chunks -> audio, via MiniMax T2A v2 (sync) with the
Disco Elysium clone voice. Content-hashed cache (reruns are free). Concatenates
chunks with short gaps and applies a single final loudnorm pass.

Lifts the proven client core from the Korean repo; differences for long-form:
  - English voice clone + language_boost=English
  - NO per-chunk silence padding (that was for discrete Anki cards); gaps are
    inserted only between chunks, and loudnorm runs once on the joined track.
"""

import hashlib
import os
import re
import subprocess
import sys
from pathlib import Path

import requests
from pydub import AudioSegment

from build import pack_paragraphs, TTS_MAX_CHARS

ENDPOINT = "https://api.minimax.io/v1/t2a_v2"
VOICE_ID = "moss_audio_2ecc213a-2d14-11f1-a0a8-2e21911c2cf8"
MODEL = os.environ.get("MINIMAX_MODEL", "speech-2.8-hd")   # cache key keys on this
CACHE_DIR = Path(".tts_cache")
GAP_MS = 400          # inter-chunk pause (paragraph-boundary breathing room)


def load_secret(env_var, pass_path):
    val = os.environ.get(env_var)
    if val:
        return val.strip()
    try:
        r = subprocess.run(["pass", "show", pass_path],
                           capture_output=True, text=True, check=True, timeout=5)
        lines = r.stdout.strip().splitlines()
        return lines[0] if lines else None
    except (subprocess.CalledProcessError, FileNotFoundError, subprocess.TimeoutExpired):
        return None


class MiniMaxTTS:
    def __init__(self, api_key, group_id):
        self.api_key, self.group_id = api_key, group_id
        CACHE_DIR.mkdir(exist_ok=True)
        self.s = requests.Session()
        self.s.headers.update({"Authorization": f"Bearer {api_key}",
                               "Content-Type": "application/json"})

    def _key(self, text):
        blob = f"{MODEL}|{VOICE_ID}|1.0|{text}".encode()
        return hashlib.sha256(blob).hexdigest()[:24]

    def synth(self, text):
        cp = CACHE_DIR / f"{self._key(text)}.mp3"
        if cp.exists():
            return cp.read_bytes()
        url = f"{ENDPOINT}?GroupId={self.group_id}" if self.group_id else ENDPOINT
        payload = {
            "model": MODEL, "text": text, "stream": False,
            "language_boost": "English", "output_format": "hex",
            "voice_setting": {"voice_id": VOICE_ID, "speed": 1.0, "vol": 1, "pitch": 0},
            "audio_setting": {"sample_rate": 32000, "bitrate": 128000,
                              "format": "mp3", "channel": 1},
        }
        import time
        RATE_LIMIT = {1002, 1039}     # TPM/QPS limits -> back off and retry
        last = None
        for attempt in range(6):
            try:
                r = self.s.post(url, json=payload, timeout=120)
                r.raise_for_status()
                body = r.json()
                base = body.get("base_resp", {})
                code = base.get("status_code")
                if code in RATE_LIMIT:
                    wait = 20 * (attempt + 1)
                    print(f"  · rate-limited ({code}); waiting {wait}s", file=sys.stderr)
                    time.sleep(wait)
                    continue
                if code not in (0, None):
                    raise RuntimeError(f"MiniMax {code}: {base.get('status_msg')}")
                hexaudio = body.get("data", {}).get("audio")
                if not hexaudio:
                    raise RuntimeError(f"no audio: {body}")
                raw = bytes.fromhex(hexaudio)
                cp.write_bytes(raw)
                return raw
            except (requests.HTTPError, requests.ConnectionError, requests.Timeout) as e:
                last = e
                time.sleep(2 ** attempt)
        raise RuntimeError(f"TTS failed after retries: {last}")


def assemble(chunk_mp3s, out_path, gap_ms=GAP_MS):
    """Concat chunk mp3s with gaps, then one EBU R128 loudnorm pass."""
    import tempfile
    combined = AudioSegment.empty()
    gap = AudioSegment.silent(duration=gap_ms)
    for i, raw in enumerate(chunk_mp3s):
        tmp = Path(tempfile.mktemp(suffix=".mp3"))
        tmp.write_bytes(raw)
        try:
            seg = AudioSegment.from_mp3(tmp)
        finally:
            tmp.unlink(missing_ok=True)
        if i:
            combined += gap
        combined += seg
    combined.export(out_path, format="mp3", bitrate="128k",
                    parameters=["-ar", "44100", "-ac", "2",
                                "-af", "loudnorm=I=-16:TP=-1.5:LRA=11"])
    return len(combined)


if __name__ == "__main__":
    a, b = int(sys.argv[1]), int(sys.argv[2])
    out = sys.argv[3] if len(sys.argv) > 3 else f"audio/ch_{a}-{b}.mp3"
    Path(out).parent.mkdir(parents=True, exist_ok=True)

    api_key = load_secret("MINIMAX_API_KEY", "api/minimax-audio")
    group_id = load_secret("MINIMAX_GROUP_ID", "api/minimax-group")
    if not api_key:
        sys.exit("Missing MiniMax API key")

    # read repaired TEXT artifact (never the PDF) and re-pack to TTS size
    rep = Path(f"text/rep_{a}-{b}.txt").read_text("utf-8")
    paras = [p.strip() for p in re.split(r"\n\s*\n", rep) if p.strip()]
    fixed = pack_paragraphs(paras, TTS_MAX_CHARS)
    tts = MiniMaxTTS(api_key, group_id)

    mp3s = []
    for i, chunk in enumerate(fixed, 1):
        print(f"  synth chunk {i}/{len(fixed)} ({len(chunk)} chars)…", file=sys.stderr)
        mp3s.append(tts.synth(chunk))

    ms = assemble(mp3s, out)
    print(f"✓ {out}  ({ms/1000:.1f}s audio from {len(fixed)} chunk(s), pages {a}-{b})")
