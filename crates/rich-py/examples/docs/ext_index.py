"""The pictures on docs/python/ext/index.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.ext.diagnostic import Diagnostic, Location


def diagnostic(console):
    # --8<-- [start:diagnostic]
    console.print(Diagnostic.error("mismatched types", code="E0308",
                                   location=Location("src/main.rs", 12, 5),
                                   causes=["expected `u16`, found `&str`"]))
    # --8<-- [end:diagnostic]


shots = Shots("ext/index")
shots.shot("diagnostic", diagnostic, width=60)
shots.finish()
