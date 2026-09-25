"""The ``rich`` command line, from Python.

``python -m rs_rich ARGS`` and the ``rich-rs ARGS`` console script run the
same command line as the ``rich`` binary built from this repository (the
Rust port of ``rich-cli``), in-process: output, errors and exit codes are the
binary's, byte for byte. The console script is ``rich-rs`` because ``rich``
belongs to ``rich-cli``.

:func:`main` with no arguments is that entry point. With an explicit
``argv`` it is the call for a running program, and runs the command line in
a child ``python -m rs_rich`` process: see :func:`main`.
"""

from __future__ import annotations

import os
import signal
import subprocess
import sys
import threading
from typing import Optional, Sequence

from ._native import cli_main as _cli_main

__all__ = ["main"]

def _flush() -> None:
    for stream in (sys.stdout, sys.stderr):
        try:
            if stream is not None:
                stream.flush()
        except (OSError, ValueError):
            pass


def _self_program() -> list:
    """How the command line starts itself again (``--batch`` workers, the
    demo's child ``--watch``): the executable is the interpreter, so
    ``python -m rs_rich``. Empty (the current executable) when Python cannot
    say where its interpreter is."""
    if not sys.executable:
        return []
    return [sys.executable, "-m", __package__ or "rs_rich"]


def _run_in_process(argv: Sequence[str]) -> int:
    """Run the command line in this process, with the GIL released.

    The process belongs to the command line here, so Ctrl-C does what it does
    to the ``rich`` binary: SIGINT gets its default action for the run (the
    batch, watch and demo modes install their own handler, as in the binary),
    and Python's handler is put back afterwards.
    """
    previous = None
    main_thread = threading.current_thread() is threading.main_thread()
    if main_thread and hasattr(signal, "SIGINT"):
        previous = signal.signal(signal.SIGINT, signal.SIG_DFL)
    _flush()
    try:
        return _cli_main(_self_program(), list(argv))
    finally:
        _flush()
        if previous is not None:
            signal.signal(signal.SIGINT, previous)


def _run_child(argv: Sequence[str]) -> int:
    """Run the command line in a child ``python -m rs_rich`` process.

    It shares this process's stdin, stdout and stderr, so output lands exactly
    where the in-process run would put it.
    """
    if not sys.executable:
        # No interpreter to start: run here instead.
        return _run_in_process(argv)
    package_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    env = dict(os.environ)
    env["PYTHONPATH"] = os.pathsep.join(
        path for path in (package_root, env.get("PYTHONPATH")) if path
    )
    _flush()
    try:
        status = subprocess.call([*_self_program(), *argv], env=env)
    finally:
        _flush()
    # A child ended by a signal reports as a shell would: 128 + the signal.
    return 128 - status if status < 0 else status


def main(argv: Optional[Sequence[str]] = None) -> int:
    """Run the ``rich`` command line and return its exit status.

    With ``argv=None`` (``python -m rs_rich`` and ``rich-rs``) the arguments
    are ``sys.argv[1:]`` and the command line runs in this process with the
    GIL released, as the program itself: it writes to file descriptors 1 and
    2, reads file descriptor 0, and Ctrl-C ends it as it ends the binary.

    With an explicit ``argv`` (a list of argument strings, without the
    program name) the command line runs in a child ``python -m rs_rich``
    process that shares this one's standard streams. The command line keeps
    process-wide state no library call should leave behind: its ``--batch``,
    ``--watch`` and demo modes install a Ctrl-C handler that replaces
    Python's and can be installed only once per process (and a config file
    can turn those modes on without any flag), and a few error paths end the
    process. The output and exit status are the same either way.
    """
    if argv is None:
        return _run_in_process(sys.argv[1:])
    if isinstance(argv, (str, bytes)):
        raise TypeError("argv must be a sequence of argument strings, not a single string")
    return _run_child([os.fsdecode(arg) for arg in argv])
