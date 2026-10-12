"""The pictures on docs/python/plugins.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich import plugins
from rs_rich.plugins import ExtensionRegistry, Plugin, PluginMetadata
from rs_rich.text import Text
from rs_rich.theme import Theme


# --8<-- [start:shout]
class Shout(Plugin):
    def metadata(self):
        return PluginMetadata("shout", "Shouting", "1.0", "upper-case everything")

    def register(self, registrar):
        registrar.transform("upper", lambda text: Text(text.plain.upper()))
        registrar.renderer("shout", lambda source: Text(source.upper() + "!", style="bold"))
        registrar.theme("loud", Theme({"loud": "bold red"}, inherit=False))


registry = ExtensionRegistry.with_defaults()
registry.add_plugin(Shout())
# --8<-- [end:shout]


def renderer(console):
    # --8<-- [start:renderer]
    print(registry.text_pipeline(["upper"]).apply(Text("quiet please")))
    console.print(registry.renderer("shout").render("hello"))
    print(registry.provided_by(plugins.Capability("theme", "loud")))
    # --8<-- [end:renderer]


def mermaid(console):
    # --8<-- [start:mermaid]
    diagrams = ExtensionRegistry()
    diagrams.add_plugin(plugins.MermaidPlugin())
    console.print(diagrams.renderer("mermaid").render("graph LR\nA-->B"))
    # --8<-- [end:mermaid]


shots = Shots("plugins")
shots.shot("renderer", renderer, width=40)
shots.shot("mermaid", mermaid, width=40)
shots.finish()
