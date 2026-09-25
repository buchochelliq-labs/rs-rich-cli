"""``rs_rich.plugins``: the plugin API (``rs-rich-plugin-api``) from Python (no Rich counterpart).

A plugin subclasses :class:`Plugin`: ``metadata()`` returns a
:class:`PluginMetadata`, and ``register(registrar)`` adds capabilities through
a :class:`PluginRegistrar` (highlighters, code highlighters, themes, box styles,
renderers, fence renderers and text transforms). :class:`ExtensionRegistry`
hosts plugins, Python and built-in alike, and installs them onto a console.
"""

from ._native import (
    PLUGIN_API_VERSION,
    BuiltinPlugin,
    Capability,
    CodeHighlighter,
    ExtensionRegistry,
    FenceRenderer,
    HighlightedCode,
    HighlightedLine,
    HighlighterChoiceError,
    HighlightError,
    HighlightSpan,
    MermaidPlugin,
    Plugin,
    PluginError,
    PluginMetadata,
    PluginRegistrar,
    RegisteredPlugin,
    Rendered,
    SourceRenderer,
    TextPipeline,
    TextTransform,
    UnknownThemeError,
    install_defaults,
    is_valid_name,
)

for _name in ("PluginError", "HighlightError", "UnknownThemeError", "HighlighterChoiceError"):
    globals()[_name].__module__ = __name__
del _name

__all__ = [
    "PLUGIN_API_VERSION",
    "BuiltinPlugin",
    "Capability",
    "CodeHighlighter",
    "ExtensionRegistry",
    "FenceRenderer",
    "HighlightedCode",
    "HighlightedLine",
    "HighlighterChoiceError",
    "HighlightError",
    "HighlightSpan",
    "MermaidPlugin",
    "Plugin",
    "PluginError",
    "PluginMetadata",
    "PluginRegistrar",
    "RegisteredPlugin",
    "Rendered",
    "SourceRenderer",
    "TextPipeline",
    "TextTransform",
    "UnknownThemeError",
    "install_defaults",
    "is_valid_name",
]

try:  # only in wheels built with the ``lumis`` feature
    from ._native import LumisPlugin  # noqa: F401
except ImportError:  # pragma: no cover - depends on the build
    pass
else:
    __all__.append("LumisPlugin")
