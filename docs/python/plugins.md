# Plugins

```python
from rs_rich import plugins
```

`rs_rich.plugins` is the plugin API of the Rust crates (`rs-rich-plugin-api`)
and rich-ext's plugin host (`ExtensionRegistry`), from Python. Rich has no
plugin API; nothing here has a Rich counterpart.

A plugin describes itself with a `PluginMetadata` and registers capabilities
through a `PluginRegistrar`. A Python plugin goes through the same Rust host as
the Rust ones: the same name rules, the same conflict checks, the same
all-or-nothing registration. Its Python code runs when Rust calls it, while
rendering.

## Writing a plugin

Subclass `Plugin` and implement `metadata()` and `register(registrar)`:

```python
from rs_rich.console import Console
from rs_rich.plugins import ExtensionRegistry, Plugin, PluginMetadata
from rs_rich.text import Text
from rs_rich.theme import Theme


class Shout(Plugin):
    def metadata(self):
        return PluginMetadata("shout", "Shouting", "1.0", "upper-case everything")

    def register(self, registrar):
        registrar.transform("upper", lambda text: Text(text.plain.upper()))
        registrar.renderer("shout", lambda source: Text(source.upper() + "!", style="bold"))
        registrar.theme("loud", Theme({"loud": "bold red"}, inherit=False))


registry = ExtensionRegistry.with_defaults()
registry.add_plugin(Shout())
for plugin in registry.plugins():
    print(plugin.metadata.id, [str(capability) for capability in plugin.capabilities])
```

```text
rich-ext ['highlighter', 'code highlighter "syntect"']
shout ['transform "upper"', 'renderer "shout"', 'theme "loud"']
```

`metadata` may also be a class attribute holding a `PluginMetadata`.
`PluginMetadata(id, name, version, description=None)` records
`PLUGIN_API_VERSION` (currently 1) as its `api_version`; a host refuses a
plugin built for another version.

## What a plugin can register

| Registrar method | Adds | A Python value is |
|---|---|---|
| `highlighter(h)` | a highlighter for printed text | an object with `highlight(text)` that styles the `Text` in place (a `rs_rich.highlighter.Highlighter`, say), or a class of them, instantiated once per console |
| `code_highlighter(name, engine)` | a syntax-highlighting engine, selectable by name | a [`CodeHighlighter`](#code-highlighters) (subclass) |
| `theme(name, theme)` | a named `Theme` | a `rs_rich.theme.Theme` |
| `box_style(name, box)` | a named box style | a `rs_rich.box` constant |
| `renderer(name, r)` | a source renderer | `f(source)`, or an object with `render(source)`, returning any renderable |
| `fence_renderer(language, r)` | a renderer for Markdown fences in `language` | `f(language, code)`, or an object with `render_fence(language, code, console, options)`, returning a renderable, a list of `Segment`s, or `None` to decline |
| `transform(name, t)` | a text transform | `f(text)`, or an object with `transform(text)`, returning a `Text` (or `None` to keep the text it styled in place) |

Anything a registry hands back (`registry.code_highlighter(name)`,
`renderer(name)`, `fence_renderer(language)`, `transform(name)`) can be
registered again, in another registry: the Rust object is registered, not a
Python wrapper around it.

Names use lowercase letters, digits, `-`, `_` and `.`, start with a letter or
digit, and are at most 64 bytes (`is_valid_name(name)`); the host checks them
when `register` returns.

## Refusals

A plugin is added whole or not at all. When its `register` raises, or it
breaks a rule, `add_plugin` raises `PluginError` and the registry is unchanged.
The error's `kind` says why (`incompatible_api`, `duplicate_plugin`,
`conflict`, `invalid_name`, `failed`), its other attributes carry the details,
and the Python exception behind it is its `__cause__`:

```python
from rs_rich.plugins import PluginError


class Broken(Plugin):
    def metadata(self):
        return PluginMetadata("broken", "Broken", "1.0")

    def register(self, registrar):
        registrar.theme("loud2", Theme({}))
        raise ValueError("not today")


try:
    registry.add_plugin(Broken())
except PluginError as error:
    print(error)
    print(error.kind, error.plugin, repr(error.__cause__))
print(registry.theme("loud2"))

try:
    registry.add_plugin(Shout())
except PluginError as error:
    print(error.kind, "-", error)
```

