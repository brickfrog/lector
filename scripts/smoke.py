#!/usr/bin/env python3
"""End-to-end model regression checks; all inputs/outputs live in a temp directory."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile


def book(path, body):
    with zipfile.ZipFile(path, 'w') as z:
        z.writestr('mimetype', 'application/epub+zip')
        z.writestr('META-INF/container.xml', '<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>')
        z.writestr('book.opf', '<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">smoke</dc:identifier><dc:title>Smoke test</dc:title><dc:language>en</dc:language></metadata><manifest><item id="c" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>')
        z.writestr('chapter.xhtml', '<html xmlns="http://www.w3.org/1999/xhtml"><body>' + body + '</body></html>')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=Path('target/debug/lector'))
    args = parser.parse_args()
    binary, assets = args.binary.resolve(), args.assets.resolve()

    def run(*argv, error=None):
        p = subprocess.run([str(binary), '--assets', str(assets), *map(str, argv)], capture_output=True, text=True)
        if error is None:
            assert p.returncode == 0, p.stderr
        else:
            assert p.returncode != 0 and error in p.stderr, (p.returncode, p.stderr)
        return p.stdout

    with tempfile.TemporaryDirectory(prefix='lector-smoke-') as temp:
        root = Path(temp)
        source, out = root / 'book.epub', root / 'out'
        attack = root / 'must-not-exist'
        paragraph = 'Lector reads a little story about a quiet garden. Birds gather beside the window, and the sun rises over the trees. '
        book(source, '<h1>Morning</h1><p>' + paragraph * 3 + '</p><h1>Evening</h1><p>' + paragraph + f'</p><ul><li>--phonout={attack}</li></ul>')
        dictionary = root / 'names.json'
        dictionary.write_text('{"Lector":"Lek tor"}')
        common = [source, '-o', out, '--no-cover', '-j', '2', '--pronunciations', dictionary, '--export-timings']
        run(*common)
        assert not attack.exists(), 'book text was interpreted as an option'
        cp = out / '.book.ckpt'
        plan = json.loads((cp / 'plan.json').read_text())
        assert len(plan['chapters']) == 2 and len(plan['units']) > 2
        pcm = cp / 'audio.s16'
        digest = hashlib.sha256(pcm.read_bytes()).hexdigest()
        words = json.loads((out / 'book.json').read_text())
        assert 'Lector' in [w['text'] for w in words], 'dictionary changed display text'
        assert all(0 <= w['start_s'] <= w['end_s'] for w in words)
        assert all(a['start_s'] <= b['start_s'] for a, b in zip(words, words[1:]))
        probe = json.loads(subprocess.check_output(['ffprobe', '-v', 'error', '-show_chapters', '-show_format', '-of', 'json', str(out / 'book.m4b')]))
        assert len(probe['chapters']) == 2
        duration = float(probe['format']['duration'])
        assert words[-1]['end_s'] <= duration + 0.1
        # Simulate a process killed between PCM and journal commit.
        with pcm.open('ab') as f:
            f.write(b'partial audio')
        with (cp / 'timed.jsonl').open('ab') as f:
            f.write(b'{"samples":')
        run(*common)
        assert hashlib.sha256(pcm.read_bytes()).hexdigest() == digest
        other = root / 'other'; other.mkdir()
        book(other / 'book.epub', '<h1>Another book</h1><p>Entirely different.</p>')
        run(other / 'book.epub', *common[1:], error='belongs to different input')
        # Re-encode from the completed cache, scaling all timelines.
        faster = root / 'faster'
        run('reencode', cp, '-o', faster, '--no-cover', '--speed', '1.5', '--export-timings')
        scaled = json.loads((faster / 'book.json').read_text())
        assert abs(scaled[-1]['end_s'] * 1.5 - words[-1]['end_s']) < 1e-8
        run('preview', source, '-o', out, '--words', '12', '--chapters', '2', '--no-cover', '-j', '2')
        preview = json.loads((out / '.book-preview.ckpt/plan.json').read_text())
        assert sum(len(u.split()) for u in preview['units']) <= 12
        missing = root / 'missing'; shutil.copytree(out, missing)
        (missing / '.book.ckpt/audio.s16').unlink()
        run(source, '-o', missing, *common[3:], error='audio missing')
        print(f'PASS: model synthesis ({duration:.1f}s, {len(words)} words), chapters, original spelling, option-injection protection, torn-write resume, input mismatch, missing audio, re-encode timing, chapter preview')


if __name__ == '__main__':
    main()
