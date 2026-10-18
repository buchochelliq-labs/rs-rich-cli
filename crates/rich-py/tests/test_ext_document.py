"""rs_rich.ext.document: Org, AsciiDoc and man pages beside Markdown and
reStructuredText, rendered in Rust, with detection and diagnostics."""

from __future__ import annotations

import pytest

from rs_rich.ext import document
from rs_rich.ext.document import DocumentView, render

from conftest import render as printed

ORG = "#+TITLE: Plan\n* TODO [#A] Ship it :work:\n- [X] parse\n- [ ] render\n"
ADOC = "= Guide\n:product: rich\n\nNOTE: {product} renders AsciiDoc.\n\ninclude::part.adoc[]\n"
MAN = ".TH GREET 1\n.SH NAME\ngreet \\- say hello\n.SH OPTIONS\n.TP\n.B \\-n\nCount.\n"


def lines(text: str) -> list[str]:
    return [line.rstrip() for line in text.splitlines()]


def test_org_renders_headlines_and_checkboxes():
    out = printed(render(ORG, "org"), width=50)
    assert "Plan" in out
    assert "TODO [#A] Ship it  :work:" in out
    assert " • [x] parse" in lines(out)


def test_asciidoc_substitutes_attributes_and_warns_about_includes():
    view = render(ADOC, "adoc", hyperlinks=False)
    assert view.format == "asciidoc"
    out = printed(view, width=50)
    assert "rich renders AsciiDoc." in out
    assert "include::part.adoc[]" in out
    (diagnostic,) = view.diagnostics
    assert diagnostic.severity == "warning"
    assert diagnostic.line == 6
    assert str(diagnostic).startswith("line 6: include::part.adoc[]")


def test_man_pages_have_their_banner_and_options():
    out = printed(DocumentView(MAN, "man"), width=40)
    assert lines(out)[0].startswith("GREET(1)")
    assert "greet - say hello" in out
    assert "-n" in lines(out)


def test_markdown_and_rst_are_formats_too():
    assert "Title" in printed(render("# Title\n\nbody", "md"))
    assert "Some text." in printed(render("Title\n=====\n\nSome *text*.", "rst"))


def test_the_format_comes_from_the_path_or_the_content():
    assert render(MAN, path="greet.1").format == "man"
    assert render(ORG).format == "org"
    with pytest.raises(ValueError, match="cannot tell"):
        render("just some words")
    with pytest.raises(ValueError, match="unknown document format"):
        render("x", "textile")


def test_links_write_their_target_without_hyperlinks():
    source = "See [[https://orgmode.org][Org]]."
    assert "Org (https://orgmode.org)" in printed(render(source, "org", hyperlinks=False), width=60)
    coloured = printed(render(source, "org"), width=60, color=True)
    assert "\x1b]8;" in coloured


def test_detection_rules():
    found = document.detect("", "notes.org")
    assert (found.format, found.by, found.confidence) == ("org", "name", 1.0)
    found = document.detect(MAN, "ls.1")
    assert (found.format, found.by) == ("man", "name_and_content")
    assert document.detect("boot ok\n", "syslog.1") is None
    assert document.detect(MAN, "gen.c") is None
    assert document.detect(MAN).by == "content"
    # Markdown never reaches CERTAIN on content alone, but may be LIKELY.
    markdown = "# Title\n\nSee [x](y).\n\n```\ncode\n```\n```\n"
    assert document.detect(markdown) is None
    assert document.detect(markdown, threshold=document.LIKELY).format == "markdown"
    assert document.LIKELY == 0.6 and document.CERTAIN == 0.9


def test_reprs_read_as_python():
    assert repr(document.detect("", "notes.org")) == (
        "DocumentDetection(format='org', confidence=1.0, by='name')"
    )
    (diagnostic,) = render(ADOC, "adoc").diagnostics
    assert repr(diagnostic).startswith("DocumentDiagnostic(line=6, severity='warning', message='")
    assert repr(render(ORG, "org")) == f"DocumentView(<{len(ORG)} characters>, format='org')"


def test_sniff_lists_every_format():
    scores = dict(document.sniff(ORG))
    assert set(scores) == {"markdown", "rst", "org", "asciidoc", "man"}
    assert max(scores, key=scores.get) == "org"


def test_formats_describe_themselves():
    by_id = {info.id: info for info in document.formats()}
    assert list(by_id) == ["markdown", "rst", "org", "asciidoc", "man"]
    assert "adoc" in by_id["asciidoc"].aliases
    assert "org" in by_id["org"].extensions
    assert "checkboxes" in by_id["org"].features
    assert by_id["man"].name == "man page"


def test_styles_are_theme_keys():
    names = dict(document.STYLES)
    assert names["document.todo"] == "bold red"
    assert "document.admonition.warning" in names


def test_garbled_input_never_raises():
    pieces = ["* ", "#+BEGIN_SRC", "|", "[[", "]]", ".TH", ".RS\n", "\\f(", "----\n", "|===\n", "漢", "\t", "\r\n"]
    for format in ("org", "asciidoc", "man"):
        source = "".join(pieces[(i * 7 + len(format)) % len(pieces)] for i in range(400))
        printed(render(source, format), width=30)