```text
plugin "broken" failed to register: ValueError: not today
failed broken ValueError('not today')
None
duplicate_plugin - a plugin with id "shout" is already registered
```

## Using what is registered

`text_pipeline(names)` chains transforms in order; a failing stage raises
`PluginError` with `kind == "pipeline"` and the stage in `stage`. (To chain
stages in code, without registering them, use `rs_rich.ext.transform.Pipeline`;
[Transforms](ext/transforms.md#pipeline-or-textpipeline) compares the two.)
A renderer's result is a renderable:

```python
print(registry.text_pipeline(["upper"]).apply(Text("quiet please")))
console = Console(width=40)
console.print(registry.renderer("shout").render("hello"))
print(registry.provided_by(plugins.Capability("theme", "loud")))
```

```text
QUIET PLEASE
HELLO!
shout
```

The built-in plugins are classes too: `BuiltinPlugin` (what `with_defaults()`
adds), `MermaidPlugin(ascii=None, backend="text")` and, in wheels built with
lumis, `LumisPlugin`.

```python
diagrams = ExtensionRegistry()
diagrams.add_plugin(plugins.MermaidPlugin())
console.print(diagrams.renderer("mermaid").render("graph LR\nA-->B"))
```

```text
┌───┐  ┌───┐
│ A ├─►│ B │
└───┘  └───┘

```

`fences()` is one fence renderer routing each fence to the one registered for
its language, as Markdown uses it; `render_fence(language, code, console,
options=None)` returns the segments, or `None` for a language nobody renders.

## Code highlighters

A `CodeHighlighter` highlights code for `Syntax` and Markdown code blocks.
Write one in Python by subclassing it: `highlight(code, language, theme)`
returns one list of `(start, end, style)` spans per line of
`code.split("\n")` (offsets are character indices in the line; a
`HighlightedCode` works too), `default_theme()` and `themes()` name the
themes, and a theme it does not have raises `UnknownThemeError(theme)`.
`languages()`, `language_for_path(path)` and `token_style(theme, token)` are
optional; `Syntax` colours its line numbers and indent guides with
`token_style(theme, "Text")` and `"Comment"` (a `Style`, a style string, or
`None` for nothing), as Rich asks its Pygments theme.

```python
from rs_rich.plugins import CodeHighlighter, UnknownThemeError


class Keywords(CodeHighlighter):
    def highlight(self, code, language=None, theme=None):
        if theme not in self.themes():
            raise UnknownThemeError(theme)
        lines = []
        for line in code.split("\n"):
            start = line.find("def")
            lines.append([(start, start + 3, "bold magenta")] if start >= 0 else [])
        return lines

    def default_theme(self):
        return "plain"

    def themes(self):
        return ["plain"]


registry.register_code_highlighter("keywords", Keywords())
engine = registry.code_highlighter("keywords")
result = engine.highlight("def f():\n    return 1", "python")
print([(span.start, span.end, str(span.style)) for span in result.lines[0].spans])
print(len(result.lines), registry.code_highlighter_names())
```

```text
[(0, 3, 'bold magenta')]
2 ['keywords', 'syntect']
```

What Rust receives is checked as core's `Syntax` checks every engine: spans
that are empty, overlap, run past their line or are out of order are dropped,
missing lines are unstyled, and styles lose links. Any other exception the
engine raises is a `HighlightError` (with the exception as its `__cause__`);
when it is raised while printing, the print raises it.

A code highlighter (yours, or a registry's handle) can be given to
`Syntax(..., highlighter=engine)` and `Markdown(..., highlighter=engine)`, and
fence renderers (`registry.fences()`, or your own) to
`Markdown(..., fences=[...])`.

`set_default_code_highlighter(name, theme=None)` makes one the default for
consoles the registry is installed onto (an unknown name or theme raises
`HighlighterChoiceError`).

## Installing onto a console

`registry.install(console)` installs the registered highlighters and the
default code highlighter onto a `Console`, as `ExtensionRegistry::install`
does in Rust; `install_defaults(console)` installs rich-ext's defaults. What
is installed stays with the console: later changes to the registry do not
reach it.

```python
console = Console()
registry.set_default_code_highlighter("keywords")
registry.install(console)
```
