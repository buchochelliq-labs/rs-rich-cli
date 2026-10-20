"""``rs_rich.ext.sql``: the SQL highlighter (``rich_ext::sql``).

No Rich counterpart: this is the port's own ``rs-rich-ext`` crate, a pure-Rust
SQL lexer with dialect keywords and Jinja blocks (for dbt), without syntect.
``SqlHighlighter`` is a code highlighter for ``Syntax`` and ``Markdown``
(``highlighter="sql"`` picks it by name); ``highlight`` styles code into a
``Text`` and ``tokenize`` lists its tokens.
"""

from .._native import (
    SQL_DEFAULT_THEME,
    SQL_LANGUAGES,
    SQL_THEMES,
    SQL_TOKEN_KINDS,
    SqlHighlighter,
    SqlToken,
    is_sql,
    sql_highlight,
    sql_token_style,
    sql_tokenize,
)

#: The crate's names for the same things.
highlight = sql_highlight
tokenize = sql_tokenize
token_style = sql_token_style
LANGUAGES = SQL_LANGUAGES
THEMES = SQL_THEMES
DEFAULT_THEME = SQL_DEFAULT_THEME
TOKEN_KINDS = SQL_TOKEN_KINDS

__all__ = [
    "SQL_DEFAULT_THEME",
    "SQL_LANGUAGES",
    "SQL_THEMES",
    "SQL_TOKEN_KINDS",
    "SqlHighlighter",
    "SqlToken",
    "is_sql",
    "sql_highlight",
    "sql_token_style",
    "sql_tokenize",
    "highlight",
    "tokenize",
    "token_style",
    "LANGUAGES",
    "THEMES",
    "DEFAULT_THEME",
    "TOKEN_KINDS",
]
