"""Rehearse a release with the installed release-kit, changing nothing.

    python3 scripts/release_rehearsal.py [--no-fetch]

Runs, from the committed HEAD, what a release would run before anything is
pushed for real:

1. `release.py check` (release-kit.yml matches every manifest) and
   `release.py vendored` (the installed copy under .github/release-kit/ is
   unmodified);
2. `release.py plan` over every package of release-kit.yml, as if all were
   due: the build jobs a release run would start;
3. `release.py tag --dry-run` against a throwaway remote that holds HEAD as
   its default branch and every tag of `origin`, so the kit's tag checks
   (records, collisions, split releases, the tags from before the kit) run
   against the real tags without pushing anything anywhere.

Needs Python 3.11+, git, PyYAML and network access (`tag` asks crates.io and
PyPI what is published). CI runs it in `release-tests`; see docs/BRANCHING.md.
Uncommitted changes are not rehearsed: `tag` releases a commit.
"""

import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
KIT = ROOT / ".github/release-kit/scripts/release.py"


def kit_module():
    """The installed kit's release.py, under a name that does not shadow
    scripts/release.py."""
    spec = importlib.util.spec_from_file_location("release_kit", KIT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def full_selection(kit, read):
    """The gate's selection (`gate`'s `selection` output) if every package of
    release-kit.yml were due."""
    release = kit.load(read)
    return json.dumps([{"name": p.name, "registry": p.registry, "version": p.version,
                        "manifest": p.manifest, "tag": p.tag, "slug": p.slug} for p in release.packages])


def git(*args, cwd=ROOT, capture=True):
    result = subprocess.run(["git", *args], cwd=cwd, check=True, text=True, capture_output=capture)
    return (result.stdout or "").strip()


def run_kit(*args, cwd=ROOT):
    print(f"$ release.py {' '.join(args)}", flush=True)
    subprocess.run([sys.executable, str(cwd / ".github/release-kit/scripts/release.py"), *args],
                   cwd=cwd, check=True)


def fetch_tags():
    """origin's tags, with their commits (only those commits in a shallow
    clone, as CI's is)."""
    shallow = git("rev-parse", "--is-shallow-repository") == "true"
    git("fetch", "--quiet", "--force", *(["--depth=1"] if shallow else []), "origin",
        "+refs/tags/*:refs/tags/*", capture=False)


def tag_dry_run(head):
    """`tag --dry-run` from a clone of HEAD, against a bare remote holding
    HEAD as main and this repository's tags. Nothing reaches `origin`."""
    with tempfile.TemporaryDirectory(prefix="release-rehearsal-") as scratch:
        scratch = Path(scratch)
        remote, work = scratch / "remote.git", scratch / "work"
        common = Path(git("rev-parse", "--path-format=absolute", "--git-common-dir"))
        git("init", "--quiet", "--bare", str(remote))
        (remote / "objects/info/alternates").write_text(str(common / "objects") + "\n", encoding="utf-8")
        git("config", "receive.shallowUpdate", "true", cwd=remote)
        git("clone", "--quiet", "--shared", "--no-checkout", str(ROOT), str(work), cwd=scratch)
        git("checkout", "--quiet", "--detach", head, cwd=work)
        git("remote", "add", "rehearsal", str(remote), cwd=work)
        git("push", "--quiet", "rehearsal", f"{head}:refs/heads/main", cwd=work)
        git("push", "--quiet", "rehearsal", "refs/tags/*:refs/tags/*", cwd=work)
        git("fetch", "--quiet", "rehearsal", cwd=work)
        run_kit("tag", "--remote", "rehearsal", "--dry-run", cwd=work)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--no-fetch", action="store_true", help="use the local tags; do not fetch origin's")
    args = parser.parse_args(argv)
    os.chdir(ROOT)
    if git("status", "--porcelain", "--untracked-files=no", "--", "release-kit.yml", ".github/release-kit"):
        print("note: release-kit.yml or the kit has uncommitted changes; `plan` and `tag` rehearse HEAD",
              file=sys.stderr)
    head = git("rev-parse", "HEAD")
    run_kit("check")
    run_kit("vendored")
    kit = kit_module()
    selection = full_selection(kit, kit.files_at(head))
    with tempfile.TemporaryDirectory() as scratch:
        output = Path(scratch) / "plan"
        print("$ release.py plan --selection <every package>", flush=True)
        subprocess.run([sys.executable, str(KIT), "plan", "--selection", selection], cwd=ROOT, check=True,
                       env={**os.environ, "GITHUB_OUTPUT": str(output)})
        jobs = output.read_text(encoding="utf-8")
    if "count=0" in jobs.splitlines():
        sys.exit("release-kit.yml plans no build job: the PyPI package's wheels are missing")
    if not args.no_fetch:
        fetch_tags()
    tag_dry_run(head)


if __name__ == "__main__":
    main()
