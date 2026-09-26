#!/usr/bin/env python3
"""Record the built guided tour at half-second pacing, including real animation."""
import argparse
import json
import math
import pyte
from pathlib import Path
import capture_release_demos as media
from release_evidence import ROOT, provenance, sha256, verify_unchanged
from test_demo_pty import capture

# Release -> media name; evidence goes to .github/evidence/<release>/demo-tour.
# 0.0.8 keeps its original names.
RECORDINGS = {'0.0.8': 'v8-demo-tour', '0.0.10': 'v10-demo-tour', '0.0.11': 'v11-demo-tour',
              '0.0.12': 'v12-demo-tour'}

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True)
parser.add_argument('--release', choices=sorted(RECORDINGS), default='0.0.8',
                    help='binary version to expect and recording to write (default 0.0.8)')
args = parser.parse_args()
binary = args.binary.resolve()
name = RECORDINGS[args.release]
manifest = provenance(binary, expected_version=args.release)
evidence = ROOT / f'.github/evidence/{args.release}/demo-tour'
evidence.mkdir(parents=True, exist_ok=True)
events, raw = capture(binary, delay='.5')
(evidence / 'tour.ansi').write_bytes(raw)
(evidence / 'tour.cast').write_text(json.dumps({'version': 2, 'width': 88, 'height': 28}) + '\n' + ''.join(json.dumps(event) + '\n' for event in events))
screen = pyte.Screen(media.WIDTH, media.HEIGHT)
stream = pyte.Stream(screen)
frames, index = [], 0
for tick in range(math.ceil((events[-1][0] + 1) * media.FPS)):
    while index < len(events) and events[index][0] <= tick / media.FPS:
        stream.feed(events[index][2])
        index += 1
    frames.append(media.frame(screen, 'rich --demo --demo-delay 0.5 | actual CLI tour'))
assert index == len(events), 'recording must include the entire tour' 
media.encode(frames, name)
poster_time = next(event[0] for event in events if 'Half-block' in event[2]) + .25
frames[min(len(frames) - 1, round(poster_time * media.FPS))].save(media.OUT / f'{name}.png')
# One frame per second keeps the lightweight GIF representative of the recording.
stills = [frame.resize((880, 528)) for frame in frames[::media.FPS]]
stills[0].save(media.OUT / f'{name}.gif', save_all=True, append_images=stills[1:], duration=1000, loop=0)
verify_unchanged(binary, manifest)
manifest.update({'args': ['--demo', '--demo-delay', '0.5'], 'exit_code': 0,
                 'terminal': {'width': 88, 'height': 28, 'RICH_SIXEL': '1'},
                 'recording_sha256': sha256(evidence / 'tour.cast'),
                 'presentation': 'Actual 88x28 PTY playback at half-second section pacing; captions added, no audio.',
                 'media': {p.name: sha256(p) for p in media.OUT.glob(f'{name}.*')}})
(evidence / 'capture.json').write_text(json.dumps(manifest, indent=2) + '\n')
print('Recorded guided tour, GIF, MP4 and source/binary provenance')
