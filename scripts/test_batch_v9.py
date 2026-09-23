#!/usr/bin/env python3
"""Bounded Linux batch signal and PTY contracts; accepts a built rich binary."""
import errno
import json
import os
from pathlib import Path
import pty
import signal
import subprocess
import sys
import tempfile
import time

BINARY = str(Path(sys.argv[1]).resolve())


def children(pid):
    rows = subprocess.check_output(['ps', '-eo', 'pid=,ppid='], text=True)
    return [int(child) for row in rows.splitlines() for child, parent in [row.split()]
            if int(parent) == pid]


def cancellation(root, jobs):
    for name in ('a', 'b', 'c'):
        (root / f'{name}.txt').write_text(name * (2 * 1024 * 1024))
    proc = subprocess.Popen([BINARY, '--no-config', '--batch', '--jobs', str(jobs),
                             '--report', 'json', '--overwrite', '--export-html', 'out.html',
                             'a.txt', 'b.txt', 'c.txt'], cwd=root,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    workers = []
    try:
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            workers = children(proc.pid)
            for pid in workers:
                os.kill(pid, signal.SIGSTOP)
            if len(workers) == jobs:
                break
            assert proc.poll() is None, proc.communicate()
            time.sleep(.01)
        assert len(workers) == jobs, f'expected {jobs} active workers, got {workers}'
        proc.send_signal(signal.SIGINT)
        stdout, stderr = proc.communicate(timeout=5)
        assert proc.returncode == 130, (proc.returncode, stderr)
        report = json.loads(stderr)
        assert report['code'] == 'interrupted', report
        assert report['exit_code'] == 130 and report['ok'] is False, report
        result = report['result']
        assert result['attempted'] == jobs and result['skipped'] == 3 - jobs, result
        assert result['interrupted'] == jobs and result['completed'] == 0, result
        assert not stdout, stdout
        assert all(not Path(f'/proc/{pid}').exists() for pid in workers), workers
        assert all((root / f'{name}.txt').read_text() == name * (2 * 1024 * 1024) for name in ('a', 'b', 'c'))
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()
        for pid in workers:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass


def planning_cancellation(root):
    # Collision planning compares input aliases before any worker can write.
    # Enough files keep planning active without large fixtures or long sleeps.
    for index in range(1200):
        (root / f'{index:04}.txt').write_text('keep')
    proc = subprocess.Popen([BINARY, '--no-config', '--batch', '--dry-run',
                             '--report', 'json', '--export-html', 'out.html', '.'],
                            cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            status = Path(f'/proc/{proc.pid}/status').read_text()
            caught = next(line.split()[1] for line in status.splitlines()
                          if line.startswith('SigCgt:'))
            if int(caught, 16) & (1 << (signal.SIGINT - 1)):
                break
            assert proc.poll() is None, proc.communicate()
            time.sleep(.001)
        else:
            raise AssertionError('SIGINT handler was not installed')
        proc.send_signal(signal.SIGINT)
        stdout, stderr = proc.communicate(timeout=5)
        report = json.loads(stderr)
        assert proc.returncode == 130 and report['code'] == 'interrupted', report
        assert report['result']['attempted'] == 0 and not stdout, report
        assert not list(root.glob('*.html'))
        assert all(path.read_text() == 'keep' for path in root.glob('*.txt'))
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()


def replay_cancellation(root):
    (root / 'input.txt').write_text('rendered content\n' * 20000)
    proc = subprocess.Popen([BINARY, '--no-config', '--batch', '--report', 'json',
                             '--no-color', 'input.txt'], cwd=root,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    try:
        # One byte proves replay started; leaving the pipe undrained blocks replay.
        import select
        assert select.select([proc.stdout], [], [], 10)[0], 'replay never started'
        assert os.read(proc.stdout.fileno(), 1)
        proc.send_signal(signal.SIGINT)
        proc.wait(timeout=5)
        report = json.loads(proc.stderr.read())
        assert proc.returncode == 130 and report['code'] == 'interrupted', report
    finally:
        if proc.poll() is None:
            proc.kill()
            proc.wait()


def progress(root, extra, terminal):
    (root / 'input.txt').write_text('rendered content\n')
    args = [BINARY, '--no-config', '--batch', '--no-color', 'input.txt', *extra]
    if not terminal:
        return subprocess.run(args, cwd=root, capture_output=True, timeout=5)
    master, slave = pty.openpty()
    try:
        proc = subprocess.Popen(args, cwd=root, stdout=subprocess.PIPE, stderr=slave)
        os.close(slave)
        slave = -1
        stdout, _ = proc.communicate(timeout=5)
        stderr = b''
        while True:
            try:
                chunk = os.read(master, 65536)
                if not chunk:
                    break
                stderr += chunk
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                break
        return subprocess.CompletedProcess(args, proc.returncode, stdout, stderr)
    finally:
        os.close(master)
        if slave >= 0:
            os.close(slave)


with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp)
    for jobs in (2, 1):
        case = root / f'cancel-{jobs}'
        case.mkdir()
        cancellation(case, jobs)
    case = root / 'planning'
    case.mkdir()
    planning_cancellation(case)
    case = root / 'replay'
    case.mkdir()
    replay_cancellation(case)
    case = root / 'progress'
    case.mkdir()
    for extra, terminal, expected in [([], True, True), (['--progress'], True, True),
                                      (['--no-progress'], True, False),
                                      ([], False, False), (['--progress'], False, False),
                                      (['--dry-run'], True, False),
                                      (['--report', 'json'], True, False)]:
        result = progress(case, extra, terminal)
        assert result.returncode == 0, result
        assert (b'Batch:' in result.stderr) == expected, result
        if expected:
            assert b'1 completed, 0 failed, 1 total' in result.stderr, result
        if '--report' in extra:
            assert json.loads(result.stderr)['ok'] is True
        if '--dry-run' not in extra:
            assert b'rendered content' in result.stdout
    failure = progress(case, ['--json'], True)
    assert failure.returncode == 4, failure
    assert b'0 completed, 1 failed, 1 total' in failure.stderr, failure
print('batch cancellation (serial/parallel), blocked replay, and progress gating passed')
