#!/usr/bin/env python3
"""Capture real same-source CLI exports; SVG/PNG are rendered, never invented."""
import argparse
import json
import os
import subprocess
from pathlib import Path
from xml.etree import ElementTree as ET
import cairosvg
from capture_v9_demos import fixture
from capture_release_demos import capture
from release_evidence import ROOT, provenance, sha256, verify_unchanged


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/rich')
    args = parser.parse_args()
    binary = args.binary.resolve()
    manifest = provenance(binary, expected_version='0.0.9')
    output = ROOT / 'docs/media/expanded-v9'
    output.mkdir(parents=True, exist_ok=True)
    source = output / 'gradient.png'
    fixture(source)
    env = dict(os.environ, TERM='xterm-sixel', RICH_SIXEL='1')
    env.pop('NO_COLOR', None)
    cases = [
        ('ASCII', 'ascii', []), ('Half-block', 'blocks', []), ('Braille', 'braille', []),
        ('Rotate 90 + flip', 'blocks', ['--image-rotate', '90', '--image-flip-horizontal']),
        ('Grayscale', 'blocks', ['--image-grayscale']),
        ('ANSI256 Bayer 4x4', 'blocks', ['--image-color', 'ansi256', '--image-dither', 'bayer4x4']),
    ]
    panels, records = [], []
    for index, (label, mode, extra) in enumerate(cases):
        svg = output / f'image-{index}.svg'
        html = output / f'image-{index}.html'
        command = [str(binary), '--no-config', 'image', str(source), '--image-mode', mode,
                   '--width', '40', '--height', '14', '--image-fit', 'contain', *extra,
                   '--export-svg', str(svg), '--export-html', str(html)]
        result = subprocess.run(command, env=env, cwd=ROOT, capture_output=True, check=True)
        assert b'\x1bP' not in svg.read_bytes()
        records.append({'label': label, 'args': command[1:], 'exit_code': result.returncode,
                        'svg_sha256': sha256(svg), 'html_sha256': sha256(html)})
        panel = ET.fromstring(svg.read_text().replace("rich-cli", f"image-{index}"))
        panel.set('x', str((index % 3) * 520))
        panel.set('y', str((index // 3) * 430 + 35))
        panel.set('width', '510'); panel.set('height', '385')
        panels.append(panel)
    ns = 'http://www.w3.org/2000/svg'
    root = ET.Element(f'{{{ns}}}svg', {'width': '1560', 'height': '860', 'viewBox': '0 0 1560 860'})
    ET.SubElement(root, f'{{{ns}}}rect', {'width': '1560', 'height': '860', 'fill': '#161922'})
    for index, ((label, _, _), panel) in enumerate(zip(cases, panels)):
        title = ET.SubElement(root, f'{{{ns}}}text', {'x': str(index % 3 * 520 + 15), 'y': str(index // 3 * 430 + 29), 'fill': '#ffffff', 'font-family': 'sans-serif', 'font-size': '21'})
        title.text = label
        root.append(panel)
    comparison = ROOT / 'docs/media/cli-v9-image-transforms.svg'
    ET.register_namespace('', ns)
    ET.ElementTree(root).write(comparison, encoding='utf-8', xml_declaration=True)
    cairosvg.svg2png(url=str(comparison), write_to=str(comparison.with_suffix('.png')))
    subprocess.run(['cargo', 'run', '-q', '-p', 'rs-rich-ext', '--example', 'expanded_release', '--', str(output)], cwd=ROOT, check=True)
    diagnostic = output / 'cli-v9-diagnostics.svg'
    cairosvg.svg2png(url=str(diagnostic), write_to=str(diagnostic.with_suffix('.png')))
    sixel_args = ['image', str(source), '--image-mode', 'sixel', '--width', '40']
    _, raw, code = capture(binary, sixel_args, environment={'RICH_SIXEL': '1', 'TERM': 'xterm-sixel'})
    if code != 0 or b'\x1bP' not in raw:
        raise RuntimeError('Expected actual Sixel protocol output on the declared PTY')
    (output / 'same-source.sixel').write_bytes(raw)
    manifest['sixel'] = {'args': sixel_args, 'exit_code': code, 'sha256': sha256(output / 'same-source.sixel'), 'presentation': 'Raw PTY Sixel protocol; not a screenshot or claim about terminal-emulator support.'}
    manifest['captures'] = records
    manifest['presentation'] = 'Six actual SVG exports of one source; only labels and grid placement added. PNG rasterized by CairoSVG. Diagnostic uses the public example.'
    verify_unchanged(binary, manifest)
    (output / 'provenance.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print('Captured six same-source image exports and structured diagnostic/layout example.')


if __name__ == '__main__':
    main()
