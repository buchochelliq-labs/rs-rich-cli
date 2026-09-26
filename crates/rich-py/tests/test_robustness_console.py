"""Robustness regressions: Console, the renderable bridge and the protocol.

Each test pins a defect found in the 0.0.12 audit, fixed since. Anything that
can hang, abort or overflow the native stack runs in a subprocess with a
timeout.
"""

from __future__ import annotations

import io
import os
import subprocess
import sys
import textwrap
import threading

import pytest

from rs_rich.console import Console
from rs_rich.errors import NotRenderableError
from rs_rich.text import Text


def run_py(source: str, timeout: float = 30) -> subprocess.CompletedProcess:
    """Run source in a fresh interpreter; a hang is reported as rc=None."""
    env = {k: v for k, v in os.environ.items() if k not in ("COLUMNS", "NO_COLOR")}
    try:
        return subprocess.run(
            [sys.executable, "-c", textwrap.dedent(source)],
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
        )
    except subprocess.TimeoutExpired as error:
        return subprocess.CompletedProcess(error.cmd, None, error.stdout or "", "TIMEOUT")


def assert_clean(result: subprocess.CompletedProcess, expected: str) -> None:
    assert result.returncode == 0, (result.returncode, result.stdout[-500:], result.stderr[-1500:])
    assert expected in result.stdout, (result.stdout[-500:], result.stderr[-1500:])


# --- deadlocks: the state mutex is held while Python code runs ------------------


def test_size_getter_does_not_deadlock_with_other_threads():
    # `Console.size` keeps the state lock (temporary lifetime extension of
    # `&self.state().settings`) while it calls the ConsoleDimensions
    # namedtuple, whose `__new__` is Python bytecode: the GIL can switch to a
    # thread that then blocks on the lock *holding the GIL* -> whole process
    # deadlocks. Rich finishes this in a few seconds.
    result = run_py(
        """
        import io, sys, threading
        from rs_rich.console import Console
        sys.setswitchinterval(1e-6)
        c = Console(file=io.StringIO(), width=40)
        def a():
            for _ in range(100000): c.size
        def b():
            for _ in range(100000): c.width
        ts = [threading.Thread(target=a), threading.Thread(target=b)]
        [t.start() for t in ts]; [t.join() for t in ts]
        print("finished")
        """,
        timeout=60,
    )
    assert_clean(result, "finished")


def test_highlighter_getter_does_not_deadlock_with_other_threads():
    # Same bug: `match &self.state().highlighter { None => ReprHighlighter() }`
    # constructs a Python object with the state lock held.
    result = run_py(
        """
        import io, sys, threading
        from rs_rich.console import Console
        sys.setswitchinterval(1e-6)
        c = Console(file=io.StringIO(), width=40)
        def a():
            for _ in range(100000): c.highlighter
        def b():
            for _ in range(100000): c.width
        ts = [threading.Thread(target=a), threading.Thread(target=b)]
        [t.start() for t in ts]; [t.join() for t in ts]
        print("finished")
        """,
        timeout=60,
    )
    assert_clean(result, "finished")


def test_replacing_file_whose_del_prints_does_not_hang():
    # `set_file` drops the old file inside the state lock; its `__del__`
    # printing to the console re-locks the same std Mutex -> self-deadlock.
    # Rich prints "bye" to the new file.
    result = run_py(
        """
        import io
        from rs_rich.console import Console
        out = io.StringIO()
        class F(io.StringIO):
            def __del__(self):
                c.print("bye")
        c = Console(file=F(), width=40)
        c.file = out
        print("finished", repr(out.getvalue()))
        """,
        timeout=15,
    )
    assert_clean(result, "finished 'bye\\n'")


def test_replacing_highlighter_whose_del_prints_does_not_hang():
    result = run_py(
        """
        import io
        from rs_rich.console import Console
        out = io.StringIO()
        class H:
            def __call__(self, text): return text
            def __del__(self): c.print("bye")
        c = Console(file=out, width=40, highlighter=H())
        c.highlighter = None
        print("finished", repr(out.getvalue()))
        """,
        timeout=15,
    )
    assert_clean(result, "finished 'bye\\n'")


