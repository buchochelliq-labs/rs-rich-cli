"""rs_rich's module layout mirrors Rich's, and matches its type stubs."""

from __future__ import annotations

import ast
import importlib
from pathlib import Path

import pytest

import rs_rich
from rs_rich import _native

PACKAGE = Path(rs_rich.__file__).parent
PYPROJECT = Path(__file__).resolve().parent.parent / "pyproject.toml"


@pytest.mark.parametrize(
    "module, names",
    [
        ("rs_rich.console", ["Console"]),
        ("rs_rich.text", ["Text"]),
        ("rs_rich.style", ["Style"]),
        ("rs_rich.table", ["Table"]),
        ("rs_rich.panel", ["Panel"]),
        ("rs_rich.markup", ["escape"]),
        ("rs_rich.errors", ["ConsoleError", "MarkupError", "StyleSyntaxError"]),
        ("rs_rich.box", ["ROUNDED", "HEAVY_HEAD", "SIMPLE", "ASCII", "MARKDOWN"]),
    ],
)
def test_rich_module_paths(module, names):
    loaded = importlib.import_module(module)
    for name in names:
        assert name in loaded.__all__
        assert getattr(loaded, name) is getattr(_native, name)


def test_the_stubs_describe_exactly_the_compiled_module():
    tree = ast.parse((PACKAGE / "_native.pyi").read_text(encoding="utf-8"))
    stubbed = set()
    for node in tree.body:
        if isinstance(node, (ast.ClassDef, ast.FunctionDef)):
            stubbed.add(node.name)
        elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            stubbed.add(node.target.id)
    aliases = {"JustifyMethod", "OverflowMethod", "AlignMethod", "StyleType", "PaddingDimensions", "RenderableType"}
    runtime = {name for name in dir(_native) if not name.startswith("_")} | {"__version__"}
    assert stubbed - aliases == runtime


def test_box_constants():
    from rs_rich import box

    assert repr(box.ROUNDED) == "box.ROUNDED"
    assert len(box.__all__) == 20


def test_escape():
    from rs_rich.markup import escape

    assert escape("[bold]x[/bold]") == "\\[bold]x\\[/bold]"


def test_the_global_print(capsys):
    rs_rich.print("[bold]hi[/]", 1)
    assert capsys.readouterr().out == "hi 1\n"
    assert rs_rich.get_console() is rs_rich.get_console()


def test_the_version_is_the_packages():
    version = next(
        line.split('"')[1] for line in PYPROJECT.read_text().splitlines() if line.startswith("version")
    )
    assert rs_rich.__version__ == version
