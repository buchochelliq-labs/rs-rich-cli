"""The pictures on docs/python/ext/inspectors.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.ext import env_inspect, hex, source_view, unicode_inspect


def source(console):
    # --8<-- [start:source]
    code = 'fn main() {\n\tlet greeting = "Hello";\n}\n'
    view = source_view.SourceView(code, "text", search="hello", start_line=10)
    console.print(view)
    print(view.matches())
    # --8<-- [end:source]


def hex_dump(console):
    # --8<-- [start:hex]
    data = b"\x00\x01\x02 Hello, world!\x7f\xff"
    console.print(hex.HexView(data, bytes_per_line=8, highlight=b"Hello"))
    print(hex.find_all(data, '"l"'), hex.byte_class(0x7f))
    # --8<-- [end:hex]


def graphemes(console):
    # --8<-- [start:unicode]
    view = unicode_inspect.UnicodeView("e\u0301👍🏽\t")
    console.print(view)
    print(view.summary_text())
    # --8<-- [end:unicode]


def environment(console):
    # --8<-- [start:env]
    console.print(env_inspect.EnvView({"HOME": "/home/ada", "API_TOKEN": "sk-live-abc"}))
    kinds = {"/usr/bin": "directory", "/etc/passwd": "file"}
    path = env_inspect.PathView("PATH", "/usr/bin:/opt/none:/etc/passwd:/usr/bin", separator=":",
                                probe=lambda entry: kinds.get(entry, "missing"))
    console.print(path)
    # --8<-- [end:env]


shots = Shots("ext/inspectors")
shots.shot("source", source, width=60)
shots.shot("hex", hex_dump, width=60)
shots.shot("unicode", graphemes, width=70)
shots.shot("env", environment, width=60)
shots.finish()
