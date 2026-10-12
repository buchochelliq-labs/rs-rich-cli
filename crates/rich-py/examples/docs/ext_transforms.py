"""The pictures on docs/python/ext/transforms.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.ext.transform import HighlightMatches, KeepLines, Pipeline
from rs_rich.text import Text


def pipeline(console):
    # --8<-- [start:pipeline]
    log = Text("ok: started\nerror: disk full\nok: retry\nerror: gave up")
    pipeline = (
        Pipeline()
        .then("errors", KeepLines("^error"))
        .then("mark", HighlightMatches("disk", "bold"))
        .then("upper", lambda text: Text(text.plain.upper()))
    )
    console.print(pipeline.apply(log))
    print(KeepLines("^ok", invert=True)(log).plain)
    # --8<-- [end:pipeline]


shots = Shots("ext/transforms")
shots.shot("pipeline", pipeline, width=60)
shots.finish()