def test_snapshot_releasing_installed_console_does_not_hang():
    # `Console::snapshot` calls `plugins::installed` while holding the state
    # lock; `installed` prunes (and drops) consoles only its table still
    # holds, so their files' `__del__` runs under the lock.
    result = run_py(
        """
        import io
        from rs_rich.console import Console
        from rs_rich.plugins import install_defaults
        y = Console(file=io.StringIO(), width=40)
        class F(io.StringIO):
            def __del__(self):
                y.print("from __del__")
        x = Console(file=F())
        install_defaults(x)
        del x
        y.print("hi")
        print("finished", repr(y.file.getvalue()))
        """,
        timeout=15,
    )
    assert_clean(result, "finished")


def test_keyboard_interrupt_while_waiting_for_another_threads_write():
    # A thread waiting for its turn blocks in `Condvar::wait` with no signal
    # check: Ctrl-C is only seen once the other thread's write finishes (never,
    # if that write is blocked). Rich's lock acquire is interruptible (0.3s).
    result = run_py(
        """
        import io, os, signal, threading, time
        from rs_rich.console import Console
        class Slow(io.StringIO):
            def write(self, text):
                if threading.current_thread() is not threading.main_thread():
                    time.sleep(6)
                return super().write(text)
        c = Console(file=Slow(), width=40)
        threading.Thread(target=c.print, args=("bg",), daemon=True).start()
        time.sleep(0.3)
        threading.Timer(0.3, lambda: os.kill(os.getpid(), signal.SIGINT)).start()
        start = time.time()
        try:
            c.print("main")
            print("no interrupt")
        except KeyboardInterrupt:
            print("interrupted after %.1f" % (time.time() - start))
        """,
        timeout=30,
    )
    assert result.returncode == 0, result.stderr[-1000:]
    assert "interrupted after" in result.stdout, result.stdout
    assert float(result.stdout.split()[-1]) < 2.0, result.stdout


# --- native stack overflow (SIGSEGV) instead of RecursionError ------------------


@pytest.mark.parametrize(
    "body",
    [
        # print -> __str__ -> print ...
        "class R:\n    def __str__(self):\n        c.print(self); return 'x'\nc.print(R())",
        # print -> __rich__ -> print ...
        "class R:\n    def __rich__(self):\n        c.print(self); return 'x'\nc.print(R())",
        # print -> render hook -> print ...
        "class H:\n    def process_renderables(self, r):\n        c.print('again'); return r\n"
        "c.push_render_hook(H())\nc.print('x')",
    ],
    ids=["str", "rich", "hook"],
)
def test_unbounded_print_recursion_raises_recursion_error(body):
    # Only `render_object` counts nesting (MAX_NESTING); a recursion through
    # `collect`/`rich_cast`/`apply_hooks` is bounded only by Python's limit,
    # and each level's native frames overflow the 8 MiB stack first. Rich
    # raises RecursionError for all three.
    source = (
        "import io\nfrom rs_rich.console import Console\n"
        "c = Console(file=io.StringIO(), width=40)\n"
        "try:\n" + textwrap.indent(body, "    ") + "\n"
        "except RecursionError:\n    print('RecursionError')\n"
    )
    result = run_py(source, timeout=60)
    assert_clean(result, "RecursionError")


# --- panics reachable from Python -------------------------------------------------


def test_render_hook_items_are_usable_from_another_thread():
    # `_PrintItem` is `unsendable` (it holds an `Rc`): printing one a hook kept
    # on another thread is a PanicException, although the docs promise every
    # rs_rich object works from any thread. Rich prints it.
    kept = []

    class Keep:
        def process_renderables(self, renderables):
            kept.extend(renderables)
            return renderables

    out = io.StringIO()
    console = Console(file=out, width=40)
    console.push_render_hook(Keep())
    console.print("hello")
    console.pop_render_hook()
    raised = []

    def worker():
        try:
            console.print(*kept)
        except BaseException as error:  # noqa: BLE001 - PanicException
            raised.append(error)

    thread = threading.Thread(target=worker)
    thread.start()
    thread.join(10)
    assert raised == []
    assert out.getvalue() == "hello\nhello\n"


