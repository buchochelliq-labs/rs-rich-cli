"""Robustness regressions: rs_rich.ext, art, mermaid, plugins and the CLI.

Each test pins a defect found in the 0.0.12 audit, fixed since. Hang- and
crash-prone code runs in a child interpreter with a timeout.
"""

from __future__ import annotations

import os
import struct
import subprocess
import sys
import textwrap
import time
import zlib
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parents[3]
RICH_BIN = REPO / "target" / "debug" / "rich"


def run_child(code: str, timeout: float = 60.0, cwd: str | None = None) -> subprocess.CompletedProcess:
    """Run ``code`` in a fresh interpreter; a hang is reported as returncode None."""
    try:
        return subprocess.run(
            [sys.executable, "-c", textwrap.dedent(code)],
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=cwd,
        )
    except subprocess.TimeoutExpired as exc:
        return subprocess.CompletedProcess(exc.cmd, None, exc.stdout or "", exc.stderr or "")


def tall_png(path: Path, width: int, height: int) -> None:
    """A grey PNG (a few hundred bytes to a few dozen KB for tall ones)."""
    raw = b"".join(b"\x00" + bytes(width) for _ in range(height))

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


# ---------------------------------------------------------------------------
# 1. Image art: a tiny, very tall image renders an unbounded number of rows


@pytest.mark.parametrize("cls", ["AsciiArt", "BlockArt", "BrailleArt", "QuadrantArt", "ImageArt"])
def test_tall_image_render_is_bounded(cls):
    # 100 KB of raw pixels (1x100000). The row count is derived from the
    # aspect ratio with no cap, so this asks for ~4 million output rows: the
    # print never finishes. At 1x20000000 it aborts the interpreter
    # ("memory allocation of 25600000000 bytes failed").
    proc = run_child(
        f"""
        import io
        from rs_rich.art import ArtImage, {cls}
        from rs_rich.console import Console
        img = ArtImage.frombytes("L", (1, 100000), bytes(100000))
        c = Console(file=io.StringIO(), width=80, height=25)
        c.print({cls}(img))
        print("LINES", c.file.getvalue().count("\\n"))
        """,
        timeout=30,
    )
    assert proc.returncode == 0, f"hung or crashed: rc={proc.returncode} {proc.stderr[-300:]}"


def test_tall_image_aborts_instead_of_raising():
    proc = run_child(
        """
        import io
        from rs_rich.art import ArtImage, AsciiArt
        from rs_rich.console import Console
        img = ArtImage.frombytes("L", (1, 20_000_000), bytes(20_000_000))
        try:
            Console(file=io.StringIO(), width=80).print(AsciiArt(img))
        except Exception as e:
            print("RAISED", type(e).__name__)
        """,
        timeout=60,
    )
    # A process abort (SIGABRT / OOM) is never acceptable from a Python call.
    assert proc.returncode == 0, f"rc={proc.returncode} {proc.stderr[:200]}"


def test_cli_image_of_tiny_tall_png_terminates(tmp_path):
    if not RICH_BIN.exists():
        pytest.skip("rich binary not built")
    png = tmp_path / "tall.png"
    tall_png(png, 1, 20_000_000)  # ~39 KB on disk
    try:
        proc = subprocess.run(
            [str(RICH_BIN), "--image", str(png)],
            capture_output=True,
            timeout=60,
        )
        rc = proc.returncode
        err = proc.stderr[:200]
    except subprocess.TimeoutExpired:
        rc, err = None, b"timeout"
    # Expected: a bounded render or a clean error (exit 3), not an abort.
    assert rc in (0, 3), f"rc={rc} {err!r}"


# ---------------------------------------------------------------------------
# 2. Re-entrant print from Python callbacks overflows the native stack


def test_plugin_highlighter_reentrant_print_raises_recursionerror():
    proc = run_child(
        """
        import io
        from rs_rich import plugins as p
        from rs_rich.console import Console
        c = Console(file=io.StringIO(), width=40)
        class H:
            def highlight(self, text):
                c.print("inner")      # re-enters the same highlighter
        reg = p.ExtensionRegistry()
        reg.register_highlighter(H())
        reg.install(c)
        try:
            c.print("hello")
        except RecursionError:
            print("RECURSION")
        """,
        timeout=60,
    )
    # Python rich raises RecursionError; here the interpreter dies (SIGSEGV).
    assert proc.returncode == 0 and "RECURSION" in proc.stdout, f"rc={proc.returncode}"


def test_console_highlighter_reentrant_print_raises_recursionerror():
    proc = run_child(
        """
        import io
        from rs_rich.console import Console
        from rs_rich.highlighter import Highlighter
        class H(Highlighter):
            def highlight(self, text):
                c.print("inner")
        c = Console(file=io.StringIO(), width=40, highlighter=H())
        try:
            c.print("hello")
        except RecursionError:
            print("RECURSION")
        """,
        timeout=60,
    )
    assert proc.returncode == 0 and "RECURSION" in proc.stdout, f"rc={proc.returncode}"


