"""rs_rich.ext: the package layout, and the parts with no rendering.

Each Rust module of `rich-ext` is a submodule of `rs_rich.ext` with the same
name; the package re-exports every native name.
"""

from __future__ import annotations

import importlib
import pkgutil

import pytest

import rs_rich.ext
from rs_rich import _native
from rs_rich.ext import registry, testing, theme

SUBMODULES = sorted(m.name for m in pkgutil.iter_modules(rs_rich.ext.__path__))


def test_every_rust_module_has_a_python_module():
    rust = {
        "a11y", "ansi_explain", "badge", "cancel", "capabilities", "cli_doc", "countdown", "dashboard", "data",
        "derive", "diagnostic", "diff", "encoding", "env_inspect", "event", "fidelity", "format", "hex",
        "highlighter", "hyperlink", "layout", "live", "log_handler", "notify", "qa", "redact", "registry",
        "sanitize", "size_bar", "source_view", "stacktrace", "table", "target", "testing", "theme",
        "transfer", "transform", "unicode_inspect", "workflow",
    }
    assert rust <= set(SUBMODULES)


@pytest.mark.parametrize("name", SUBMODULES)
def test_submodules_export_native_objects(name):
    module = importlib.import_module(f"rs_rich.ext.{name}")
    assert module.__all__
    for exported in module.__all__:
        value = getattr(module, exported)
        native = [n for n in dir(_native) if getattr(_native, n) is value]
        assert native, f"rs_rich.ext.{name}.{exported} is not from _native"


def test_classes_say_where_they_live():
    for name in rs_rich.ext.__all__:
        value = getattr(rs_rich.ext, name)
        if isinstance(value, type) and not issubclass(value, BaseException):
            assert value.__module__.startswith("rs_rich.ext."), name


def test_errors_share_a_base():
    for name in ("DataError", "SelectError", "TransformError", "PatchParseError", "ConstraintError"):
        assert issubclass(getattr(rs_rich.ext, name), rs_rich.ext.ExtError)
    assert issubclass(rs_rich.ext.PipelineError, rs_rich.ext.TransformError)


def test_extended_theme():
    extended = theme.extended_theme()
    names = {name for table in theme.STYLE_TABLES for name, _ in table}
    assert {"error", "warning", "diff.added", "help.usage"} <= names
    assert dict(theme.EXTRA_STYLES)["success"] == "bold green"
    assert extended is not None


def test_registry_is_the_plugin_host():
    assert registry.ExtensionRegistry is _native.ExtensionRegistry
    assert registry.NumberHighlighter is _native.NumberHighlighter


