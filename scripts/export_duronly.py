#!/usr/bin/env python3
"""Compatibility wrapper for the experimental timing-only model export.

Uses the same pinned model and locked CPU environment as the full exporter.
Not needed by the native Lector pipeline. Default output directory: assets/.
"""
from pathlib import Path
import subprocess
import sys

if __name__ == "__main__":
    sys.exit(subprocess.call(["uv", "run", "--locked", str(Path(__file__).with_name("export_dur.py")), "--duration-only", *sys.argv[1:]]))
