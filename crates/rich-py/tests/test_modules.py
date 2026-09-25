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
        ("rs_rich.console", ["Console", "ConsoleOptions", "ConsoleDimensions", "Capture", "CaptureError"]),
        ("rs_rich.segment", ["Segment"]),
        ("rs_rich.measure", ["Measurement"]),
        ("rs_rich.theme", ["Theme", "ThemeStackError"]),
        ("rs_rich.terminal_theme", ["TerminalTheme", "DEFAULT_TERMINAL_THEME", "MONOKAI"]),
        ("rs_rich.text", ["Text"]),
        ("rs_rich.style", ["Style"]),
        ("rs_rich.table", ["Table"]),
        ("rs_rich.panel", ["Panel"]),
        ("rs_rich.markup", ["escape"]),
        ("rs_rich.errors", ["ConsoleError", "MarkupError", "StyleSyntaxError", "NotRenderableError", "MissingStyle"]),
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
    aliases = {
        "JustifyMethod",
        "OverflowMethod",
        "AlignMethod",
        "StyleType",
        "PaddingDimensions",
        "RenderableType",
        "RichCast",
        "ConsoleRenderable",
    }
    runtime = {name for name in dir(_native) if not name.startswith("_")} | {"__version__"}
    assert stubbed - aliases == runtime


# Every module an area fills in later exists now, so areas never create
# (and collide on) shared files. Each imports, even while empty.
AREA_MODULES = [
    "rule", "padding", "align", "columns", "constrain", "styled", "tree", "layout", "bar", "spinner",
    "markdown", "syntax", "json", "pretty", "traceback", "highlighter",
    "live", "progress", "status", "screen", "pager", "prompt", "logging",
    "color", "emoji", "theme", "segment", "measure", "terminal_theme",
    "ext", "art", "mermaid", "plugins",
]


@pytest.mark.parametrize("name", AREA_MODULES)
def test_every_area_module_imports(name):
    module = importlib.import_module(f"rs_rich.{name}")
    assert isinstance(module.__all__, list)
    for exported in module.__all__:
        assert getattr(module, exported) is getattr(_native, exported)


def test_the_stub_file_has_a_section_per_area():
    sections = [
        line for line in (PACKAGE / "_native.pyi").read_text(encoding="utf-8").splitlines()
        if line.startswith("# --- area: ")
    ]
    for area in ["text-style", "renderables", "code", "live", "ext", "art", "plugins", "cli"]:
        assert any(line.startswith(f"# --- area: {area} ") for line in sections), area


def test_python_dash_m_is_reserved_for_the_cli():
    import rs_rich.__main__ as main

    with pytest.raises(NotImplementedError):
        main.main()


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
