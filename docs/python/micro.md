# Micro assets

```python
from rs_rich.micro import MicroRegistry, MicroAsset, MicroMarkup, micro_markup
```

`rs_rich.micro` is the port's `rs-rich-micro` crate from Python: emoji-sized
inline images and animations, written `:micro:name:`. Rich has none, so the
reference is the Rust crate (see the
[micro assets guide](https://buchochelliq-labs.github.io/rs-rich-cli/guide/micro/)).

## In text

`MicroMarkup` is console markup with `:micro:name:` tokens. On a terminal
that can show images (Kitty, iTerm2, Sixel) it draws them; on another colour
terminal and in captures, files and pipes each asset shows its fallback,
its emoji or its text:

```python
from rs_rich.console import Console
from rs_rich.micro import MicroMarkup

console = Console()
console.print(MicroMarkup("Deploying :micro:status/loading: then :micro:status/success:"))
```

```text
Deploying ⏳ then ✅
```

`mode="text"` never draws images, and `preference="text"` (or `"alt"`)
picks the text fallback (or the alt text) over the emoji. An unknown name
stays as typed. `micro_markup(markup)` returns the parsed `Text` and the
tokens it left as written:

```python
from rs_rich.micro import micro_markup

text, left = micro_markup("ok :micro:status/success: :micro:nope:")
print(text.plain)
print(left)
```

```text
ok ✅ :micro:nope:
['unknown micro asset "nope" at byte 26']
```

## The registry

`MicroRegistry()` holds the built-in library (`status/success`,
`status/warning`, `status/error`, `status/info`, `status/loading`, `dev/…`
and `fun/…`). Layers resolve built-in < user < trusted project < inline:

```python
from rs_rich.micro import MicroAsset, MicroRegistry

registry = MicroRegistry(project=".rich/micro", trust_project=False)
print(registry.names()[:3])
asset = registry.require("status/loading")
print(asset.kind, asset.size, asset.alt, asset.license)

registry.add(MicroAsset("team/ok", "a green tick", emoji="✅", text="ok"))
print(registry.require("team/ok").layer)
```

```text
['dev/branch', 'dev/bug', 'dev/package']
animated 2x1 ring of dots turning, for work in progress MIT
inline
```

A project's assets load only with `trust_project=True`; `rejected` lists
what did not load, including an untrusted project. `explain(name)` shows
which layer won. A `MicroAsset` is a renderable that always takes exactly
its columns; `fallback()` and `placeholder()` give its cells as a `str` and
a `Text`.

## Making assets

`micro_create_package` runs an image (a path, or PNG, APNG, GIF or JPEG
bytes, here made with Pillow) through the pipeline, fitted to the asset's cells with optional
contrast, sharpening and transparency, and writes a package the registry
accepts:

```python
import io
import tempfile
from pathlib import Path

from PIL import Image
from rs_rich.micro import MicroRegistry, micro_create_package

png = io.BytesIO()
Image.new("RGBA", (64, 64), (40, 90, 200, 255)).save(png, "PNG")

user = Path(tempfile.mkdtemp())  # ~/.config/rich/micro, say
asset = micro_create_package(
    png.getvalue(), user / "team.square",
    name="team/square", alt="a blue square", emoji="🟦", text="[]",
    fit="cover", sharpen=0.8, license="MIT",
)
print(asset.size, asset.kind)
print(MicroRegistry(user=user).require("team/square").layer)
```

```text
2x1 static
user
```

`archive=True` writes one `.richmicro` zip file instead of a folder, and
`micro_load_package(path)` reads one back. `micro_mode()` says how this
terminal would draw assets: `("blocks", "no graphics protocol; colour
cells")`, say.

`rs_rich.interact.AssetPicker("micro")` picks one by name.
