"""``rs_rich.ext.sql``: the SQL highlighter.

Rich has no SQL lexer of its own, so the cases are the Rust crate's tests
(``crates/rich-ext/src/sql/tests.rs``), run through the bindings.
"""

from __future__ import annotations

import io

import pytest

from rs_rich import plugins
from rs_rich.console import Console
from rs_rich.ext import sql
from rs_rich.markdown import Markdown
from rs_rich.style import Style
from rs_rich.syntax import Syntax, code_highlighters
from rs_rich.text import Text


def tokens(code: str):
    return [(token.kind, token.text) for token in sql.tokenize(code) if token.kind != "whitespace"]


def test_tokens_cover_the_input_in_order():
    code = "SELECT a.b, count(*) FROM \"T\" WHERE x <> 'y''z' -- é\n/* d */;"
    at = 0
    for token in sql.tokenize(code):
        assert token.start == at and token.end > token.start
        assert code[token.start : token.end] == token.text
        at = token.end
    assert at == len(code)


def test_keywords_names_functions_and_types():
    assert tokens("select Count(id), t.select, cast(x AS VARCHAR(10)) from tbl") == [
        ("keyword", "select"),
        ("function", "Count"),
        ("punctuation", "("),
        ("name", "id"),
        ("punctuation", ")"),
        ("punctuation", ","),
        ("name", "t"),
        ("punctuation", "."),
        ("name", "select"),
        ("punctuation", ","),
        ("keyword", "cast"),
        ("punctuation", "("),
        ("name", "x"),
        ("keyword", "AS"),
        ("type", "VARCHAR"),
        ("punctuation", "("),
        ("number", "10"),
        ("punctuation", ")"),
        ("punctuation", ")"),
        ("keyword", "from"),
        ("name", "tbl"),
    ]
    assert tokens("QUALIFY PRAGMA null TRUE int64 timestamptz") == [
        ("keyword", "QUALIFY"),
        ("keyword", "PRAGMA"),
        ("constant", "null"),
        ("constant", "TRUE"),
        ("type", "int64"),
        ("type", "timestamptz"),
    ]


def test_jinja_blocks_are_templates():
    assert tokens("select * from {{ ref('orders') }} {%- if x %}where 1{% endif -%} {# note #}") == [
        ("keyword", "select"),
        ("operator", "*"),
        ("keyword", "from"),
        ("template", "{{ ref('orders') }}"),
        ("template", "{%- if x %}"),
        ("keyword", "where"),
        ("number", "1"),
        ("template", "{% endif -%}"),
        ("template", "{# note #}"),
    ]


def test_highlight_and_token_styles():
    text = sql.highlight("select 'x'", "ansi_light")
    assert isinstance(text, Text) and text.plain == "select 'x'"
    assert len(text.spans) == 2
    assert sql.token_style("keyword", "ansi_dark") == Style.parse("bright_blue")
    assert sql.token_style("name", "ansi_dark") is None
    assert sql.token_style("keyword") == Style.parse("#66d9ef")
    with pytest.raises(ValueError, match="token kind"):
        sql.token_style("verb")
    assert sql.TOKEN_KINDS[0] == "whitespace" and len(sql.TOKEN_KINDS) == 15
    assert sql.THEMES == ["ansi_dark", "ansi_light", "monokai"] and sql.DEFAULT_THEME == "monokai"
    assert sql.is_sql("DuckDB") and not sql.is_sql("python")
    assert {"sql", "postgresql", "snowflake"} <= set(sql.LANGUAGES)
    assert repr(sql.tokenize("x")[0]) == "SqlToken(kind='name', start=0, end=1, text='x')"


def console() -> Console:
    return Console(file=io.StringIO(), force_terminal=True, color_system="standard", width=40)


def test_a_sql_code_block_is_highlighted_by_name():
    assert "sql" in code_highlighters()
    out = console()
    out.print(Syntax("SELECT 'x' -- c", "sql", theme="ansi_dark", highlighter="sql"))
    printed = out.file.getvalue()
    assert "\x1b[94mSELECT" in printed and "\x1b[33m'x'" in printed and "\x1b[2m-- c" in printed


def test_a_sql_markdown_fence_is_highlighted():
    out = console()
    markdown = Markdown("```sql\nselect 1 from {{ ref('t') }}\n```\n", code_theme="ansi_dark", highlighter=sql.SqlHighlighter())
    out.print(markdown)
    printed = out.file.getvalue()
    assert "\x1b[94mselect" in printed and "\x1b[96m{{ ref('t') }}" in printed


def test_the_highlighter_and_its_fallback():
    alone = sql.SqlHighlighter()
    assert isinstance(alone, plugins.CodeHighlighter)
    assert alone.default_theme() == "monokai"
    assert alone.language_for_path("models/orders.SQL") == "sql"
    assert alone.language_for_path("a.py") is None
    assert not alone.highlight("select", "python", "ansi_dark").lines[0].spans
    assert len(alone.highlight("select", "sql", "ansi_dark").lines[0].spans) == 1
    both = sql.SqlHighlighter("syntect")
    assert "Python" in both.languages() and "duckdb" in both.languages()
    # The default theme is the SQL one (monokai), which syntect does not have.
    assert both.highlight("x = 1", "python", "base16-ocean.dark").lines[0].spans
    with pytest.raises(ValueError, match="unknown code highlighter"):
        sql.SqlHighlighter("nope")


def test_the_plugin_registers_sql():
    registry = plugins.ExtensionRegistry()
    registry.add_plugin(plugins.SqlPlugin())
    assert "sql" in registry.code_highlighter_names()
