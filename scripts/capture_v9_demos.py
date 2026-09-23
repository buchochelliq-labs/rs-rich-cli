#!/usr/bin/env python3
"""Capture real CLI 0.0.9 release output as a compact 15-second workflow tour.

Requires the docs-media Python dependencies and FFmpeg. Build the release binary
first. The five scenes hold actual PTY output for three seconds each; the image
scene places two independently captured outputs side by side. No output is drawn
or invented. Raw ANSI/asciicast recordings and provenance accompany the media.
"""
import argparse
import hashlib
import json
import math
import tempfile
import zipfile
from pathlib import Path

from PIL import Image
import capture_release_demos as media
from release_evidence import ROOT, provenance, sha256, verify_unchanged

EVIDENCE = ROOT / '.github/evidence/0.0.9'
NAME = 'v9-workflows'


def fixture(path):
    """Smooth RGB gradients make palette banding and diffusion visible."""
    image = Image.new('RGB', (160, 96))
    for y in range(image.height):
        for x in range(image.width):
            wave = (math.sin(x / 25 + y / 19) + 1) / 2
            image.putpixel((x, y), (round(255*x/159), round(255*y/95), round(255*wave)))
    image.save(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/rich')
    args = parser.parse_args()
    binary = args.binary.resolve()
    manifest = provenance(binary, expected_version='0.0.9')
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    media.OUT.mkdir(parents=True, exist_ok=True)
    source = EVIDENCE / 'gradient.png'
    fixture(source)
    config = EVIDENCE / 'theme.toml'
    config.write_text('[defaults]\ntheme = "release"\npager = false\n\n[themes.release]\n'
                      'title = "bold #bba0ff"\nsuccess = "bold #55dda0"\n'
                      'detail = "#80c7ff"\n', encoding='utf-8')
    recordings, results = {}, []

    def record(name, command, label, *, no_config=True):
        events, raw, code = media.capture(binary, command, no_config=no_config)
        if code != 0:
            raise RuntimeError(f'{name}: capture exit code must be zero, got {code}')
        recordings[name + '.ansi'] = raw
        header = {'version': 2, 'width': media.WIDTH, 'height': media.HEIGHT,
                  'title': name, 'command': 'rich ' + ' '.join(command)}
        recordings[name + '.cast'] = (json.dumps(header) + '\n' + ''.join(
            json.dumps(event) + '\n' for event in events)).encode()
        results.append({'case': name, 'args': (['--no-config'] if no_config else []) + command,
                        'exit_code': code, 'stdout_stderr_sha256': hashlib.sha256(raw).hexdigest(),
                        'bytes': len(raw), 'recorded_duration_seconds': events[-1][0] if events else 0})
        return media.replay(events, label)[-1], raw

    images = []
    image_args = ['image', str(source.relative_to(ROOT)), '--image-mode', 'blocks',
                  '--image-fit', 'contain', '--width', '40', '--height', '20']
    record('truecolor', [*image_args, '--image-color', 'truecolor'], 'Truecolor reference')
    for name, dither, label in [('ansi256', 'none', 'ANSI256: no dither'),
                              ('dither', 'floyd-steinberg', 'ANSI256: Floyd-Steinberg')]:
        still, raw = record(name, [*image_args, '--image-color', 'ansi256', '--image-dither', dither], label)
        if b'38;5;' not in raw or b'38;2;' in raw:
            raise RuntimeError(f'{name}: expected indexed ANSI output')
        images.append(still.crop((0, 0, 550, 660)))
    if recordings['ansi256.ansi'] == recordings['dither.ansi']:
        raise RuntimeError('Gradient fixture did not demonstrate dithering')
    comparison = Image.new('RGB', (1100, 660), media.BG)
    for column, image in enumerate(images):
        comparison.paste(image, (column * 550, 0))

    theme_text = ('[title]CLI 0.0.9 — named themes[/title]\n\n'
                  '[success]Ready to render[/success]\n'
                  '[detail]Styles come from [themes.release] in theme.toml[/detail]\n\n'
                  '[success]The same named styles travel to batch workers.[/success]')
    # Escape literal brackets so the markup parser does not interpret TOML syntax.
    theme_text = theme_text.replace('[themes.release]', r'\[themes.release]')
    theme, _ = record('theme', ['--config', str(config.relative_to(ROOT)), '--theme', 'release',
                              '--print', theme_text, '--width', '80'],
                      'rich --config theme.toml --theme release --print ...', no_config=False)
    doctor, _ = record('doctor', ['doctor'], 'rich doctor  |  detected versus inferred capabilities')
    listing, raw = record('demo-list', ['--demo-list'], 'rich --demo-list  |  select core, workflows, or art')
    if not all(section in raw for section in [b'core', b'workflows', b'art']):
        raise RuntimeError('Demo listing is incomplete')
    with tempfile.TemporaryDirectory(prefix='rich-v9-demo-') as directory:
        dest = Path(directory)
        batch, raw = record('batch-progress', ['json', '--batch', 'scripts/fixtures/workflows/a.json',
                            'scripts/fixtures/workflows/b.json', '--jobs', '2', '--progress',
                            '--export-html', str(dest / 'document.html'), '--width', '60'],
                            'rich json --batch a.json b.json --jobs 2 --progress --export-html ...')
        if b'2 completed, 0 failed, 2 total' not in raw:
            raise RuntimeError('Missing final interactive batch progress')
        if sorted(path.name for path in dest.iterdir()) != ['a.html', 'b.html']:
            raise RuntimeError('Expected two batch exports')
        exports = {}
        for path in sorted(dest.iterdir()):
            recordings['exports/' + path.name] = path.read_bytes()
            exports[path.name] = sha256(path)

    scenes = [comparison, theme, doctor, listing, batch]
    media.encode([scene for scene in scenes for _ in range(3 * media.FPS)], NAME)
    # A real side-by-side image comparison also serves as the static poster.
    comparison.save(media.OUT / f'{NAME}.png', optimize=True)
    previews = [scene.resize((880, 528)) for scene in scenes]
    previews[0].save(media.OUT / f'{NAME}.gif', save_all=True, append_images=previews[1:],
                     duration=3000, loop=0, optimize=True)
    with zipfile.ZipFile(EVIDENCE / 'recordings.zip', 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data in recordings.items():
            archive.writestr(name, data)
    manifest.update({'cases': results, 'fixtures': {path.name: sha256(path) for path in [source, config]},
                     'exports': exports, 'capture_exit_codes_verified_zero': True,
                     'recordings_sha256': sha256(EVIDENCE / 'recordings.zip'),
                     'terminal': {'width': media.WIDTH, 'height': media.HEIGHT, 'TERM': 'xterm-256color',
                                  'COLORTERM': 'truecolor', 'RICH_SIXEL': '0', 'NO_COLOR': 'unset'},
                     'presentation': '15 seconds: five completed PTY results held 3 seconds each; first scene compares two cropped captures side by side. Captions added; no audio; MP4 10 fps; GIF 5 frames.',
                     'media': {path.name: sha256(path) for path in sorted(media.OUT.glob(NAME + '.*'))}})
    verify_unchanged(binary, manifest)
    (EVIDENCE / 'demos.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'cases': len(results), 'binary_sha256': manifest['binary_sha256'],
                      'media_bytes': sum(path.stat().st_size for path in media.OUT.glob(NAME + '.*'))}))


if __name__ == '__main__':
    main()
