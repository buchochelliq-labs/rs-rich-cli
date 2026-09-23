#!/usr/bin/env python3
"""Capture 0.0.10 image modes and adjustments from one source; nothing is invented.

Every panel is the binary's own SVG export of the same gradient fixture; this
script only adds labels and grid placement, then rasterizes with CairoSVG.

    cargo build --release -p rs-rich-cli
    python scripts/capture_image_modes_010.py --binary target/release/rich
"""
import argparse
import json
import os
import subprocess
from pathlib import Path
from xml.etree import ElementTree as ET

import cairosvg
from capture_v9_demos import fixture
from release_evidence import ROOT, provenance, sha256, verify_unchanged

CASES = [
    ('Half-block (truecolor)', 'blocks', []),
    ('Quadrants (truecolor)', 'quadrants', []),
    ('Quadrants, ANSI16 + Floyd-Steinberg', 'quadrants',
     ['--image-color', 'ansi16', '--image-dither', 'floyd-steinberg']),
    ('Half-block, ANSI16', 'blocks', ['--image-color', 'ansi16']),
    ('Half-block, grayscale + Bayer 4x4', 'blocks',
     ['--image-color', 'grayscale', '--image-dither', 'bayer4x4']),
    ('Brightness 1.3, contrast 1.6, gamma 0.7', 'quadrants',
     ['--image-brightness', '1.3', '--image-contrast', '1.6', '--image-gamma', '0.7']),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/rich')
    args = parser.parse_args()
    binary = args.binary.resolve()
    manifest = provenance(binary, expected_version='0.0.10')
    output = ROOT / 'docs/media/image-modes-010'
    output.mkdir(parents=True, exist_ok=True)
    source = output / 'gradient.png'
    fixture(source)
    env = dict(os.environ)
    env.pop('NO_COLOR', None)
    panels, records = [], []
    for index, (label, mode, extra) in enumerate(CASES):
        svg = output / f'image-{index}.svg'
        command = [str(binary), '--no-config', 'image', str(source), '--image-mode', mode,
                   '--width', '40', '--height', '14', '--image-fit', 'contain', *extra,
                   '--export-svg', str(svg)]
        result = subprocess.run(command, env=env, cwd=ROOT, capture_output=True, check=True)
        records.append({'label': label, 'args': command[1:], 'exit_code': result.returncode,
                        'svg_sha256': sha256(svg)})
        panel = ET.fromstring(svg.read_text().replace('rich-cli', f'image-{index}'))
        panel.set('x', str((index % 3) * 520))
        panel.set('y', str((index // 3) * 430 + 35))
        panel.set('width', '510')
        panel.set('height', '385')
        panels.append(panel)
    ns = 'http://www.w3.org/2000/svg'
    root = ET.Element(f'{{{ns}}}svg', {'width': '1560', 'height': '860', 'viewBox': '0 0 1560 860'})
    ET.SubElement(root, f'{{{ns}}}rect', {'width': '1560', 'height': '860', 'fill': '#161922'})
    for index, ((label, _, _), panel) in enumerate(zip(CASES, panels)):
        title = ET.SubElement(root, f'{{{ns}}}text', {
            'x': str(index % 3 * 520 + 15), 'y': str(index // 3 * 430 + 29), 'fill': '#ffffff',
            'font-family': 'sans-serif', 'font-size': '21'})
        title.text = label
        root.append(panel)
    comparison = ROOT / 'docs/media/cli-010-image-modes.svg'
    ET.register_namespace('', ns)
    ET.ElementTree(root).write(comparison, encoding='utf-8', xml_declaration=True)
    cairosvg.svg2png(url=str(comparison), write_to=str(comparison.with_suffix('.png')))
    manifest['captures'] = records
    manifest['presentation'] = ('Six actual SVG exports of one source; only labels and grid '
                                'placement added. PNG rasterized by CairoSVG.')
    verify_unchanged(binary, manifest)
    (output / 'provenance.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'Captured {len(CASES)} same-source image exports into {comparison}')


if __name__ == '__main__':
    main()
