#!/usr/bin/env python3
"""Check locked Rust and Python export packages against OSV (requires Python 3.11+)."""
import json
from pathlib import Path
import sys
import tomllib
import urllib.request


def main():
    lock = tomllib.loads(Path(__file__).resolve().parents[1].joinpath('Cargo.lock').read_text())
    packages = [dict(p, ecosystem='crates.io') for p in lock['package'] if p.get('source', '').startswith('registry+')]
    exporter = tomllib.loads(Path(__file__).with_name('export_dur.py.lock').read_text())
    packages += [dict(p, ecosystem='PyPI') for p in exporter['package'] if 'registry' in p.get('source', {})]
    payload = {'queries': [
        {'package': {'name': p['name'], 'ecosystem': p['ecosystem']}, 'version': p['version']}
        for p in packages
    ]}
    request = urllib.request.Request('https://api.osv.dev/v1/querybatch',
                                    data=json.dumps(payload).encode(),
                                    headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=60) as response:
        results = json.load(response)['results']
    if len(results) != len(packages):
        raise RuntimeError('incomplete OSV response')
    findings = 0
    for package, result in zip(packages, results):
        if result.get('next_page_token'):
            raise RuntimeError('OSV results require pagination; run a full scanner')
        for advisory in result.get('vulns', []):
            findings += 1
            print(f"{package['ecosystem']} {package['name']} {package['version']}: https://osv.dev/vulnerability/{advisory['id']}")
    print(f'{len(packages)} locked registry packages checked; {findings} advisory matches')
    return bool(findings)


if __name__ == '__main__':
    sys.exit(main())