def test_fence_renderer_that_prints_markdown_raises_recursionerror():
    proc = run_child(
        """
        import io
        from rs_rich.console import Console
        from rs_rich.markdown import Markdown
        from rs_rich.text import Text
        def fence(language, code):
            Console(file=io.StringIO(), width=40).print(Markdown("```x\\ny\\n```\\n", fences=[fence]))
            return Text("ok")
        try:
            Console(file=io.StringIO(), width=40).print(Markdown("```x\\ny\\n```\\n", fences=[fence]))
        except RecursionError:
            print("RECURSION")
        """,
        timeout=60,
    )
    assert proc.returncode == 0 and "RECURSION" in proc.stdout, f"rc={proc.returncode}"


# ---------------------------------------------------------------------------
# 3. DataNode.from_python embeds DataNodes without counting their depth


def test_datanode_nesting_is_capped():
    proc = run_child(
        """
        from rs_rich.ext import DataNode
        n = DataNode.from_python([])
        try:
            for _ in range(20000):
                n = DataNode.from_python([n])
        except (RecursionError, ValueError) as e:
            print("CAPPED", type(e).__name__)
        del n
        print("DONE")
        """,
        timeout=120,
    )
    # Parsed documents stop at 512 levels; wrapping DataNodes bypasses the
    # Nesting guard and the interpreter dies (SIGSEGV) at ~7000 levels.
    assert proc.returncode == 0 and "DONE" in proc.stdout, f"rc={proc.returncode}"


# ---------------------------------------------------------------------------
# 4. TaskTree: a deep chain is quadratic to build and overflows the stack


def test_task_tree_deep_chain_renders():
    proc = run_child(
        """
        import io
        from rs_rich.ext import TaskTree
        from rs_rich.console import Console
        t = TaskTree(); p = None
        for _ in range(10000):
            p = t.add("x", p)
        try:
            Console(file=io.StringIO(), width=80).print(t)
        except RecursionError:
            pass
        print("DONE")
        """,
        timeout=120,
    )
    assert proc.returncode == 0 and "DONE" in proc.stdout, f"rc={proc.returncode}"


def test_task_tree_add_is_not_quadratic():
    proc = run_child(
        """
        import time
        from rs_rich.ext import TaskTree
        def build(n):
            t = TaskTree(); p = None; s = time.perf_counter()
            for _ in range(n):
                p = t.add("x", p)
            return time.perf_counter() - s
        build(200)
        a, b = build(1000), build(4000)
        print("RATIO", b / a)
        """,
        timeout=120,
    )
    assert proc.returncode == 0, f"rc={proc.returncode}"
    ratio = float(proc.stdout.split("RATIO")[1])
    # 4x the tasks should be ~4x the time; CancelToken::child deep-clones the
    # ancestor chain, so it is ~16x (and O(n^2) memory).
    assert ratio < 8, ratio


# ---------------------------------------------------------------------------
# 5. Character-offset conversion is quadratic


def test_tokenize_is_linear():
    from rs_rich import ext

    ext.tokenize("a " * 1000)
    small = "a " * 20000
    big = "a " * 80000
    t0 = time.perf_counter()
    ext.tokenize(small)
    t1 = time.perf_counter()
    ext.tokenize(big)
    t2 = time.perf_counter()
    # 4x the input: ~4x the time for a linear conversion, ~16x today
    # (common::char_index recounts the prefix for every offset).
    assert (t2 - t1) < 8 * max(t1 - t0, 1e-3), ((t1 - t0), (t2 - t1))


# ---------------------------------------------------------------------------
# 6. Non-UTF-8 arguments


@pytest.mark.skipif(sys.platform == "win32", reason="POSIX byte paths")
def test_non_utf8_path_argument_opens_the_file(tmp_path):
    name = os.fsdecode(b"\xff.txt")
    (tmp_path / name).write_text("hello-from-latin1-name\n")
    proc = subprocess.run(
        [sys.executable, "-m", "rs_rich", name],
        capture_output=True,
        cwd=tmp_path,
        timeout=60,
    )
    # upstream rich-cli (click, surrogateescape) prints the file; the port
    # converts the argument lossily to "�.txt" and fails with exit 3.
    assert proc.returncode == 0, proc.stderr
    assert b"hello-from-latin1-name" in proc.stdout


@pytest.mark.skipif(sys.platform == "win32", reason="POSIX byte paths")
def test_binary_does_not_panic_on_non_utf8_argument(tmp_path):
    if not RICH_BIN.exists():
        pytest.skip("rich binary not built")
    (tmp_path / os.fsdecode(b"\xff.txt")).write_text("x\n")
    proc = subprocess.run([os.fsencode(RICH_BIN), b"\xff.txt"], capture_output=True, cwd=tmp_path, timeout=60)
    assert b"panicked" not in proc.stderr and proc.returncode != 101, proc.stderr[:200]
