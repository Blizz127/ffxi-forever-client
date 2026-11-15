#!/usr/bin/env python3
"""Package a self-contained preview. No credentials or browser runtime included."""
import argparse
import hashlib
import json
from pathlib import Path
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
VERSION = '0.1.0-preview2'

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    files = []
    for folder in ['dist','LICENSES','tools']:
        files.extend(p for p in (ROOT / folder).rglob('*') if p.is_file() and '__pycache__' not in p.parts and p.suffix != '.pyc')
    files.extend(ROOT / p for p in ['start-companion.py','server.js','package.json','README.md','INTEGRATION.md','THIRD_PARTY_NOTICES.md','tests/domain.test.mjs','tests/browser-smoke.mjs'])
    files = sorted(set(files))
    manifest = {'version': VERSION, 'files': {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}}
    receipt = args.output / 'BUILD.json'
    receipt.write_text(json.dumps(manifest,indent=2)+'\n')
    archive = args.output / f'forever-companion-{VERSION}.tar.gz'
    with tarfile.open(archive,'w:gz') as target:
        for p in files: target.add(p, arcname=str(p.relative_to(ROOT)), recursive=False)
        target.add(receipt, arcname='BUILD.json')
    portable = args.output / f'forever-companion-{VERSION}.zip'
    with zipfile.ZipFile(portable,'w',zipfile.ZIP_DEFLATED) as target:
        for p in files: target.write(p,arcname=f'forever-companion/{p.relative_to(ROOT)}')
        target.write(receipt,arcname='forever-companion/BUILD.json')
    print(json.dumps({'version':VERSION,'archive':str(archive),'zip':str(portable),'files':len(files),'sha256':hashlib.sha256(archive.read_bytes()).hexdigest()},indent=2))

if __name__ == '__main__':
    main()
