"""The pictures on docs/python/syntax.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.syntax import Syntax

# --8<-- [start:code]
code = '''def fib(n):
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)
'''
# --8<-- [end:code]


def fib(console):
    # --8<-- [start:syntax]
    console.print(Syntax(code, "python", line_numbers=True, highlight_lines={3}))
    console.print(Syntax(code, "python", line_range=(2, 3), indent_guides=True))
    # --8<-- [end:syntax]


def signed_range(console):
    # --8<-- [start:range]
    code = "x = 1\ny = 2\nz = 3\n"
    console.print(Syntax(code, "python", line_numbers=True, start_line=-1, line_range=(2, -1)))
    # --8<-- [end:range]


shots = Shots("syntax")
shots.shot("syntax", fib, width=44)
shots.shot("range", signed_range, width=30)
shots.finish()
