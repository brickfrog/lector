#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["requests>=2.31.0"]
# ///
"""
MiniMax T2A v2 smoke test for the audiobook pipeline.

Validates three things in one cheap sync call:
  1. The cloned voice_id resolves on this account
  2. It synthesizes English cleanly
  3. The plan unlocks the chosen model

Auth (same convention as the Korean repo):
    MINIMAX_API_KEY   or  `pass show api/minimax-audio`
    MINIMAX_GROUP_ID  or  `pass show api/minimax-group`

Writes smoke.mp3 for you to actually listen to.
"""

import os
import subprocess
import sys

import requests

ENDPOINT = "https://api.minimax.io/v1/t2a_v2"
VOICE_ID = "moss_audio_2ecc213a-2d14-11f1-a0a8-2e21911c2cf8"
MODEL = "speech-2.8-hd"
TEXT = ("The basic concept of exit, voice, and loyalty was developed "
        "by Prof. Hirschman in 1970.")
OUT = "smoke.mp3"


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


def main():
    api_key = load_secret("MINIMAX_API_KEY", "api/minimax-audio")
    group_id = load_secret("MINIMAX_GROUP_ID", "api/minimax-group")
    if not api_key:
        sys.exit("Missing MiniMax API key (env MINIMAX_API_KEY or pass api/minimax-audio)")

    url = f"{ENDPOINT}?GroupId={group_id}" if group_id else ENDPOINT
    payload = {
        "model": MODEL,
        "text": TEXT,
        "stream": False,
        "language_boost": "English",
        "output_format": "hex",
        "voice_setting": {"voice_id": VOICE_ID, "speed": 1.0, "vol": 1, "pitch": 0},
        "audio_setting": {"sample_rate": 32000, "bitrate": 128000,
                          "format": "mp3", "channel": 1},
    }

    print(f"Model:   {MODEL}")
    print(f"Voice:   {VOICE_ID}")
    print(f"Text:    {TEXT}")
    print(f"Chars:   {len(TEXT)}\n")

    resp = requests.post(url, json=payload,
                         headers={"Authorization": f"Bearer {api_key}",
                                  "Content-Type": "application/json"}, timeout=60)
    resp.raise_for_status()
    body = resp.json()

    base = body.get("base_resp", {})
    if base.get("status_code") not in (0, None):
        sys.exit(f"MiniMax error {base.get('status_code')}: {base.get('status_msg')}")

    audio_hex = body.get("data", {}).get("audio")
    if not audio_hex:
        sys.exit(f"No audio in response: {body}")

    raw = bytes.fromhex(audio_hex)
    with open(OUT, "wb") as f:
        f.write(raw)

    info = body.get("extra_info", {})
    print(f"✓ Wrote {OUT} ({len(raw)} bytes)")
    if info:
        print(f"  audio length: {info.get('audio_length')} ms")
        print(f"  word count:   {info.get('word_count')}")
        print(f"  chars billed: {info.get('usage_characters')}")


if __name__ == "__main__":
    main()
