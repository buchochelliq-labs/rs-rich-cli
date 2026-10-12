"""The pictures on docs/python/ext/testing.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.ext import qa
from rs_rich.panel import Panel


def stress(console):
    # --8<-- [start:stress]
    report = qa.stress(Panel("hello world"), widths=[1, 20])
    print(report.ok, report.data["renders"], report.data["issues"][0]["kind"])
    console.print(report)
    print(qa.lint("[bold]x[/italic]").data["findings"][0]["rule"])
    print([shot["key"] for shot in qa.screenshot("p", Panel("x"), widths=[10], color=["none"])])
    # --8<-- [end:stress]


shots = Shots("ext/testing")
shots.shot("stress", stress, width=60)
shots.finish()
