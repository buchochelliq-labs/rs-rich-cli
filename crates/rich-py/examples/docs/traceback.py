"""The pictures on docs/python/traceback.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots, stable_file

from rs_rich.traceback import Traceback


def division(console):
    # --8<-- [start:traceback]
    def ratio(total, count):
        return total / count

    try:
        ratio(1, 0)
    except ZeroDivisionError:
        console.print(Traceback(extra_lines=1))
    # --8<-- [end:traceback]


shots = Shots("traceback")
# Not this file's real path, which differs from machine to machine.
shots.shot("traceback", stable_file(division), width=64)
shots.finish()
