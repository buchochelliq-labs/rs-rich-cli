#!/usr/bin/env python3
"""Measure serial/jobs-2/jobs-4 file exports from deterministic JSON inputs.

Includes spawning, rendering, output serialization and disk writes. Each invocation
uses a fresh directory; validates reports and exact exported bytes after timing.
No terminal rendering or Python CLI comparison. Does not promise a speedup.
"""
import argparse
import json
import os
import platform
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

from release_evidence import ROOT, provenance, sha256, verify_unchanged


def positive(value):
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError('must be positive')
    return number


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/rich')
    parser.add_argument('--runs', type=positive, default=5)
    parser.add_argument('--warmup', type=positive, default=1)
    parser.add_argument('--files', type=positive, default=12)
    parser.add_argument('--rows', type=positive, default=120)
    parser.add_argument('--json-out', type=Path, default=ROOT / '.github/evidence/0.0.8/benchmark.json')
    args = parser.parse_args()
    binary = args.binary.resolve()
    result = provenance(binary)
    result.update({'platform': platform.platform(), 'cpu_count': os.cpu_count(),
                   'runs': args.runs, 'warmup': args.warmup, 'files': args.files, 'rows_per_file': args.rows,
                   'width': 100, 'units': 'milliseconds',
                   'method': 'One batch CLI process per run; stdout /dev/null, stderr captured. Includes process spawn, rendering and file writes. Fresh output directory each run; verifies success envelope and exact HTML hashes after timing. Jobs order rotates between rounds. No cache flushing; warm filesystem caches. Shared host timings are descriptive, not a speed guarantee.',
                   'generator': 'JSON list for i in range(rows): id=file_index*rows+i, name=item-{file_index}-{i}, active=i%3==0, score=i*1.5, tags=[t{i%7},u{i%11}]; json.dumps(indent=2) + newline'})
    env = dict(os.environ, TERM='dumb')
    env.pop('NO_COLOR', None)
    samples = {jobs: [] for jobs in (1, 2, 4)}
    invocations = []
    with tempfile.TemporaryDirectory(prefix='rich-v8-bench-') as directory:
        base = Path(directory)
        inputs = base / 'inputs'
        inputs.mkdir()
        for n in range(args.files):
            payload = [{'id': n*args.rows+i, 'name': f'item-{n}-{i}', 'active': i % 3 == 0,
                        'score': i*1.5, 'tags': [f't{i%7}', f'u{i%11}']} for i in range(args.rows)]
            (inputs / f'input-{n:02}.json').write_text(json.dumps(payload, indent=2) + '\n', encoding='utf-8')
        paths = sorted(inputs.iterdir())
        result['input_sha256'] = {p.name: sha256(p) for p in paths}
        expected_hashes = None
        for round_index in range(args.warmup + args.runs):
            jobs_order = [1, 2, 4]
            rotation = round_index % 3
            jobs_order = jobs_order[rotation:] + jobs_order[:rotation]
            for jobs in jobs_order:
                destination = base / f'round-{round_index}-jobs-{jobs}'
                destination.mkdir()
                command = ['--no-config', 'json', '--batch', *map(str, paths), '--export-html', str(destination / 'document.html'), '--jobs', str(jobs), '--width', '100', '--report', 'json']
                started = time.perf_counter()
                process = subprocess.run([str(binary), *command], cwd=ROOT, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
                elapsed = (time.perf_counter() - started)*1000
                if process.returncode:
                    raise RuntimeError(process.stderr.decode(errors='replace'))
                report = json.loads(process.stderr)
                if not report.get('ok'):
                    raise RuntimeError(report)
                outputs = sorted(destination.glob('*.html'))
                hashes = {p.name: sha256(p) for p in outputs}
                if len(outputs) != args.files or any(p.stat().st_size < 200 for p in outputs):
                    raise RuntimeError('missing or empty HTML export')
                if expected_hashes is None:
                    expected_hashes = hashes
                elif hashes != expected_hashes:
                    raise RuntimeError('serial/parallel exported bytes differ')
                warmup = round_index < args.warmup
                invocations.append({'round': round_index, 'warmup': warmup, 'jobs': jobs,
                                    'args': command, 'exit_code': process.returncode, 'elapsed_ms': elapsed,
                                    'report': report})
                if not warmup:
                    samples[jobs].append(elapsed)
        result['output_sha256'] = expected_hashes
    result['invocations'] = invocations
    result['results'] = [{'jobs': jobs, 'samples_ms': times, 'median_ms': statistics.median(times),
                          'min_ms': min(times), 'max_ms': max(times)} for jobs, times in samples.items()]
    verify_unchanged(binary, result)
    args.json_out.parent.mkdir(parents=True, exist_ok=True)
    args.json_out.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print('| Jobs | Median | Minimum | Maximum |')
    print('|---:|---:|---:|---:|')
    for row in result['results']:
        print(f"| {row['jobs']} | {row['median_ms']:.1f} ms | {row['min_ms']:.1f} ms | {row['max_ms']:.1f} ms |")


if __name__ == '__main__':
    main()
