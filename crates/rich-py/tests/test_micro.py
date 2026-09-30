"""``rs_rich.micro``: the ``rs-rich-micro`` crate from Python.

Rich has no micro assets, so these check the Rust crate's behaviour through
the bindings: the built-in library, layers and trust, markup, rendering as
exactly an asset's cells (fallbacks off a terminal), and the pipeline
writing a package the registry reads back.
"""

from __future__ import annotations

import io
import json
from pathlib import Path

import pytest

from rs_rich import micro
from rs_rich.console import Console
from rs_rich.text import Text

BUILTIN = Path(__file__).resolve().parents[2] / "rich-micro" / "builtin"


def capture(renderable, width=40):
    console = Console(file=io.StringIO(), width=width, color_system=None)
    console.print(renderable)
    return console.file.getvalue()


def test_the_builtin_library():
    registry = micro.MicroRegistry()
    names = registry.names()
    for name in ["status/success", "status/warning", "status/error", "status/info", "status/loading"]:
        assert name in names
        assert name in registry
    loading = registry.require("status/loading")
    assert loading.kind == "animated"
    assert loading.size == "2x1" and loading.cols == 2
    assert loading.layer == "built-in"
    for asset in registry.assets():
        assert asset.alt
        assert asset.emoji and asset.text
        assert asset.license == "MIT"
    assert registry.get("nope") is None
    with pytest.raises(micro.MicroError):
        registry.require("nope")
    assert "built-in" in registry.explain("status/success")
    assert len(micro.MicroRegistry(builtin=False)) == 0


def test_inline_assets_win_and_render_as_their_cells():
    registry = micro.MicroRegistry()
    ship = micro.MicroAsset("status/success", "a ship", text="=>")
    assert registry.add(ship) == []
    assert registry.require("status/success").alt == "a ship"
    assert registry.require("status/success").layer == "inline"
    assert ship.fallback("text") == "=>"
    assert capture(ship) == "=>\n"
    with pytest.raises(micro.MicroError):
        micro.MicroAsset("Bad Name", "alt")
    with pytest.raises(micro.MicroError):
        micro.MicroAsset("ok", "")
    with pytest.raises(micro.MicroError):
        micro.MicroAsset("ok", "alt", text="TOO-WIDE")
    placeholder = ship.placeholder()
    assert isinstance(placeholder, Text)
    assert placeholder.plain == "=>"


def test_markup_expands_tokens_and_reports_the_rest():
    text, left = micro.micro_markup("ok :micro:status/success: :micro:nope: [b]done[/b]")
    assert text.plain == "ok ✅ :micro:nope: done"
    assert len(left) == 1 and "nope" in left[0]
    shown = capture(micro.MicroMarkup("Deploying :micro:status/loading: :micro:status/error:"))
    assert shown == "Deploying ⏳ ❌\n"
    text_only = micro.MicroMarkup(":micro:status/info:", preference="text", mode="text")
    assert capture(text_only) == "i⠀\n"
    with pytest.raises(ValueError):
        micro.MicroMarkup("x", mode="sixel")


def test_project_assets_load_only_when_trusted(tmp_path):
    project = tmp_path / "project"
    asset = micro.micro_create_package(
        (BUILTIN / "fun" / "star" / "static.png").read_bytes(),
        project / "team.star",
        name="team/star",
        alt="a star",
        text="*",
    )
    assert asset.name == "team/star"
    untrusted = micro.MicroRegistry(project=project)
    assert "team/star" not in untrusted
    assert any("not trusted" in reason for reason in untrusted.rejected)
    trusted = micro.MicroRegistry(project=project, trust_project=True)
    assert trusted.require("team/star").layer == "project"


def test_create_runs_the_pipeline_and_writes_a_package(tmp_path):
    gif = BUILTIN / "fun" / "heart" / "animation.gif"
    dest = tmp_path / "love.richmicro"
    asset = micro.micro_create_package(
        gif,
        dest,
        name="team/love",
        alt="a beating heart",
        emoji="💖",
        text="<3",
        fit="cover",
        sharpen=0.6,
        contrast=1.1,
        license="MIT",
        archive=True,
    )
    assert dest.is_file()
    assert asset.kind == "animated"
    loaded = micro.micro_load_package(dest)
    assert loaded.name == "team/love" and loaded.license == "MIT"
    registry = micro.MicroRegistry(user=tmp_path)
    assert registry.require("team/love").layer == "user"
    # Manifests are checked before anything is written.
    with pytest.raises(micro.MicroError):
        micro.micro_create_package(gif, tmp_path / "bad", name="x", alt="x", text="TOO-WIDE")
    assert not (tmp_path / "bad").exists()
    manifest = json.loads((BUILTIN / "status" / "success" / "manifest.json").read_text())
    assert manifest["license"] == "MIT"


def test_the_mode_off_a_terminal_is_text():
    mode, reason = micro.micro_mode()
    assert mode in {"kitty", "iterm", "sixel", "blocks", "text"}
    assert reason


def test_the_asset_picker_lists_micro_assets():
    from rs_rich import interact

    record = interact.AssetPicker("micro", query="heart").headless("enter", width=60, height=12)
    assert record.value == "fun/heart"
    assert any("💖" in frame for frame in record.frames)