def test_render_hook_items_dropped_on_another_thread_are_freed():
    # Dropping a kept `_PrintItem` on another thread reports an unraisable
    # RuntimeError ("is unsendable, but is being dropped on another thread")
    # and leaks it.
    kept = []

    class Keep:
        def process_renderables(self, renderables):
            kept.extend(renderables)
            return renderables

    console = Console(file=io.StringIO(), width=40)
    console.push_render_hook(Keep())
    console.print("hello")
    console.pop_render_hook()
    unraisable = []
    old_hook = sys.unraisablehook
    sys.unraisablehook = unraisable.append
    try:
        thread = threading.Thread(target=kept.clear)
        thread.start()
        thread.join(10)
    finally:
        sys.unraisablehook = old_hook
    assert [str(u.exc_value) for u in unraisable] == []


def test_update_screen_lines_huge_offsets_do_not_panic():
    # `ScreenUpdate` -> `Control::move_to` computes `x + 1` / `y + 1` in
    # usize: PanicException ("attempt to add with overflow"). Rich writes the
    # escape sequence.
    out = io.StringIO()
    console = Console(file=out, width=40, force_terminal=True)
    console.set_alt_screen(True)
    try:
        console.update_screen_lines([[]], 2**64 - 1, 2**64 - 1)
    except BaseException as error:  # noqa: BLE001
        assert "Panic" not in type(error).__name__, repr(error)


# --- process aborts on huge sizes (Rich raises MemoryError) ----------------------


@pytest.mark.parametrize(
    "body",
    [
        "Console(file=io.StringIO(), tab_size=2**40).print('a\\tb')",
        "Console(file=io.StringIO()).line(2**40)",
        "c = Console(file=io.StringIO()); c.render_lines('x', c.options.update_width(2**40))",
        "c = Console(file=io.StringIO()); c.render_lines('x', c.options.update(height=2**40))",
    ],
    ids=["tab_size", "line", "options_width", "options_height"],
)
def test_huge_sizes_raise_instead_of_aborting(body):
    # limits.rs caps Console(width=) at 65536 "because core would abort the
    # process", but tab_size, line(count) and ConsoleOptions widths/heights
    # reach core uncapped: "memory allocation of N bytes failed" + SIGABRT.
    source = (
        "import io, resource\n"
        "resource.setrlimit(resource.RLIMIT_AS, (4 << 30, 4 << 30))\n"
        "from rs_rich.console import Console\n"
        "try:\n    " + body + "\n"
        "except (MemoryError, ValueError, OverflowError) as e:\n    print('raised', type(e).__name__)\n"
        "else:\n    print('raised nothing')\n"
    )
    result = run_py(source, timeout=60)
    assert result.returncode == 0, (result.returncode, result.stderr[-500:])
    assert "raised" in result.stdout


# --- parity with rich 15.0.0 ----------------------------------------------------


def test_rule_accepts_text_title():
    # Rich: `rule(title: TextType = "")`. rs_rich: TypeError.
    out = io.StringIO()
    console = Console(file=out, width=20)
    console.rule(Text("tt"))
    assert out.getvalue() == "──────── tt ────────\n"


def test_rule_with_empty_characters_raises_like_rich():
    # Rich: ValueError("'characters' argument must have a cell width of at
    # least 1"); rs_rich prints a line of spaces.
    console = Console(file=io.StringIO(), width=20)
    with pytest.raises(ValueError):
        console.rule(characters="")


def test_render_lines_padding_has_no_style():
    # Rich pads with `Segment(" " * n, None)`; rs_rich with `Style()`, so the
    # segments compare unequal to Rich's.
    console = Console(file=io.StringIO(), width=10)
    line = console.render_lines("ab")[0]
    assert repr(line[-1]) == "Segment('        ')"
    assert line[-1].style is None


def test_hook_returning_non_renderable_raises():
    # Rich renders what a hook returns and raises NotRenderableError for 5;
    # rs_rich re-collects it as print does and prints "5".
    class Hook:
        def process_renderables(self, renderables):
            return [*renderables, 5]

    out = io.StringIO()
    console = Console(file=out, width=20)
    console.push_render_hook(Hook())
    with pytest.raises(NotRenderableError):
        console.print("a")
    assert out.getvalue() == ""


def test_print_negative_width_prints_nothing():
    # Rich: `width=min(-1, self.width)` -> max_width < 1 -> nothing printed.
    # rs_rich: OverflowError from the usize argument.
    out = io.StringIO()
    console = Console(file=out, width=20)
    console.print("x", width=-1)
    assert out.getvalue() == ""
