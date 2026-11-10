#!/usr/bin/env python3
"""Fetch pinned Kokoro companion assets; verify before atomic publication."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import tempfile
import urllib.request


def fetch(entry, base, destination):
    target = destination / Path(entry['path']).name
    if target.is_file() and hashlib.sha256(target.read_bytes()).hexdigest() == entry['sha256']:
        print(f'verified {target.name}')
        return
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=destination, delete=False) as out:
            temporary = Path(out.name)
            digest = hashlib.sha256()
            size = 0
            with urllib.request.urlopen(base + '/' + entry['path'], timeout=60) as response:
                while chunk := response.read(65536):
                    size += len(chunk)
                    if size > entry['size']:
                        raise ValueError(f'oversized download: {target.name}')
                    digest.update(chunk)
                    out.write(chunk)
            if size != entry['size'] or digest.hexdigest() != entry['sha256']:
                raise ValueError(f'checksum/size mismatch: {target.name}')
            out.flush()
            os.fsync(out.fileno())
        os.replace(temporary, target)
        print(f'installed {target.name}')
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=Path(os.environ.get('LECTOR_ASSETS', 'assets')))
    args = parser.parse_args()
    manifest = json.loads(Path(__file__).with_name('assets.json').read_text())
    base = f"https://huggingface.co/{manifest['repository']}/resolve/{manifest['revision']}"
    args.out.mkdir(parents=True, exist_ok=True)
    for entry in manifest['files']:
        fetch(entry, base, args.out)
    print('Model: run scripts/export_dur.py with --out pointing to this directory.')


if __name__ == '__main__':
    main()
