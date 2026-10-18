"""The pictures on docs/python/ext/documents.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich.ext import document

# --8<-- [start:notes]
notes = """#+TITLE: Release notes
* TODO [#A] Ship the parser :work:
- [X] headlines and lists
- [ ] tables
#+BEGIN_SRC python
print("hello")
#+END_SRC
"""
# --8<-- [end:notes]

# --8<-- [start:page]
page = r""".TH GREET 1 2026-10-18 "greet 1.0"
.SH NAME
greet \- say hello
.SH OPTIONS
.TP
.BR \-n ", " \-\-count " " \fIN\fR
Repeat each greeting \fIN\fR times.
"""
# --8<-- [end:page]


def org(console):
    # --8<-- [start:org]
    console.print(document.render(notes, "org"))
    # --8<-- [end:org]


def diagnostics(console):
    # --8<-- [start:diagnostics]
    guide = "= Guide\n:product: rich\n\nNOTE: {product} renders AsciiDoc.\n\ninclude::part.adoc[]\n"
    view = document.render(guide, "adoc", hyperlinks=False)
    console.print(view)
    for diagnostic in view.diagnostics:
        print(diagnostic.severity, diagnostic)
    # --8<-- [end:diagnostics]


def man(console):
    # --8<-- [start:man]
    console.print(document.render(page, path="greet.1"))
    # --8<-- [end:man]


def detect(console):
    # --8<-- [start:detect]
    print(document.detect("", "notes.org"))
    print(document.detect(page, "greet.1"))
    print(document.detect("boot ok\n", "syslog.1"))
    print(document.detect(page))
    print(document.sniff(notes)[:2])
    # --8<-- [end:detect]


shots = Shots("ext/documents")
shots.shot("org", org, width=60)
shots.shot("diagnostics", diagnostics, width=76)
shots.shot("man", man, width=60)
shots.shot("detect", detect, width=72)
shots.finish()
