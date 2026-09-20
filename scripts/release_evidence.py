"""Shared provenance for reproducible release evidence (standard library only)."""
import hashlib
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def provenance(binary):
    sources = sorted(set([*ROOT.glob('crates/**/*.rs'),
                          *ROOT.glob('crates/**/Cargo.toml'), ROOT / 'Cargo.toml',
                          ROOT / 'Cargo.lock']))
    digest = hashlib.sha256()
    for source in sources:
        digest.update(str(source.relative_to(ROOT)).encode() + b'\0' + source.read_bytes())
    version = subprocess.check_output([str(binary), '--no-config', '--version'], text=True).strip()
    if version != 'rich (rs-rich-cli) 0.0.8':
        raise RuntimeError(f'Expected 0.0.8 binary, got {version}')
    return {'version': version, 'binary_path': str(binary),
            'binary_sha256': sha256(binary), 'source_sha256': digest.hexdigest(),
            'source_digest_recipe': 'Sorted crates/**/*.rs, crates/**/Cargo.toml, Cargo.toml, Cargo.lock; relative path + NUL + bytes',
            'git_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
            'git_worktree_changes': subprocess.check_output(['git', 'status', '--short'], cwd=ROOT, text=True).splitlines()}


def verify_unchanged(binary, initial):
    current = provenance(binary)
    for key in ('binary_sha256', 'source_sha256'):
        if current[key] != initial[key]:
            raise RuntimeError(f'{key} changed while collecting evidence; rebuild and rerun')
