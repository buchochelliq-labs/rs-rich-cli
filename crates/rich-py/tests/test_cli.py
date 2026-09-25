"""``python -m rs_rich`` is the ``rich`` binary, byte for byte.

Every case runs one command line twice, under ``python -m rs_rich`` and under
the ``rich`` binary built from the same source (``crates/rich-cli``), with the
same stdin, environment and working directory, and compares stdout, stderr
and the exit status exactly. The binary is built with cargo (up to date is a
no-op); set ``RS_RICH_CLI_BIN`` to use a binary built elsewhere.
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

import rs_rich.cli
from rs_rich import _native

REPO = Path(__file__).resolve().parents[3]
FIXTURE_PNG = REPO / "crates" / "rich-art" / "tests" / "fixtures" / "halo-before.png"


@pytest.fixture(scope="session")
def rich_bin() -> str:
    explicit = os.environ.get("RS_RICH_CLI_BIN")
    if explicit:
        return explicit
    cargo = shutil.which("cargo")
    if cargo is None:
        pytest.skip("no cargo to build the rich binary (set RS_RICH_CLI_BIN)")
    built = subprocess.run(
        [cargo, "build", "-p", "rs-rich-cli", "--bin", "rich", "--message-format=json"],
        cwd=REPO,
        capture_output=True,
        text=True,
    )
    assert built.returncode == 0, built.stderr
    for line in built.stdout.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-artifact" and message.get("executable"):
            if message["target"]["name"] == "rich":
                return message["executable"]
    raise AssertionError("cargo built no `rich` executable")


@pytest.fixture()
def workdir(tmp_path: Path) -> Path:
    (tmp_path / "home").mkdir()
    (tmp_path / "doc.md").write_text(
        "# Title\n\nSome *emphasis*, `code` and a [link](https://example.com).\n\n"
        "- one\n- two\n\n```python\nprint('hi')\n```\n\n> quoted\n",
        encoding="utf-8",
    )
    (tmp_path / "data.json").write_text('{"name": "rich", "values": [1, 2.5, true, null], "nested": {"a": "b"}}')
    (tmp_path / "table.csv").write_text("name,age,city\nAlice,30,Paris\nBob,7,Oslo\n")
    (tmp_path / "code.py").write_text("def greet(name):\n    return f'hello {name}'\n\nprint(greet('world'))\n")
    (tmp_path / "deploy.yaml").write_text("service:\n  name: api\n  replicas: 3\n  ports: [80, 443]\n")
    (tmp_path / "old.txt").write_text("alpha\nbeta\ngamma\n")
    (tmp_path / "new.txt").write_text("alpha\nBETA\ngamma\ndelta\n")
    (tmp_path / "flow.mmd").write_text("graph TD\n  A[Start] --> B{Ready?}\n  B -->|yes| C[Ship]\n  B -->|no| A\n")
    (tmp_path / "rich.toml").write_text("[defaults]\nwidth = 60\n[profiles.wide]\nwidth = 100\n")
    (tmp_path / "bad.toml").write_text("[defaults]\nwidth = 'wide'\n")
    shutil.copy(FIXTURE_PNG, tmp_path / "picture.png")
    return tmp_path


def environment(workdir: Path, **extra: str) -> dict:
    env = {
        key: value
        for key, value in os.environ.items()
        if key not in {"NO_COLOR", "FORCE_COLOR", "COLUMNS", "LINES", "COLORTERM", "PAGER", "RICH_CONFIG"}
        and not key.startswith(("RICH_", "XDG_"))
    }
    # No user config file, and a fixed terminal description.
    env.update(HOME=str(workdir / "home"), USERPROFILE=str(workdir / "home"), TERM="xterm-256color")
    env.update(extra)
    return env


def run_both(rich_bin: str, workdir: Path, args, stdin: bytes = b"", **env):
    environ = environment(workdir, **env)
    python = subprocess.run(
        [sys.executable, "-m", "rs_rich", *args], cwd=workdir, input=stdin, capture_output=True, env=environ
    )
    binary = subprocess.run([rich_bin, *args], cwd=workdir, input=stdin, capture_output=True, env=environ)
    return python, binary


def assert_same(python, binary):
    assert python.stdout == binary.stdout
    assert python.stderr == binary.stderr
    assert python.returncode == binary.returncode


CASES = {
    "print": (["--print", "[bold red]Hello[/] [italic]world[/] 123 https://example.com"], b"", {}),
    "print colour": (["-p", "[bold magenta]colour[/] and 1.5 True None", "-w", "40"], b"", {"FORCE_COLOR": "1"}),
    "print stdin": (["-p", "-"], b"[green]from stdin[/]\n", {}),
    "markdown": (["--markdown", "doc.md"], b"", {}),
    "markdown colour hyperlinks": (["markdown", "-y", "doc.md", "--width", "50"], b"", {"FORCE_COLOR": "1"}),
    "json": (["--json", "data.json"], b"", {"FORCE_COLOR": "1"}),
    "json stdin": (["json", "-"], b'[1, "two", {"three": 3}]', {}),
    "csv": (["--csv", "table.csv", "--title", "People"], b"", {"FORCE_COLOR": "1"}),
    "syntax": (["--syntax", "code.py", "--code-theme", "ansi_dark"], b"", {"FORCE_COLOR": "1"}),
    "auto-detect": (["code.py", "--panel", "rounded", "--title", "code"], b"", {}),
    "rule": (["--rule", "Section", "-w", "30"], b"", {}),
    "inspect": (["inspect", "deploy.yaml"], b"", {"FORCE_COLOR": "1"}),
    "text diff": (["diff", "old.txt", "new.txt"], b"", {"FORCE_COLOR": "1"}),
    "mermaid": (["mermaid", "flow.mmd"], b"", {}),
    "image": (["image", "picture.png", "--image-mode", "ascii", "--width", "40"], b"", {}),
    "image blocks": (["--image", "picture.png", "--image-mode", "blocks", "-w", "30"], b"", {"FORCE_COLOR": "1"}),
    "doctor": (["doctor"], b"", {}),
    "doctor json": (["doctor", "--report", "json"], b"", {}),
    "help": (["--help"], b"", {}),
    "subcommand help": (["config", "--help"], b"", {}),
    "version": (["--version"], b"", {}),
    "config validate": (["config", "validate", "--config", "rich.toml"], b"", {}),
    "config validate invalid": (["config", "validate", "--config", "bad.toml"], b"", {}),
    "config show": (["config", "show", "--config", "rich.toml", "--profile", "wide"], b"", {}),
    "unicode": (["unicode", "-"], "é \U0001F600 中".encode(), {}),
    "hex": (["hex", "picture.png", "--length", "64"], b"", {}),
    "env": (["env"], b"", {"API_TOKEN": "secret-value"}),
    "env path": (["env", "PATH"], b"", {}),
    "export svg": (["-p", "[b]svg[/]", "--export-svg", "-"], b"", {}),
    "unknown option": (["--no-such-option"], b"", {}),
    "unknown option json report": (["--no-such-option", "--report", "json"], b"", {}),
    "missing file": (["--markdown", "no-such-file.md"], b"", {}),
    "conflicting modes": (["--json", "--csv", "data.json"], b"", {}),
    "bad json": (["--json", "-"], b"{not json", {}),
    "bad width": (["-p", "x", "--width", "zero"], b"", {}),
    "no color env": (["-p", "[red]plain[/]"], b"", {"NO_COLOR": "1", "FORCE_COLOR": "1"}),
}


@pytest.mark.parametrize("name", list(CASES))
def test_python_m_rs_rich_matches_the_binary(rich_bin, workdir, name):
    args, stdin, env = CASES[name]
    python, binary = run_both(rich_bin, workdir, args, stdin, **env)
    assert_same(python, binary)


def test_the_cases_cover_success_and_failure(rich_bin, workdir):
    codes = set()
    for args, stdin, env in CASES.values():
        codes.add(run_both(rich_bin, workdir, args, stdin, **env)[1].returncode)
    assert 0 in codes and len(codes) >= 3


@pytest.mark.parametrize("section", ["core", "workflows", "art"])
def test_the_demo_matches_the_binary(rich_bin, workdir, section):
    # The workflows section runs `--batch`, whose workers start the command
    # line again: under Python that is `python -m rs_rich`, not the interpreter.
    args = ["--demo", "--demo-section", section, "--demo-delay", "0"]
    python, binary = run_both(rich_bin, workdir, args)
    # Each run shows its own temporary directory (same length, so layout
    # holds) and how long its captured command took.
    temporary = re.compile(rb"\.tmp[A-Za-z0-9]{6}")
    timing = re.compile(rb"^.* exit \d+ \xc2\xb7 [\d.]+m?s .*$", re.MULTILINE)
    for result in (python, binary):
        result.stdout = timing.sub(b"<timing>", temporary.sub(b".tmpXXXXXX", result.stdout))
    assert_same(python, binary)
    assert binary.returncode == 0


def test_batch_writes_the_same_files(rich_bin, workdir):
    # Batch installs a process-wide Ctrl-C handler; in `python -m rs_rich` the
    # process belongs to the command line, as it does to the binary.
    results = []
    for name, command in (("py", [sys.executable, "-m", "rs_rich"]), ("bin", [rich_bin])):
        out = workdir / f"out-{name}"
        out.mkdir()
        run = subprocess.run(
            [*command, "--batch", "data.json", "table.csv", "-o", str(out)],
            cwd=workdir,
            capture_output=True,
            env=environment(workdir),
        )
        files = {path.name: path.read_bytes() for path in sorted(out.glob("*"))}
        results.append((run.returncode, run.stdout.replace(str(out).encode(), b"OUT"), run.stderr, files))
    assert results[0] == results[1]
    assert results[0][0] == 0 and len(results[0][3]) == 2


def test_main_with_argv_runs_the_same_command_line(rich_bin, workdir):
    # `main(argv)` from a live program runs the command line in a child
    # process sharing this one's streams; its output is still the binary's.
    program = (
        "import sys, rs_rich.cli\n"
        "code = rs_rich.cli.main(['--json', 'data.json'])\n"
        "code2 = rs_rich.cli.main(['--no-such-option'])\n"
        "print('codes', code, code2)\n"
    )
    python = subprocess.run(
        [sys.executable, "-c", program], cwd=workdir, capture_output=True, env=environment(workdir)
    )
    first = subprocess.run([rich_bin, "--json", "data.json"], cwd=workdir, capture_output=True, env=environment(workdir))
    second = subprocess.run([rich_bin, "--no-such-option"], cwd=workdir, capture_output=True, env=environment(workdir))
    expected = first.stdout + second.stdout + f"codes {first.returncode} {second.returncode}\n".encode()
    assert python.stdout == expected
    assert python.stderr == first.stderr + second.stderr
    assert python.returncode == 0


def test_python_output_interleaves_in_order(workdir):
    # In-process: Python's buffered stdout is flushed before and after the run.
    program = (
        "import sys\n"
        "sys.argv = ['rich-rs', '-p', 'middle']\n"
        "from rs_rich.cli import main\n"
        "sys.stdout.write('before\\n')\n"
        "code = main()\n"
        "sys.stdout.write(f'after {code}\\n')\n"
    )
    result = subprocess.run(
        [sys.executable, "-c", program], cwd=workdir, capture_output=True, env=environment(workdir)
    )
    assert result.stdout == b"before\nmiddle\nafter 0\n"
    assert result.returncode == 0


def test_in_process_run_restores_python_signal_handling(workdir):
    program = (
        "import signal, sys\n"
        "sys.argv = ['rich-rs', '-p', 'x']\n"
        "from rs_rich.cli import main\n"
        "before = signal.getsignal(signal.SIGINT)\n"
        "main()\n"
        "assert signal.getsignal(signal.SIGINT) is before\n"
        "try:\n"
        "    signal.raise_signal(signal.SIGINT)\n"
        "except KeyboardInterrupt:\n"
        "    print('interrupted')\n"
    )
    result = subprocess.run(
        [sys.executable, "-c", program], cwd=workdir, capture_output=True, env=environment(workdir)
    )
    assert result.stdout == b"x\ninterrupted\n", result.stderr
    assert result.returncode == 0


@pytest.mark.skipif(sys.platform == "win32", reason="POSIX signals")
def test_ctrl_c_ends_python_m_rs_rich_as_it_ends_the_binary(rich_bin, workdir):
    # Waiting on stdin, SIGINT has its default action in both: the process
    # ends by the signal, with no Python traceback.
    import signal
    import time

    outcomes = []
    for command in ([sys.executable, "-m", "rs_rich"], [rich_bin]):
        process = subprocess.Popen(
            [*command, "--json", "-"],
            cwd=workdir,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=environment(workdir),
        )
        time.sleep(3)  # past the interpreter's start-up, into the read
        process.send_signal(signal.SIGINT)
        stdout, stderr = process.communicate(timeout=30)
        outcomes.append((process.returncode, stdout, stderr))
    assert outcomes[0] == outcomes[1]
    assert outcomes[1][0] == -signal.SIGINT


@pytest.mark.skipif(not hasattr(os, "openpty"), reason="needs a pseudo-terminal")
def test_a_terminal_sees_the_same_bytes(rich_bin, workdir):
    # On a terminal the CLI detects colour and size itself.
    import pty  # noqa: F401  (POSIX only)

    def on_tty(command):
        leader, follower = os.openpty()
        env = environment(workdir, COLUMNS="50", LINES="20")
        process = subprocess.Popen(
            command, cwd=workdir, stdin=subprocess.DEVNULL, stdout=follower, stderr=follower, env=env
        )
        os.close(follower)
        chunks = []
        while True:
            try:
                chunk = os.read(leader, 65536)
            except OSError:
                break
            if not chunk:
                break
            chunks.append(chunk)
        os.close(leader)
        return process.wait(), b"".join(chunks)

    args = ["--json", "data.json", "--no-pager"]
    assert on_tty([sys.executable, "-m", "rs_rich", *args]) == on_tty([rich_bin, *args])


def test_console_script_is_declared():
    pyproject = (Path(__file__).resolve().parent.parent / "pyproject.toml").read_text(encoding="utf-8")
    assert 'rich-rs = "rs_rich.cli:main"' in pyproject


def test_main_rejects_a_single_string():
    with pytest.raises(TypeError):
        rs_rich.cli.main("--help")


def test_native_entry_point_is_exposed():
    assert rs_rich.cli.__all__ == ["main"]
    assert callable(_native.cli_main)
