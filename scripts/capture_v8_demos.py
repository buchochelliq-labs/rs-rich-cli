#!/usr/bin/env python3
"""Record v0.0.8 release-binary PTY workflows; requires docs-media dependencies.

Build with cargo build --release -p rs-rich-cli --locked before running.
Captions describe real commands; terminal content is exclusively recorded output.
"""
import argparse
import hashlib
import json
import tempfile
import zipfile
from pathlib import Path

from PIL import Image, ImageDraw
import capture_release_demos as media
from release_evidence import ROOT, provenance, sha256, verify_unchanged

EVIDENCE = ROOT / '.github/evidence/0.0.8'


def fixture(path):
    image = Image.new('RGB', (180, 120), '#201e36')
    draw = ImageDraw.Draw(image)
    colors = ['#ff5f56', '#f5bc42', '#51b882', '#528bea', '#bf70d9', '#ed87ad', '#f78941', '#78cede', '#d0de69']
    for y in range(3):
        for x in range(3):
            draw.rectangle((x*60, y*40, x*60+57, y*40+37), fill=colors[y*3+x])
            draw.ellipse((x*60+5, y*40+5, x*60+12+8*x, y*40+12+6*y), fill='white')
            draw.text((x*60+37, y*40+25), str(y*3+x+1), fill='#111111')
    image.save(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/rich')
    args = parser.parse_args()
    binary = args.binary.resolve()
    manifest = provenance(binary)
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    media.OUT.mkdir(parents=True, exist_ok=True)
    source = EVIDENCE / 'asymmetric.png'
    fixture(source)
    config = EVIDENCE / 'demo.toml'
    config.write_text('version = 1\n\n[profiles.preview]\nwidth = 48\npager = false\n\n[defaults]\nwidth = 80\npager = true\nno_color = true\n', encoding='utf-8')
    results, recordings = [], {}

    def record(name, command, label, no_config=True):
        events, raw, code = media.capture(binary, command, no_config=no_config)
        recordings[name + '.ansi'] = raw
        recordings[name + '.cast'] = (json.dumps({'version': 2, 'width': media.WIDTH, 'height': media.HEIGHT, 'title': name}) + '\n' + ''.join(json.dumps(e) + '\n' for e in events)).encode()
        results.append({'case': name, 'args': (['--no-config'] if no_config else []) + command,
                        'exit_code': code, 'stdout_stderr_sha256': hashlib.sha256(raw).hexdigest(), 'bytes': len(raw)})
        return media.replay(events, label)

    anchors = ['top-left', 'top', 'top-right', 'left', 'center', 'right', 'bottom-left', 'bottom', 'bottom-right']
    crops = []
    for shape, width, height in [('wide', '60', '12'), ('tall', '20', '22')]:
        for anchor in anchors:
            command = ['image', str(source.relative_to(ROOT)), '--image-mode', 'blocks', '--image-fit', 'cover', '--image-anchor', anchor, '--width', width, '--height', height]
            crops.extend(record(f'{shape}-{anchor}', command, f'cover: --image-anchor {anchor} --width {width} --height {height}'))
    batch = []
    with tempfile.TemporaryDirectory(prefix='rich-v8-demo-') as directory:
        dest = Path(directory)
        command = ['json', '--batch', 'scripts/fixtures/workflows/a.json', 'scripts/fixtures/workflows/b.json', '--export-html', str(dest / 'document.html'), '--jobs', '2', '--width', '60']
        batch.extend(record('batch-plan', [*command, '--dry-run'], 'rich json --batch a.json b.json --jobs 2 --export-html … --dry-run'))
        if list(dest.iterdir()):
            raise RuntimeError('dry-run wrote output files')
        batch.extend(record('batch-export', [*command, '--report', 'json'], 'rich json --batch a.json b.json --jobs 2 --export-html … --report json'))
        if sorted(p.name for p in dest.iterdir()) != ['a.html', 'b.html']:
            raise RuntimeError('batch export files missing')
        exports = {}
        for path in sorted(dest.iterdir()):
            recordings['exports/' + path.name] = path.read_bytes()
            exports[path.name] = sha256(path)
    configs = []
    for name, suffix in [('config-validate', ['validate']), ('config-defaults', ['show']), ('config-profile', ['show', '--profile', 'preview']), ('config-overrides', ['show', '--profile', 'preview', '--width', '64', '--color', '--no-pager'])]:
        command = ['config', *suffix, '--config', str(config.relative_to(ROOT))]
        configs.extend(record(name, command, 'rich config ' + ' '.join(suffix), no_config=False))
    for name, frames in [('v8-crop-anchors', crops), ('v8-batch', batch), ('v8-config', configs)]:
        media.encode(frames, name)
        stills = [frames[i].resize((880, 528)) for i in range(0, len(frames), 2*media.FPS)]
        stills[0].save(media.OUT / (name + '.gif'), save_all=True, append_images=stills[1:], duration=2000, loop=0)
    with zipfile.ZipFile(EVIDENCE / 'recordings.zip', 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data in recordings.items():
            archive.writestr(name, data)
    manifest.update({'cases': results, 'fixtures': {p.name: sha256(p) for p in [source, config]},
                     'exports': exports, 'dry_run_no_writes_verified': True,
                     'terminal': {'width': media.WIDTH, 'height': media.HEIGHT, 'TERM': 'xterm-256color', 'COLORTERM': 'truecolor', 'RICH_SIXEL': '0'},
                     'presentation': 'Actual PTY bytes replayed with command captions; completed results held 2 seconds; 10 fps; no audio. Nine anchors shown in both wide and tall cover viewports because cover crops only one axis at a time.',
                     'media': {p.name: sha256(p) for p in sorted(media.OUT.glob('v8-*'))}})
    verify_unchanged(binary, manifest)
    (EVIDENCE / 'demos.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps({'cases': len(results), 'binary_sha256': manifest['binary_sha256']}))


if __name__ == '__main__':
    main()
